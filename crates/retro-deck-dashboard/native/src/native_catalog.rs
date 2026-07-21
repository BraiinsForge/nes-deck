//! Narrow catalog loading for the BMC-native dashboard.

use std::collections::TryReserveError;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use retro_deck_config::{Catalog, CatalogError, MAXIMUM_CATALOG_BYTES};
use rustix::fs::{Mode, OFlags, open};

use crate::{DashboardCatalog, DashboardCatalogError};

/// Load one validated dashboard catalog without pulling in the legacy platform crate.
///
/// The final path component may not be a symlink, the opened object must be a
/// nonempty regular file, and both its reported and observed sizes are bounded.
///
/// # Errors
///
/// Returns [`NativeCatalogError`] for an unsafe path, filesystem failure,
/// allocation failure, malformed catalog, or invalid combined dashboard view.
pub fn load_native_catalog(path: impl AsRef<Path>) -> Result<DashboardCatalog, NativeCatalogError> {
    let catalog = load_catalog(path.as_ref())?;
    if catalog.is_empty() {
        return Err(NativeCatalogError::EmptyCatalog);
    }
    DashboardCatalog::from_catalog(&catalog).map_err(NativeCatalogError::Dashboard)
}

/// Load the required installed catalog plus an optional web-upload catalog.
///
/// A missing or malformed supplemental file is recoverable state and leaves
/// the checked-in catalog usable. Valid supplemental rows still pass the
/// combined identifier, path, and capacity checks.
///
/// # Errors
///
/// Returns [`NativeCatalogError`] when the required catalog cannot be loaded
/// or when two individually valid sources violate a combined invariant.
pub fn load_native_catalog_with_uploads(
    base_path: impl AsRef<Path>,
    upload_path: impl AsRef<Path>,
) -> Result<DashboardCatalog, NativeCatalogError> {
    let base = load_catalog(base_path.as_ref())?;
    if base.is_empty() {
        return Err(NativeCatalogError::EmptyCatalog);
    }
    let uploads = match load_catalog(upload_path.as_ref()) {
        Ok(catalog) => Some(catalog),
        Err(NativeCatalogError::Open { source, .. })
            if source.kind() == io::ErrorKind::NotFound =>
        {
            None
        }
        Err(error) => {
            tracing::warn!(?error, "ignoring invalid supplemental upload catalog");
            None
        }
    };
    DashboardCatalog::from_entries(
        base.entries().iter().cloned().chain(
            uploads
                .iter()
                .flat_map(|catalog| catalog.entries().iter().cloned()),
        ),
    )
    .map_err(NativeCatalogError::Dashboard)
}

fn load_catalog(path: &Path) -> Result<Catalog, NativeCatalogError> {
    if !path.is_absolute() {
        return Err(NativeCatalogError::UnsafePath(path.to_path_buf()));
    }
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|source| NativeCatalogError::Open {
        path: path.to_path_buf(),
        source: source.into(),
    })?;
    let file = File::from(descriptor);
    let metadata = file.metadata().map_err(NativeCatalogError::Metadata)?;
    if !metadata.file_type().is_file() {
        return Err(NativeCatalogError::NotRegular);
    }
    if metadata.len() == 0 {
        return Err(NativeCatalogError::EmptyFile);
    }
    let maximum = u64::try_from(MAXIMUM_CATALOG_BYTES).unwrap_or(u64::MAX);
    if metadata.len() > maximum {
        return Err(NativeCatalogError::Oversized {
            size: metadata.len(),
        });
    }
    let initial = usize::try_from(metadata.len()).map_err(|_| NativeCatalogError::Oversized {
        size: metadata.len(),
    })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(initial)
        .map_err(NativeCatalogError::Allocate)?;
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(NativeCatalogError::Read)?;
    if bytes.len() > MAXIMUM_CATALOG_BYTES {
        return Err(NativeCatalogError::Oversized {
            size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        });
    }

    Catalog::parse(&bytes).map_err(NativeCatalogError::Parse)
}

/// Failure while loading the BMC-native dashboard catalog.
#[derive(Debug)]
pub enum NativeCatalogError {
    /// The configured path is not absolute.
    UnsafePath(PathBuf),
    /// Opening without following a final symlink failed.
    Open {
        /// Requested catalog path.
        path: PathBuf,
        /// Operating-system failure.
        source: io::Error,
    },
    /// Descriptor metadata could not be read.
    Metadata(io::Error),
    /// The opened object is not a regular file.
    NotRegular,
    /// The catalog file contains no bytes.
    EmptyFile,
    /// The file exceeded [`MAXIMUM_CATALOG_BYTES`].
    Oversized {
        /// Observed byte count.
        size: u64,
    },
    /// Reserving the bounded payload failed.
    Allocate(TryReserveError),
    /// Reading the bounded descriptor failed.
    Read(io::Error),
    /// Catalog syntax or field validation failed.
    Parse(CatalogError),
    /// A syntactically valid catalog has no owner-supplied entries.
    EmptyCatalog,
    /// Combining the catalog with native applications violated an invariant.
    Dashboard(DashboardCatalogError),
}

