//! Typed dashboard palette parsing and durable override storage.

use std::{
    fmt, io,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

use retro_deck_policy::{DecodeError, Limits, Value, decode_with_limits};

use crate::file::{FileError, atomic_write, read_bounded_regular};

const MAXIMUM_PALETTE_BYTES: usize = 4_096;
const MAXIMUM_PALETTE_FILE_BYTES: u64 = 4_096;
const PALETTE_FILE_MODE: u32 = 0o600;
const PALETTE_DIRECTORY_MODE: u32 = 0o700;
const MAXIMUM_OVERRIDE_VALUES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PaletteSpec {
    name: &'static str,
    label: &'static str,
}

impl PaletteSpec {
    const fn new(name: &'static str, label: &'static str) -> Self {
        Self { name, label }
    }
}

const PALETTE_SPECS: [PaletteSpec; 22] = [
    PaletteSpec::new("background", "Background"),
    PaletteSpec::new("text-dark", "Dark text"),
    PaletteSpec::new("field", "Field"),
    PaletteSpec::new("surface", "Surface"),
    PaletteSpec::new("inactive-border", "Inactive border"),
    PaletteSpec::new("control-border", "Control border"),
    PaletteSpec::new("footer", "Footer"),
    PaletteSpec::new("inactive-text", "Inactive text"),
    PaletteSpec::new("text", "Text"),
    PaletteSpec::new("white", "Bright white"),
    PaletteSpec::new("title", "Title"),
    PaletteSpec::new("volume-off", "Volume off"),
    PaletteSpec::new("volume-on", "Volume on"),
    PaletteSpec::new("selected", "Selected item"),
    PaletteSpec::new("wifi-active", "Wi-Fi active"),
    PaletteSpec::new("wifi-focus", "Wi-Fi focus"),
    PaletteSpec::new("wifi-active-border", "Wi-Fi active border"),
    PaletteSpec::new("field-label", "Field label"),
    PaletteSpec::new("accent", "Accent"),
    PaletteSpec::new("active", "Active control"),
    PaletteSpec::new("control-surface", "Control surface"),
    PaletteSpec::new("muted", "Muted control"),
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct Rgb(String);

impl Rgb {
    fn parse(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        if bytes.len() != 7
            || bytes.first() != Some(&b'#')
            || !bytes
                .get(1..)
                .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
        {
            return None;
        }
        Some(Self(value.to_ascii_uppercase()))
    }

    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    fn as_str(&self) -> &str {
        &self.0
    }
}

/// One complete, ordered set of dashboard semantic colors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Palette {
    colors: Vec<Rgb>,
}

impl Palette {
    /// Parse the launcher's complete headerless TSV palette.
    ///
    /// A legacy `settings-icon` row is validated and ignored. RGB values are
    /// normalized to uppercase, while every semantic role remains mandatory.
    ///
    /// # Errors
    ///
    /// Returns [`PaletteError`] for excessive input, malformed rows, duplicate
    /// or unknown roles, invalid colors, or an incomplete palette.
    pub fn parse_tsv(contents: &[u8]) -> Result<Self, PaletteError> {
        validate_size(contents)?;
        let text = std::str::from_utf8(contents).map_err(|_| PaletteError::InvalidUtf8)?;
        let mut builder = PaletteBuilder::new();
        let mut legacy_icon_seen = false;
        for raw_line in text.split_terminator('\n') {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let Some((name, value)) = line.split_once('\t') else {
                return Err(PaletteError::MalformedTsv);
            };
            if name.is_empty() || value.contains('\t') {
                return Err(PaletteError::MalformedTsv);
            }
            if name == "settings-icon" {
                if legacy_icon_seen || !valid_legacy_icon(value) {
                    return Err(PaletteError::InvalidLegacyIcon);
                }
                legacy_icon_seen = true;
                continue;
            }
            builder.insert(name, value)?;
        }
        builder.finish()
    }

    /// Parse a complete version 2 or legacy version 3 S-expression override.
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
        let mut legacy_icon = None;
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
                "settings-icon" if legacy_icon.is_none() => legacy_icon = Some(value),
                "palette" if palette.is_none() => palette = Some(value),
                "version" | "settings-icon" | "palette" => {
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

        let version = match version {
            Some(Value::Integer(version @ (2 | 3))) => *version,
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
        };
        match (version, legacy_icon) {
            (2, None) => {}
            (2, Some(_)) => {
                return Err(PaletteError::InvalidOverride(
                    "version 2 cannot select a settings icon",
                ));
            }
            (3, Some(Value::String(icon))) if valid_legacy_icon(icon) => {}
            (3, Some(_)) => return Err(PaletteError::InvalidLegacyIcon),
            (3, None) => {
                return Err(PaletteError::InvalidOverride(
                    "version 3 settings icon is missing",
                ));
            }
            _ => {
                return Err(PaletteError::InvalidOverride(
                    "schema version is unsupported",
                ));
            }
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
        PALETTE_SPECS
            .iter()
            .zip(&self.colors)
            .map(|(spec, color)| PaletteField {
                name: spec.name,
                label: spec.label,
                value: color.0.clone(),
            })
            .collect()
    }

    /// Return one canonical `#RRGGBB` value by semantic role.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<&str> {
        spec_index(name)
            .and_then(|index| self.colors.get(index))
            .map(Rgb::as_str)
    }

    /// Encode the Common Lisp compatible version 2 override schema.
    #[must_use]
    pub fn encode_override(&self) -> Vec<u8> {
        let mut output = String::from("(:version 2\n :palette\n  (");
        for (index, (spec, color)) in PALETTE_SPECS.iter().zip(&self.colors).enumerate() {
            if index > 0 {
                output.push_str("\n   ");
            }
            output.push(':');
            output.push_str(spec.name);
            output.push_str(" \"");
            output.push_str(color.as_str());
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
    colors: Vec<Option<Rgb>>,
}

impl PaletteBuilder {
    fn new() -> Self {
        Self {
            colors: vec![None; PALETTE_SPECS.len()],
        }
    }

    fn insert(&mut self, name: &str, value: &str) -> Result<(), PaletteError> {
        let index = spec_index(name).ok_or_else(|| PaletteError::UnknownRole(name.to_owned()))?;
        let slot = self
            .colors
            .get_mut(index)
            .ok_or(PaletteError::InvalidOverride("palette index is invalid"))?;
        if slot.is_some() {
            return Err(PaletteError::DuplicateRole(name.to_owned()));
        }
        *slot = Some(Rgb::parse(value).ok_or_else(|| PaletteError::InvalidColor(name.to_owned()))?);
        Ok(())
    }

    fn finish(self) -> Result<Palette, PaletteError> {
        let mut colors = Vec::with_capacity(PALETTE_SPECS.len());
        for (color, spec) in self.colors.into_iter().zip(PALETTE_SPECS) {
            colors.push(color.ok_or(PaletteError::MissingRole(spec.name))?);
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

fn spec_index(name: &str) -> Option<usize> {
    PALETTE_SPECS.iter().position(|spec| spec.name == name)
}

fn valid_legacy_icon(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
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
    /// A retired settings-icon value violates its compatibility contract.
    InvalidLegacyIcon,
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
            Self::InvalidLegacyIcon => {
                formatter.write_str("palette contains an invalid legacy settings icon")
            }
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

/// No-follow palette file access or persistence failure.
#[derive(Debug)]
pub enum PaletteStorageError {
    /// A path or file type violated the storage contract.
    UnsafeFile(&'static str),
    /// File access or durable replacement failed.
    Io(io::Error),
    /// Operating-system entropy failed while naming a temporary file.
    Random(String),
}

impl fmt::Display for PaletteStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeFile(reason) => write!(formatter, "unsafe palette file: {reason}"),
            Self::Io(error) => write!(formatter, "palette file I/O failed: {error}"),
            Self::Random(error) => write!(formatter, "cannot name palette file: {error}"),
        }
    }
}

impl std::error::Error for PaletteStorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::UnsafeFile(_) | Self::Random(_) => None,
        }
    }
}

impl From<FileError> for PaletteStorageError {
    fn from(error: FileError) -> Self {
        match error {
            FileError::Io(error) => Self::Io(error),
            FileError::Unsafe(reason) => Self::UnsafeFile(reason),
            FileError::Random(error) => Self::Random(error),
        }
    }
}

/// Failure while opening and decoding one palette source.
#[derive(Debug)]
pub enum PaletteLoadError {
    /// The file could not be opened through the no-follow boundary.
    Storage(PaletteStorageError),
    /// The file contents violate the complete palette schema.
    Format(PaletteError),
}

impl fmt::Display for PaletteLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => error.fmt(formatter),
            Self::Format(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PaletteLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Format(error) => Some(error),
        }
    }
}

