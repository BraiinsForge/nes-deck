//! Typed launch meaning kept separate from catalog display identity.

use std::fmt;
use std::path::Path;

use retro_deck_config::{CatalogEntry, CatalogSystem, System};

/// Terminal or REPL mode accepted by the managed terminal launcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalMode {
    /// Login shell.
    Shell,
    /// Lua read-eval-print loop.
    Lua,
    /// Common Lisp read-eval-print loop.
    Lisp,
    /// Python read-eval-print loop.
    Python,
    /// Scheme read-eval-print loop.
    Scheme,
}

impl TerminalMode {
    /// Stable launcher argument.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Lua => "lua",
            Self::Lisp => "lisp",
            Self::Python => "python",
            Self::Scheme => "scheme",
        }
    }
}

/// Validated meaning of one selected catalog entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchTarget<'entry> {
    /// Console content opened by the Rust libretro or CHIP-8 host.
    Emulator {
        /// Emulator family selected from typed catalog identity.
        system: System,
        /// Validated absolute content path.
        content: &'entry Path,
    },
    /// Native 10 Seconds game.
    TenSeconds,
    /// Managed terminal or language REPL.
    Terminal(TerminalMode),
    /// Native chiptune player.
    Chiptunes,
    /// Explicitly confirmed system reboot.
    Reboot,
}

impl<'entry> LaunchTarget<'entry> {
    /// Resolve display identity to a closed launch variant.
    ///
    /// Executable paths are deliberately absent. The runtime supplies those
    /// from trusted setup configuration after this pure classification.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchTargetError`] for an unknown Deck application.
    pub fn from_entry(entry: &'entry CatalogEntry) -> Result<Self, LaunchTargetError> {
        match entry.system() {
            CatalogSystem::Rom(system) => Ok(Self::Emulator {
                system,
                content: entry.rom(),
            }),
            CatalogSystem::Deck => match entry.identifier() {
                "ten-seconds" => Ok(Self::TenSeconds),
                "lua-repl" => Ok(Self::Terminal(TerminalMode::Lua)),
                "lisp-repl" => Ok(Self::Terminal(TerminalMode::Lisp)),
                "python-repl" => Ok(Self::Terminal(TerminalMode::Python)),
                "scheme-repl" => Ok(Self::Terminal(TerminalMode::Scheme)),
                "chiptunes" => Ok(Self::Chiptunes),
                "terminal" => Ok(Self::Terminal(TerminalMode::Shell)),
                "reboot" => Ok(Self::Reboot),
                _ => Err(LaunchTargetError),
            },
        }
    }
}

/// A Deck catalog entry has no compiled launch meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchTargetError;

impl fmt::Display for LaunchTargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown native dashboard application")
    }
}

impl std::error::Error for LaunchTargetError {}

#[cfg(test)]
mod tests {
    use retro_deck_config::{Catalog, CatalogEntry, CatalogSystem, System};

    use super::{LaunchTarget, LaunchTargetError, TerminalMode};
    use crate::DashboardCatalog;

    const DEPLOYED_CATALOG: &[u8] = include_bytes!("../../../deploy/menu/games.tsv");

    #[test]
    fn every_standard_entry_has_one_closed_launch_meaning() {
        let Some(catalog) = Catalog::parse(DEPLOYED_CATALOG).ok() else {
            return;
        };
        let Some(dashboard) = DashboardCatalog::with_standard_apps(&catalog).ok() else {
            return;
        };
        let targets = dashboard
            .entries()
            .iter()
            .map(LaunchTarget::from_entry)
            .collect::<Result<Vec<_>, _>>();
        assert!(targets.is_ok());
        let Some(targets) = targets.ok() else {
            return;
        };
        assert!(matches!(
            targets.first(),
            Some(LaunchTarget::Emulator {
                system: System::Nes,
                content,
            }) if content.ends_with("super-mario-bros.nes")
        ));
        assert_eq!(
            targets.iter().copied().skip(15).collect::<Vec<_>>(),
            [
                LaunchTarget::Terminal(TerminalMode::Lua),
                LaunchTarget::Terminal(TerminalMode::Lisp),
                LaunchTarget::Terminal(TerminalMode::Python),
                LaunchTarget::Terminal(TerminalMode::Scheme),
                LaunchTarget::Chiptunes,
                LaunchTarget::Terminal(TerminalMode::Shell),
                LaunchTarget::Reboot,
            ]
        );
    }

    #[test]
    fn unknown_native_entries_fail_closed() {
        let unknown = CatalogEntry::new(
            "unknown",
            "UNKNOWN",
            CatalogSystem::Deck,
            "/mnt/data/nes-deck/games/unknown",
            "#5F87D7",
        );
        let Some(unknown) = unknown.ok() else {
            return;
        };
        assert_eq!(LaunchTarget::from_entry(&unknown), Err(LaunchTargetError));
        assert_eq!(TerminalMode::Shell.as_str(), "shell");
        assert_eq!(TerminalMode::Scheme.as_str(), "scheme");
    }
}
