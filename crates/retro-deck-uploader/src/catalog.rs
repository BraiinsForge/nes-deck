//! Typed, bounded parsing and encoding for dashboard game catalogs.

use std::{
    collections::HashSet,
    fmt, io,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use crate::{
    file::{FileError, read_bounded_regular},
    rom::{System, SystemError},
};

/// Maximum catalog size shared with the dashboard.
pub const MAXIMUM_CATALOG_BYTES: u64 = 64 * 1_024;
/// Maximum combined built-in and uploaded game count.
pub const MAXIMUM_GAMES: usize = 64;
const MAXIMUM_LINE_BYTES: usize = 4_096;
const MAXIMUM_ID_BYTES: usize = 48;
const MAXIMUM_TITLE_CHARACTERS: usize = 64;
const MAXIMUM_PATH_BYTES: usize = 4_095;

/// A system name accepted by the dashboard catalog.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CatalogSystem {
    /// A ROM-backed console system.
    Rom(System),
    /// A native Deck application.
    Deck,
}

impl CatalogSystem {
    /// Canonical TSV field value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rom(system) => system.as_str(),
            Self::Deck => "deck",
        }
    }
}

impl fmt::Display for CatalogSystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<System> for CatalogSystem {
    fn from(system: System) -> Self {
        Self::Rom(system)
    }
}

impl FromStr for CatalogSystem {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "deck" {
            Ok(Self::Deck)
        } else {
            System::from_str(value).map(Self::Rom)
        }
    }
}

/// An RGB color constrained to the xterm-256 palette used for game tiles.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GameColor {
    red: u8,
    green: u8,
    blue: u8,
}

impl GameColor {
    /// Parse one `#RRGGBB` value present in the xterm-256 palette.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidField`] for malformed or non-palette
    /// colors.
    pub fn parse(value: &str) -> Result<Self, CatalogError> {
        let Some(hex) = value.strip_prefix('#') else {
            return Err(CatalogError::InvalidField("invalid game color"));
        };
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(CatalogError::InvalidField("invalid game color"));
        }
        let value = u32::from_str_radix(hex, 16)
            .map_err(|_| CatalogError::InvalidField("invalid game color"))?;
        let color = Self {
            red: u8::try_from((value >> 16) & 0xff)
                .map_err(|_| CatalogError::InvalidField("invalid game color"))?,
            green: u8::try_from((value >> 8) & 0xff)
                .map_err(|_| CatalogError::InvalidField("invalid game color"))?,
            blue: u8::try_from(value & 0xff)
                .map_err(|_| CatalogError::InvalidField("invalid game color"))?,
        };
        if (0_u16..=255).any(|index| Self::xterm(index) == color) {
            Ok(color)
        } else {
            Err(CatalogError::InvalidField(
                "game color is not in the xterm-256 palette",
            ))
        }
    }

    fn xterm(index: u16) -> Self {
        if index < 16 {
            let (red, green, blue) = match index {
                0 => (0, 0, 0),
                1 => (128, 0, 0),
                2 => (0, 128, 0),
                3 => (128, 128, 0),
                4 => (0, 0, 128),
                5 => (128, 0, 128),
                6 => (0, 128, 128),
                7 => (192, 192, 192),
                8 => (128, 128, 128),
                9 => (255, 0, 0),
                10 => (0, 255, 0),
                11 => (255, 255, 0),
                12 => (0, 0, 255),
                13 => (255, 0, 255),
                14 => (0, 255, 255),
                _ => (255, 255, 255),
            };
            Self { red, green, blue }
        } else if index < 232 {
            let cube = index - 16;
            Self {
                red: cube_level(cube / 36),
                green: cube_level((cube / 6) % 6),
                blue: cube_level(cube % 6),
            }
        } else {
            let level = u8::try_from(8 + (index - 232) * 10).unwrap_or_default();
            Self {
                red: level,
                green: level,
                blue: level,
            }
        }
    }
}

impl fmt::Display for GameColor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "#{:02X}{:02X}{:02X}",
            self.red, self.green, self.blue
        )
    }
}

const fn cube_level(index: u16) -> u8 {
    match index {
        0 => 0,
        1 => 95,
        2 => 135,
        3 => 175,
        4 => 215,
        _ => 255,
    }
}

/// One validated five-field dashboard catalog row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogEntry {
    identifier: String,
    title: String,
    system: CatalogSystem,
    rom: PathBuf,
    color: GameColor,
}

