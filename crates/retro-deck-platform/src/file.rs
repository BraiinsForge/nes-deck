//! Bounded file reads that reject symlinks and non-regular descriptors.

use std::collections::TryReserveError;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{
    AtFlags, Mode, OFlags, fchmod, fsync, ftruncate, open, openat, renameat, unlinkat,
};

const PRIVATE_MODE: Mode = Mode::from_raw_mode(0o600);
const TEMPORARY_ATTEMPTS: u64 = 8;
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Read one nonempty regular file without following its final symlink.
///
/// The size is validated both before and after reading. At most one byte past
/// `maximum_bytes` is accepted from the descriptor, so concurrent growth
/// cannot turn the operation into an unbounded allocation.
///
/// # Errors
///
/// Returns [`BoundedReadError`] when opening, metadata inspection, allocation,
/// reading, type validation, or either size bound fails.
pub fn read_regular_bounded(
    path: impl AsRef<Path>,
    maximum_bytes: usize,
) -> Result<Vec<u8>, BoundedReadError> {
    let path = path.as_ref();
    if maximum_bytes == 0 {
        return Err(BoundedReadError::InvalidLimit);
    }
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|source| BoundedReadError::Open {
        path: path.to_path_buf(),
        source: source.into(),
    })?;
    let file = File::from(descriptor);
    let metadata = file.metadata().map_err(BoundedReadError::Metadata)?;
    if !metadata.file_type().is_file() {
        return Err(BoundedReadError::NotRegular);
    }
    if metadata.len() == 0 {
        return Err(BoundedReadError::Empty);
    }
    let maximum_u64 = u64::try_from(maximum_bytes).map_err(|_| BoundedReadError::InvalidLimit)?;
    if metadata.len() > maximum_u64 {
        return Err(BoundedReadError::Oversized {
            size: metadata.len(),
            maximum: maximum_bytes,
        });
    }
    let initial = usize::try_from(metadata.len()).map_err(|_| BoundedReadError::Oversized {
        size: metadata.len(),
        maximum: maximum_bytes,
    })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(initial)
        .map_err(BoundedReadError::Allocate)?;
    let read_limit = u64::try_from(maximum_bytes)
        .ok()
        .and_then(|limit| limit.checked_add(1))
        .ok_or(BoundedReadError::InvalidLimit)?;
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(BoundedReadError::Read)?;
    if bytes.is_empty() {
        return Err(BoundedReadError::Empty);
    }
    if bytes.len() > maximum_bytes {
        return Err(BoundedReadError::Oversized {
            size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            maximum: maximum_bytes,
        });
    }
    Ok(bytes)
}

