//! Dashboard-specific category view over shared validated catalog entries.

use std::{collections::HashSet, fmt, path::Path};

#[cfg(feature = "bmc-native")]
use retro_deck_config::CatalogError;
use retro_deck_config::{Catalog, CatalogEntry, CatalogSystem, MAXIMUM_GAMES, System};
#[cfg(feature = "bmc-native")]
use retro_deck_policy::Value;

const SYSTEM_ORDER: [CatalogSystem; 6] = [
    CatalogSystem::Rom(System::Nes),
    CatalogSystem::Rom(System::GameBoy),
    CatalogSystem::Rom(System::GameBoyColor),
    CatalogSystem::Rom(System::ZxSpectrum),
    CatalogSystem::Rom(System::Chip8),
    CatalogSystem::Deck,
];

const MAXIMUM_POLICY_APPLICATIONS: usize = 7;

/// Maximum shared catalog entries plus the closed native application set.
pub const MAXIMUM_DASHBOARD_ENTRIES: usize = MAXIMUM_GAMES + MAXIMUM_POLICY_APPLICATIONS;

/// One nonempty dashboard category and its entries in catalog order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Category {
    system: CatalogSystem,
    entry_indices: Vec<usize>,
}

impl Category {
    /// Typed console or Deck application identity.
    #[must_use]
    pub const fn system(&self) -> CatalogSystem {
        self.system
    }

    /// Stable user-facing label used by the dashboard tabs.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self.system {
            CatalogSystem::Rom(System::Nes) => "NES",
            CatalogSystem::Rom(System::GameBoy) => "GAME BOY",
            CatalogSystem::Rom(System::GameBoyColor) => "GBC",
            CatalogSystem::Rom(System::ZxSpectrum) => "ZX SPECTRUM",
            CatalogSystem::Rom(System::Chip8) => "CHIP-8",
            CatalogSystem::Deck => "DECK",
        }
    }

    /// Indices into the owning [`DashboardCatalog`] in display order.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Vec slicing is not const on the pinned Rust toolchain"
    )]
    pub fn entry_indices(&self) -> &[usize] {
        &self.entry_indices
    }

    /// Number of entries in this category.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entry_indices.len()
    }

    /// Categories exposed by this type are always nonempty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entry_indices.is_empty()
    }
}

/// Validated entries grouped into the dashboard's stable category order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardCatalog {
    entries: Vec<CatalogEntry>,
    categories: Vec<Category>,
}

impl DashboardCatalog {
    /// Clone one shared catalog into a dashboard category view.
    ///
    /// # Errors
    ///
    /// Returns [`DashboardCatalogError`] when the input is empty, excessive,
    /// or contains an identifier or path conflict.
    pub fn from_catalog(catalog: &Catalog) -> Result<Self, DashboardCatalogError> {
        Self::from_entries(catalog.entries().iter().cloned())
    }