impl CatalogEntry {
    /// Construct and validate one entry independently of a surrounding file.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidField`] when any field violates the
    /// dashboard's identifier, text, system, path, or color contracts.
    pub fn new(
        identifier: &str,
        title: &str,
        system: CatalogSystem,
        rom: &str,
        color: &str,
    ) -> Result<Self, CatalogError> {
        if !valid_identifier(identifier) {
            return Err(CatalogError::InvalidField("invalid game identifier"));
        }
        if !valid_text(title, MAXIMUM_TITLE_CHARACTERS) {
            return Err(CatalogError::InvalidField("invalid game title"));
        }
        let rom = validate_path(rom, system)?;
        let color = GameColor::parse(color)?;
        Ok(Self {
            identifier: identifier.to_owned(),
            title: title.to_owned(),
            system,
            rom,
            color,
        })
    }

    /// Stable dashboard identifier.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "String slicing is not const on the pinned Rust toolchain"
    )]
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// User-facing title.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "String slicing is not const on the pinned Rust toolchain"
    )]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Console or Deck application identity.
    #[must_use]
    pub const fn system(&self) -> CatalogSystem {
        self.system
    }

    /// Absolute ROM or application-data path.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "PathBuf borrowing is not const on the pinned Rust toolchain"
    )]
    pub fn rom(&self) -> &Path {
        &self.rom
    }

    /// Tile color.
    #[must_use]
    pub const fn color(&self) -> GameColor {
        self.color
    }
}

/// A validated catalog with unique identifiers and data paths.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Catalog {
    entries: Vec<CatalogEntry>,
}

impl Catalog {
    /// Parse bounded UTF-8 TSV catalog bytes.
    ///
    /// Blank lines, comment lines, CRLF, and the dashboard's optional header
    /// are accepted. Every data row is validated and duplicate identifiers or
    /// paths are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for excessive input, invalid UTF-8, an
    /// excessive or malformed line, invalid fields, duplicates, or too many
    /// entries.
    pub fn parse(contents: &[u8]) -> Result<Self, CatalogError> {
        if u64::try_from(contents.len()).unwrap_or(u64::MAX) > MAXIMUM_CATALOG_BYTES {
            return Err(CatalogError::UnsafeFile("catalog exceeds its size limit"));
        }
        let text = std::str::from_utf8(contents).map_err(|_| CatalogError::NotUtf8)?;
        let mut entries = Vec::new();
        let mut identifiers = HashSet::new();
        let mut paths = HashSet::new();
        let mut saw_data = false;
        for (line_index, raw_line) in text.split('\n').enumerate() {
            let line_number = line_index.saturating_add(1);
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            if line.len() > MAXIMUM_LINE_BYTES {
                return Err(CatalogError::Line {
                    number: line_number,
                    reason: "line exceeds 4096 bytes",
                });
            }
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields = line.split('\t').collect::<Vec<_>>();
            if !saw_data && is_header(&fields) {
                saw_data = true;
                continue;
            }
            saw_data = true;
            let fields: [&str; 5] = fields.try_into().map_err(|_| CatalogError::Line {
                number: line_number,
                reason: "row must have exactly five TSV fields",
            })?;
            let system = CatalogSystem::from_str(fields[2]).map_err(|_| CatalogError::Line {
                number: line_number,
                reason: "invalid system",
            })?;
            let entry = CatalogEntry::new(fields[0], fields[1], system, fields[3], fields[4])
                .map_err(|error| error.at_line(line_number))?;
            if !identifiers.insert(entry.identifier.clone()) {
                return Err(CatalogError::Line {
                    number: line_number,
                    reason: "duplicate game identifier",
                });
            }
            if !paths.insert(entry.rom.clone()) {
                return Err(CatalogError::Line {
                    number: line_number,
                    reason: "duplicate ROM path",
                });
            }
            entries.push(entry);
            if entries.len() > MAXIMUM_GAMES {
                return Err(CatalogError::TooManyEntries);
            }
        }
        Ok(Self { entries })
    }

    /// Load a required bounded regular catalog without following a final
    /// symlink.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for file access or parse failures.
    pub fn load(path: &Path) -> Result<Self, CatalogError> {
        let file = read_bounded_regular(path, MAXIMUM_CATALOG_BYTES).map_err(map_file_error)?;
        Self::parse(&file.contents)
    }

