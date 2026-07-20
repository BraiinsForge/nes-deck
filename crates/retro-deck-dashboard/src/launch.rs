//! Typed launch meaning kept separate from catalog display identity.

use std::ffi::OsStr;
use std::fmt;
use std::path::Path;

use retro_deck_config::{CatalogEntry, CatalogSystem, System};

use crate::{Keymap, VolumeState};

const NES_PROGRAM: &str = "/mnt/data/nes-deck/nes-deck";
const GAME_BOY_PROGRAM: &str = "/mnt/data/nes-deck/gb-deck";
const ZX_PROGRAM: &str = "/mnt/data/nes-deck/zx-deck";
const CHIP8_PROGRAM: &str = "/mnt/data/nes-deck/chip8-deck";
const TEN_SECONDS_PROGRAM: &str = "/mnt/data/nes-deck/ten-seconds-deck";
const CHIPTUNE_PROGRAM: &str = "/mnt/data/nes-deck/chiptune-deck";
const CHIPTUNE_DIRECTORY: &str = "/mnt/data/chiptunes";
const TERMINAL_PROGRAM: &str = "/mnt/data/nes-deck/terminal/retro-terminal";
const VOLUME_STATE: &str = "/mnt/data/nes-deck/state/menu-volume.state";
const REBOOT_PROGRAM: &str = "/sbin/reboot";

/// Terminal or REPL mode accepted by the managed terminal launcher.
#[cfg_attr(
    feature = "application-wire",
    derive(serde::Deserialize, serde::Serialize)
)]
#[cfg_attr(feature = "application-wire", serde(rename_all = "snake_case"))]
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
    /// System reboot request that still requires explicit confirmation.
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

/// Which process owns touchscreen interaction while a child is active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitPolicy {
    /// The dashboard supervisor owns touch and recognizes a hold-to-exit gesture.
    SupervisorTouchHold,
    /// The native child owns touch and exposes its own exit control.
    ChildOwnsTouch,
    /// No interactive exit path applies, as for an explicitly confirmed reboot.
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LaunchArgument<'entry> {
    Path(&'entry Path),
    Text(&'static str),
}

impl LaunchArgument<'_> {
    fn as_os_str(&self) -> &OsStr {
        match self {
            Self::Path(path) => path.as_os_str(),
            Self::Text(text) => OsStr::new(text),
        }
    }
}

/// Fixed executable and bounded runtime settings for one managed child.
///
/// Catalog data can supply only the content argument. Executable paths and
/// environment meanings are compiled here and cannot be redirected by a
/// catalog row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchPlan<'entry> {
    program: &'static str,
    argument: Option<LaunchArgument<'entry>>,
    volume_percent: Option<u8>,
    keymap: Option<Keymap>,
    exit_hint: bool,
    shares_volume_state: bool,
    exit_policy: ExitPolicy,
}