/// Durably replace one absolute private state file without following a final
/// destination symlink.
///
/// A mode-0600 sibling is fully written and synced before atomic rename. The
/// containing directory is then synced, and any failed temporary is removed.
///
/// # Errors
///
/// Returns [`AtomicWriteError`] for an unsafe path, oversized value,
/// temporary-name exhaustion, or filesystem failure.
pub fn write_private_atomic(
    path: impl AsRef<Path>,
    contents: &[u8],
    maximum_bytes: usize,
) -> Result<(), AtomicWriteError> {
    let path = path.as_ref();
    if !path.is_absolute() || maximum_bytes == 0 {
        return Err(AtomicWriteError::UnsafePath(path.to_path_buf()));
    }
    if contents.len() > maximum_bytes {
        return Err(AtomicWriteError::Oversized {
            size: contents.len(),
            maximum: maximum_bytes,
        });
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| AtomicWriteError::UnsafePath(path.to_path_buf()))?;
    let filename = path
        .file_name()
        .filter(|filename| !filename.is_empty())
        .ok_or_else(|| AtomicWriteError::UnsafePath(path.to_path_buf()))?;
    let directory = open(
        parent,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::DIRECTORY,
        Mode::empty(),
    )
    .map_err(|source| AtomicWriteError::Io {
        path: path.to_path_buf(),
        operation: "open parent directory",
        source: source.into(),
    })?;

    let serial = NEXT_TEMPORARY.fetch_add(TEMPORARY_ATTEMPTS, Ordering::Relaxed);
    let mut temporary = None;
    let mut last_error = None;
    for attempt in 0..TEMPORARY_ATTEMPTS {
        let name = format!(
            ".retro-deck.{}.{}.tmp",
            std::process::id(),
            serial.saturating_add(attempt)
        );
        match openat(
            &directory,
            name.as_str(),
            OFlags::WRONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::CREATE | OFlags::EXCL,
            PRIVATE_MODE,
        ) {
            Ok(descriptor) => {
                temporary = Some((name, File::from(descriptor)));
                break;
            }
            Err(source) => last_error = Some(source),
        }
    }
    let Some((temporary_name, mut file)) = temporary else {
        let source = last_error.map_or_else(
            || io::Error::other("temporary name attempts exhausted"),
            io::Error::from,
        );
        return Err(AtomicWriteError::Io {
            path: path.to_path_buf(),
            operation: "create temporary state",
            source,
        });
    };

    let result = (|| {
        fchmod(&file, PRIVATE_MODE).map_err(io::Error::from)?;
        file.write_all(contents)?;
        file.sync_all()?;
        renameat(&directory, temporary_name.as_str(), &directory, filename)
            .map_err(io::Error::from)?;
        fsync(&directory).map_err(io::Error::from)?;
        Ok(())
    })();
    if result.is_err() {
        let _ignored = unlinkat(&directory, temporary_name.as_str(), AtFlags::empty());
    }
    result.map_err(|source| AtomicWriteError::Io {
        path: path.to_path_buf(),
        operation: "replace private state",
        source,
    })
}

/// Read a bounded nonempty regular device attribute without trusting its
/// reported length or following its final symlink.
///
/// Sysfs attributes commonly report a synthetic page-sized length regardless
/// of their small textual value. This reader bounds the descriptor stream
/// itself instead of rejecting metadata length.
///
/// # Errors
///
/// Returns [`DeviceFileError`] for an unsafe path, invalid limit, non-regular
/// descriptor, allocation failure, read failure, empty value, or overflow.
pub fn read_device_bounded(
    path: impl AsRef<Path>,
    maximum_bytes: usize,
) -> Result<Vec<u8>, DeviceFileError> {
    let path = path.as_ref();
    validate_device_request(path, maximum_bytes, 0)?;
    let read_limit = maximum_bytes
        .checked_add(1)
        .ok_or_else(|| DeviceFileError::UnsafePath(path.to_path_buf()))?;
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|source| device_io_error(path, "open for bounded read", source.into()))?;
    let file = File::from(descriptor);
    require_regular_device(path, &file)?;

    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(read_limit)
        .map_err(DeviceFileError::Allocate)?;
    file.take(
        u64::try_from(read_limit).map_err(|_| DeviceFileError::UnsafePath(path.to_path_buf()))?,
    )
    .read_to_end(&mut bytes)
    .map_err(|source| device_io_error(path, "read bounded value", source))?;
    if bytes.is_empty() {
        return Err(DeviceFileError::Empty(path.to_path_buf()));
    }
    if bytes.len() > maximum_bytes {
        return Err(DeviceFileError::Oversized {
            path: path.to_path_buf(),
            maximum: maximum_bytes,
        });
    }
    Ok(bytes)
}

