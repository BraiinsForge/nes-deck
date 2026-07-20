//! Secure startup loading with explicit optional-asset fallbacks.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use retro_deck_config::{
    Catalog, CatalogError, Credits, CreditsError, MAXIMUM_CATALOG_BYTES, MAXIMUM_CREDITS_BYTES,
    MAXIMUM_PALETTE_BYTES, Palette, PaletteError,
};
use retro_deck_platform::file::{BoundedReadError, read_regular_bounded};

use crate::{CreditsCrawl, DashboardCatalog, DashboardCatalogError};

/// Absolute, distinct files consumed during dashboard startup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardAssetPaths {
    manifest: PathBuf,
    credits: PathBuf,
    palette: PathBuf,
}

impl DashboardAssetPaths {
    /// Validate the three configured asset paths before any file is opened.
    ///
    /// # Errors
    ///
    /// Returns [`AssetPathError`] unless every path is absolute and the three
    /// roles name distinct locations.
    pub fn new(
        manifest: impl Into<PathBuf>,
        credits: impl Into<PathBuf>,
        palette: impl Into<PathBuf>,
    ) -> Result<Self, AssetPathError> {
        let paths = Self {
            manifest: manifest.into(),
            credits: credits.into(),
            palette: palette.into(),
        };
        if !paths.manifest.is_absolute() {
            return Err(AssetPathError::NotAbsolute(AssetKind::Manifest));
        }
        if !paths.credits.is_absolute() {
            return Err(AssetPathError::NotAbsolute(AssetKind::Credits));
        }
        if !paths.palette.is_absolute() {
            return Err(AssetPathError::NotAbsolute(AssetKind::Palette));
        }
        if paths.manifest == paths.credits
            || paths.manifest == paths.palette
            || paths.credits == paths.palette
        {
            return Err(AssetPathError::Overlapping);
        }
        Ok(paths)
    }

    /// Required catalog manifest path.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "PathBuf borrowing is not const on the pinned Rust toolchain"
    )]
    pub fn manifest(&self) -> &Path {
        &self.manifest
    }

    /// Optional credits manifest path.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "PathBuf borrowing is not const on the pinned Rust toolchain"
    )]
    pub fn credits(&self) -> &Path {
        &self.credits
    }

    /// Optional generated palette path.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "PathBuf borrowing is not const on the pinned Rust toolchain"
    )]
    pub fn palette(&self) -> &Path {
        &self.palette
    }
}

/// Loaded assets ready to construct the runtime model and first frame.
#[derive(Debug)]
pub struct DashboardAssets {
    catalog: DashboardCatalog,
    credits: CreditsCrawl,
    palette: Palette,
    credits_fallback: Option<CreditsFallback>,
    palette_fallback: Option<PaletteFallback>,
}

impl DashboardAssets {
    /// Load the required catalog and optional credits and palette securely.
    ///
    /// The required manifest fails startup. Missing or malformed credits show
    /// the renderer's unavailable view, while a missing or malformed palette
    /// uses compiled colors. Optional failures remain available for logging.
    ///
    /// # Errors
    ///
    /// Returns [`DashboardAssetsError`] only when the required catalog cannot
    /// be read, parsed, or combined with standard native applications.
    pub fn load(paths: &DashboardAssetPaths) -> Result<Self, DashboardAssetsError> {
        let manifest = read_regular_bounded(paths.manifest(), MAXIMUM_CATALOG_BYTES)
            .map_err(DashboardAssetsError::ManifestRead)?;
        let catalog = Catalog::parse(&manifest).map_err(DashboardAssetsError::ManifestParse)?;
        if catalog.is_empty() {
            return Err(DashboardAssetsError::EmptyManifest);
        }
        let catalog = DashboardCatalog::with_standard_apps(&catalog)
            .map_err(DashboardAssetsError::Catalog)?;

        let (credits, credits_fallback) =
            match read_regular_bounded(paths.credits(), MAXIMUM_CREDITS_BYTES) {
                Ok(contents) => match Credits::parse(&contents) {
                    Ok(credits) => (CreditsCrawl::from_credits(&credits), None),
                    Err(error) => (
                        CreditsCrawl::unavailable(),
                        Some(CreditsFallback::Parse(error)),
                    ),
                },
                Err(error) => (
                    CreditsCrawl::unavailable(),
                    Some(CreditsFallback::Read(error)),
                ),
            };
        let (palette, palette_fallback) =
            match read_regular_bounded(paths.palette(), MAXIMUM_PALETTE_BYTES) {
                Ok(contents) => match Palette::parse_tsv(&contents) {
                    Ok(palette) => (palette, None),
                    Err(error) => (Palette::default(), Some(PaletteFallback::Parse(error))),
                },
                Err(error) => (Palette::default(), Some(PaletteFallback::Read(error))),
            };

        Ok(Self {
            catalog,
            credits,
            palette,
            credits_fallback,
            palette_fallback,
        })
    }

