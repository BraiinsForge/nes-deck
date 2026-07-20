//! Native bounded persistence for libretro memory regions.

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

use rustix::fs::{AtFlags, Mode, OFlags, fchmod, fsync, open, openat, renameat, unlinkat};
use rustix::io::Errno;

use super::{Content, LibretroCore, MemoryFile, MemoryKind};

/// Largest accepted persistent-memory region reported by a core.
pub const MAXIMUM_SAVE_BYTES: usize = 1_024 * 1_024;

const SAVE_MODE: Mode = Mode::from_raw_mode(0o600);
const TEMPORARY_RANDOM_BYTES: usize = 16;

/// Save paths derived from one loaded content image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveStore {
    core: LibretroCore,
    directory: PathBuf,
    stem: OsString,
}

impl SaveStore {
    /// Derive a same-directory save store from loaded content.
    ///
    /// # Errors
    ///
    /// Returns [`SaveError`] if the content path has no usable filename stem.
    pub fn for_content(content: &Content) -> Result<Self, SaveError> {
        let path = content.path();
        let Some(stem) = path
            .file_stem()
            .filter(|value| !value.is_empty())
            .map(OsString::from)
        else {
            return Err(SaveError::new(path, SaveFailure::InvalidContentPath));
        };
        let directory = path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_owned();
        Ok(Self {
            core: content.core(),
            directory,
            stem,
        })
    }

    /// Filesystem path used by one declared core memory region.
    ///
    /// # Errors
    ///
    /// Returns [`SaveError`] if `memory` does not belong to this core.
    pub fn path(&self, memory: MemoryFile) -> Result<PathBuf, SaveError> {
        self.validate_memory(memory)?;
        let mut filename = self.stem.clone();
        filename.push(memory.extension());
        Ok(self.directory.join(filename))
    }

    /// Load one exact native memory file without partially changing `destination`.
    ///
    /// # Errors
    ///
    /// Returns [`SaveError`] for an undeclared or oversized memory region, an
    /// unsafe file type, an unexpected file size, or an I/O failure.
    pub fn load(
        &self,
        memory: MemoryFile,
        destination: &mut [u8],
    ) -> Result<LoadOutcome, SaveError> {
        let path = self.path(memory)?;
        if destination.is_empty() {
            return Ok(LoadOutcome::SkippedEmpty);
        }
        validate_size(&path, destination.len())?;
        let descriptor = match open(
            &path,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(Errno::NOENT) => return Ok(LoadOutcome::Missing),
            Err(source) => {
                return Err(SaveError::new(
                    &path,
                    SaveFailure::Open {
                        source: io::Error::from(source),
                    },
                ));
            }
        };
        let mut file = File::from(descriptor);
        let metadata = file
            .metadata()
            .map_err(|source| SaveError::new(&path, SaveFailure::Read { source }))?;
        if !metadata.is_file() {
            return Err(SaveError::new(&path, SaveFailure::NotRegular));
        }
        let actual = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if actual != destination.len() {
            return Err(SaveError::new(
                &path,
                SaveFailure::UnexpectedSize {
                    expected: destination.len(),
                    actual,
                },
            ));
        }

        let maximum_read = u64::try_from(destination.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut bytes = Vec::with_capacity(destination.len());
        io::Read::by_ref(&mut file)
            .take(maximum_read)
            .read_to_end(&mut bytes)
            .map_err(|source| SaveError::new(&path, SaveFailure::Read { source }))?;
        if bytes.len() != destination.len() {
            return Err(SaveError::new(
                &path,
                SaveFailure::Changed {
                    expected: destination.len(),
                    actual: bytes.len(),
                },
            ));
        }
        destination.copy_from_slice(&bytes);
        Ok(LoadOutcome::Loaded)
    }

    /// Durably replace one exact native memory file beside the content image.
    ///
    /// # Errors
    ///
    /// Returns [`SaveError`] for an undeclared or oversized memory region, an
    /// unsafe parent directory, random-name failure, or an I/O failure.
    pub fn save(&self, memory: MemoryFile, source: &[u8]) -> Result<SaveOutcome, SaveError> {
        let path = self.path(memory)?;
        if source.is_empty() {
            return Ok(SaveOutcome::SkippedEmpty);
        }
        validate_size(&path, source.len())?;

        let filename = path
            .file_name()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| SaveError::new(&path, SaveFailure::InvalidContentPath))?;
        let directory = open(
            &self.directory,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::DIRECTORY,
            Mode::empty(),
        )
        .map_err(|source| {
            SaveError::new(
                &path,
                SaveFailure::Open {
                    source: io::Error::from(source),
                },
            )
        })?;
        let temporary_name = temporary_name(&path)?;
        let descriptor = openat(
            &directory,
            temporary_name.as_str(),
            OFlags::WRONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::CREATE | OFlags::EXCL,
            SAVE_MODE,
        )
        .map_err(|source| {
            SaveError::new(
                &path,
                SaveFailure::Write {
                    source: io::Error::from(source),
                },
            )
        })?;
        let mut temporary = File::from(descriptor);

        let result = (|| {
            fchmod(&temporary, SAVE_MODE).map_err(io::Error::from)?;
            temporary.write_all(source)?;
            temporary.sync_all()?;
            renameat(&directory, temporary_name.as_str(), &directory, filename)
                .map_err(io::Error::from)?;
            fsync(&directory).map_err(io::Error::from)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = unlinkat(&directory, temporary_name.as_str(), AtFlags::empty());
        }
        result.map_err(|source| SaveError::new(&path, SaveFailure::Write { source }))?;
        Ok(SaveOutcome::Written)
    }

    fn validate_memory(&self, memory: MemoryFile) -> Result<(), SaveError> {
        if self.core.memory_files().contains(&memory) {
            Ok(())
        } else {
            Err(SaveError::new(
                &self.directory,
                SaveFailure::UnsupportedMemory {
                    core: self.core,
                    kind: memory.kind(),
                },
            ))
        }
    }
}

/// Result of trying to load one core memory region.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadOutcome {
    /// The core exposes no bytes for this region.
    SkippedEmpty,
    /// No native save file exists yet.
    Missing,
    /// An exact native save file was copied into core memory.
    Loaded,
}

