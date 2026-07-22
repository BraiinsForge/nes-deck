use rustix::fs::{FileType, Mode, OFlags, fstat, open};
use rustix::io::Errno;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub fn read_regular(
    path: &Path,
    minimum_bytes: u64,
    maximum_bytes: u64,
    label: &str,
) -> Result<Option<Vec<u8>>, String> {
    let descriptor = match open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    ) {
        Ok(descriptor) => descriptor,
        Err(Errno::NOENT) => return Ok(None),
        Err(error) => return Err(format!("cannot open {label} {}: {error}", path.display())),
    };
    let metadata = fstat(&descriptor)
        .map_err(|error| format!("cannot inspect {label} {}: {error}", path.display()))?;
    let size = u64::try_from(metadata.st_size).unwrap_or(u64::MAX);
    if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile
        || size < minimum_bytes
        || size > maximum_bytes
    {
        return Err(format!(
            "{label} must be a regular file between {minimum_bytes} and {maximum_bytes} bytes: {}",
            path.display()
        ));
    }
    let length = usize::try_from(size)
        .map_err(|_| format!("{label} is too large to read: {}", path.display()))?;
    let mut data = Vec::new();
    data.try_reserve_exact(length)
        .map_err(|_| format!("cannot allocate {label} {}", path.display()))?;
    data.resize(length, 0);
    File::from(descriptor)
        .read_exact(&mut data)
        .map_err(|error| format!("cannot read {label} {}: {error}", path.display()))?;
    Ok(Some(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn reads_only_bounded_regular_files_without_following_links() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "retrodeck-regular-file-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("fixture.tsv");
        let link = directory.join("fixture-link.tsv");
        std::fs::write(&path, b"alpha\tbeta\n").unwrap();
        symlink(&path, &link).unwrap();

        assert_eq!(
            read_regular(&path, 1, 32, "fixture").unwrap().unwrap(),
            b"alpha\tbeta\n"
        );
        assert!(read_regular(&path, 32, 64, "fixture").is_err());
        assert!(read_regular(&path, 1, 4, "fixture").is_err());
        assert!(read_regular(&link, 1, 32, "fixture").is_err());
        assert!(
            read_regular(&directory.join("missing"), 1, 32, "fixture")
                .unwrap()
                .is_none()
        );

        std::fs::remove_dir_all(directory).unwrap();
    }
}
