//! Constrained wire contract between the dashboard and its trusted launcher.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use retro_deck_config::System;

use crate::{Keymap, LaunchPlan, LaunchTarget, TerminalMode, VolumeState};

/// Logical BMC application registered by the native Retro Deck package.
pub const BMC_APPLICATION_ID: &str = "retro-deck";
/// Stricter bound than BMC's generic application-input transport limit.
pub const MAXIMUM_APPLICATION_INPUT_BYTES: usize = 8 * 1_024;

/// Closed product intent accepted by the trusted Retro Deck launcher.
///
/// BMC chooses the executable. This message carries no program path, arbitrary
/// arguments, environment, or reboot capability.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApplicationRequest {
    /// Start one ROM in its matching emulator.
    Emulator {
        /// Emulator family.
        system: System,
        /// Canonical Deck ROM path.
        content: PathBuf,
        /// Initial emulator volume.
        volume_percent: u8,
    },
    /// Start the native 10 Seconds game.
    TenSeconds {
        /// Initial game volume.
        volume_percent: u8,
    },
    /// Open the terminal or one language REPL.
    Terminal {
        /// Fixed terminal mode.
        mode: TerminalMode,
        /// Installed keyboard layout.
        keymap: Keymap,
    },
    /// Open the native chiptune player.
    Chiptunes {
        /// Initial player volume.
        volume_percent: u8,
    },
}

impl ApplicationRequest {
    /// Convert one validated dashboard target to owned wire data.
    ///
    /// # Errors
    ///
    /// Reboot must use a separately authorized BMC system action.
    pub fn from_target(
        target: LaunchTarget<'_>,
        volume: VolumeState,
        keymap: Keymap,
    ) -> Result<Self, ApplicationRequestError> {
        let volume_percent = volume.percent();
        let request = match target {
            LaunchTarget::Emulator { system, content } => Self::Emulator {
                system,
                content: content.to_path_buf(),
                volume_percent,
            },
            LaunchTarget::TenSeconds => Self::TenSeconds { volume_percent },
            LaunchTarget::Terminal(mode) => Self::Terminal { mode, keymap },
            LaunchTarget::Chiptunes => Self::Chiptunes { volume_percent },
            LaunchTarget::Reboot => return Err(ApplicationRequestError::SystemActionRequired),
        };
        request.validate()?;
        Ok(request)
    }

    /// Validate untrusted wire data without performing external work.
    ///
    /// # Errors
    ///
    /// Rejects excessive volume and ROM paths outside their canonical system
    /// directory or with the wrong extension.
    pub fn validate(&self) -> Result<(), ApplicationRequestError> {
        match self {
            Self::Emulator {
                system,
                content,
                volume_percent,
            } => {
                validate_volume(*volume_percent)?;
                validate_content_path(*system, content)
            }
            Self::TenSeconds { volume_percent } | Self::Chiptunes { volume_percent } => {
                validate_volume(*volume_percent)
            }
            Self::Terminal { .. } => Ok(()),
        }
    }

    /// Turn a validated request into a fixed executable plan.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationRequestError`] for invalid decoded fields.
    pub fn launch_plan(&self) -> Result<LaunchPlan<'_>, ApplicationRequestError> {
        self.validate()?;
        let (target, volume_percent, keymap) = match self {
            Self::Emulator {
                system,
                content,
                volume_percent,
            } => (
                LaunchTarget::Emulator {
                    system: *system,
                    content,
                },
                *volume_percent,
                Keymap::Us,
            ),
            Self::TenSeconds { volume_percent } => {
                (LaunchTarget::TenSeconds, *volume_percent, Keymap::Us)
            }
            Self::Terminal { mode, keymap } => (LaunchTarget::Terminal(*mode), 0, *keymap),
            Self::Chiptunes { volume_percent } => {
                (LaunchTarget::Chiptunes, *volume_percent, Keymap::Us)
            }
        };
        let volume =
            VolumeState::new(volume_percent).map_err(|_| ApplicationRequestError::InvalidVolume)?;
        LaunchPlan::from_target(target, volume, keymap)
            .map_err(|_| ApplicationRequestError::SystemActionRequired)
    }

    /// ROM path that must resolve to a regular file before execution.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "PathBuf deref coercion is not const on the pinned Rust toolchain"
    )]
    pub fn content_path(&self) -> Option<&Path> {
        match self {
            Self::Emulator { content, .. } => Some(content),
            Self::TenSeconds { .. } | Self::Terminal { .. } | Self::Chiptunes { .. } => None,
        }
    }
}