    /// Build a view from base, uploaded, and generated native entries.
    ///
    /// Every entry has already passed the shared field schema. This boundary
    /// validates invariants that can be violated only while combining sources.
    ///
    /// # Errors
    ///
    /// Returns [`DashboardCatalogError`] when the combined input is empty,
    /// excessive, or contains an identifier or path conflict.
    pub fn from_entries(
        entries: impl IntoIterator<Item = CatalogEntry>,
    ) -> Result<Self, DashboardCatalogError> {
        let mut collected = Vec::new();
        let mut identifiers = HashSet::new();
        let mut paths = HashSet::new();
        for entry in entries {
            if collected.len() >= MAXIMUM_DASHBOARD_ENTRIES {
                return Err(DashboardCatalogError::TooManyEntries);
            }
            if !identifiers.insert(entry.identifier().to_owned()) {
                return Err(DashboardCatalogError::DuplicateIdentifier);
            }
            if !paths.insert(entry.rom().to_path_buf()) {
                return Err(DashboardCatalogError::DuplicatePath);
            }
            collected.push(entry);
        }
        if collected.is_empty() {
            return Err(DashboardCatalogError::Empty);
        }

        let mut categories = Vec::new();
        for system in SYSTEM_ORDER {
            let entry_indices = collected
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| (entry.system() == system).then_some(index))
                .collect::<Vec<_>>();
            if !entry_indices.is_empty() {
                categories.push(Category {
                    system,
                    entry_indices,
                });
            }
        }
        Ok(Self {
            entries: collected,
            categories,
        })
    }

    /// All entries in their source order.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Vec slicing is not const on the pinned Rust toolchain"
    )]
    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }

    /// One entry by a category-provided index.
    #[must_use]
    pub fn entry(&self, index: usize) -> Option<&CatalogEntry> {
        self.entries.get(index)
    }

    /// Nonempty categories in fixed console order.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Vec slicing is not const on the pinned Rust toolchain"
    )]
    pub fn categories(&self) -> &[Category] {
        &self.categories
    }

    /// Find an entry by its stable identifier.
    #[must_use]
    pub fn find(&self, identifier: &str) -> Option<&CatalogEntry> {
        self.entries
            .iter()
            .find(|entry| entry.identifier() == identifier)
    }

    /// Whether a path is already represented in the combined dashboard.
    #[must_use]
    pub fn contains_path(&self, path: &Path) -> bool {
        self.entries.iter().any(|entry| entry.rom() == path)
    }
}

/// Combined dashboard catalog invariant failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DashboardCatalogError {
    /// A dashboard cannot navigate an empty catalog.
    Empty,
    /// Combined catalogs exceed the fixed touch-target capacity.
    TooManyEntries,
    /// Two sources use the same stable identifier.
    DuplicateIdentifier,
    /// Two sources refer to the same launch path.
    DuplicatePath,
}

impl fmt::Display for DashboardCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("dashboard catalog is empty"),
            Self::TooManyEntries => {
                write!(
                    formatter,
                    "dashboard catalog exceeds {MAXIMUM_DASHBOARD_ENTRIES} entries"
                )
            }
            Self::DuplicateIdentifier => {
                formatter.write_str("dashboard catalog repeats an identifier")
            }
            Self::DuplicatePath => formatter.write_str("dashboard catalog repeats a launch path"),
        }
    }
}

impl std::error::Error for DashboardCatalogError {}

/// Decode the ordered startup application rows returned by Common Lisp.
///
/// Lisp controls presence, order, title, and color. Rust maps each closed kind
/// to a stable identity and validates the resulting catalog entry; policy can
/// never supply an executable or arbitrary application path.
///
/// # Errors
///
/// Returns [`DashboardApplicationPolicyError`] for malformed, excessive,
/// duplicate, unknown, or schema-invalid rows.
#[cfg(feature = "bmc-native")]
pub fn applications_from_policy(
    value: &Value,
) -> Result<Vec<CatalogEntry>, DashboardApplicationPolicyError> {
    let Value::List(rows) = value else {
        return Err(DashboardApplicationPolicyError::InvalidShape);
    };
    if rows.len() > MAXIMUM_POLICY_APPLICATIONS {
        return Err(DashboardApplicationPolicyError::TooManyApplications);
    }
    let mut kinds = HashSet::new();
    let mut applications = Vec::new();
    applications
        .try_reserve_exact(rows.len())
        .map_err(|_| DashboardApplicationPolicyError::Allocation)?;
    for row in rows {
        let Value::List(fields) = row else {
            return Err(DashboardApplicationPolicyError::InvalidShape);
        };
        let [
            Value::Keyword(kind),
            Value::String(title),
            Value::String(color),
        ] = fields.as_slice()
        else {
            return Err(DashboardApplicationPolicyError::InvalidShape);
        };
        if !kinds.insert(kind.as_str()) {
            return Err(DashboardApplicationPolicyError::DuplicateApplication);
        }
        let (identifier, path) = application_identity(kind)
            .ok_or_else(|| DashboardApplicationPolicyError::UnknownApplication(kind.clone()))?;
        applications.push(
            CatalogEntry::new(identifier, title, CatalogSystem::Deck, path, color)
                .map_err(DashboardApplicationPolicyError::InvalidEntry)?,
        );
    }
    Ok(applications)
}

