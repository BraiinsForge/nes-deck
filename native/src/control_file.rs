use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, IntoRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const MAXIMUM_BYTES: usize = 64;

fn validate_path(path: &Path) -> Result<(), String> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.first() != Some(&b'/') || bytes.len() >= libc::PATH_MAX as usize {
        return Err("control file path must be absolute".to_owned());
    }
    Ok(())
}

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    validate_path(path)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| format!("cannot open control file {}: {error}", path.display()))?;
    let mut buffer = [0; MAXIMUM_BYTES - 1];
    let mut used = 0;
    while used < buffer.len() {
        match file.read(&mut buffer[used..]) {
            Ok(0) => break,
            Ok(amount) => used += amount,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(format!(
                    "cannot read control file {}: {error}",
                    path.display()
                ));
            }
        }
    }
    Ok(buffer[..used].to_vec())
}

fn close(file: File, path: &Path) -> Result<(), String> {
    let descriptor = file.into_raw_fd();
    if unsafe { libc::close(descriptor) } == 0 {
        Ok(())
    } else {
        Err(format!(
            "cannot close control file {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ))
    }
}

pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    validate_path(path)?;
    if bytes.len() > MAXIMUM_BYTES {
        return Err(format!("control file exceeds {MAXIMUM_BYTES} bytes"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| format!("cannot open control file {}: {error}", path.display()))?;
    if let Ok(metadata) = file.metadata()
        && metadata.is_file()
        && metadata.len() > 0
        && unsafe { libc::ftruncate(file.as_raw_fd(), 0) } != 0
    {
        let error = std::io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(libc::EINVAL | libc::EPERM)) {
            return Err(format!(
                "cannot truncate control file {}: {error}",
                path.display()
            ));
        }
    }
    let result = file
        .write_all(bytes)
        .map_err(|error| format!("cannot write control file {}: {error}", path.display()));
    let closed = close(file, path);
    result.and(closed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    fn fixture_directory() -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "retrodeck-control-file-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        directory
    }

    #[test]
    fn reads_exact_bounded_control_bytes_and_follows_links() {
        let directory = fixture_directory();
        let path = directory.join("brightness");
        let link = directory.join("brightness-link");
        std::fs::write(&path, b" 12\n").unwrap();
        symlink(&path, &link).unwrap();
        assert_eq!(read(&path).unwrap(), b" 12\n");
        assert_eq!(read(&link).unwrap(), b" 12\n");
        std::fs::write(&path, vec![b'X'; 80]).unwrap();
        assert_eq!(read(&path).unwrap(), vec![b'X'; MAXIMUM_BYTES - 1]);
        assert!(read(Path::new("relative")).is_err());
        assert!(read(&directory.join("missing")).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn writes_exact_existing_control_bytes() {
        let directory = fixture_directory();
        let path = directory.join("brightness");
        let link = directory.join("brightness-link");
        std::fs::write(&path, b"12345\n").unwrap();
        symlink(&path, &link).unwrap();
        write(&link, b"7\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"7\n");
        assert!(write(Path::new("relative"), b"7\n").is_err());
        assert!(write(&directory.join("missing"), b"7\n").is_err());
        assert!(write(&path, &[b'X'; MAXIMUM_BYTES + 1]).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"7\n");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
