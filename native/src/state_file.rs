use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, IntoRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const MAXIMUM_BYTES: usize = 64;
const TEMPORARY_ATTEMPTS: u32 = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateRead {
    Missing,
    Value(Vec<u8>),
}

fn validate_path(path: &Path) -> Result<(), String> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.first() != Some(&b'/') || bytes.len() >= libc::PATH_MAX as usize {
        return Err("state path must be absolute".to_owned());
    }
    Ok(())
}

pub fn read(path: &Path) -> Result<StateRead, String> {
    validate_path(path)?;
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StateRead::Missing);
        }
        Err(error) => return Err(format!("cannot open state {}: {error}", path.display())),
    };
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect state {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() > MAXIMUM_BYTES as u64 {
        return Err(format!(
            "state must be a regular file no larger than {MAXIMUM_BYTES} bytes: {}",
            path.display()
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAXIMUM_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read state {}: {error}", path.display()))?;
    if bytes.len() > MAXIMUM_BYTES {
        return Err(format!("state is too large: {}", path.display()));
    }
    Ok(StateRead::Value(bytes))
}

fn temporary_path(path: &Path, attempt: u32) -> PathBuf {
    let mut bytes = path.as_os_str().as_bytes().to_vec();
    bytes.extend_from_slice(format!(".tmp.{}.{attempt}", std::process::id()).as_bytes());
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

fn remove_temporary(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn close(file: File, path: &Path) -> Result<(), String> {
    let descriptor = file.into_raw_fd();
    if unsafe { libc::close(descriptor) } == 0 {
        Ok(())
    } else {
        Err(format!(
            "cannot close state temporary file {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ))
    }
}

pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    validate_path(path)?;
    if bytes.len() > MAXIMUM_BYTES {
        return Err(format!("state exceeds {MAXIMUM_BYTES} bytes"));
    }
    let mut opened = None;
    for attempt in 0..TEMPORARY_ATTEMPTS {
        let temporary = temporary_path(path, attempt);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC)
            .open(&temporary)
        {
            Ok(file) => {
                opened = Some((temporary, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(format!(
                    "cannot create state temporary file {}: {error}",
                    temporary.display()
                ));
            }
        }
    }
    let Some((temporary, mut file)) = opened else {
        return Err("cannot allocate a state temporary file".to_owned());
    };
    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| format!("cannot write state {}: {error}", path.display()))?;
        file.sync_all()
            .map_err(|error| format!("cannot sync state {}: {error}", path.display()))?;
        close(file, &temporary)?;
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("cannot replace state {}: {error}", path.display()))?;
        if let Some(parent) = path.parent()
            && let Ok(directory) = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
                .open(parent)
        {
            let _ = unsafe { libc::fsync(directory.as_raw_fd()) };
        }
        Ok(())
    })();
    if result.is_err() {
        remove_temporary(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn fixture_directory() -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "retrodeck-state-file-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        directory
    }

    #[test]
    fn reads_missing_empty_bounded_and_linked_state() {
        let directory = fixture_directory();
        let path = directory.join("volume.state");
        let link = directory.join("volume-link.state");
        assert_eq!(read(&path).unwrap(), StateRead::Missing);
        std::fs::write(&path, b"").unwrap();
        assert_eq!(read(&path).unwrap(), StateRead::Value(Vec::new()));
        std::fs::write(&path, b"42\n").unwrap();
        symlink(&path, &link).unwrap();
        assert_eq!(read(&link).unwrap(), StateRead::Value(b"42\n".to_vec()));
        std::fs::write(&path, vec![b'X'; MAXIMUM_BYTES + 1]).unwrap();
        assert!(read(&path).is_err());
        assert!(read(Path::new("relative.state")).is_err());
        assert!(read(&directory).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn writes_exact_private_atomic_state() {
        let directory = fixture_directory();
        let path = directory.join("volume.state");
        write(&path, b"42\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"42\n");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        write(&path, b"0\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"0\n");
        assert!(write(Path::new("relative.state"), b"42\n").is_err());
        assert!(write(&path, &[b'X'; MAXIMUM_BYTES + 1]).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn leaves_prior_state_when_temporary_names_are_exhausted() {
        let directory = fixture_directory();
        let path = directory.join("volume.state");
        std::fs::write(&path, b"37\n").unwrap();
        for attempt in 0..TEMPORARY_ATTEMPTS {
            std::fs::write(temporary_path(&path, attempt), b"occupied").unwrap();
        }
        assert!(write(&path, b"42\n").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"37\n");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