/// Write one complete bounded value to a regular device attribute without
/// following its final symlink.
///
/// Ordinary regular-file fixtures are truncated first. Virtual attributes may
/// reject truncation with `EINVAL` or `EPERM`; those responses are expected and
/// writing proceeds from offset zero. This function does not call `fsync`
/// because virtual device attributes are not durable storage.
///
/// # Errors
///
/// Returns [`DeviceFileError`] for an unsafe path, empty or oversized value,
/// non-regular descriptor, unexpected truncation failure, or write failure.
pub fn write_device_bounded(
    path: impl AsRef<Path>,
    contents: &[u8],
    maximum_bytes: usize,
) -> Result<(), DeviceFileError> {
    let path = path.as_ref();
    validate_device_request(path, maximum_bytes, contents.len())?;
    if contents.is_empty() {
        return Err(DeviceFileError::Empty(path.to_path_buf()));
    }
    let descriptor = open(
        path,
        OFlags::WRONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|source| device_io_error(path, "open for bounded write", source.into()))?;
    let mut file = File::from(descriptor);
    let metadata = file
        .metadata()
        .map_err(|source| device_io_error(path, "inspect opened device", source))?;
    if !metadata.file_type().is_file() {
        return Err(DeviceFileError::NotRegular(path.to_path_buf()));
    }
    if metadata.len() > 0 {
        match ftruncate(&file, 0) {
            Ok(()) | Err(rustix::io::Errno::INVAL | rustix::io::Errno::PERM) => {}
            Err(source) => {
                return Err(device_io_error(
                    path,
                    "truncate regular device value",
                    source.into(),
                ));
            }
        }
    }
    file.write_all(contents)
        .map_err(|source| device_io_error(path, "write bounded value", source))
}

fn validate_device_request(
    path: &Path,
    maximum_bytes: usize,
    content_bytes: usize,
) -> Result<(), DeviceFileError> {
    if !path.is_absolute() || maximum_bytes == 0 {
        return Err(DeviceFileError::UnsafePath(path.to_path_buf()));
    }
    if content_bytes > maximum_bytes {
        return Err(DeviceFileError::Oversized {
            path: path.to_path_buf(),
            maximum: maximum_bytes,
        });
    }
    Ok(())
}

fn require_regular_device(path: &Path, file: &File) -> Result<(), DeviceFileError> {
    let metadata = file
        .metadata()
        .map_err(|source| device_io_error(path, "inspect opened device", source))?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(DeviceFileError::NotRegular(path.to_path_buf()))
    }
}

fn device_io_error(path: &Path, operation: &'static str, source: io::Error) -> DeviceFileError {
    DeviceFileError::Io {
        path: path.to_path_buf(),
        operation,
        source,
    }
}

/// Failure while securely reading one bounded regular file.
#[derive(Debug)]
pub enum BoundedReadError {
    /// A zero or unrepresentable maximum was supplied.
    InvalidLimit,
    /// Opening without following a final symlink failed.
    Open {
        /// Requested path.
        path: PathBuf,
        /// Operating-system failure.
        source: io::Error,
    },
    /// Descriptor metadata could not be read.
    Metadata(io::Error),
    /// The opened descriptor is not a regular file.
    NotRegular,
    /// Empty files are not playable payloads.
    Empty,
    /// The file exceeded the configured maximum before or during the read.
    Oversized {
        /// Observed byte count.
        size: u64,
        /// Accepted byte count.
        maximum: usize,
    },
    /// Reserving the validated payload allocation failed.
    Allocate(TryReserveError),
    /// Reading from the validated descriptor failed.
    Read(io::Error),
}

impl fmt::Display for BoundedReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit => formatter.write_str("file size limit is invalid"),
            Self::Open { path, source } => {
                write!(formatter, "cannot open {}: {source}", path.display())
            }
            Self::Metadata(source) => write!(formatter, "cannot inspect opened file: {source}"),
            Self::NotRegular => formatter.write_str("opened object is not a regular file"),
            Self::Empty => formatter.write_str("file is empty"),
            Self::Oversized { size, maximum } => {
                write!(formatter, "file has {size} bytes; maximum is {maximum}")
            }
            Self::Allocate(source) => write!(formatter, "cannot allocate file payload: {source}"),
            Self::Read(source) => write!(formatter, "cannot read file: {source}"),
        }
    }
}