/// Result of trying to persist one core memory region.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveOutcome {
    /// The core exposes no bytes for this region.
    SkippedEmpty,
    /// A private file was atomically and durably replaced.
    Written,
}

/// Native save persistence failure with its source path retained.
#[derive(Debug)]
pub struct SaveError {
    path: PathBuf,
    failure: SaveFailure,
}

impl SaveError {
    fn new(path: &Path, failure: SaveFailure) -> Self {
        Self {
            path: path.to_owned(),
            failure,
        }
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.failure {
            SaveFailure::InvalidContentPath => {
                write!(
                    formatter,
                    "content path has no usable save stem: {}",
                    self.path.display()
                )
            }
            SaveFailure::UnsupportedMemory { core, kind } => write!(
                formatter,
                "{} does not expose {kind:?} persistence",
                core.core_name()
            ),
            SaveFailure::TooLarge { bytes } => write!(
                formatter,
                "save {} contains {bytes} bytes; maximum is {MAXIMUM_SAVE_BYTES}",
                self.path.display()
            ),
            SaveFailure::Open { source } => {
                write!(
                    formatter,
                    "cannot open save {}: {source}",
                    self.path.display()
                )
            }
            SaveFailure::Read { source } => {
                write!(
                    formatter,
                    "cannot read save {}: {source}",
                    self.path.display()
                )
            }
            SaveFailure::Write { source } => {
                write!(
                    formatter,
                    "cannot write save {}: {source}",
                    self.path.display()
                )
            }
            SaveFailure::NotRegular => {
                write!(
                    formatter,
                    "save {} is not a regular file",
                    self.path.display()
                )
            }
            SaveFailure::UnexpectedSize { expected, actual } => write!(
                formatter,
                "save {} contains {actual} bytes; core requires exactly {expected}",
                self.path.display()
            ),
            SaveFailure::Changed { expected, actual } => write!(
                formatter,
                "save {} changed while reading: expected {expected} bytes, read {actual}",
                self.path.display()
            ),
            SaveFailure::Random(source) => write!(
                formatter,
                "cannot create a private temporary name for {}: {source}",
                self.path.display()
            ),
        }
    }
}