#[cfg(feature = "bmc-native")]
fn application_identity(kind: &str) -> Option<(&'static str, &'static str)> {
    match kind {
        "lua" => Some(("lua-repl", "/mnt/data/nes-deck/games/lua-repl")),
        "lisp" => Some(("lisp-repl", "/mnt/data/nes-deck/games/lisp-repl")),
        "python" => Some(("python-repl", "/mnt/data/nes-deck/games/python-repl")),
        "scheme" => Some(("scheme-repl", "/mnt/data/nes-deck/games/scheme-repl")),
        "chiptunes" => Some(("chiptunes", "/mnt/data/nes-deck/games/chiptunes")),
        "terminal" => Some(("terminal", "/mnt/data/nes-deck/games/terminal")),
        "reboot" => Some(("reboot", "/mnt/data/nes-deck/games/reboot")),
        _ => None,
    }
}

/// A Common Lisp dashboard application result violated its closed schema.
#[cfg(feature = "bmc-native")]
#[derive(Debug)]
pub enum DashboardApplicationPolicyError {
    /// The outer value or one row had the wrong positional shape or types.
    InvalidShape,
    /// More rows were returned than the closed application set contains.
    TooManyApplications,
    /// One application kind occurred more than once.
    DuplicateApplication,
    /// Policy named a kind outside the closed Rust launch set.
    UnknownApplication(String),
    /// Reserving the small validated result failed.
    Allocation,
    /// A title or color violated the shared catalog schema.
    InvalidEntry(CatalogError),
}

#[cfg(feature = "bmc-native")]
impl fmt::Display for DashboardApplicationPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidShape => {
                formatter.write_str("dashboard applications must be (kind title color) rows")
            }
            Self::TooManyApplications => formatter.write_str("too many dashboard applications"),
            Self::DuplicateApplication => {
                formatter.write_str("dashboard application kind is repeated")
            }
            Self::UnknownApplication(kind) => {
                write!(formatter, "unknown dashboard application kind :{kind}")
            }
            Self::Allocation => formatter.write_str("cannot allocate dashboard applications"),
            Self::InvalidEntry(source) => {
                write!(formatter, "invalid dashboard application: {source}")
            }
        }
    }
}

