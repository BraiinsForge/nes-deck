//! Bounded file reads that reject symlinks and non-regular descriptors.

use std::collections::TryReserveError;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use rustix::fs::{Mode, OFlags, open};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
}
