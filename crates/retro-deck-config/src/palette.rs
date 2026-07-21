//! Complete full-RGB dashboard palette schema.

use std::{
    fmt::{self, Write as _},
    str::FromStr,
};

use retro_deck_policy::{DecodeError, Limits, Value, decode_with_limits};

/// Maximum accepted size of one palette document.
pub const MAXIMUM_PALETTE_BYTES: usize = 4_096;
const MAXIMUM_OVERRIDE_VALUES: usize = 64;

/// One semantic dashboard color role.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PaletteRole {
    /// Entire dashboard background.
    Background,
    /// Dark text placed on bright controls.
    TextDark,
    /// Input field background.
    Field,
    /// Raised content surface.
    Surface,
    /// Inactive selection border.
    InactiveBorder,
    /// General control border.
    ControlBorder,
    /// Footer and position indicator.
    Footer,
    /// De-emphasized text.
    InactiveText,
    /// Primary text.
    Text,
    /// Brightest icon color.
    White,
    /// Screen title.
    Title,
    /// Muted volume label.
    VolumeOff,
    /// Audible volume label.
    VolumeOn,
    /// Selected carousel tile.
    Selected,
    /// Active Wi-Fi network.
    WifiActive,
    /// Focused Wi-Fi network.
    WifiFocus,
    /// Active Wi-Fi border.
    WifiActiveBorder,
    /// Form field label.
    FieldLabel,
    /// Primary orange accent.
    Accent,
    /// Active control fill.
    Active,
    /// Secondary control surface.
    ControlSurface,
    /// Muted control.
    Muted,
}

impl PaletteRole {
    /// Every role in the stable storage and form order.
    pub const ALL: [Self; 22] = [
        Self::Background,
        Self::TextDark,
        Self::Field,
        Self::Surface,
        Self::InactiveBorder,
        Self::ControlBorder,
        Self::Footer,
        Self::InactiveText,
        Self::Text,
        Self::White,
        Self::Title,
        Self::VolumeOff,
        Self::VolumeOn,
        Self::Selected,
        Self::WifiActive,
        Self::WifiFocus,
        Self::WifiActiveBorder,
        Self::FieldLabel,
        Self::Accent,
        Self::Active,
        Self::ControlSurface,
        Self::Muted,
    ];

    /// Canonical TSV and S-expression key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::TextDark => "text-dark",
            Self::Field => "field",
            Self::Surface => "surface",
            Self::InactiveBorder => "inactive-border",
            Self::ControlBorder => "control-border",
            Self::Footer => "footer",
            Self::InactiveText => "inactive-text",
            Self::Text => "text",
            Self::White => "white",
            Self::Title => "title",
            Self::VolumeOff => "volume-off",
            Self::VolumeOn => "volume-on",
            Self::Selected => "selected",
            Self::WifiActive => "wifi-active",
            Self::WifiFocus => "wifi-focus",
            Self::WifiActiveBorder => "wifi-active-border",
            Self::FieldLabel => "field-label",
            Self::Accent => "accent",
            Self::Active => "active",
            Self::ControlSurface => "control-surface",
            Self::Muted => "muted",
        }
    }

    /// Human-readable web form label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Background => "Background",
            Self::TextDark => "Dark text",
            Self::Field => "Field",
            Self::Surface => "Surface",
            Self::InactiveBorder => "Inactive border",
            Self::ControlBorder => "Control border",
            Self::Footer => "Footer",
            Self::InactiveText => "Inactive text",
            Self::Text => "Text",
            Self::White => "Bright white",
            Self::Title => "Title",
            Self::VolumeOff => "Volume off",
            Self::VolumeOn => "Volume on",
            Self::Selected => "Selected item",
            Self::WifiActive => "Wi-Fi active",
            Self::WifiFocus => "Wi-Fi focus",
            Self::WifiActiveBorder => "Wi-Fi active border",
            Self::FieldLabel => "Field label",
            Self::Accent => "Accent",
            Self::Active => "Active control",
            Self::ControlSurface => "Control surface",
            Self::Muted => "Muted control",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Background => 0,
            Self::TextDark => 1,
            Self::Field => 2,
            Self::Surface => 3,
            Self::InactiveBorder => 4,
            Self::ControlBorder => 5,
            Self::Footer => 6,
            Self::InactiveText => 7,
            Self::Text => 8,
            Self::White => 9,
            Self::Title => 10,
            Self::VolumeOff => 11,
            Self::VolumeOn => 12,
            Self::Selected => 13,
            Self::WifiActive => 14,
            Self::WifiFocus => 15,
            Self::WifiActiveBorder => 16,
            Self::FieldLabel => 17,
            Self::Accent => 18,
            Self::Active => 19,
            Self::ControlSurface => 20,
            Self::Muted => 21,
        }
    }
}