impl Error for SaveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.failure {
            SaveFailure::Open { source }
            | SaveFailure::Read { source }
            | SaveFailure::Write { source } => Some(source),
            SaveFailure::InvalidContentPath
            | SaveFailure::UnsupportedMemory { .. }
            | SaveFailure::TooLarge { .. }
            | SaveFailure::NotRegular
            | SaveFailure::UnexpectedSize { .. }
            | SaveFailure::Changed { .. }
            | SaveFailure::Random(_) => None,
        }
    }
}

#[derive(Debug)]
enum SaveFailure {
    InvalidContentPath,
    UnsupportedMemory {
        core: LibretroCore,
        kind: MemoryKind,
    },
    TooLarge {
        bytes: usize,
    },
    Open {
        source: io::Error,
    },
    Read {
        source: io::Error,
    },
    Write {
        source: io::Error,
    },
    NotRegular,
    UnexpectedSize {
        expected: usize,
        actual: usize,
    },
    Changed {
        expected: usize,
        actual: usize,
    },
    Random(String),
}

fn validate_size(path: &Path, bytes: usize) -> Result<(), SaveError> {
    if bytes > MAXIMUM_SAVE_BYTES {
        Err(SaveError::new(path, SaveFailure::TooLarge { bytes }))
    } else {
        Ok(())
    }
}