/// Palette store configuration, source, locking, or persistence failure.
#[derive(Debug)]
pub enum PaletteStoreError {
    /// Installed palette paths are relative, traversing, or overlap.
    InvalidConfiguration,
    /// Neither checked-in nor generated palette could be loaded.
    BaseUnavailable {
        /// Why the checked-in fallback failed.
        fallback: Box<PaletteLoadError>,
        /// Why the generated active palette failed.
        active: Box<PaletteLoadError>,
    },
    /// Another thread panicked while holding the palette store lock.
    LockPoisoned,
    /// The persistent override could not be replaced durably.
    Save(PaletteStorageError),
}

impl fmt::Display for PaletteStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => formatter.write_str("invalid palette store paths"),
            Self::BaseUnavailable { fallback, active } => write!(
                formatter,
                "no installed dashboard palette is usable; fallback: {fallback}; active: {active}"
            ),
            Self::LockPoisoned => formatter.write_str("palette store lock was poisoned"),
            Self::Save(error) => write!(formatter, "cannot save dashboard palette: {error}"),
        }
    }
}

impl std::error::Error for PaletteStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BaseUnavailable { fallback, .. } => Some(fallback),
            Self::Save(error) => Some(error),
            Self::InvalidConfiguration | Self::LockPoisoned => None,
        }
    }
}

