//! Serialized, rollback-aware installation of validated ROM uploads.

use std::{
    fmt, io,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

use crate::{
    catalog::{
        Catalog, CatalogEntry, CatalogError, CatalogLoadError, MAXIMUM_GAMES, load,
        load_if_present, uploaded_entry,
    },
    file::{FileError, atomic_write, install_exclusive, remove_file},
    rom::{GameTitle, System, TitleError, UploadError, decode_upload},
};

const ROM_FILE_MODE: u32 = 0o600;
const ROM_DIRECTORY_MODE: u32 = 0o755;
const CATALOG_FILE_MODE: u32 = 0o600;
const CATALOG_DIRECTORY_MODE: u32 = 0o700;

/// Persistent ROM and supplemental-catalog locations guarded as one store.
pub struct RomStore {
    lock: Mutex<()>,
    rom_root: PathBuf,
    base_catalog: PathBuf,
    upload_catalog: PathBuf,
    catalog_writer: fn(&Path, &[u8]) -> Result<(), FileError>,
}

impl RomStore {
    /// Configure a ROM store without touching the filesystem.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidConfiguration`] unless all three paths
    /// are absolute, traversal-free, and the two catalog paths differ.
    pub fn new(
        rom_root: impl Into<PathBuf>,
        base_catalog: impl Into<PathBuf>,
        upload_catalog: impl Into<PathBuf>,
    ) -> Result<Self, StoreError> {
        let rom_root = rom_root.into();
        let base_catalog = base_catalog.into();
        let upload_catalog = upload_catalog.into();
        if !safe_absolute(&rom_root)
            || !safe_absolute(&base_catalog)
            || !safe_absolute(&upload_catalog)
            || base_catalog == upload_catalog
        {
            return Err(StoreError::InvalidConfiguration);
        }
        Ok(Self {
            lock: Mutex::new(()),
            rom_root,
            base_catalog,
            upload_catalog,
            catalog_writer: write_catalog,
        })
    }

    /// Load the optional supplemental upload catalog under the store lock.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] if the lock is poisoned or the catalog cannot be
    /// read and validated.
    pub fn entries(&self) -> Result<Catalog, StoreError> {
        let _guard = self.lock.lock().map_err(|_| StoreError::LockPoisoned)?;
        load_if_present(&self.upload_catalog).map_err(StoreError::UploadCatalog)
    }

    /// Validate and install one raw or ZIP-wrapped ROM without replacement.
    ///
    /// Input decoding happens before taking the store lock. Catalog reads,
    /// duplicate checks, the exclusive ROM link, and the supplemental catalog
    /// replacement are serialized. A catalog-write failure removes the newly
    /// installed ROM through a no-follow directory walk.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for title, upload, catalog, capacity,
    /// duplication, locking, or durable filesystem failures.
    pub fn add(
        &self,
        system: System,
        title: &str,
        filename: &str,
        input: impl io::Read,
    ) -> Result<CatalogEntry, StoreError> {
        let title = GameTitle::new(title)?;
        let rom = decode_upload(system, filename, input)?;
        let _guard = self.lock.lock().map_err(|_| StoreError::LockPoisoned)?;
        let base = load(&self.base_catalog).map_err(StoreError::BaseCatalog)?;
        let mut uploads =
            load_if_present(&self.upload_catalog).map_err(StoreError::UploadCatalog)?;
        if base.len().saturating_add(uploads.len()) >= MAXIMUM_GAMES {
            return Err(StoreError::Full);
        }

        let destination = self.rom_root.join(system.as_str()).join(format!(
            "{}{}",
            title.slug(),
            system.extension()
        ));
        let entry =
            uploaded_entry(&title, system, &destination).map_err(StoreError::CatalogEntry)?;
        if base.contains_identifier(entry.identifier())
            || uploads.contains_identifier(entry.identifier())
            || base.contains_rom(entry.rom())
            || uploads.contains_rom(entry.rom())
        {
            return Err(StoreError::Duplicate);
        }

        match install_exclusive(
            &destination,
            rom.as_bytes(),
            ROM_FILE_MODE,
            ROM_DIRECTORY_MODE,
        ) {
            Ok(()) => {}
            Err(FileError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(StoreError::RomExists);
            }
            Err(error) => return Err(StoreError::Storage(error.into())),
        }

        if let Err(error) = uploads.insert_sorted(entry.clone()) {
            return Err(rollback_error(&destination, StorageError::Catalog(error)));
        }
        if let Err(error) = (self.catalog_writer)(&self.upload_catalog, &uploads.encode()) {
            return Err(rollback_error(&destination, error.into()));
        }
        Ok(entry)
    }
}

impl fmt::Debug for RomStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RomStore")
            .field("rom_root", &self.rom_root)
            .field("base_catalog", &self.base_catalog)
            .field("upload_catalog", &self.upload_catalog)
            .finish_non_exhaustive()
    }
}