impl fmt::Display for PaletteRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for PaletteRole {
    type Err = PaletteRoleError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|role| role.as_str() == value)
            .ok_or(PaletteRoleError)
    }
}

/// A string is not one of the complete dashboard palette roles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaletteRoleError;

impl fmt::Display for PaletteRoleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown dashboard palette role")
    }
}

impl std::error::Error for PaletteRoleError {}

/// One exact 24-bit RGB color.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Rgb {
    red: u8,
    green: u8,
    blue: u8,
}

impl Rgb {
    /// Construct an RGB color without quantization.
    #[must_use]
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    /// Red, green, and blue components in channel order.
    #[must_use]
    pub const fn components(self) -> [u8; 3] {
        [self.red, self.green, self.blue]
    }

    fn parse(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        let [
            b'#',
            red_high,
            red_low,
            green_high,
            green_low,
            blue_high,
            blue_low,
        ] = bytes
        else {
            return None;
        };
        Some(Self {
            red: decode_hex_pair(*red_high, *red_low)?,
            green: decode_hex_pair(*green_high, *green_low)?,
            blue: decode_hex_pair(*blue_high, *blue_low)?,
        })
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "#{:02X}{:02X}{:02X}",
            self.red, self.green, self.blue
        )
    }
}

const fn decode_hex_pair(high: u8, low: u8) -> Option<u8> {
    let Some(high) = decode_hex_digit(high) else {
        return None;
    };
    let Some(low) = decode_hex_digit(low) else {
        return None;
    };
    Some(high * 16 + low)
}

const fn decode_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// One complete set of dashboard semantic colors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Palette {
    colors: [Rgb; 22],
}

impl Palette {
    /// Parse the launcher's complete headerless TSV palette.
    ///
    /// Blank lines and comments are ignored. Every semantic role remains
    /// mandatory.
    ///
    /// # Errors
    ///
    /// Returns [`PaletteError`] for excessive input, malformed rows, duplicate
    /// or unknown roles, invalid colors, or an incomplete palette.
    pub fn parse_tsv(contents: &[u8]) -> Result<Self, PaletteError> {
        validate_size(contents)?;
        let text = std::str::from_utf8(contents).map_err(|_| PaletteError::InvalidUtf8)?;
        let mut builder = PaletteBuilder::new();
        for raw_line in text.split_terminator('\n') {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once('\t') else {
                return Err(PaletteError::MalformedTsv);
            };
            if name.is_empty() || value.contains('\t') {
                return Err(PaletteError::MalformedTsv);
            }
            builder.insert(name, value)?;
        }
        builder.finish()
    }