/// Serialized access to installed palettes and the persistent override.
pub struct PaletteStore {
    lock: Mutex<()>,
    active_path: PathBuf,
    fallback_path: PathBuf,
    override_path: PathBuf,
}

impl PaletteStore {
    /// Configure palette sources without touching the filesystem.
    ///
    /// # Errors
    ///
    /// Returns [`PaletteStoreError::InvalidConfiguration`] unless every path is
    /// absolute and traversal-free and all three paths differ.
    pub fn new(
        active_path: impl Into<PathBuf>,
        fallback_path: impl Into<PathBuf>,
        override_path: impl Into<PathBuf>,
    ) -> Result<Self, PaletteStoreError> {
        let active_path = active_path.into();
        let fallback_path = fallback_path.into();
        let override_path = override_path.into();
        if !safe_absolute(&active_path)
            || !safe_absolute(&fallback_path)
            || !safe_absolute(&override_path)
            || active_path == fallback_path
            || active_path == override_path
            || fallback_path == override_path
        {
            return Err(PaletteStoreError::InvalidConfiguration);
        }
        Ok(Self {
            lock: Mutex::new(()),
            active_path,
            fallback_path,
            override_path,
        })
    }

    /// Load fields for the web form using launcher-compatible precedence.
    ///
    /// The checked-in fallback is preferred over a stale generated palette. A
    /// malformed optional override is ignored so appearance configuration can
    /// never prevent the dashboard or uploader from starting.
    ///
    /// # Errors
    ///
    /// Returns [`PaletteStoreError`] only when the store lock is poisoned or
    /// neither installed base palette can be loaded and validated.
    pub fn current(&self) -> Result<Vec<PaletteField>, PaletteStoreError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| PaletteStoreError::LockPoisoned)?;
        let palette = match load_tsv(&self.fallback_path) {
            Ok(palette) => palette,
            Err(fallback) => match load_tsv(&self.active_path) {
                Ok(palette) => palette,
                Err(active) => {
                    return Err(PaletteStoreError::BaseUnavailable {
                        fallback: Box::new(fallback),
                        active: Box::new(active),
                    });
                }
            },
        };
        let palette = load_override(&self.override_path).unwrap_or(palette);
        Ok(palette.fields())
    }

    /// Durably replace the optional version 2 appearance override.
    ///
    /// Dashboard process control deliberately remains outside this storage
    /// type, so a successful write is not coupled to service supervision.
    ///
    /// # Errors
    ///
    /// Returns [`PaletteStoreError`] if the store lock is poisoned or a
    /// no-follow, same-directory atomic replacement fails.
    pub fn save(&self, palette: &Palette) -> Result<(), PaletteStoreError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| PaletteStoreError::LockPoisoned)?;
        atomic_write(
            &self.override_path,
            &palette.encode_override(),
            PALETTE_FILE_MODE,
            PALETTE_DIRECTORY_MODE,
        )
        .map_err(PaletteStorageError::from)
        .map_err(PaletteStoreError::Save)?;
        Ok(())
    }
}

