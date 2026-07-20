//! Durable storage for the shared dashboard palette schema.

use std::{
    fmt, io,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

pub use retro_deck_config::{Palette, PaletteError, PaletteField, PaletteRole, Rgb};

use crate::file::{FileError, atomic_write, read_bounded_regular};

const MAXIMUM_PALETTE_FILE_BYTES: u64 = 4_096;
const PALETTE_FILE_MODE: u32 = 0o600;
const PALETTE_DIRECTORY_MODE: u32 = 0o700;

/// No-follow palette file access or persistence failure.
#[derive(Debug)]
pub enum PaletteStorageError {
    /// A path or file type violated the storage contract.
    UnsafeFile(&'static str),
    /// File access or durable replacement failed.
    Io(io::Error),
    /// Operating-system entropy failed while naming a temporary file.
    Random(String),
}

impl fmt::Display for PaletteStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeFile(reason) => write!(formatter, "unsafe palette file: {reason}"),
            Self::Io(error) => write!(formatter, "palette file I/O failed: {error}"),
            Self::Random(error) => write!(formatter, "cannot name palette file: {error}"),
        }
    }
}

impl std::error::Error for PaletteStorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::UnsafeFile(_) | Self::Random(_) => None,
        }
    }
}

impl From<FileError> for PaletteStorageError {
    fn from(error: FileError) -> Self {
        match error {
            FileError::Io(error) => Self::Io(error),
            FileError::Unsafe(reason) => Self::UnsafeFile(reason),
            FileError::Random(error) => Self::Random(error),
        }
    }
}

/// Failure while opening and decoding one palette source.
#[derive(Debug)]
pub enum PaletteLoadError {
    /// The file could not be opened through the no-follow boundary.
    Storage(PaletteStorageError),
    /// The file contents violate the complete palette schema.
    Format(PaletteError),
}

impl fmt::Display for PaletteLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => error.fmt(formatter),
            Self::Format(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PaletteLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Format(error) => Some(error),
        }
    }
}

/// Palette store configuration, source, locking, or persistence failure.
#[derive(Debug)]
pub enum PaletteStoreError {
    /// Installed palette paths are relative, traversing, or overlap.
    InvalidConfiguration,
    /// Neither checked-in nor generated palette could be loaded.
    BaseUnavailable {
        /// Why the checked-in fallback failed.
        fallback: Box<PaletteLoadError>,
        /// Why the generated active palette failed.
        active: Box<PaletteLoadError>,
    },
    /// Another thread panicked while holding the palette store lock.
    LockPoisoned,
    /// The persistent override could not be replaced durably.
    Save(PaletteStorageError),
}

impl fmt::Display for PaletteStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => formatter.write_str("invalid palette store paths"),
            Self::BaseUnavailable { fallback, active } => write!(
                formatter,
                "no installed dashboard palette is usable; fallback: {fallback}; active: {active}"
            ),
            Self::LockPoisoned => formatter.write_str("palette store lock was poisoned"),
            Self::Save(error) => write!(formatter, "cannot save dashboard palette: {error}"),
        }
    }
}

impl std::error::Error for PaletteStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BaseUnavailable { fallback, .. } => Some(fallback),
            Self::Save(error) => Some(error),
            Self::InvalidConfiguration | Self::LockPoisoned => None,
        }
    }
}

/// Serialized access to installed palettes and the persistent override.
pub struct PaletteStore {
    lock: Mutex<()>,
    active_path: PathBuf,
    fallback_path: PathBuf,
    override_path: PathBuf,
}

impl PaletteStore {
    /// Configure palette sources without touching the filesystem.
    ///
    /// # Errors
    ///
    /// Returns [`PaletteStoreError::InvalidConfiguration`] unless every path is
    /// absolute and traversal-free and all three paths differ.
    pub fn new(
        active_path: impl Into<PathBuf>,
        fallback_path: impl Into<PathBuf>,
        override_path: impl Into<PathBuf>,
    ) -> Result<Self, PaletteStoreError> {
        let active_path = active_path.into();
        let fallback_path = fallback_path.into();
        let override_path = override_path.into();
        if !safe_absolute(&active_path)
            || !safe_absolute(&fallback_path)
            || !safe_absolute(&override_path)
            || active_path == fallback_path
            || active_path == override_path
            || fallback_path == override_path
        {
            return Err(PaletteStoreError::InvalidConfiguration);
        }
        Ok(Self {
            lock: Mutex::new(()),
            active_path,
            fallback_path,
            override_path,
        })
    }

    /// Load fields for the web form using launcher-compatible precedence.
    ///
    /// The checked-in fallback is preferred over a stale generated palette. A
    /// malformed optional override is ignored so appearance configuration can
    /// never prevent the dashboard or uploader from starting.
    ///
    /// # Errors
    ///
    /// Returns [`PaletteStoreError`] only when the store lock is poisoned or
    /// neither installed base palette can be loaded and validated.
    pub fn current(&self) -> Result<Vec<PaletteField>, PaletteStoreError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| PaletteStoreError::LockPoisoned)?;
        let palette = match load_tsv(&self.fallback_path) {
            Ok(palette) => palette,
            Err(fallback) => match load_tsv(&self.active_path) {
                Ok(palette) => palette,
                Err(active) => {
                    return Err(PaletteStoreError::BaseUnavailable {
                        fallback: Box::new(fallback),
                        active: Box::new(active),
                    });
                }
            },
        };
        let palette = load_override(&self.override_path).unwrap_or(palette);
        Ok(palette.fields())
    }

    /// Durably replace the optional version 2 appearance override.
    ///
    /// Dashboard process control deliberately remains outside this storage
    /// type, so a successful write is not coupled to service supervision.
    ///
    /// # Errors
    ///
    /// Returns [`PaletteStoreError`] if the store lock is poisoned or a
    /// no-follow, same-directory atomic replacement fails.
    pub fn save(&self, palette: &Palette) -> Result<(), PaletteStoreError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| PaletteStoreError::LockPoisoned)?;
        atomic_write(
            &self.override_path,
            &palette.encode_override(),
            PALETTE_FILE_MODE,
            PALETTE_DIRECTORY_MODE,
        )
        .map_err(PaletteStorageError::from)
        .map_err(PaletteStoreError::Save)?;
        Ok(())
    }
}