impl<'entry> LaunchPlan<'entry> {
    /// Build a managed-child plan for a non-reboot target.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchPlanError::RebootConfirmationRequired`] rather than
    /// turning an unconfirmed catalog activation into a reboot command.
    pub fn from_target(
        target: LaunchTarget<'entry>,
        volume: VolumeState,
        keymap: Keymap,
    ) -> Result<Self, LaunchPlanError> {
        let volume_percent = volume.percent();
        match target {
            LaunchTarget::Emulator { system, content } => {
                let program = match system {
                    System::Nes => NES_PROGRAM,
                    System::GameBoy | System::GameBoyColor => GAME_BOY_PROGRAM,
                    System::ZxSpectrum => ZX_PROGRAM,
                    System::Chip8 => CHIP8_PROGRAM,
                };
                Ok(Self {
                    program,
                    argument: Some(LaunchArgument::Path(content)),
                    volume_percent: Some(volume_percent),
                    keymap: None,
                    exit_hint: true,
                    shares_volume_state: false,
                    exit_policy: ExitPolicy::SupervisorTouchHold,
                })
            }
            LaunchTarget::TenSeconds => Ok(Self {
                program: TEN_SECONDS_PROGRAM,
                argument: None,
                volume_percent: Some(volume_percent),
                keymap: None,
                exit_hint: false,
                shares_volume_state: false,
                exit_policy: ExitPolicy::ChildOwnsTouch,
            }),
            LaunchTarget::Terminal(mode) => Ok(Self {
                program: TERMINAL_PROGRAM,
                argument: Some(LaunchArgument::Text(mode.as_str())),
                volume_percent: None,
                keymap: Some(keymap),
                exit_hint: false,
                shares_volume_state: false,
                exit_policy: ExitPolicy::SupervisorTouchHold,
            }),
            LaunchTarget::Chiptunes => Ok(Self {
                program: CHIPTUNE_PROGRAM,
                argument: Some(LaunchArgument::Path(Path::new(CHIPTUNE_DIRECTORY))),
                volume_percent: Some(volume_percent),
                keymap: None,
                exit_hint: false,
                shares_volume_state: true,
                exit_policy: ExitPolicy::ChildOwnsTouch,
            }),
            LaunchTarget::Reboot => Err(LaunchPlanError::RebootConfirmationRequired),
        }
    }

    /// Build the reboot command only after the caller has confirmed it.
    #[must_use]
    pub const fn confirmed_reboot() -> Self {
        Self {
            program: REBOOT_PROGRAM,
            argument: None,
            volume_percent: None,
            keymap: None,
            exit_hint: false,
            shares_volume_state: false,
            exit_policy: ExitPolicy::None,
        }
    }

    /// Trusted absolute executable path.
    #[must_use]
    pub fn program(self) -> &'static Path {
        Path::new(self.program)
    }

    /// Optional single argument, either validated content or fixed application data.
    #[must_use]
    pub fn argument(&self) -> Option<&OsStr> {
        self.argument.as_ref().map(LaunchArgument::as_os_str)
    }

    /// Initial game or application volume supplied through the managed environment.
    #[must_use]
    pub const fn volume_percent(self) -> Option<u8> {
        self.volume_percent
    }

    /// Terminal keymap supplied through the managed environment.
    #[must_use]
    pub const fn keymap(self) -> Option<Keymap> {
        self.keymap
    }

    /// Whether the gameplay surface should render the hold-to-exit hint.
    #[must_use]
    pub const fn exit_hint(self) -> bool {
        self.exit_hint
    }

    /// Shared menu volume state used by the chiptune player.
    #[must_use]
    pub fn volume_state(self) -> Option<&'static Path> {
        self.shares_volume_state.then(|| Path::new(VOLUME_STATE))
    }

    /// Touch ownership and exit behavior for process supervision.
    #[must_use]
    pub const fn exit_policy(self) -> ExitPolicy {
        self.exit_policy
    }
}

/// A launch target cannot yet be turned into an executable plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchPlanError {
    /// Reboot needs a second activation within the confirmation window.
    RebootConfirmationRequired,
}

impl fmt::Display for LaunchPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RebootConfirmationRequired => {
                formatter.write_str("reboot requires explicit confirmation")
            }
        }
    }
}