    /// Load an optional catalog, treating only a missing final file as empty.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for all failures except a missing file.
    pub fn load_if_present(path: &Path) -> Result<Self, CatalogError> {
        match read_bounded_regular(path, MAXIMUM_CATALOG_BYTES) {
            Ok(file) => Self::parse(&file.contents),
            Err(FileError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                Ok(Self::default())
            }
            Err(error) => Err(map_file_error(error)),
        }
    }

    /// Encode canonical newline-terminated TSV without a header.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut output = String::new();
        for entry in &self.entries {
            use fmt::Write as _;
            let _ = writeln!(
                output,
                "{}\t{}\t{}\t{}\t{}",
                entry.identifier,
                entry.title,
                entry.system,
                entry.rom.display(),
                entry.color
            );
        }
        output.into_bytes()
    }

    /// Borrow entries in catalog order.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Vec slicing is not const on the pinned Rust toolchain"
    )]
    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }

    /// Number of entries.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Vec length is not const on the pinned Rust toolchain"
    )]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the catalog contains no entries.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Vec emptiness is not const on the pinned Rust toolchain"
    )]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Catalog access or validation failure.
#[derive(Debug)]
pub enum CatalogError {
    /// Filesystem access failed.
    Io(io::Error),
    /// File type or bound is unsafe.
    UnsafeFile(&'static str),
    /// Catalog bytes are not valid UTF-8.
    NotUtf8,
    /// A specific row is malformed.
    Line { number: usize, reason: &'static str },
    /// A field is invalid outside line-oriented parsing.
    InvalidField(&'static str),
    /// Catalog exceeds the dashboard's touch-target capacity.
    TooManyEntries,
}

impl CatalogError {
    fn at_line(self, number: usize) -> Self {
        match self {
            Self::InvalidField(reason) => Self::Line { number, reason },
            error => error,
        }
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::UnsafeFile(reason) | Self::InvalidField(reason) => formatter.write_str(reason),
            Self::NotUtf8 => formatter.write_str("catalog is not UTF-8"),
            Self::Line { number, reason } => {
                write!(formatter, "catalog line {number}: {reason}")
            }
            Self::TooManyEntries => {
                write!(formatter, "catalog has more than {MAXIMUM_GAMES} entries")
            }
        }
    }
}

impl std::error::Error for CatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

fn map_file_error(error: FileError) -> CatalogError {
    match error {
        FileError::Io(error) => CatalogError::Io(error),
        FileError::Unsafe(reason) => CatalogError::UnsafeFile(reason),
        FileError::Random(_) => CatalogError::UnsafeFile("unexpected random-name failure"),
    }
}

fn valid_identifier(identifier: &str) -> bool {
    if identifier.is_empty() || identifier.len() > MAXIMUM_ID_BYTES {
        return false;
    }
    let mut previous_hyphen = false;
    for (index, byte) in identifier.bytes().enumerate() {
        let valid = byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
        if !valid || (index == 0 && byte == b'-') || (byte == b'-' && previous_hyphen) {
            return false;
        }
        previous_hyphen = byte == b'-';
    }
    !identifier.ends_with('-')
}

fn valid_text(value: &str, maximum_characters: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= maximum_characters
        && !value.chars().any(char::is_control)
}

fn validate_path(value: &str, system: CatalogSystem) -> Result<PathBuf, CatalogError> {
    if value.len() > MAXIMUM_PATH_BYTES || !valid_text(value, MAXIMUM_PATH_BYTES) {
        return Err(CatalogError::InvalidField("invalid ROM path"));
    }
    let path = PathBuf::from(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(CatalogError::InvalidField("invalid ROM path"));
    }
    if let CatalogSystem::Rom(rom_system) = system {
        let expected = rom_system.extension().trim_start_matches('.');
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
        {
            return Err(CatalogError::InvalidField(
                "ROM path extension does not match its system",
            ));
        }
    }
    Ok(path)
}

fn is_header(fields: &[&str]) -> bool {
    matches!(
        fields,
        ["id", "title", "system", "rom", "color" | "#RRGGBB"]
    )
}

#[cfg(test)]
mod tests {
    use super::{Catalog, CatalogError, CatalogSystem, GameColor, MAXIMUM_GAMES};
    use crate::rom::System;
    use std::{fs, os::unix::fs::symlink};

    const DEPLOYED_CATALOG: &[u8] = include_bytes!("../../../deploy/menu/games.tsv");

    #[test]
    fn parses_the_deployed_catalog_and_round_trips_canonically() {
        let catalog = Catalog::parse(DEPLOYED_CATALOG);
        assert!(matches!(catalog, Ok(ref catalog) if catalog.len() == 15));
        let Some(catalog) = catalog.ok() else {
            return;
        };
        assert_eq!(catalog.encode(), DEPLOYED_CATALOG);
        assert!(matches!(
            catalog.entries().first(),
            Some(entry)
                if entry.identifier() == "mario"
                    && entry.title() == "SUPER MARIO BROS."
                    && entry.system() == CatalogSystem::Rom(System::Nes)
                    && entry.rom().ends_with("super-mario-bros.nes")
                    && entry.color().to_string() == "#D78787"
        ));
    }

    #[test]
    fn accepts_comments_crlf_and_the_optional_header() {
        let contents = b"# generated\r\nid\ttitle\tsystem\trom\t#RRGGBB\r\n\
upload-nes-test\tTEST\tnes\t/mnt/data/roms/nes/test.nes\t#ff5f00\r\n";
        let catalog = Catalog::parse(contents);
        assert!(matches!(catalog, Ok(ref catalog) if catalog.len() == 1));
        let Some(catalog) = catalog.ok() else {
            return;
        };
        assert_eq!(
            catalog.encode(),
            b"upload-nes-test\tTEST\tnes\t/mnt/data/roms/nes/test.nes\t#FF5F00\n"
        );
    }

    #[test]
    fn rejects_invalid_fields_duplicates_and_excess() {
        for contents in [
            "-bad\tTITLE\tnes\t/mnt/data/roms/nes/a.nes\t#FF5F00\n",
            "bad--id\tTITLE\tnes\t/mnt/data/roms/nes/a.nes\t#FF5F00\n",
            "bad\t padded\tnes\t/mnt/data/roms/nes/a.nes\t#FF5F00\n",
            "bad\tTITLE\tother\t/mnt/data/roms/nes/a.nes\t#FF5F00\n",
            "bad\tTITLE\tnes\trelative.nes\t#FF5F00\n",
            "bad\tTITLE\tnes\t/mnt/data/roms/nes/a.gb\t#FF5F00\n",
            "bad\tTITLE\tnes\t/mnt/data/roms/nes/a.nes\t#010203\n",
        ] {
            assert!(
                Catalog::parse(contents.as_bytes()).is_err(),
                "accepted {contents:?}"
            );
        }
        let duplicates = b"one\tONE\tnes\t/mnt/data/roms/nes/a.nes\t#FF5F00\n\
one\tTWO\tnes\t/mnt/data/roms/nes/b.nes\t#FF5F00\n";
        assert!(matches!(
            Catalog::parse(duplicates),
            Err(CatalogError::Line {
                reason: "duplicate game identifier",
                ..
            })
        ));
        let mut excessive = String::new();
        for index in 0..=MAXIMUM_GAMES {
            use std::fmt::Write as _;
            assert!(writeln!(excessive, "g{index}\tG{index}\tdeck\t/app/{index}\t#000000").is_ok());
        }
        assert!(matches!(
            Catalog::parse(excessive.as_bytes()),
            Err(CatalogError::TooManyEntries)
        ));
    }

    #[test]
    fn loads_only_bounded_regular_or_missing_optional_catalogs() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let missing = directory.path().join("missing.tsv");
        assert!(matches!(Catalog::load_if_present(&missing), Ok(catalog) if catalog.is_empty()));
        assert!(Catalog::load(&missing).is_err());

        let catalog_path = directory.path().join("games.tsv");
        assert!(fs::write(&catalog_path, DEPLOYED_CATALOG).is_ok());
        assert!(matches!(Catalog::load(&catalog_path), Ok(catalog) if catalog.len() == 15));
        let link = directory.path().join("link.tsv");
        assert!(symlink(&catalog_path, &link).is_ok());
        assert!(Catalog::load(&link).is_err());
    }

    #[test]
    fn game_colors_are_typed_and_canonical() {
        assert!(matches!(GameColor::parse("#ff5f00"), Ok(color) if color.to_string() == "#FF5F00"));
        for invalid in ["FF5F00", "#FFF", "#GG0000", "#010203"] {
            assert!(GameColor::parse(invalid).is_err());
        }
    }
}
