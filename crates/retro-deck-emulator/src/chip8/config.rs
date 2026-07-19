use std::error::Error;
use std::fmt;
use std::str;

/// Largest accepted per-ROM compatibility sidecar.
pub const MAXIMUM_CONFIG_BYTES: usize = 4_096;
/// Smallest supported c-octo instruction budget per 60 Hz frame.
pub const MINIMUM_INSTRUCTIONS_PER_FRAME: u32 = 1;
/// Largest supported c-octo instruction budget per 60 Hz frame.
pub const MAXIMUM_INSTRUCTIONS_PER_FRAME: u32 = 50_000;

const DEFAULT_INSTRUCTIONS_PER_FRAME: u32 = 20;
const DEFAULT_PALETTE: [u32; 4] = [0x00_00_00, 0xff_cc_00, 0xff_66_00, 0x66_22_00];

const KEY_TICKRATE: u16 = 1 << 0;
const KEY_SHIFT_QUIRK: u16 = 1 << 1;
const KEY_LOAD_STORE_QUIRK: u16 = 1 << 2;
const KEY_JUMP_QUIRK: u16 = 1 << 3;
const KEY_LOGIC_QUIRK: u16 = 1 << 4;
const KEY_CLIP_QUIRK: u16 = 1 << 5;
const KEY_VBLANK_QUIRK: u16 = 1 << 6;
const KEY_COLOR_0: u16 = 1 << 7;
const KEY_COLOR_1: u16 = 1 << 8;
const KEY_COLOR_2: u16 = 1 << 9;
const KEY_COLOR_3: u16 = 1 << 10;
const KEY_INPUT: u16 = 1 << 11;

/// One c-octo compatibility switch stored beside a ROM.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Quirk {
    /// Shift instructions read only `VX`, matching SCHIP.
    Shift,
    /// Bulk loads and stores leave the index register unchanged.
    LoadStore,
    /// `BNNN` selects its offset register from the high address nibble.
    Jump,
    /// Boolean operations clear `VF`.
    Logic,
    /// Sprite pixels clip instead of wrapping at display edges.
    Clip,
    /// Drawing yields until the next 60 Hz frame.
    Vblank,
}

impl Quirk {
    const fn mask(self) -> u8 {
        match self {
            Self::Shift => 1 << 0,
            Self::LoadStore => 1 << 1,
            Self::Jump => 1 << 2,
            Self::Logic => 1 << 3,
            Self::Clip => 1 << 4,
            Self::Vblank => 1 << 5,
        }
    }
}

/// Compact set of c-octo compatibility switches.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Quirks(u8);

impl Quirks {
    /// Construct a set with every compatibility quirk disabled.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Return whether one compatibility quirk is enabled.
    #[must_use]
    pub const fn contains(self, quirk: Quirk) -> bool {
        self.0 & quirk.mask() != 0
    }

    /// Return a copy with one compatibility quirk changed.
    #[must_use]
    pub const fn with(mut self, quirk: Quirk, enabled: bool) -> Self {
        self.set(quirk, enabled);
        self
    }

    const fn set(&mut self, quirk: Quirk, enabled: bool) {
        if enabled {
            self.0 |= quirk.mask();
        } else {
            self.0 &= !quirk.mask();
        }
    }
}

/// Validated c-octo execution and presentation options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoreOptions {
    instructions_per_frame: u32,
    quirks: Quirks,
    palette: [u32; 4],
}

impl CoreOptions {
    /// Construct options with a bounded instruction budget and RGB palette.
    #[must_use]
    pub const fn new(
        instructions_per_frame: u32,
        quirks: Quirks,
        palette: [u32; 4],
    ) -> Option<Self> {
        if instructions_per_frame < MINIMUM_INSTRUCTIONS_PER_FRAME
            || instructions_per_frame > MAXIMUM_INSTRUCTIONS_PER_FRAME
            || palette[0] > 0x00ff_ffff
            || palette[1] > 0x00ff_ffff
            || palette[2] > 0x00ff_ffff
            || palette[3] > 0x00ff_ffff
        {
            return None;
        }
        Some(Self {
            instructions_per_frame,
            quirks,
            palette,
        })
    }

    /// Number of instructions c-octo may execute in one 60 Hz frame.
    #[must_use]
    pub const fn instructions_per_frame(self) -> u32 {
        self.instructions_per_frame
    }

    /// Selected compatibility quirks.
    #[must_use]
    pub const fn quirks(self) -> Quirks {
        self.quirks
    }

    /// Four RGB colors indexed by the XO-CHIP pixel planes.
    #[must_use]
    pub const fn palette(self) -> [u32; 4] {
        self.palette
    }
}

impl Default for CoreOptions {
    fn default() -> Self {
        Self {
            instructions_per_frame: DEFAULT_INSTRUCTIONS_PER_FRAME,
            quirks: Quirks::default(),
            palette: DEFAULT_PALETTE,
        }
    }
}

