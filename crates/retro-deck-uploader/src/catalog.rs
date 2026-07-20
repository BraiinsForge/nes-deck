//! Secure catalog file access and upload-specific entry construction.

use std::{fmt, io, path::Path};

pub use retro_deck_config::{
    Catalog, CatalogEntry, CatalogError, CatalogSystem, GameColor, MAXIMUM_CATALOG_BYTES,
    MAXIMUM_GAMES,
};
use retro_deck_config::{MAXIMUM_IDENTIFIER_BYTES, System};

use crate::{
    file::{FileError, read_bounded_regular},
    rom::GameTitle,
};

/// Load a required bounded regular catalog without following a final symlink.
///
/// # Errors
///
/// Returns [`CatalogLoadError`] for file access, file type, size, or parse
/// failures.
pub fn load(path: &Path) -> Result<Catalog, CatalogLoadError> {
    let file =
        read_bounded_regular(path, maximum_catalog_bytes()).map_err(CatalogLoadError::from_file)?;
    Catalog::parse(&file.contents).map_err(CatalogLoadError::Parse)
}

/// Load an optional catalog, treating only a missing final file as empty.
///
/// # Errors
///
/// Returns [`CatalogLoadError`] for all failures except a missing file.
pub fn load_if_present(path: &Path) -> Result<Catalog, CatalogLoadError> {
    match read_bounded_regular(path, maximum_catalog_bytes()) {
        Ok(file) => Catalog::parse(&file.contents).map_err(CatalogLoadError::Parse),
        Err(FileError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            Ok(Catalog::default())
        }
        Err(error) => Err(CatalogLoadError::from_file(error)),
    }
}

/// Construct the canonical catalog row for one validated web upload.
///
/// # Errors
///
/// Returns [`CatalogError`] if the destination cannot be represented by the
/// shared dashboard catalog contract.
pub(crate) fn uploaded_entry(
    title: &GameTitle,
    system: System,
    destination: &Path,
) -> Result<CatalogEntry, CatalogError> {
    let mut identifier = format!("upload-{}-{}", system.as_str(), title.slug());
    identifier.truncate(MAXIMUM_IDENTIFIER_BYTES);
    while identifier.ends_with('-') {
        identifier.pop();
    }
    let destination = destination
        .to_str()
        .ok_or(CatalogError::InvalidField("ROM path is not UTF-8"))?;
    CatalogEntry::new(
        &identifier,
        title.as_str(),
        system.into(),
        destination,
        system.color(),
    )
}

fn maximum_catalog_bytes() -> u64 {
    u64::try_from(MAXIMUM_CATALOG_BYTES).unwrap_or(u64::MAX)
}

/// Catalog file access or syntax failure.
#[derive(Debug)]
pub enum CatalogLoadError {
    /// Operating-system file access failed.
    Io(io::Error),
    /// File type or size violated the no-follow access contract.
    UnsafeFile(&'static str),
    /// The bounded file did not contain a valid catalog.
    Parse(CatalogError),
}

impl CatalogLoadError {
    fn from_file(error: FileError) -> Self {
        match error {
            FileError::Io(error) => Self::Io(error),
            FileError::Unsafe(reason) => Self::UnsafeFile(reason),
            FileError::Random(_) => Self::UnsafeFile("unexpected random-name failure"),
        }
    }
}

impl fmt::Display for CatalogLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::UnsafeFile(reason) => formatter.write_str(reason),
            Self::Parse(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CatalogLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Parse(error) => Some(error),
            Self::UnsafeFile(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{load, load_if_present};
    use std::{fs, os::unix::fs::symlink};

    const DEPLOYED_CATALOG: &[u8] = include_bytes!("../../../deploy/menu/games.tsv");

    #[test]
    fn loads_only_bounded_regular_or_missing_optional_catalogs() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let missing = directory.path().join("missing.tsv");
        assert!(matches!(load_if_present(&missing), Ok(catalog) if catalog.is_empty()));
        assert!(load(&missing).is_err());

        let catalog_path = directory.path().join("games.tsv");
        assert!(fs::write(&catalog_path, DEPLOYED_CATALOG).is_ok());
        assert!(matches!(load(&catalog_path), Ok(catalog) if catalog.len() == 15));
        let link = directory.path().join("link.tsv");
        assert!(symlink(&catalog_path, &link).is_ok());
        assert!(load(&link).is_err());
    }
}