    /// Parse a complete version 2 S-expression override.
    ///
    /// # Errors
    ///
    /// Returns [`PaletteError`] when bounded S-expression decoding, schema
    /// validation, role validation, or RGB validation fails.
    pub fn parse_override(contents: &[u8]) -> Result<Self, PaletteError> {
        validate_size(contents)?;
        let text = std::str::from_utf8(contents).map_err(|_| PaletteError::InvalidUtf8)?;
        let value = decode_with_limits(
            text,
            Limits {
                max_bytes: MAXIMUM_PALETTE_BYTES,
                max_depth: 4,
                max_values: MAXIMUM_OVERRIDE_VALUES,
            },
        )?;
        let Value::List(fields) = value else {
            return Err(PaletteError::InvalidOverride(
                "top-level value is not a list",
            ));
        };
        if fields.len() % 2 != 0 {
            return Err(PaletteError::InvalidOverride(
                "top-level property list has odd length",
            ));
        }

        let mut version = None;
        let mut palette = None;
        for pair in fields.chunks_exact(2) {
            let [key, value] = pair else {
                return Err(PaletteError::InvalidOverride(
                    "top-level property list is malformed",
                ));
            };
            let Some(key) = key.as_keyword() else {
                return Err(PaletteError::InvalidOverride(
                    "top-level property name is not a keyword",
                ));
            };
            match key {
                "version" if version.is_none() => version = Some(value),
                "palette" if palette.is_none() => palette = Some(value),
                "version" | "palette" => {
                    return Err(PaletteError::InvalidOverride(
                        "top-level property is repeated",
                    ));
                }
                _ => {
                    return Err(PaletteError::InvalidOverride(
                        "top-level property is unknown",
                    ));
                }
            }
        }

        match version {
            Some(Value::Integer(2)) => {}
            Some(Value::Integer(_)) => {
                return Err(PaletteError::InvalidOverride(
                    "schema version is unsupported",
                ));
            }
            Some(_) => {
                return Err(PaletteError::InvalidOverride(
                    "schema version is not an integer",
                ));
            }
            None => return Err(PaletteError::InvalidOverride("schema version is missing")),
        }
        let palette = palette.ok_or(PaletteError::InvalidOverride("palette is missing"))?;
        Self::parse_palette_value(palette)
    }

    /// Build a complete palette from submitted name/value pairs.
    ///
    /// # Errors
    ///
    /// Returns [`PaletteError`] for unknown, duplicate, missing, or invalid
    /// values.
    pub fn from_pairs<K, V, I>(pairs: I) -> Result<Self, PaletteError>
    where
        K: AsRef<str>,
        V: AsRef<str>,
        I: IntoIterator<Item = (K, V)>,
    {
        let mut builder = PaletteBuilder::new();
        for (name, value) in pairs {
            builder.insert(name.as_ref(), value.as_ref())?;
        }
        builder.finish()
    }

    /// Return fields in the dashboard's stable semantic order.
    #[must_use]
    pub fn fields(&self) -> Vec<PaletteField> {
        PaletteRole::ALL
            .into_iter()
            .map(|role| PaletteField {
                name: role.as_str(),
                label: role.label(),
                value: self.color(role).to_string(),
            })
            .collect()
    }

    /// Return one exact color by typed semantic role.
    #[must_use]
    pub fn color(&self, role: PaletteRole) -> Rgb {
        self.colors
            .get(role.index())
            .copied()
            .unwrap_or(Rgb::new(0, 0, 0))
    }

    /// Encode the Common Lisp compatible version 2 override schema.
    #[must_use]
    pub fn encode_override(&self) -> Vec<u8> {
        let mut output = String::from("(:version 2\n :palette\n  (");
        for (index, role) in PaletteRole::ALL.into_iter().enumerate() {
            if index > 0 {
                output.push_str("\n   ");
            }
            output.push(':');
            output.push_str(role.as_str());
            output.push_str(" \"");
            let _ = write!(output, "{}", self.color(role));
            output.push('"');
        }
        output.push_str("))\n");
        output.into_bytes()
    }