/// A native application request violates the closed launch contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationRequestError {
    /// Reboot belongs to a separately authorized BMC system interface.
    SystemActionRequired,
    /// Volume must be between zero and 100 percent.
    InvalidVolume,
    /// ROM content must be directly below its canonical system directory.
    InvalidContentPath,
}

impl fmt::Display for ApplicationRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SystemActionRequired => {
                formatter.write_str("request requires an authorized BMC system action")
            }
            Self::InvalidVolume => formatter.write_str("volume must be between 0 and 100"),
            Self::InvalidContentPath => formatter
                .write_str("ROM must be directly below its matching /mnt/data/roms directory"),
        }
    }
}

impl std::error::Error for ApplicationRequestError {}

const fn validate_volume(volume_percent: u8) -> Result<(), ApplicationRequestError> {
    if volume_percent <= 100 {
        Ok(())
    } else {
        Err(ApplicationRequestError::InvalidVolume)
    }
}

fn validate_content_path(system: System, content: &Path) -> Result<(), ApplicationRequestError> {
    let directory = Path::new("/mnt/data/roms").join(system.as_str());
    let extension = OsStr::new(system.extension().trim_start_matches('.'));
    if content.parent() != Some(directory.as_path())
        || content.file_name().is_none()
        || content.extension() != Some(extension)
    {
        return Err(ApplicationRequestError::InvalidContentPath);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn requests_round_trip_to_fixed_plans() {
        let target = LaunchTarget::Emulator {
            system: System::Nes,
            content: Path::new("/mnt/data/roms/nes/super-mario-bros.nes"),
        };
        let request = ApplicationRequest::from_target(
            target,
            VolumeState::new(55).unwrap_or(VolumeState::DEFAULT),
            Keymap::Czech,
        );
        let Some(request) = request.ok() else {
            return;
        };
        let encoded = serde_json::to_string(&request);
        let Some(encoded) = encoded.ok() else {
            return;
        };
        assert!(encoded.len() < MAXIMUM_APPLICATION_INPUT_BYTES);
        assert!(encoded.contains(r#""system":"nes""#));
        let decoded = serde_json::from_str::<ApplicationRequest>(&encoded);
        let Some(decoded) = decoded.ok() else {
            return;
        };
        assert_eq!(decoded, request);
        let Some(plan) = decoded.launch_plan().ok() else {
            return;
        };
        assert_eq!(plan.program(), Path::new("/mnt/data/nes-deck/nes-deck"));
        assert_eq!(
            plan.argument(),
            Some(OsStr::new("/mnt/data/roms/nes/super-mario-bros.nes"))
        );
        assert_eq!(plan.volume_percent(), Some(55));
    }

    #[test]
    fn requests_reject_open_ended_effects() {
        for content in [
            "/tmp/game.nes",
            "/mnt/data/roms/gb/game.nes",
            "/mnt/data/roms/nes/subdirectory/game.nes",
            "/mnt/data/roms/nes/game.NES",
        ] {
            let request = ApplicationRequest::Emulator {
                system: System::Nes,
                content: PathBuf::from(content),
                volume_percent: 42,
            };
            assert_eq!(
                request.validate(),
                Err(ApplicationRequestError::InvalidContentPath)
            );
        }
        assert_eq!(
            ApplicationRequest::TenSeconds {
                volume_percent: 101
            }
            .validate(),
            Err(ApplicationRequestError::InvalidVolume)
        );
        assert_eq!(
            ApplicationRequest::from_target(LaunchTarget::Reboot, VolumeState::DEFAULT, Keymap::Us,),
            Err(ApplicationRequestError::SystemActionRequired)
        );
        let open_ended = r#"{
            "kind":"ten_seconds",
            "volume_percent":42,
            "executable":"/bin/sh"
        }"#;
        assert!(serde_json::from_str::<ApplicationRequest>(open_ended).is_err());
    }
}