fn temporary_name(path: &Path) -> Result<String, SaveError> {
    let mut random = [0_u8; TEMPORARY_RANDOM_BYTES];
    getrandom::getrandom(&mut random)
        .map_err(|source| SaveError::new(path, SaveFailure::Random(source.to_string())))?;
    let mut name = String::from(".retro-deck-save-");
    for byte in random {
        write!(&mut name, "{byte:02x}")
            .map_err(|source| SaveError::new(path, SaveFailure::Random(source.to_string())))?;
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{MetadataExt as _, symlink};

    fn fixture(core: LibretroCore, extension: &str, size: usize) -> (tempfile::TempDir, Content) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(format!("game.{extension}"));
        assert!(fs::write(&path, vec![0_u8; size]).is_ok());
        let content = Content::load(core, &path).expect("valid content fixture");
        (directory, content)
    }

    fn memory(core: LibretroCore, kind: MemoryKind) -> MemoryFile {
        core.memory_files()
            .iter()
            .copied()
            .find(|memory| memory.kind() == kind)
            .expect("core exposes requested test memory")
    }

    #[test]
    fn paths_use_native_extensions_beside_content() {
        let (directory, content) = fixture(LibretroCore::Gambatte, "gbc", 0x150);
        let store = SaveStore::for_content(&content).expect("usable content path");
        assert!(matches!(
            store.path(memory(LibretroCore::Gambatte, MemoryKind::SaveRam)),
            Ok(path) if path == directory.path().join("game.sav")
        ));
        assert!(matches!(
            store.path(memory(LibretroCore::Gambatte, MemoryKind::Rtc)),
            Ok(path) if path == directory.path().join("game.rtc")
        ));
        assert!(
            store
                .path(memory(LibretroCore::Fceumm, MemoryKind::SaveRam))
                .is_err()
        );
    }

    #[test]
    fn missing_empty_and_exact_native_saves_are_distinct() {
        let (_directory, content) = fixture(LibretroCore::Fceumm, "nes", 16);
        let store = SaveStore::for_content(&content).expect("usable content path");
        let memory = memory(LibretroCore::Fceumm, MemoryKind::SaveRam);
        assert!(matches!(
            store.load(memory, &mut []),
            Ok(LoadOutcome::SkippedEmpty)
        ));
        let mut destination = [0x55_u8; 8];
        assert!(matches!(
            store.load(memory, &mut destination),
            Ok(LoadOutcome::Missing)
        ));
        assert_eq!(destination, [0x55; 8]);
        assert!(matches!(
            store.save(memory, &[]),
            Ok(SaveOutcome::SkippedEmpty)
        ));
        assert!(matches!(
            store.save(memory, b"native!!"),
            Ok(SaveOutcome::Written)
        ));
        assert!(matches!(
            store.load(memory, &mut destination),
            Ok(LoadOutcome::Loaded)
        ));
        assert_eq!(destination, *b"native!!");
    }

    #[test]
    fn replacement_is_private_durable_and_does_not_follow_a_symlink() {
        let (directory, content) = fixture(LibretroCore::Fceumm, "nes", 16);
        let store = SaveStore::for_content(&content).expect("usable content path");
        let memory = memory(LibretroCore::Fceumm, MemoryKind::SaveRam);
        let path = store.path(memory).expect("declared memory path");
        assert!(matches!(
            store.save(memory, b"first"),
            Ok(SaveOutcome::Written)
        ));
        let metadata = fs::metadata(&path).expect("written save metadata");
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert!(matches!(
            store.save(memory, b"second"),
            Ok(SaveOutcome::Written)
        ));
        assert!(matches!(fs::read(&path), Ok(bytes) if bytes == b"second"));

        let victim = directory.path().join("victim");
        assert!(fs::write(&victim, b"untouched").is_ok());
        assert!(fs::remove_file(&path).is_ok());
        assert!(symlink(&victim, &path).is_ok());
        assert!(matches!(
            store.save(memory, b"replace"),
            Ok(SaveOutcome::Written)
        ));
        assert!(matches!(fs::read(&victim), Ok(bytes) if bytes == b"untouched"));
        assert!(matches!(fs::read(&path), Ok(bytes) if bytes == b"replace"));
    }

    #[test]
    fn malformed_files_never_partially_change_core_memory() {
        let (directory, content) = fixture(LibretroCore::Fceumm, "nes", 16);
        let store = SaveStore::for_content(&content).expect("usable content path");
        let memory = memory(LibretroCore::Fceumm, MemoryKind::SaveRam);
        let path = store.path(memory).expect("declared memory path");
        assert!(fs::write(&path, b"short").is_ok());
        let mut destination = [0x77_u8; 8];
        assert!(store.load(memory, &mut destination).is_err());
        assert_eq!(destination, [0x77; 8]);

        assert!(fs::remove_file(&path).is_ok());
        let victim = directory.path().join("victim");
        assert!(fs::write(&victim, [0_u8; 8]).is_ok());
        assert!(symlink(&victim, &path).is_ok());
        assert!(store.load(memory, &mut destination).is_err());
        assert_eq!(destination, [0x77; 8]);
    }

    #[test]
    fn failed_replacement_removes_its_private_temporary_file() {
        let (directory, content) = fixture(LibretroCore::Fceumm, "nes", 16);
        let store = SaveStore::for_content(&content).expect("usable content path");
        let memory = memory(LibretroCore::Fceumm, MemoryKind::SaveRam);
        let path = store.path(memory).expect("declared memory path");
        assert!(fs::create_dir(&path).is_ok());
        assert!(store.save(memory, b"cannot replace a directory").is_err());
        let entries = fs::read_dir(directory.path()).expect("read fixture directory");
        for entry in entries {
            let entry = entry.expect("read fixture entry");
            assert!(
                !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".retro-deck-save-")
            );
        }
    }

    #[test]
    fn save_sizes_and_parent_symlinks_are_bounded() {
        let (directory, content) = fixture(LibretroCore::Fceumm, "nes", 16);
        let store = SaveStore::for_content(&content).expect("usable content path");
        let memory = memory(LibretroCore::Fceumm, MemoryKind::SaveRam);
        assert!(
            store
                .save(memory, &vec![0_u8; MAXIMUM_SAVE_BYTES + 1])
                .is_err()
        );
        let mut oversized = vec![0_u8; MAXIMUM_SAVE_BYTES + 1];
        assert!(store.load(memory, &mut oversized).is_err());

        let actual = directory.path().join("actual");
        let alias = directory.path().join("alias");
        assert!(fs::create_dir(&actual).is_ok());
        assert!(symlink(&actual, &alias).is_ok());
        let aliased_content_path = alias.join("game.nes");
        assert!(fs::write(actual.join("game.nes"), [0_u8; 16]).is_ok());
        let aliased_content = Content::load(LibretroCore::Fceumm, &aliased_content_path)
            .expect("content read may traverse its parent");
        let aliased_store = SaveStore::for_content(&aliased_content).expect("usable aliased path");
        assert!(aliased_store.save(memory, b"blocked").is_err());
        assert!(!actual.join("game.srm").exists());
    }
}