    fn parse_palette_value(value: &Value) -> Result<Self, PaletteError> {
        let Value::List(fields) = value else {
            return Err(PaletteError::InvalidOverride("palette is not a list"));
        };
        if fields.len() % 2 != 0 {
            return Err(PaletteError::InvalidOverride(
                "palette property list has odd length",
            ));
        }
        let mut builder = PaletteBuilder::new();
        for pair in fields.chunks_exact(2) {
            let [name, color] = pair else {
                return Err(PaletteError::InvalidOverride(
                    "palette property list is malformed",
                ));
            };
            let Some(name) = name.as_keyword() else {
                return Err(PaletteError::InvalidOverride(
                    "palette property name is not a keyword",
                ));
            };
            let Value::String(color) = color else {
                return Err(PaletteError::InvalidColor(name.to_owned()));
            };
            builder.insert(name, color)?;
        }
        builder.finish()
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            colors: [
                Rgb::new(0x00, 0x00, 0x00),
                Rgb::new(0x12, 0x12, 0x12),
                Rgb::new(0x12, 0x12, 0x12),
                Rgb::new(0x1c, 0x1c, 0x1c),
                Rgb::new(0x5f, 0x5f, 0x5f),
                Rgb::new(0x6c, 0x6c, 0x6c),
                Rgb::new(0xbc, 0xbc, 0xbc),
                Rgb::new(0xda, 0xda, 0xda),
                Rgb::new(0xee, 0xee, 0xee),
                Rgb::new(0xff, 0xff, 0xff),
                Rgb::new(0xff, 0xff, 0xaf),
                Rgb::new(0xaf, 0x87, 0x87),
                Rgb::new(0x87, 0xaf, 0x87),
                Rgb::new(0xec, 0xb6, 0xe7),
                Rgb::new(0x5f, 0x87, 0xaf),
                Rgb::new(0x87, 0xaf, 0xff),
                Rgb::new(0xaf, 0xaf, 0xff),
                Rgb::new(0xaf, 0xaf, 0xaf),
                Rgb::new(0xfe, 0x6c, 0x27),
                Rgb::new(0x50, 0x33, 0x11),
                Rgb::new(0x30, 0x30, 0x30),
                Rgb::new(0x94, 0x94, 0x94),
            ],
        }
    }
}

/// One palette form field ready for HTML rendering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteField {
    /// Stable form field and semantic role name.
    pub name: &'static str,
    /// Human-readable role label.
    pub label: &'static str,
    /// Canonical `#RRGGBB` value.
    pub value: String,
}

struct PaletteBuilder {
    colors: [Option<Rgb>; 22],
}

impl PaletteBuilder {
    const fn new() -> Self {
        Self { colors: [None; 22] }
    }

    fn insert(&mut self, name: &str, value: &str) -> Result<(), PaletteError> {
        let role =
            PaletteRole::from_str(name).map_err(|_| PaletteError::UnknownRole(name.to_owned()))?;
        let slot = self
            .colors
            .get_mut(role.index())
            .ok_or(PaletteError::InvalidOverride("palette index is invalid"))?;
        if slot.is_some() {
            return Err(PaletteError::DuplicateRole(name.to_owned()));
        }
        *slot = Some(Rgb::parse(value).ok_or_else(|| PaletteError::InvalidColor(name.to_owned()))?);
        Ok(())
    }

    fn finish(self) -> Result<Palette, PaletteError> {
        let mut colors = [Rgb::new(0, 0, 0); 22];
        for role in PaletteRole::ALL {
            let color = self
                .colors
                .get(role.index())
                .copied()
                .flatten()
                .ok_or(PaletteError::MissingRole(role.as_str()))?;
            let slot = colors
                .get_mut(role.index())
                .ok_or(PaletteError::InvalidOverride("palette index is invalid"))?;
            *slot = color;
        }
        Ok(Palette { colors })
    }
}

const fn validate_size(contents: &[u8]) -> Result<(), PaletteError> {
    if contents.is_empty() || contents.len() > MAXIMUM_PALETTE_BYTES {
        Err(PaletteError::InvalidSize)
    } else {
        Ok(())
    }
}

