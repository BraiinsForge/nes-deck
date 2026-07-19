//! No-follow bounded reads and durable same-directory replacement.

use std::{
    fmt,
    fs::{DirBuilder, File, Metadata},
    io::{self, Read as _, Write as _},
    os::unix::fs::DirBuilderExt as _,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rustix::fs::{AtFlags, Mode, OFlags, fchmod, fsync, open, openat, renameat, unlinkat};

pub(crate) struct BoundedFile {
    pub(crate) contents: Vec<u8>,
    pub(crate) metadata: Metadata,
}

#[derive(Debug)]
pub(crate) enum FileError {
    Io(io::Error),
    Unsafe(&'static str),
    Random(String),
}

impl fmt::Display for FileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Unsafe(reason) => formatter.write_str(reason),
            Self::Random(error) => write!(formatter, "cannot name temporary file: {error}"),
        }
    }
}

impl std::error::Error for FileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Unsafe(_) | Self::Random(_) => None,
        }
    }
}

impl From<io::Error> for FileError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) fn read_bounded_regular(
    path: &Path,
    maximum_bytes: u64,
) -> Result<BoundedFile, FileError> {
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    let mut file = File::from(descriptor);
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(FileError::Unsafe("not a regular file"));
    }
    if metadata.len() > maximum_bytes {
        return Err(FileError::Unsafe("file exceeds its size limit"));
    }
    let initial_capacity = usize::try_from(metadata.len())
        .map_err(|_| FileError::Unsafe("file size cannot be represented"))?;
    let maximum_read = maximum_bytes.saturating_add(1);
    let mut contents = Vec::with_capacity(initial_capacity);
    io::Read::by_ref(&mut file)
        .take(maximum_read)
        .read_to_end(&mut contents)?;
    if u64::try_from(contents.len()).unwrap_or(u64::MAX) > maximum_bytes {
        return Err(FileError::Unsafe("file exceeds its size limit"));
    }
    Ok(BoundedFile { contents, metadata })
}

pub(crate) fn atomic_write(
    path: &Path,
    contents: &[u8],
    file_mode: u32,
    directory_mode: u32,
) -> Result<(), FileError> {
    let parent = usable_parent(path);
    let filename = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(FileError::Unsafe("destination has no filename"))?;

    let mut builder = DirBuilder::new();
    builder.recursive(true).mode(directory_mode);
    builder.create(&parent)?;
    let directory = open(
        &parent,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::DIRECTORY,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;

    let temporary_name = temporary_name()?;
    let descriptor = openat(
        &directory,
        temporary_name.as_str(),
        OFlags::WRONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::CREATE | OFlags::EXCL,
        Mode::from_raw_mode(file_mode),
    )
    .map_err(io::Error::from)?;
    let mut temporary = File::from(descriptor);

    let result = (|| {
        fchmod(&temporary, Mode::from_raw_mode(file_mode)).map_err(io::Error::from)?;
        temporary.write_all(contents)?;
        temporary.sync_all()?;
        renameat(&directory, temporary_name.as_str(), &directory, filename)
            .map_err(io::Error::from)?;
        fsync(&directory).map_err(io::Error::from)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = unlinkat(&directory, temporary_name.as_str(), AtFlags::empty());
    }
    result.map_err(FileError::Io)
}

fn usable_parent(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn temporary_name() -> Result<String, FileError> {
    let mut random = [0_u8; 18];
    getrandom::getrandom(&mut random).map_err(|error| FileError::Random(error.to_string()))?;
    Ok(format!(".retro-deck-{}", URL_SAFE_NO_PAD.encode(random)))
}

#[cfg(test)]
mod tests {
    use super::{FileError, atomic_write, read_bounded_regular};
    use std::{
        fs,
        os::unix::fs::{MetadataExt as _, symlink},
    };

    #[test]
    fn bounded_reads_reject_non_files_symlinks_and_excess() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let file = directory.path().join("value");
        assert!(fs::write(&file, b"bounded").is_ok());
        assert!(matches!(
            read_bounded_regular(&file, 7),
            Ok(value) if value.contents == b"bounded"
        ));
        assert!(matches!(
            read_bounded_regular(&file, 6),
            Err(FileError::Unsafe(_))
        ));
        assert!(matches!(
            read_bounded_regular(directory.path(), 64),
            Err(FileError::Unsafe(_))
        ));
        let link = directory.path().join("link");
        assert!(symlink(&file, &link).is_ok());
        assert!(read_bounded_regular(&link, 64).is_err());
    }

    #[test]
    fn atomic_replacement_is_private_durable_and_symlink_safe() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let destination = directory.path().join("nested/config/value");
        assert!(atomic_write(&destination, b"first", 0o600, 0o700).is_ok());
        let metadata = fs::metadata(&destination);
        assert!(matches!(metadata, Ok(metadata) if metadata.mode() & 0o777 == 0o600));
        assert!(matches!(fs::read(&destination), Ok(contents) if contents == b"first"));

        assert!(atomic_write(&destination, b"second", 0o640, 0o700).is_ok());
        let metadata = fs::metadata(&destination);
        assert!(matches!(metadata, Ok(metadata) if metadata.mode() & 0o777 == 0o640));
        assert!(matches!(fs::read(&destination), Ok(contents) if contents == b"second"));

        let victim = directory.path().join("victim");
        assert!(fs::write(&victim, b"untouched").is_ok());
        assert!(fs::remove_file(&destination).is_ok());
        assert!(symlink(&victim, &destination).is_ok());
        assert!(atomic_write(&destination, b"replacement", 0o600, 0o700).is_ok());
        assert!(matches!(fs::read(&victim), Ok(contents) if contents == b"untouched"));
        assert!(matches!(fs::read(&destination), Ok(contents) if contents == b"replacement"));
    }

    #[test]
    fn atomic_replacement_rejects_a_symlink_parent() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let actual = directory.path().join("actual");
        assert!(fs::create_dir(&actual).is_ok());
        let alias = directory.path().join("alias");
        assert!(symlink(&actual, &alias).is_ok());
        assert!(atomic_write(&alias.join("value"), b"blocked", 0o600, 0o700).is_err());
        assert!(!actual.join("value").exists());
    }
}