impl fmt::Debug for PaletteStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PaletteStore")
            .field("active_path", &self.active_path)
            .field("fallback_path", &self.fallback_path)
            .field("override_path", &self.override_path)
            .finish_non_exhaustive()
    }
}

fn load_tsv(path: &Path) -> Result<Palette, PaletteLoadError> {
    let file = read_bounded_regular(path, MAXIMUM_PALETTE_FILE_BYTES)
        .map_err(PaletteStorageError::from)
        .map_err(PaletteLoadError::Storage)?;
    Palette::parse_tsv(&file.contents).map_err(PaletteLoadError::Format)
}

fn load_override(path: &Path) -> Result<Palette, PaletteLoadError> {
    let file = read_bounded_regular(path, MAXIMUM_PALETTE_FILE_BYTES)
        .map_err(PaletteStorageError::from)
        .map_err(PaletteLoadError::Storage)?;
    Palette::parse_override(&file.contents).map_err(PaletteLoadError::Format)
}

fn safe_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::{PALETTE_SPECS, Palette, PaletteError, PaletteStore, PaletteStoreError};
    use std::{
        fs,
        os::unix::fs::{MetadataExt as _, symlink},
    };

    fn rgb(index: usize) -> String {
        format!(
            "#{:02X}{:02X}{:02X}",
            index * 3 + 1,
            index * 3 + 2,
            index * 3 + 3
        )
    }

    fn pairs(offset: usize) -> Vec<(&'static str, String)> {
        PALETTE_SPECS
            .iter()
            .enumerate()
            .map(|(index, spec)| (spec.name, rgb(index + offset)))
            .collect()
    }

    fn palette(offset: usize) -> Option<Palette> {
        Palette::from_pairs(pairs(offset)).ok()
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
    fn parses_complete_tsv_and_normalizes_rgb_values() {
        let mut contents = String::from("settings-icon\tgear-rivet\r\n");
        for (name, value) in pairs(0) {
            contents.push_str(name);
            contents.push('\t');
            contents.push_str(&value.to_ascii_lowercase());
            contents.push_str("\r\n");
        }
        let parsed = Palette::parse_tsv(contents.as_bytes());
        assert!(matches!(parsed, Ok(palette) if palette.value("accent") == Some("#373839")));

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
    }

    #[test]
    fn parses_the_checked_in_dashboard_palette() {
        let parsed = Palette::parse_tsv(include_bytes!("../../../deploy/menu/palette.tsv"));
        assert!(matches!(
            parsed,
            Ok(palette)
                if palette.fields().len() == PALETTE_SPECS.len()
                    && palette.value("accent") == Some("#FE6C27")
        ));
    }

    #[test]
    fn override_round_trips_and_accepts_the_retired_version_three_icon() {
        let palette = palette(0);
        assert!(palette.is_some(), "fixture palette should be valid");
        let Some(palette) = palette else {
            return;
        };
        let encoded = palette.encode_override();
        assert_eq!(
            Palette::parse_override(&encoded).ok(),
            Some(palette.clone())
        );

        let encoded = String::from_utf8(encoded);
        assert!(encoded.is_ok());
        let encoded = encoded.unwrap_or_default().replacen(
            "(:version 2\n",
            "(:version 3\n :settings-icon \"gear-rivet\"\n",
            1,
        );
        assert_eq!(
            Palette::parse_override(encoded.as_bytes()).ok(),
            Some(palette)
        );
        assert!(Palette::parse_override(b"(:version 2 :version 2 :palette ())").is_err());
        assert!(
            Palette::parse_override(b"(:version 2 :settings-icon \"gear\" :palette ())").is_err()
        );
    }

    #[test]
    fn store_prefers_fallback_uses_valid_override_and_ignores_bad_override() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let fallback = directory.path().join("fallback.tsv");
        let active = directory.path().join("active.tsv");
        let override_path = directory.path().join("state/palette.sexp");
        assert!(fs::write(&fallback, tsv(0)).is_ok());
        assert!(fs::write(&active, tsv(32)).is_ok());
        let store = PaletteStore::new(&active, &fallback, &override_path);
        assert!(store.is_ok());
        let Some(store) = store.ok() else {
            return;
        };
        assert!(matches!(
            store.current(),
            Ok(fields) if fields.first().is_some_and(|field| field.value == "#010203")
        ));

        let Some(override_palette) = palette(32) else {
            return;
        };
        assert!(store.save(&override_palette).is_ok());
        assert!(matches!(
            fs::metadata(&override_path),
            Ok(metadata) if metadata.mode() & 0o777 == 0o600
        ));
        assert!(matches!(
            store.current(),
            Ok(fields) if fields.first().is_some_and(|field| field.value == "#616263")
        ));

        assert!(fs::write(&override_path, b"(:version 2 :palette ())\n").is_ok());
        assert!(matches!(
            store.current(),
            Ok(fields) if fields.first().is_some_and(|field| field.value == "#010203")
        ));
        assert!(fs::remove_file(&fallback).is_ok());
        assert!(matches!(
            store.current(),
            Ok(fields) if fields.first().is_some_and(|field| field.value == "#616263")
        ));
    }

    #[test]
    fn save_replaces_only_the_override_symlink() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let fallback = directory.path().join("fallback.tsv");
        let active = directory.path().join("active.tsv");
        let override_path = directory.path().join("override.sexp");
        let victim = directory.path().join("victim");
        assert!(fs::write(&fallback, tsv(0)).is_ok());
        assert!(fs::write(&active, tsv(1)).is_ok());
        assert!(fs::write(&victim, b"untouched").is_ok());
        assert!(symlink(&victim, &override_path).is_ok());
        let store = PaletteStore::new(&active, &fallback, &override_path);
        assert!(store.is_ok());
        let Some(store) = store.ok() else {
            return;
        };
        let Some(palette) = palette(0) else {
            return;
        };
        assert!(store.save(&palette).is_ok());
        assert!(matches!(fs::read(&victim), Ok(contents) if contents == b"untouched"));
        assert!(matches!(
            fs::symlink_metadata(&override_path),
            Ok(metadata) if metadata.is_file()
        ));
    }

    #[test]
    fn rejects_relative_or_overlapping_store_paths() {
        assert!(matches!(
            PaletteStore::new("active", "/fallback", "/override"),
            Err(PaletteStoreError::InvalidConfiguration)
        ));
        assert!(matches!(
            PaletteStore::new("/same", "/same", "/override"),
            Err(PaletteStoreError::InvalidConfiguration)
        ));
    }
}
