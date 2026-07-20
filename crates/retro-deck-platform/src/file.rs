//! Bounded file reads that reject symlinks and non-regular descriptors.

use std::collections::TryReserveError;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{AtFlags, Mode, OFlags, fchmod, fsync, open, openat, renameat, unlinkat};

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
}