impl Error for BoundedReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Open { source, .. } | Self::Metadata(source) | Self::Read(source) => Some(source),
            Self::Allocate(source) => Some(source),
            Self::InvalidLimit | Self::NotRegular | Self::Empty | Self::Oversized { .. } => None,
        }
    }
}

/// Failure while durably replacing one private state file.
#[derive(Debug)]
pub enum AtomicWriteError {
    /// The destination is relative, lacks a filename, or has no parent.
    UnsafePath(PathBuf),
    /// The caller-provided value exceeds its explicit cap.
    Oversized {
        /// Requested byte count.
        size: usize,
        /// Accepted byte count.
        maximum: usize,
    },
    /// One named filesystem operation failed.
    Io {
        /// Requested final destination.
        path: PathBuf,
        /// Failed operation.
        operation: &'static str,
        /// Operating-system failure.
        source: io::Error,
    },
}

impl fmt::Display for AtomicWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafePath(path) => write!(
                formatter,
                "{} is not a safe absolute state path",
                path.display()
            ),
            Self::Oversized { size, maximum } => {
                write!(formatter, "state has {size} bytes; maximum is {maximum}")
            }
            Self::Io {
                path,
                operation,
                source,
            } => write!(formatter, "cannot {operation} {}: {source}", path.display()),
        }
    }
}

impl Error for AtomicWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::UnsafePath(_) | Self::Oversized { .. } => None,
        }
    }
}

/// Failure while accessing one small virtual device attribute.
#[derive(Debug)]
pub enum DeviceFileError {
    /// The path is relative or its byte bound is zero or unrepresentable.
    UnsafePath(PathBuf),
    /// The opened descriptor is not a regular or virtual regular file.
    NotRegular(PathBuf),
    /// A read returned no bytes or a write supplied no value.
    Empty(PathBuf),
    /// The observed or supplied value exceeded its explicit cap.
    Oversized {
        /// Device path.
        path: PathBuf,
        /// Accepted byte count.
        maximum: usize,
    },
    /// A bounded read buffer could not be reserved.
    Allocate(TryReserveError),
    /// One named operating-system operation failed.
    Io {
        /// Device path.
        path: PathBuf,
        /// Failed operation.
        operation: &'static str,
        /// Operating-system failure.
        source: io::Error,
    },
}

impl fmt::Display for DeviceFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafePath(path) => write!(
                formatter,
                "{} is not a safe bounded device path",
                path.display()
            ),
            Self::NotRegular(path) => write!(
                formatter,
                "{} is not a regular device attribute",
                path.display()
            ),
            Self::Empty(path) => write!(formatter, "{} has an empty device value", path.display()),
            Self::Oversized { path, maximum } => write!(
                formatter,
                "{} exceeds the {maximum}-byte device value bound",
                path.display()
            ),
            Self::Allocate(source) => write!(formatter, "cannot allocate device value: {source}"),
            Self::Io {
                path,
                operation,
                source,
            } => write!(formatter, "cannot {operation} {}: {source}", path.display()),
        }
    }
}

impl Error for DeviceFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Allocate(source) => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::UnsafePath(_) | Self::NotRegular(_) | Self::Empty(_) | Self::Oversized { .. } => {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "retro-deck-bounded-read-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("bounded-read fixture is created");
            Self { root }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn reads_one_regular_payload_at_the_exact_limit() {
        let fixture = Fixture::new();
        let path = fixture.root.join("track.nsf");
        fs::write(&path, b"NESM").expect("bounded-read payload is written");
        assert_eq!(
            read_regular_bounded(&path, 4).expect("bounded-read payload succeeds"),
            b"NESM"
        );
    }