/// Palette schema, color, or S-expression failure.
#[derive(Debug)]
pub enum PaletteError {
    /// Input is empty or exceeds 4096 bytes.
    InvalidSize,
    /// Input is not UTF-8.
    InvalidUtf8,
    /// A TSV row is missing exactly one tab-separated value.
    MalformedTsv,
    /// A semantic role is unknown.
    UnknownRole(String),
    /// A semantic role appears more than once.
    DuplicateRole(String),
    /// A required semantic role is absent.
    MissingRole(&'static str),
    /// A semantic role does not contain a full RGB value.
    InvalidColor(String),
    /// The bounded S-expression schema is malformed.
    InvalidOverride(&'static str),
    /// The bounded S-expression decoder rejected the override.
    SExpression(DecodeError),
}

impl fmt::Display for PaletteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSize => formatter.write_str("palette has an invalid size"),
            Self::InvalidUtf8 => formatter.write_str("palette is not UTF-8"),
            Self::MalformedTsv => formatter.write_str("palette contains a malformed TSV row"),
            Self::UnknownRole(role) => write!(formatter, "palette contains unknown role {role}"),
            Self::DuplicateRole(role) => write!(formatter, "palette repeats role {role}"),
            Self::MissingRole(role) => write!(formatter, "palette is missing role {role}"),
            Self::InvalidColor(role) => {
                write!(formatter, "palette role {role} is not a full RGB color")
            }
            Self::InvalidOverride(reason) => {
                write!(formatter, "invalid palette override: {reason}")
            }
            Self::SExpression(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PaletteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SExpression(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DecodeError> for PaletteError {
    fn from(error: DecodeError) -> Self {
        Self::SExpression(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{Palette, PaletteError, PaletteRole, Rgb};

    fn rgb(index: usize) -> String {
        format!(
            "#{:02X}{:02X}{:02X}",
            index * 3 + 1,
            index * 3 + 2,
            index * 3 + 3
        )
    }

    fn pairs(offset: usize) -> Vec<(&'static str, String)> {
        PaletteRole::ALL
            .into_iter()
            .enumerate()
            .map(|(index, role)| (role.as_str(), rgb(index + offset)))
            .collect()
    }

    fn tsv(offset: usize) -> Vec<u8> {
        let mut output = String::new();
        for (name, value) in pairs(offset) {
            output.push_str(name);
            output.push('\t');
            output.push_str(&value);
            output.push('\n');
        }
        output.into_bytes()
    }

    #[test]
    fn parses_complete_tsv_comments_and_full_rgb() {
        let mut contents = String::from("# complete palette\r\n\r\n");
        for (name, value) in pairs(0) {
            contents.push_str(name);
            contents.push('\t');
            contents.push_str(&value.to_ascii_lowercase());
            contents.push_str("\r\n");
        }
        let parsed = Palette::parse_tsv(contents.as_bytes());
        assert!(matches!(
            parsed,
            Ok(palette)
                if palette.color(PaletteRole::Accent) == Rgb::new(0x37, 0x38, 0x39)
        ));

        let mut duplicate = tsv(0);
        duplicate.extend_from_slice(b"accent\t#010203\n");
        assert!(matches!(
            Palette::parse_tsv(&duplicate),
            Err(PaletteError::DuplicateRole(role)) if role == "accent"
        ));
        assert!(matches!(
            Palette::parse_tsv(b"unknown\t#010203\n"),
            Err(PaletteError::UnknownRole(role)) if role == "unknown"
        ));
        assert!(matches!(
            Palette::parse_tsv(b"settings-icon\tgear-rivet\n"),
            Err(PaletteError::UnknownRole(role)) if role == "settings-icon"
        ));
    }

    #[test]
    fn compiled_fallback_exactly_matches_the_checked_in_palette() {
        let parsed = Palette::parse_tsv(include_bytes!("../../../deploy/menu/palette.tsv"));
        assert_eq!(parsed.ok(), Some(Palette::default()));
        assert_eq!(
            Palette::default().color(PaletteRole::Accent).components(),
            [0xfe, 0x6c, 0x27]
        );
    }

    #[test]
    fn override_round_trips_and_rejects_retired_schemas() {
        let palette = Palette::from_pairs(pairs(0));
        assert!(palette.is_ok(), "fixture palette should be valid");
        let Some(palette) = palette.ok() else {
            return;
        };
        let encoded = palette.encode_override();
        assert_eq!(Palette::parse_override(&encoded).ok(), Some(palette));

        assert!(Palette::parse_override(b"(:version 2 :version 2 :palette ())").is_err());
        assert!(Palette::parse_override(b"(:version 3 :palette ())").is_err());
        assert!(
            Palette::parse_override(b"(:version 2 :settings-icon \"gear\" :palette ())").is_err()
        );
    }
}