impl fmt::Display for NativeCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafePath(path) => {
                write!(
                    formatter,
                    "catalog path is not absolute: {}",
                    path.display()
                )
            }
            Self::Open { path, source } => {
                write!(formatter, "cannot open {}: {source}", path.display())
            }
            Self::Metadata(source) => write!(formatter, "cannot inspect catalog: {source}"),
            Self::NotRegular => formatter.write_str("catalog is not a regular file"),
            Self::EmptyFile => formatter.write_str("catalog file is empty"),
            Self::Oversized { size } => write!(
                formatter,
                "catalog has {size} bytes; maximum is {MAXIMUM_CATALOG_BYTES}"
            ),
            Self::Allocate(source) => write!(formatter, "cannot allocate catalog: {source}"),
            Self::Read(source) => write!(formatter, "cannot read catalog: {source}"),
            Self::Parse(source) => write!(formatter, "invalid catalog: {source}"),
            Self::EmptyCatalog => formatter.write_str("catalog contains no entries"),
            Self::Dashboard(source) => write!(formatter, "cannot build dashboard: {source}"),
        }
    }
}

impl Error for NativeCatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Open { source, .. } | Self::Metadata(source) | Self::Read(source) => Some(source),
            Self::Allocate(source) => Some(source),
            Self::Parse(source) => Some(source),
            Self::Dashboard(source) => Some(source),
            Self::UnsafePath(_)
            | Self::NotRegular
            | Self::EmptyFile
            | Self::Oversized { .. }
            | Self::EmptyCatalog => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    const CATALOG: &[u8] = include_bytes!("../../../../deploy/menu/games.tsv");
    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    fn fixture_path(name: &str) -> PathBuf {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "retro-deck-native-catalog-{}-{serial}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn loads_the_deployed_catalog_without_legacy_platform_code() {
        let path = fixture_path("games.tsv");
        fs::write(&path, CATALOG).expect("catalog fixture is written");
        let catalog = load_native_catalog(&path).expect("native catalog loads");
        assert_eq!(catalog.entries().len(), 15);
        let _ignored = fs::remove_file(path);
    }

    #[test]
    fn rejects_relative_symlinked_and_oversized_catalogs() {
        assert!(matches!(
            load_native_catalog("relative.tsv"),
            Err(NativeCatalogError::UnsafePath(_))
        ));

        let target = fixture_path("target.tsv");
        let alias = fixture_path("alias.tsv");
        fs::write(&target, CATALOG).expect("catalog target is written");
        symlink(&target, &alias).expect("catalog alias is created");
        assert!(matches!(
            load_native_catalog(&alias),
            Err(NativeCatalogError::Open { .. })
        ));
        let _ignored = fs::remove_file(alias);
        let _ignored = fs::remove_file(target);

        let oversized = fixture_path("oversized.tsv");
        fs::write(&oversized, vec![b'x'; MAXIMUM_CATALOG_BYTES + 1])
            .expect("oversized catalog is written");
        assert!(matches!(
            load_native_catalog(&oversized),
            Err(NativeCatalogError::Oversized { .. })
        ));
        let _ignored = fs::remove_file(oversized);
    }

    #[test]
    fn combines_valid_uploads_and_ignores_a_broken_optional_catalog() {
        let base = fixture_path("base.tsv");
        let uploads = fixture_path("uploads.tsv");
        fs::write(&base, CATALOG).expect("base catalog is written");
        fs::write(
            &uploads,
            b"uploaded\tUPLOADED\tnes\t/mnt/data/roms/nes/uploaded.nes\t#D78787\n",
        )
        .expect("upload catalog is written");
        let combined =
            load_native_catalog_with_uploads(&base, &uploads).expect("valid catalogs combine");
        assert_eq!(combined.entries().len(), 16);
        assert!(combined.find("uploaded").is_some());

        fs::write(&uploads, b"broken\n").expect("broken upload catalog is written");
        let fallback = load_native_catalog_with_uploads(&base, &uploads)
            .expect("broken optional catalog falls back");
        assert_eq!(fallback.entries().len(), 15);
        let _ignored = fs::remove_file(base);
        let _ignored = fs::remove_file(uploads);
    }
}