/// A filesystem or catalog mutation failure used during transactional writes.
#[derive(Debug)]
pub enum StorageError {
    /// Operating-system file access failed.
    Io(io::Error),
    /// A path, file type, or size violated a no-follow storage contract.
    Unsafe(&'static str),
    /// Entropy failed while naming a same-directory temporary file.
    Random(String),
    /// An in-memory catalog mutation violated an invariant.
    Catalog(CatalogError),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Unsafe(reason) => formatter.write_str(reason),
            Self::Random(error) => write!(formatter, "cannot name temporary file: {error}"),
            Self::Catalog(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Catalog(error) => Some(error),
            Self::Unsafe(_) | Self::Random(_) => None,
        }
    }
}

impl From<FileError> for StorageError {
    fn from(error: FileError) -> Self {
        match error {
            FileError::Io(error) => Self::Io(error),
            FileError::Unsafe(reason) => Self::Unsafe(reason),
            FileError::Random(error) => Self::Random(error),
        }
    }
}

/// ROM store configuration, validation, or mutation failure.
#[derive(Debug)]
pub enum StoreError {
    /// Configured paths are not safe absolute paths or overlap.
    InvalidConfiguration,
    /// A title violates the uploader contract.
    Title(TitleError),
    /// Raw or ZIP upload validation failed.
    Upload(UploadError),
    /// The checked-in base catalog cannot be trusted.
    BaseCatalog(CatalogLoadError),
    /// The supplemental upload catalog cannot be trusted.
    UploadCatalog(CatalogLoadError),
    /// A generated catalog row violates an invariant.
    CatalogEntry(CatalogError),
    /// The combined catalog already has 64 entries.
    Full,
    /// The identifier or destination is already cataloged.
    Duplicate,
    /// A file already occupies the generated ROM destination.
    RomExists,
    /// The store mutex was poisoned by a prior panic.
    LockPoisoned,
    /// Durable filesystem mutation failed before a ROM was cataloged.
    Storage(StorageError),
    /// Catalog persistence failed after installation, with rollback status.
    SaveRolledBack {
        save: StorageError,
        rollback: Option<StorageError>,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => formatter.write_str("ROM store paths are invalid"),
            Self::Title(error) => error.fmt(formatter),
            Self::Upload(error) => error.fmt(formatter),
            Self::BaseCatalog(error) => write!(formatter, "read built-in catalog: {error}"),
            Self::UploadCatalog(error) => write!(formatter, "read upload catalog: {error}"),
            Self::CatalogEntry(error) => write!(formatter, "create catalog entry: {error}"),
            Self::Full => formatter.write_str("the menu catalog is full"),
            Self::Duplicate => formatter.write_str("a game with this title is already cataloged"),
            Self::RomExists => formatter.write_str("a ROM with this name already exists"),
            Self::LockPoisoned => formatter.write_str("the ROM store lock is unavailable"),
            Self::Storage(error) => error.fmt(formatter),
            Self::SaveRolledBack {
                save,
                rollback: None,
            } => write!(
                formatter,
                "save upload catalog: {save}; ROM was rolled back"
            ),
            Self::SaveRolledBack {
                save,
                rollback: Some(rollback),
            } => write!(
                formatter,
                "save upload catalog: {save}; ROM rollback also failed: {rollback}"
            ),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Title(error) => Some(error),
            Self::Upload(error) => Some(error),
            Self::BaseCatalog(error) | Self::UploadCatalog(error) => Some(error),
            Self::CatalogEntry(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::SaveRolledBack { save, .. } => Some(save),
            Self::InvalidConfiguration
            | Self::Full
            | Self::Duplicate
            | Self::RomExists
            | Self::LockPoisoned => None,
        }
    }
}

impl From<TitleError> for StoreError {
    fn from(error: TitleError) -> Self {
        Self::Title(error)
    }
}

impl From<UploadError> for StoreError {
    fn from(error: UploadError) -> Self {
        Self::Upload(error)
    }
}

fn rollback_error(destination: &Path, save: StorageError) -> StoreError {
    let rollback = remove_file(destination).err().map(Into::into);
    StoreError::SaveRolledBack { save, rollback }
}