impl std::error::Error for LaunchPlanError {}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use retro_deck_config::{Catalog, CatalogEntry, CatalogSystem, System};

    use super::{
        ExitPolicy, LaunchPlan, LaunchPlanError, LaunchTarget, LaunchTargetError, TerminalMode,
    };
    use crate::{DashboardCatalog, Keymap, VolumeState};

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

    #[test]
    fn console_plans_keep_executables_out_of_catalog_data() {
        let Some(volume) = VolumeState::new(55, 55).ok() else {
            return;
        };
        for (system, program, content) in [
            (
                System::Nes,
                "/mnt/data/nes-deck/nes-deck",
                "/mnt/data/roms/nes/a.nes",
            ),
            (
                System::GameBoy,
                "/mnt/data/nes-deck/gb-deck",
                "/mnt/data/roms/gb/a.gb",
            ),
            (
                System::GameBoyColor,
                "/mnt/data/nes-deck/gb-deck",
                "/mnt/data/roms/gbc/a.gbc",
            ),
            (
                System::ZxSpectrum,
                "/mnt/data/nes-deck/zx-deck",
                "/mnt/data/roms/zx/a.tap",
            ),
            (
                System::Chip8,
                "/mnt/data/nes-deck/chip8-deck",
                "/mnt/data/roms/chip8/a.ch8",
            ),
        ] {
            let target = LaunchTarget::Emulator {
                system,
                content: Path::new(content),
            };
            let Some(plan) = LaunchPlan::from_target(target, volume, Keymap::Czech).ok() else {
                return;
            };
            assert_eq!(plan.program(), Path::new(program));
            assert_eq!(plan.argument(), Some(OsStr::new(content)));
            assert_eq!(plan.volume_percent(), Some(55));
            assert_eq!(plan.keymap(), None);
            assert!(plan.exit_hint());
            assert_eq!(plan.volume_state(), None);
            assert_eq!(plan.exit_policy(), ExitPolicy::SupervisorTouchHold);
        }
    }

    #[test]
    fn native_application_plans_have_narrow_distinct_effects() {
        let Some(volume) = VolumeState::new(42, 42).ok() else {
            return;
        };
        let Some(timer) =
            LaunchPlan::from_target(LaunchTarget::TenSeconds, volume, Keymap::Us).ok()
        else {
            return;
        };
        assert_eq!(
            timer.program(),
            Path::new("/mnt/data/nes-deck/ten-seconds-deck")
        );
        assert_eq!(timer.argument(), None);
        assert_eq!(timer.volume_percent(), Some(42));
        assert_eq!(timer.exit_policy(), ExitPolicy::ChildOwnsTouch);

        let Some(chiptunes) =
            LaunchPlan::from_target(LaunchTarget::Chiptunes, volume, Keymap::Us).ok()
        else {
            return;
        };
        assert_eq!(
            chiptunes.program(),
            Path::new("/mnt/data/nes-deck/chiptune-deck")
        );
        assert_eq!(
            chiptunes.argument(),
            Some(OsStr::new("/mnt/data/chiptunes"))
        );
        assert_eq!(
            chiptunes.volume_state(),
            Some(Path::new("/mnt/data/nes-deck/state/menu-volume.state"))
        );
        assert_eq!(chiptunes.exit_policy(), ExitPolicy::ChildOwnsTouch);

        let Some(terminal) = LaunchPlan::from_target(
            LaunchTarget::Terminal(TerminalMode::Lisp),
            volume,
            Keymap::Czech,
        )
        .ok() else {
            return;
        };
        assert_eq!(
            terminal.program(),
            Path::new("/mnt/data/nes-deck/terminal/retro-terminal")
        );
        assert_eq!(terminal.argument(), Some(OsStr::new("lisp")));
        assert_eq!(terminal.volume_percent(), None);
        assert_eq!(terminal.keymap(), Some(Keymap::Czech));
        assert_eq!(terminal.exit_policy(), ExitPolicy::SupervisorTouchHold);
    }

    #[test]
    fn reboot_plan_cannot_follow_one_catalog_activation() {
        assert_eq!(
            LaunchPlan::from_target(LaunchTarget::Reboot, VolumeState::DEFAULT, Keymap::Us),
            Err(LaunchPlanError::RebootConfirmationRequired)
        );
        let reboot = LaunchPlan::confirmed_reboot();
        assert_eq!(reboot.program(), Path::new("/sbin/reboot"));
        assert_eq!(reboot.argument(), None);
        assert_eq!(reboot.volume_percent(), None);
        assert_eq!(reboot.keymap(), None);
        assert_eq!(reboot.exit_policy(), ExitPolicy::None);
    }
}