#[cfg(feature = "bmc-native")]
impl std::error::Error for DashboardApplicationPolicyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidEntry(source) => Some(source),
            Self::InvalidShape
            | Self::TooManyApplications
            | Self::DuplicateApplication
            | Self::UnknownApplication(_)
            | Self::Allocation => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DashboardCatalog, DashboardCatalogError};
    use retro_deck_config::{Catalog, CatalogEntry, CatalogSystem};

    const DEPLOYED_CATALOG: &[u8] = include_bytes!("../../../deploy/menu/games.tsv");

    fn deployed() -> Option<Catalog> {
        Catalog::parse(DEPLOYED_CATALOG).ok()
    }

    fn native(identifier: &str, path: &str) -> Option<CatalogEntry> {
        CatalogEntry::new(identifier, "NATIVE", CatalogSystem::Deck, path, "#5F87D7").ok()
    }

    #[test]
    fn deployed_catalog_has_fixed_labels_order_and_counts() {
        let Some(catalog) = deployed() else {
            return;
        };
        let dashboard = DashboardCatalog::from_catalog(&catalog);
        assert!(dashboard.is_ok());
        let Some(dashboard) = dashboard.ok() else {
            return;
        };
        assert_eq!(
            dashboard
                .categories()
                .iter()
                .map(|category| (category.label(), category.len()))
                .collect::<Vec<_>>(),
            [
                ("NES", 5),
                ("GAME BOY", 3),
                ("GBC", 2),
                ("ZX SPECTRUM", 2),
                ("CHIP-8", 2),
                ("DECK", 1),
            ]
        );
        assert_eq!(dashboard.entries().len(), 15);
        assert!(dashboard.find("mario").is_some());
    }

    #[test]
    fn generated_native_entries_join_deck_without_reordering_sources() {
        let Some(catalog) = deployed() else {
            return;
        };
        let Some(terminal) = native("terminal", "/mnt/data/nes-deck/games/terminal") else {
            return;
        };
        let Some(chiptunes) = native("chiptunes", "/mnt/data/nes-deck/games/chiptunes") else {
            return;
        };
        let entries = catalog
            .entries()
            .iter()
            .cloned()
            .chain([terminal, chiptunes]);
        let dashboard = DashboardCatalog::from_entries(entries);
        assert!(dashboard.is_ok());
        let Some(dashboard) = dashboard.ok() else {
            return;
        };
        let Some(deck) = dashboard.categories().last() else {
            return;
        };
        assert_eq!(deck.label(), "DECK");
        assert_eq!(deck.len(), 3);
        assert_eq!(
            deck.entry_indices()
                .iter()
                .filter_map(|index| dashboard.entry(*index))
                .map(CatalogEntry::identifier)
                .collect::<Vec<_>>(),
            ["ten-seconds", "terminal", "chiptunes"]
        );
    }

    #[test]
    fn source_combination_rechecks_global_identity_and_path_bounds() {
        let Some(first) = native("terminal", "/mnt/data/nes-deck/games/terminal") else {
            return;
        };
        assert_eq!(
            DashboardCatalog::from_entries([first.clone(), first.clone()]),
            Err(DashboardCatalogError::DuplicateIdentifier)
        );
        let Some(same_path) = native("shell", "/mnt/data/nes-deck/games/terminal") else {
            return;
        };
        assert_eq!(
            DashboardCatalog::from_entries([first, same_path]),
            Err(DashboardCatalogError::DuplicatePath)
        );
        assert_eq!(
            DashboardCatalog::from_entries(Vec::<CatalogEntry>::new()),
            Err(DashboardCatalogError::Empty)
        );
    }

    #[cfg(feature = "bmc-native")]
    #[test]
    fn lisp_policy_controls_only_order_title_and_color() {
        use super::{DashboardApplicationPolicyError, applications_from_policy};
        use retro_deck_policy::Value;

        let row = |kind: &str, title: &str, color: &str| {
            Value::List(vec![
                Value::Keyword(kind.to_owned()),
                Value::String(title.to_owned()),
                Value::String(color.to_owned()),
            ])
        };
        let value = Value::List(vec![
            row("terminal", "MY SHELL", "#5F87AF"),
            row("lisp", "PATCHABLE LISP", "#AFD75F"),
        ]);
        let applications = applications_from_policy(&value);
        let Some(applications) = applications.ok() else {
            return;
        };
        assert_eq!(
            applications
                .iter()
                .map(|entry| (entry.identifier(), entry.title(), entry.rom()))
                .collect::<Vec<_>>(),
            [
                (
                    "terminal",
                    "MY SHELL",
                    std::path::Path::new("/mnt/data/nes-deck/games/terminal")
                ),
                (
                    "lisp-repl",
                    "PATCHABLE LISP",
                    std::path::Path::new("/mnt/data/nes-deck/games/lisp-repl")
                ),
            ]
        );
        assert!(matches!(
            applications_from_policy(&Value::List(vec![row("unknown", "UNKNOWN", "#5F87AF")])),
            Err(DashboardApplicationPolicyError::UnknownApplication(_))
        ));
        assert!(matches!(
            applications_from_policy(&Value::List(vec![
                row("terminal", "ONE", "#5F87AF"),
                row("terminal", "TWO", "#5F87AF"),
            ])),
            Err(DashboardApplicationPolicyError::DuplicateApplication)
        ));
    }
}