fn write_catalog(path: &Path, contents: &[u8]) -> Result<(), FileError> {
    atomic_write(path, contents, CATALOG_FILE_MODE, CATALOG_DIRECTORY_MODE)
}

fn safe_absolute(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::{RomStore, StoreError};
    use crate::{catalog::Catalog, file::FileError, rom::System};
    use std::{fs, os::unix::fs::MetadataExt as _, path::Path, sync::Arc, thread};

    fn nes_rom() -> Vec<u8> {
        let mut rom = vec![0_u8; 16 + 16_384];
        if let Some(header) = rom.get_mut(..4) {
            header.copy_from_slice(b"NES\x1a");
        }
        if let Some(prg_banks) = rom.get_mut(4) {
            *prg_banks = 1;
        }
        rom
    }

    fn test_store(directory: &Path) -> Result<RomStore, StoreError> {
        let base = directory.join("base.tsv");
        fs::write(&base, b"").map_err(|_| StoreError::InvalidConfiguration)?;
        RomStore::new(
            directory.join("roms"),
            base,
            directory.join("uploads/games.tsv"),
        )
    }

    fn fail_catalog_write(_path: &Path, _contents: &[u8]) -> Result<(), FileError> {
        Err(FileError::Unsafe("injected catalog failure"))
    }

    #[test]
    fn installs_rom_and_sorted_catalog_without_replacement() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let store = test_store(directory.path());
        assert!(store.is_ok());
        let Some(store) = store.ok() else {
            return;
        };
        let first = store.add(System::Nes, "Test Game", "source.nes", nes_rom().as_slice());
        assert!(matches!(
            first,
            Ok(ref entry)
                if entry.identifier() == "upload-nes-test-game"
                    && entry.rom() == directory.path().join("roms/nes/test-game.nes")
        ));
        let destination = directory.path().join("roms/nes/test-game.nes");
        assert!(matches!(fs::read(&destination), Ok(contents) if contents == nes_rom()));
        assert!(
            matches!(fs::metadata(&destination), Ok(metadata) if metadata.mode() & 0o777 == 0o600)
        );
        assert!(
            store
                .add(System::Nes, "Alpha Game", "alpha.nes", nes_rom().as_slice())
                .is_ok()
        );
        assert!(matches!(
            store.entries(),
            Ok(catalog)
                if catalog.len() == 2
                    && catalog.entries().first().is_some_and(|entry| entry.title() == "Alpha Game")
        ));

        assert!(matches!(
            store.add(System::Nes, "Test Game", "again.nes", nes_rom().as_slice()),
            Err(StoreError::Duplicate)
        ));
        assert!(matches!(fs::read(&destination), Ok(contents) if contents == nes_rom()));
    }

    #[test]
    fn catalog_failure_rolls_back_the_new_rom() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let store = test_store(directory.path());
        assert!(store.is_ok());
        let Some(mut store) = store.ok() else {
            return;
        };
        store.catalog_writer = fail_catalog_write;
        assert!(matches!(
            store.add(System::Nes, "Rollback", "source.nes", nes_rom().as_slice()),
            Err(StoreError::SaveRolledBack { rollback: None, .. })
        ));
        assert!(!directory.path().join("roms/nes/rollback.nes").exists());
        assert!(!directory.path().join("uploads/games.tsv").exists());
    }

    #[test]
    fn concurrent_duplicate_uploads_are_serialized() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let store = test_store(directory.path()).map(Arc::new);
        assert!(store.is_ok());
        let Some(store) = store.ok() else {
            return;
        };
        let mut workers = Vec::new();
        for _ in 0..2 {
            let store = Arc::clone(&store);
            workers.push(thread::spawn(move || {
                store.add(System::Nes, "Race", "source.nes", nes_rom().as_slice())
            }));
        }
        let outcomes = workers
            .into_iter()
            .filter_map(|worker| worker.join().ok())
            .collect::<Vec<_>>();
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.is_err()).count(),
            1
        );
        let catalog = store.entries();
        assert!(matches!(catalog, Ok(ref catalog) if catalog.len() == 1));
        if let Ok(catalog) = catalog {
            assert!(Catalog::parse(&catalog.encode()).is_ok());
        }
    }

    #[test]
    fn rejects_relative_or_traversing_configuration() {
        assert!(matches!(
            RomStore::new("roms", "/base.tsv", "/uploads.tsv"),
            Err(StoreError::InvalidConfiguration)
        ));
        assert!(matches!(
            RomStore::new("/roms/../escape", "/base.tsv", "/uploads.tsv"),
            Err(StoreError::InvalidConfiguration)
        ));
    }
}