impl fmt::Debug for PaletteStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PaletteStore")
            .field("active_path", &self.active_path)
            .field("fallback_path", &self.fallback_path)
            .field("override_path", &self.override_path)
            .finish_non_exhaustive()
    }
}

fn load_tsv(path: &Path) -> Result<Palette, PaletteLoadError> {
    let file = read_bounded_regular(path, MAXIMUM_PALETTE_FILE_BYTES)
        .map_err(PaletteStorageError::from)
        .map_err(PaletteLoadError::Storage)?;
    Palette::parse_tsv(&file.contents).map_err(PaletteLoadError::Format)
}

fn load_override(path: &Path) -> Result<Palette, PaletteLoadError> {
    let file = read_bounded_regular(path, MAXIMUM_PALETTE_FILE_BYTES)
        .map_err(PaletteStorageError::from)
        .map_err(PaletteLoadError::Storage)?;
    Palette::parse_override(&file.contents).map_err(PaletteLoadError::Format)
}

fn safe_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::{Palette, PaletteRole, PaletteStore, PaletteStoreError};
    use std::{
        fs,
        os::unix::fs::{MetadataExt as _, symlink},
    };

    fn rgb(index: usize) -> String {
        format!(
            "#{:02X}{:02X}{:02X}",
            index * 3 + 1,
            index * 3 + 2,
            index * 3 + 3
        )
    }

    fn pairs(offset: usize) -> Vec<(&'static str, String)> {
        PaletteRole::ALL
            .into_iter()
            .enumerate()
            .map(|(index, role)| (role.as_str(), rgb(index + offset)))
            .collect()
    }

    fn palette(offset: usize) -> Option<Palette> {
        Palette::from_pairs(pairs(offset)).ok()
    }

    fn tsv(offset: usize) -> Vec<u8> {
        let mut output = String::new();
        for (name, value) in pairs(offset) {
            output.push_str(name);
            output.push('\t');
            output.push_str(&value);
            output.push('\n');
        }
        output.into_bytes()
    }

    #[test]
    fn store_prefers_fallback_uses_valid_override_and_ignores_bad_override() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let fallback = directory.path().join("fallback.tsv");
        let active = directory.path().join("active.tsv");
        let override_path = directory.path().join("state/palette.sexp");
        assert!(fs::write(&fallback, tsv(0)).is_ok());
        assert!(fs::write(&active, tsv(32)).is_ok());
        let store = PaletteStore::new(&active, &fallback, &override_path);
        assert!(store.is_ok());
        let Some(store) = store.ok() else {
            return;
        };
        assert!(matches!(
            store.current(),
            Ok(fields) if fields.first().is_some_and(|field| field.value == "#010203")
        ));

        let Some(override_palette) = palette(32) else {
            return;
        };
        assert!(store.save(&override_palette).is_ok());
        assert!(matches!(
            fs::metadata(&override_path),
            Ok(metadata) if metadata.mode() & 0o777 == 0o600
        ));
        assert!(matches!(
            store.current(),
            Ok(fields) if fields.first().is_some_and(|field| field.value == "#616263")
        ));

        assert!(fs::write(&override_path, b"(:version 2 :palette ())\n").is_ok());
        assert!(matches!(
            store.current(),
            Ok(fields) if fields.first().is_some_and(|field| field.value == "#010203")
        ));
        assert!(fs::remove_file(&fallback).is_ok());
        assert!(matches!(
            store.current(),
            Ok(fields) if fields.first().is_some_and(|field| field.value == "#616263")
        ));
    }

    #[test]
    fn save_replaces_only_the_override_symlink() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let fallback = directory.path().join("fallback.tsv");
        let active = directory.path().join("active.tsv");
        let override_path = directory.path().join("override.sexp");
        let victim = directory.path().join("victim");
        assert!(fs::write(&fallback, tsv(0)).is_ok());
        assert!(fs::write(&active, tsv(1)).is_ok());
        assert!(fs::write(&victim, b"untouched").is_ok());
        assert!(symlink(&victim, &override_path).is_ok());
        let store = PaletteStore::new(&active, &fallback, &override_path);
        assert!(store.is_ok());
        let Some(store) = store.ok() else {
            return;
        };
        let Some(palette) = palette(0) else {
            return;
        };
        assert!(store.save(&palette).is_ok());
        assert!(matches!(fs::read(&victim), Ok(contents) if contents == b"untouched"));
        assert!(matches!(
            fs::symlink_metadata(&override_path),
            Ok(metadata) if metadata.is_file()
        ));
    }

    #[test]
    fn rejects_relative_or_overlapping_store_paths() {
        assert!(matches!(
            PaletteStore::new("active", "/fallback", "/override"),
            Err(PaletteStoreError::InvalidConfiguration)
        ));
        assert!(matches!(
            PaletteStore::new("/same", "/same", "/override"),
            Err(PaletteStoreError::InvalidConfiguration)
        ));
    }
}