    #[test]
    fn rejects_empty_oversized_and_zero_limit_reads() {
        let fixture = Fixture::new();
        let empty = fixture.root.join("empty");
        let large = fixture.root.join("large");
        fs::write(&empty, []).expect("empty bounded-read payload is written");
        fs::write(&large, b"12345").expect("large bounded-read payload is written");
        assert!(matches!(
            read_regular_bounded(&empty, 4),
            Err(BoundedReadError::Empty)
        ));
        assert!(matches!(
            read_regular_bounded(&large, 4),
            Err(BoundedReadError::Oversized {
                size: 5,
                maximum: 4
            })
        ));
        assert!(matches!(
            read_regular_bounded(&large, 0),
            Err(BoundedReadError::InvalidLimit)
        ));
    }

    #[test]
    fn rejects_a_final_symlink_and_a_directory() {
        let fixture = Fixture::new();
        let path = fixture.root.join("track.ogg");
        let link = fixture.root.join("linked.ogg");
        fs::write(&path, b"OggS").expect("bounded-read payload is written");
        symlink(&path, &link).expect("bounded-read symlink is created");
        assert!(matches!(
            read_regular_bounded(&link, 4),
            Err(BoundedReadError::Open { .. })
        ));
        assert!(matches!(
            read_regular_bounded(&fixture.root, 4),
            Err(BoundedReadError::NotRegular)
        ));
    }

    #[test]
    fn private_state_replacement_is_atomic_and_mode_restricted() {
        let fixture = Fixture::new();
        let path = fixture.root.join("volume.state");
        write_private_atomic(&path, b"42\n", 8).expect("initial private state is written");
        write_private_atomic(&path, b"55\n", 8).expect("private state is replaced");
        assert_eq!(fs::read(&path).expect("private state is readable"), b"55\n");
        let mode = fs::metadata(&path)
            .expect("private state metadata is readable")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn replacement_overwrites_a_symlink_without_touching_its_target() {
        let fixture = Fixture::new();
        let target = fixture.root.join("target");
        let path = fixture.root.join("volume.state");
        fs::write(&target, b"target").expect("atomic-write target is created");
        symlink(&target, &path).expect("atomic-write symlink is created");
        write_private_atomic(&path, b"42\n", 8).expect("symlink is atomically replaced");
        assert_eq!(
            fs::read(&target).expect("target remains readable"),
            b"target"
        );
        assert_eq!(
            fs::read(&path).expect("state replacement is readable"),
            b"42\n"
        );
        assert!(
            !fs::symlink_metadata(&path)
                .expect("replacement metadata is readable")
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn atomic_state_write_requires_an_absolute_bounded_path() {
        assert!(matches!(
            write_private_atomic("relative.state", b"42\n", 8),
            Err(AtomicWriteError::UnsafePath(_))
        ));
        assert!(matches!(
            write_private_atomic("/tmp/oversized.state", b"12345", 4),
            Err(AtomicWriteError::Oversized {
                size: 5,
                maximum: 4
            })
        ));
    }

    #[test]
    fn bounded_device_values_replace_regular_fixtures_exactly() {
        let fixture = Fixture::new();
        let path = fixture.root.join("brightness");
        fs::write(&path, b"100\n").expect("device fixture is written");
        assert_eq!(
            read_device_bounded(&path, 4).expect("device fixture is read"),
            b"100\n"
        );

        write_device_bounded(&path, b"7\n", 4).expect("device fixture is replaced");
        assert_eq!(
            fs::read(&path).expect("device fixture remains readable"),
            b"7\n"
        );
        assert!(matches!(
            read_device_bounded(&path, 1),
            Err(DeviceFileError::Oversized { maximum: 1, .. })
        ));
    }

    #[test]
    fn bounded_device_values_reject_final_symlinks() {
        let fixture = Fixture::new();
        let target = fixture.root.join("target");
        let alias = fixture.root.join("alias");
        fs::write(&target, b"42\n").expect("device target is written");
        symlink(&target, &alias).expect("device alias is created");

        assert!(read_device_bounded(&alias, 4).is_err());
        assert!(write_device_bounded(&alias, b"0\n", 4).is_err());
        assert_eq!(fs::read(&target).expect("target remains readable"), b"42\n");
    }
}
