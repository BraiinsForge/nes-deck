//! Validated options passed to managed Retro Deck applications.

use std::fmt;

/// Compiled audible level used until BMC exposes global volume to widgets.
pub const DEFAULT_VOLUME_PERCENT: u8 = 42;

/// Terminal keymap selected for the managed terminal application.
#[cfg_attr(
    feature = "application-wire",
    derive(serde::Deserialize, serde::Serialize)
)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Keymap {
    /// US ANSI key positions.
    #[cfg_attr(feature = "application-wire", serde(rename = "us"))]
    #[default]
    Us,
    /// Czech key positions.
    #[cfg_attr(feature = "application-wire", serde(rename = "cz"))]
    Czech,
}

impl Keymap {
    /// Toggle between the installed layouts.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Us => Self::Czech,
            Self::Czech => Self::Us,
        }
    }

    /// Persistent environment value understood by the terminal launcher.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Us => "us",
            Self::Czech => "cz",
        }
    }
}

/// Validated launch volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeState(u8);

impl VolumeState {
    /// Compiled startup value.
    pub const DEFAULT: Self = Self(DEFAULT_VOLUME_PERCENT);

    /// Validate one percentage.
    ///
    /// # Errors
    ///
    /// Returns [`VolumeError`] when `percent` exceeds 100.
    pub const fn new(percent: u8) -> Result<Self, VolumeError> {
        if percent > 100 {
            Err(VolumeError)
        } else {
            Ok(Self(percent))
        }
    }

    /// Validated percentage.
    #[must_use]
    pub const fn percent(self) -> u8 {
        self.0
    }

    /// Whether finite menu cues should remain silent.
    #[must_use]
    pub const fn is_muted(self) -> bool {
        self.0 == 0
    }
}

/// A launch volume is outside zero through 100 percent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeError;

impl fmt::Display for VolumeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("volume must be 0 through 100")
    }
}

impl std::error::Error for VolumeError {}

#[cfg(test)]
mod tests {
    use super::{Keymap, VolumeState};

    #[test]
    fn volume_accepts_mute_and_rejects_excess() {
        assert_eq!(VolumeState::new(0).map(VolumeState::percent), Ok(0));
        assert_eq!(VolumeState::new(100).map(VolumeState::percent), Ok(100));
        assert!(VolumeState::new(101).is_err());
    }

    #[test]
    fn keymaps_have_closed_environment_values() {
        assert_eq!(Keymap::Us.as_str(), "us");
        assert_eq!(Keymap::Us.toggled(), Keymap::Czech);
        assert_eq!(Keymap::Czech.toggled(), Keymap::Us);
    }
}
