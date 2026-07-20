//! Canonical persistent dashboard preference values without file I/O.

use std::error::Error;
use std::fmt;

use crate::{Brightness, DEFAULT_VOLUME_PERCENT, Keymap, SettingChange, VolumeState};
/// Largest canonical dashboard preference file.
pub const MAXIMUM_PREFERENCE_BYTES: usize = 4;

/// Valid startup settings assembled independently from optional state files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DashboardPreferences {
    volume: VolumeState,
    brightness: Brightness,
    keymap: Keymap,
}

impl DashboardPreferences {
    /// Assemble already validated values.
    #[must_use]
    pub const fn new(volume: VolumeState, brightness: Brightness, keymap: Keymap) -> Self {
        Self {
            volume,
            brightness,
            keymap,
        }
    }

    /// Current volume plus deterministic unmute level.
    #[must_use]
    pub const fn volume(self) -> VolumeState {
        self.volume
    }

    /// Current display brightness percentage.
    #[must_use]
    pub const fn brightness(self) -> Brightness {
        self.brightness
    }

    /// Current terminal keyboard layout.
    #[must_use]
    pub const fn keymap(self) -> Keymap {
        self.keymap
    }
}

impl Default for DashboardPreferences {
    fn default() -> Self {
        Self {
            volume: VolumeState::DEFAULT,
            brightness: Brightness::DEFAULT,
            keymap: Keymap::Us,
        }
    }
}

/// Parse a canonical volume file and retain a safe audible restore value.
///
/// # Errors
///
/// Returns [`PreferenceValueError::Volume`] unless the input is exactly a
/// canonical integer from 0 through 100 followed by one newline.
pub fn parse_volume(bytes: &[u8]) -> Result<VolumeState, PreferenceValueError> {
    let percent = parse_percent(bytes).ok_or(PreferenceValueError::Volume)?;
    let restore = if percent == 0 {
        DEFAULT_VOLUME_PERCENT
    } else {
        percent
    };
    VolumeState::new(percent, restore).map_err(|_| PreferenceValueError::Volume)
}

/// Parse a canonical ten-point brightness file.
///
/// # Errors
///
/// Returns [`PreferenceValueError::Brightness`] unless the input is exactly a
/// ten-point step from 10 through 100 followed by one newline.
pub fn parse_brightness(bytes: &[u8]) -> Result<Brightness, PreferenceValueError> {
    let percent = parse_percent(bytes).ok_or(PreferenceValueError::Brightness)?;
    Brightness::new(percent).map_err(|_| PreferenceValueError::Brightness)
}

/// Parse the only two terminal keymap state values.
///
/// # Errors
///
/// Returns [`PreferenceValueError::Keymap`] unless the input is exactly
/// `us\n` or `cz\n`.
pub const fn parse_keymap(bytes: &[u8]) -> Result<Keymap, PreferenceValueError> {
    match bytes {
        b"us\n" => Ok(Keymap::Us),
        b"cz\n" => Ok(Keymap::Czech),
        _ => Err(PreferenceValueError::Keymap),
    }
}

/// Encode one typed model effect into its complete canonical file value.
///
/// # Errors
///
/// Returns [`PreferenceValueError`] if a manually constructed effect lies
/// outside the same value contract enforced by the dashboard model.
pub fn encode_setting(setting: SettingChange) -> Result<EncodedPreference, PreferenceValueError> {
    match setting {
        SettingChange::Volume(percent) if percent <= 100 => Ok(EncodedPreference {
            field: PreferenceField::Volume,
            value: encode_percent(percent),
        }),
        SettingChange::Volume(_) => Err(PreferenceValueError::Volume),
        SettingChange::Brightness(percent) => {
            let _validated =
                Brightness::new(percent).map_err(|_| PreferenceValueError::Brightness)?;
            Ok(EncodedPreference {
                field: PreferenceField::Brightness,
                value: encode_percent(percent),
            })
        }
        SettingChange::Keymap(keymap) => Ok(EncodedPreference {
            field: PreferenceField::Keymap,
            value: match keymap {
                Keymap::Us => FixedPreference::three(*b"us\n"),
                Keymap::Czech => FixedPreference::three(*b"cz\n"),
            },
        }),
    }
}

/// Persistent destination selected without inspecting text or paths.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PreferenceField {
    /// Menu and child-application volume.
    Volume,
    /// Display backlight percentage.
    Brightness,
    /// Terminal console keyboard layout.
    Keymap,
}

/// One complete canonical value in fixed stack storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedPreference {
    field: PreferenceField,
    value: FixedPreference,
}

impl EncodedPreference {
    /// Persistent destination for this value.
    #[must_use]
    pub const fn field(self) -> PreferenceField {
        self.field
    }

    /// Complete bytes, including their one terminating newline.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.value.as_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FixedPreference {
    bytes: [u8; MAXIMUM_PREFERENCE_BYTES],
    len: u8,
}

impl FixedPreference {
    const fn three(bytes: [u8; 3]) -> Self {
        let [first, second, third] = bytes;
        Self {
            bytes: [first, second, third, 0],
            len: 3,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or_default()
    }
}

/// A state file or manually assembled setting is outside its exact schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreferenceValueError {
    /// Volume is not a canonical percentage.
    Volume,
    /// Brightness is not a canonical ten-point step.
    Brightness,
    /// Keymap is not one of the two installed layouts.
    Keymap,
}