    /// Combined owner, uploaded, and standard native catalog.
    #[must_use]
    pub const fn catalog(&self) -> &DashboardCatalog {
        &self.catalog
    }

    /// Prepared crawl or safe unavailable value.
    #[must_use]
    pub const fn credits(&self) -> &CreditsCrawl {
        &self.credits
    }

    /// Parsed full-RGB palette or compiled fallback.
    #[must_use]
    pub const fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Why the credits unavailable view is in use, if applicable.
    #[must_use]
    pub const fn credits_fallback(&self) -> Option<&CreditsFallback> {
        self.credits_fallback.as_ref()
    }

    /// Why compiled palette colors are in use, if applicable.
    #[must_use]
    pub const fn palette_fallback(&self) -> Option<&PaletteFallback> {
        self.palette_fallback.as_ref()
    }
}

/// Asset role used in path diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetKind {
    /// Required catalog manifest.
    Manifest,
    /// Optional attribution manifest.
    Credits,
    /// Optional full-RGB palette.
    Palette,
}

impl fmt::Display for AssetKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest => formatter.write_str("manifest"),
            Self::Credits => formatter.write_str("credits"),
            Self::Palette => formatter.write_str("palette"),
        }
    }
}

/// Startup asset path contract failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetPathError {
    /// One role has a relative path.
    NotAbsolute(AssetKind),
    /// Two roles name the same path.
    Overlapping,
}

impl fmt::Display for AssetPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAbsolute(kind) => write!(formatter, "dashboard {kind} path is not absolute"),
            Self::Overlapping => formatter.write_str("dashboard asset paths overlap"),
        }
    }
}

impl Error for AssetPathError {}

/// Required catalog startup failure.
#[derive(Debug)]
pub enum DashboardAssetsError {
    /// Secure bounded file read failed.
    ManifestRead(BoundedReadError),
    /// Catalog bytes violate the shared schema.
    ManifestParse(CatalogError),
    /// Catalog contains no owner or uploaded entries.
    EmptyManifest,
    /// Catalog conflicts with a standard native application.
    Catalog(DashboardCatalogError),
}

impl fmt::Display for DashboardAssetsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestRead(error) => {
                write!(formatter, "cannot load dashboard manifest: {error}")
            }
            Self::ManifestParse(error) => write!(formatter, "invalid dashboard manifest: {error}"),
            Self::EmptyManifest => formatter.write_str("dashboard manifest contains no entries"),
            Self::Catalog(error) => write!(formatter, "cannot build dashboard catalog: {error}"),
        }
    }
}

impl Error for DashboardAssetsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ManifestRead(error) => Some(error),
            Self::ManifestParse(error) => Some(error),
            Self::Catalog(error) => Some(error),
            Self::EmptyManifest => None,
        }
    }
}

/// Optional credits failure retained for startup logging.
#[derive(Debug)]
pub enum CreditsFallback {
    /// Secure bounded file read failed.
    Read(BoundedReadError),
    /// Manifest violates the credits schema.
    Parse(CreditsError),
}

impl fmt::Display for CreditsFallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(formatter, "cannot load credits: {error}"),
            Self::Parse(error) => write!(formatter, "invalid credits: {error}"),
        }
    }
}