/// Controller layout selected for one CHIP-8 program.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum InputProfile {
    /// Conventional Octo WASD/QEZV keypad subset on Player 1.
    #[default]
    Octo,
    /// Two-player vertical controls used by Space Racer.
    SpaceRacer,
}

/// Complete validated per-ROM configuration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Configuration {
    core: CoreOptions,
    input: InputProfile,
}

impl Configuration {
    /// Parse one complete bounded UTF-8 sidecar.
    ///
    /// Empty lines and lines beginning with `#` are ignored. Keys are exact,
    /// values are not trimmed, and duplicate keys are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] for oversized, non-UTF-8, malformed, unknown,
    /// duplicate, or invalid input.
    pub fn parse(bytes: &[u8]) -> Result<Self, ConfigError> {
        if bytes.len() > MAXIMUM_CONFIG_BYTES {
            return Err(ConfigError::TooLarge { bytes: bytes.len() });
        }
        let text = str::from_utf8(bytes).map_err(|_| ConfigError::NotUtf8)?;
        let mut configuration = Self::default();
        let mut seen = 0_u16;

        for (offset, raw_line) in text.split('\n').enumerate() {
            let line_number = offset.saturating_add(1);
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(ConfigError::MalformedLine { line: line_number });
            };
            if key.is_empty() || value.is_empty() {
                return Err(ConfigError::MalformedLine { line: line_number });
            }
            let key_bit = key_bit(key).ok_or(ConfigError::UnknownKey { line: line_number })?;
            if seen & key_bit != 0 {
                return Err(ConfigError::DuplicateKey { line: line_number });
            }
            seen |= key_bit;
            apply_value(&mut configuration, key, value)
                .map_err(|()| ConfigError::InvalidValue { line: line_number })?;
        }
        Ok(configuration)
    }

    /// Validated c-octo execution options.
    #[must_use]
    pub const fn core(self) -> CoreOptions {
        self.core
    }

    /// Controller-to-keypad mapping selected by the ROM sidecar.
    #[must_use]
    pub const fn input(self) -> InputProfile {
        self.input
    }
}

fn key_bit(key: &str) -> Option<u16> {
    match key {
        "tickrate" => Some(KEY_TICKRATE),
        "shift_quirk" => Some(KEY_SHIFT_QUIRK),
        "load_store_quirk" => Some(KEY_LOAD_STORE_QUIRK),
        "jump_quirk" => Some(KEY_JUMP_QUIRK),
        "logic_quirk" => Some(KEY_LOGIC_QUIRK),
        "clip_quirk" => Some(KEY_CLIP_QUIRK),
        "vblank_quirk" => Some(KEY_VBLANK_QUIRK),
        "color0" => Some(KEY_COLOR_0),
        "color1" => Some(KEY_COLOR_1),
        "color2" => Some(KEY_COLOR_2),
        "color3" => Some(KEY_COLOR_3),
        "input" => Some(KEY_INPUT),
        _ => None,
    }
}

fn apply_value(configuration: &mut Configuration, key: &str, value: &str) -> Result<(), ()> {
    match key {
        "tickrate" => {
            let parsed = value.parse::<u32>().map_err(|_| ())?;
            if !(MINIMUM_INSTRUCTIONS_PER_FRAME..=MAXIMUM_INSTRUCTIONS_PER_FRAME).contains(&parsed)
            {
                return Err(());
            }
            configuration.core.instructions_per_frame = parsed;
        }
        "shift_quirk" => configuration
            .core
            .quirks
            .set(Quirk::Shift, parse_boolean(value)?),
        "load_store_quirk" => configuration
            .core
            .quirks
            .set(Quirk::LoadStore, parse_boolean(value)?),
        "jump_quirk" => configuration
            .core
            .quirks
            .set(Quirk::Jump, parse_boolean(value)?),
        "logic_quirk" => configuration
            .core
            .quirks
            .set(Quirk::Logic, parse_boolean(value)?),
        "clip_quirk" => configuration
            .core
            .quirks
            .set(Quirk::Clip, parse_boolean(value)?),
        "vblank_quirk" => configuration
            .core
            .quirks
            .set(Quirk::Vblank, parse_boolean(value)?),
        "color0" => configuration.core.palette[0] = parse_color(value)?,
        "color1" => configuration.core.palette[1] = parse_color(value)?,
        "color2" => configuration.core.palette[2] = parse_color(value)?,
        "color3" => configuration.core.palette[3] = parse_color(value)?,
        "input" => {
            configuration.input = match value {
                "octo" => InputProfile::Octo,
                "space-racer" => InputProfile::SpaceRacer,
                _ => return Err(()),
            };
        }
        _ => return Err(()),
    }
    Ok(())
}

const fn parse_boolean(value: &str) -> Result<bool, ()> {
    match value.as_bytes() {
        [b'0'] => Ok(false),
        [b'1'] => Ok(true),
        _ => Err(()),
    }
}

fn parse_color(value: &str) -> Result<u32, ()> {
    let Some(hexadecimal) = value.strip_prefix('#') else {
        return Err(());
    };
    if hexadecimal.len() != 6 {
        return Err(());
    }
    u32::from_str_radix(hexadecimal, 16).map_err(|_| ())
}