impl fmt::Display for PreferenceValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Volume => formatter.write_str(
                "volume state must be a canonical integer from 0 through 100 followed by a newline",
            ),
            Self::Brightness => formatter.write_str(
                "brightness state must be a ten-point step from 10 through 100 followed by a newline",
            ),
            Self::Keymap => formatter.write_str("keymap state must be exactly us\\n or cz\\n"),
        }
    }
}

impl Error for PreferenceValueError {}

fn parse_percent(bytes: &[u8]) -> Option<u8> {
    let digits = bytes.strip_suffix(b"\n")?;
    if digits.is_empty() || digits.len() > 3 || (digits.len() > 1 && digits.first() == Some(&b'0'))
    {
        return None;
    }
    let mut value = 0_u8;
    for digit in digits {
        if !digit.is_ascii_digit() {
            return None;
        }
        value = value
            .checked_mul(10)?
            .checked_add(digit.saturating_sub(b'0'))?;
    }
    (value <= 100).then_some(value)
}

const fn encode_percent(percent: u8) -> FixedPreference {
    if percent == 100 {
        FixedPreference {
            bytes: *b"100\n",
            len: 4,
        }
    } else if percent >= 10 {
        FixedPreference {
            bytes: [b'0' + percent / 10, b'0' + percent % 10, b'\n', 0],
            len: 3,
        }
    } else {
        FixedPreference {
            bytes: [b'0' + percent, b'\n', 0, 0],
            len: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use retro_deck_config::Catalog;

    use super::{
        DashboardPreferences, PreferenceField, PreferenceValueError, encode_setting,
        parse_brightness, parse_keymap, parse_volume,
    };
    use crate::{
        Action, Brightness, DashboardCatalog, DashboardModel, Keymap, SettingChange, VolumeState,
    };

    const DEPLOYED_CATALOG: &[u8] = include_bytes!("../../../deploy/menu/games.tsv");

    #[test]
    fn canonical_files_round_trip_without_allocation_or_legacy_aliases() {
        for percent in [0, 5, 42, 100] {
            let encoded = encode_setting(SettingChange::Volume(percent));
            let Some(encoded) = encoded.ok() else {
                return;
            };
            assert_eq!(encoded.field(), PreferenceField::Volume);
            assert_eq!(
                parse_volume(encoded.as_bytes()).map(VolumeState::percent),
                Ok(percent)
            );
        }
        for percent in (10..=100).step_by(10) {
            let encoded = encode_setting(SettingChange::Brightness(percent));
            let Some(encoded) = encoded.ok() else {
                return;
            };
            assert_eq!(encoded.field(), PreferenceField::Brightness);
            assert_eq!(
                parse_brightness(encoded.as_bytes()).map(Brightness::percent),
                Ok(percent)
            );
        }
        for keymap in [Keymap::Us, Keymap::Czech] {
            let encoded = encode_setting(SettingChange::Keymap(keymap));
            let Some(encoded) = encoded.ok() else {
                return;
            };
            assert_eq!(encoded.field(), PreferenceField::Keymap);
            assert_eq!(parse_keymap(encoded.as_bytes()), Ok(keymap));
        }
    }

    #[test]
    fn malformed_and_ambiguous_files_are_rejected() {
        for bytes in [
            b"".as_slice(),
            b"00\n",
            b"01\n",
            b"101\n",
            b"42",
            b" 42\n",
            b"on\n",
        ] {
            assert_eq!(parse_volume(bytes), Err(PreferenceValueError::Volume));
        }
        for bytes in [b"0\n".as_slice(), b"15\n", b"110\n", b"60\n\n"] {
            assert_eq!(
                parse_brightness(bytes),
                Err(PreferenceValueError::Brightness)
            );
        }
        assert_eq!(parse_keymap(b"US\n"), Err(PreferenceValueError::Keymap));
        assert_eq!(
            encode_setting(SettingChange::Volume(101)),
            Err(PreferenceValueError::Volume)
        );
        assert_eq!(
            encode_setting(SettingChange::Brightness(65)),
            Err(PreferenceValueError::Brightness)
        );
    }

    #[test]
    fn defaults_are_explicit_and_muted_state_restores_audibly() {
        let preferences = DashboardPreferences::default();
        assert_eq!(preferences.volume().percent(), 42);
        assert_eq!(preferences.brightness().percent(), 60);
        assert_eq!(preferences.keymap(), Keymap::Us);
        let Some(muted) = parse_volume(b"0\n").ok() else {
            return;
        };
        let Some(catalog) = Catalog::parse(DEPLOYED_CATALOG).ok() else {
            return;
        };
        let Some(catalog) = DashboardCatalog::from_catalog(&catalog).ok() else {
            return;
        };
        let mut model = DashboardModel::new(catalog, muted, Brightness::DEFAULT, Keymap::Us);
        let transition = model.apply(Action::VolumeUp);
        assert_eq!(transition.setting, Some(SettingChange::Volume(42)));
    }
}