impl Error for CreditsFallback {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::Parse(error) => Some(error),
        }
    }
}

/// Optional palette failure retained for startup logging.
#[derive(Debug)]
pub enum PaletteFallback {
    /// Secure bounded file read failed.
    Read(BoundedReadError),
    /// Document violates the complete palette schema.
    Parse(PaletteError),
}

impl fmt::Display for PaletteFallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(formatter, "cannot load dashboard palette: {error}"),
            Self::Parse(error) => write!(formatter, "invalid dashboard palette: {error}"),
        }
    }
}

impl Error for PaletteFallback {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::Parse(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use retro_deck_config::{Palette, PaletteRole};

    use super::{AssetKind, AssetPathError, DashboardAssetPaths, DashboardAssets};

    const CATALOG: &[u8] = include_bytes!("../../../deploy/menu/games.tsv");
    const CREDITS: &[u8] = include_bytes!("../../../deploy/menu/credits.tsv");
    const PALETTE: &[u8] = include_bytes!("../../../deploy/menu/palette.tsv");
    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct Fixture {
        root: PathBuf,
    }

    use std::path::PathBuf;

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "retro-deck-dashboard-assets-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("asset fixture directory is created");
            Self { root }
        }

        fn paths(&self) -> DashboardAssetPaths {
            DashboardAssetPaths::new(
                self.root.join("games.tsv"),
                self.root.join("credits.tsv"),
                self.root.join("palette.tsv"),
            )
            .expect("fixture paths are absolute and distinct")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn loads_required_and_optional_assets_with_standard_apps() {
        let fixture = Fixture::new();
        let paths = fixture.paths();
        fs::write(paths.manifest(), CATALOG).expect("catalog fixture is written");
        fs::write(paths.credits(), CREDITS).expect("credits fixture is written");
        fs::write(paths.palette(), PALETTE).expect("palette fixture is written");

        let loaded = DashboardAssets::load(&paths);
        assert!(loaded.is_ok());
        let Some(loaded) = loaded.ok() else {
            return;
        };
        assert_eq!(loaded.catalog().entries().len(), 22);
        assert!(loaded.credits().is_available());
        assert_eq!(
            loaded.palette().color(PaletteRole::Accent),
            Palette::default().color(PaletteRole::Accent)
        );
        assert!(loaded.credits_fallback().is_none());
        assert!(loaded.palette_fallback().is_none());
    }

    #[test]
    fn optional_failures_are_observable_but_never_block_startup() {
        let fixture = Fixture::new();
        let paths = fixture.paths();
        fs::write(paths.manifest(), CATALOG).expect("catalog fixture is written");
        fs::write(paths.credits(), b"broken\n").expect("bad credits fixture is written");
        fs::write(paths.palette(), b"accent\tnope\n").expect("bad palette fixture is written");

        let loaded = DashboardAssets::load(&paths);
        assert!(loaded.is_ok());
        let Some(loaded) = loaded.ok() else {
            return;
        };
        assert!(!loaded.credits().is_available());
        assert!(loaded.credits_fallback().is_some());
        assert!(loaded.palette_fallback().is_some());
        assert_eq!(
            loaded.palette().color(PaletteRole::Title),
            Palette::default().color(PaletteRole::Title)
        );
    }

    #[test]
    fn required_manifest_and_path_contracts_fail_closed() {
        let fixture = Fixture::new();
        let paths = fixture.paths();
        fs::write(paths.manifest(), b"broken\n").expect("bad catalog fixture is written");
        assert!(DashboardAssets::load(&paths).is_err());

        assert_eq!(
            DashboardAssetPaths::new("relative", "/tmp/credits", "/tmp/palette"),
            Err(AssetPathError::NotAbsolute(AssetKind::Manifest))
        );
        assert_eq!(
            DashboardAssetPaths::new("/tmp/same", "/tmp/same", "/tmp/palette"),
            Err(AssetPathError::Overlapping)
        );
    }
}