/// Reason a CHIP-8 compatibility sidecar was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigError {
    /// Input exceeds the fixed sidecar limit.
    TooLarge {
        /// Actual byte count.
        bytes: usize,
    },
    /// Input is not valid UTF-8.
    NotUtf8,
    /// A non-comment line does not contain a nonempty key and value.
    MalformedLine {
        /// One-based source line.
        line: usize,
    },
    /// A key is not part of the supported sidecar schema.
    UnknownKey {
        /// One-based source line.
        line: usize,
    },
    /// A key occurs more than once.
    DuplicateKey {
        /// One-based source line.
        line: usize,
    },
    /// A known key has an invalid value.
    InvalidValue {
        /// One-based source line.
        line: usize,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes } => write!(
                formatter,
                "CHIP-8 config contains {bytes} bytes; maximum is {MAXIMUM_CONFIG_BYTES}"
            ),
            Self::NotUtf8 => formatter.write_str("CHIP-8 config is not valid UTF-8"),
            Self::MalformedLine { line } => {
                write!(formatter, "invalid CHIP-8 config line {line}")
            }
            Self::UnknownKey { line } => {
                write!(formatter, "unknown CHIP-8 config key on line {line}")
            }
            Self::DuplicateKey { line } => {
                write!(formatter, "duplicate CHIP-8 config key on line {line}")
            }
            Self::InvalidValue { line } => {
                write!(formatter, "invalid CHIP-8 config value on line {line}")
            }
        }
    }
}

impl Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_existing_deck_frontend() {
        let configuration = Configuration::default();
        assert_eq!(configuration.core().instructions_per_frame(), 20);
        assert_eq!(configuration.core().quirks(), Quirks::default());
        assert_eq!(configuration.core().palette(), DEFAULT_PALETTE);
        assert_eq!(configuration.input(), InputProfile::Octo);
    }

    #[test]
    fn complete_sidecar_is_parsed_exactly() {
        let configuration = Configuration::parse(
            b"# fixture\r\ntickrate=15\r\nshift_quirk=1\nload_store_quirk=1\n\
              jump_quirk=1\nlogic_quirk=1\nclip_quirk=1\nvblank_quirk=1\n\
              color0=#010203\ncolor1=#A0b0C0\ncolor2=#112233\ncolor3=#abcdef\n\
              input=space-racer\n",
        )
        .expect("valid fixture");
        assert_eq!(configuration.core().instructions_per_frame(), 15);
        assert_eq!(
            configuration.core().quirks(),
            Quirks::empty()
                .with(Quirk::Shift, true)
                .with(Quirk::LoadStore, true)
                .with(Quirk::Jump, true)
                .with(Quirk::Logic, true)
                .with(Quirk::Clip, true)
                .with(Quirk::Vblank, true)
        );
        assert_eq!(
            configuration.core().palette(),
            [0x01_02_03, 0xa0_b0_c0, 0x11_22_33, 0xab_cd_ef]
        );
        assert_eq!(configuration.input(), InputProfile::SpaceRacer);
    }

    #[test]
    fn schema_errors_identify_the_source_line() {
        assert_eq!(
            Configuration::parse(b"tickrate=20\ntickrate=15\n"),
            Err(ConfigError::DuplicateKey { line: 2 })
        );
        assert_eq!(
            Configuration::parse(b"unknown=1\n"),
            Err(ConfigError::UnknownKey { line: 1 })
        );
        assert_eq!(
            Configuration::parse(b"tickrate =20\n"),
            Err(ConfigError::UnknownKey { line: 1 })
        );
        assert_eq!(
            Configuration::parse(b"tickrate=\n"),
            Err(ConfigError::MalformedLine { line: 1 })
        );
    }

    #[test]
    fn invalid_values_and_bounds_are_rejected() {
        for sidecar in [
            b"tickrate=0\n".as_slice(),
            b"tickrate=50001\n",
            b"shift_quirk=true\n",
            b"color0=#12345\n",
            b"color0=#12345g\n",
            b"input=keyboard\n",
        ] {
            assert_eq!(
                Configuration::parse(sidecar),
                Err(ConfigError::InvalidValue { line: 1 })
            );
        }
        assert_eq!(
            Configuration::parse(&vec![b'#'; MAXIMUM_CONFIG_BYTES + 1]),
            Err(ConfigError::TooLarge {
                bytes: MAXIMUM_CONFIG_BYTES + 1
            })
        );
        assert_eq!(Configuration::parse(&[0xff]), Err(ConfigError::NotUtf8));
    }

    #[test]
    fn options_constructor_enforces_ffi_invariants() {
        assert!(CoreOptions::new(1, Quirks::default(), DEFAULT_PALETTE).is_some());
        assert!(CoreOptions::new(0, Quirks::default(), DEFAULT_PALETTE).is_none());
        assert!(CoreOptions::new(20, Quirks::default(), [0, 1, 2, 0x0100_0000]).is_none());
    }
}
