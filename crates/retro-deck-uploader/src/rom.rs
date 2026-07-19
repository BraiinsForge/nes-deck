//! Typed ROM-system, title, filename, and raw format validation.

use std::{
    ffi::OsStr,
    fmt,
    io::{Cursor, Read},
    path::Path,
    str::FromStr,
};

use zip::ZipArchive;

/// Maximum compressed or raw upload size accepted before format inspection.
pub const MAXIMUM_ARCHIVE_BYTES: usize = 10 * 1_024 * 1_024;

const NINTENDO_LOGO: [u8; 48] = [
    0xce, 0xed, 0x66, 0x66, 0xcc, 0x0d, 0x00, 0x0b, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0c, 0x00, 0x0d,
    0x00, 0x08, 0x11, 0x1f, 0x88, 0x89, 0x00, 0x0e, 0xdc, 0xcc, 0x6e, 0xe6, 0xdd, 0xdd, 0xd9, 0x99,
    0xbb, 0xbb, 0x67, 0x63, 0x6e, 0x0e, 0xec, 0xcc, 0xdd, 0xdc, 0x99, 0x9f, 0xbb, 0xb9, 0x33, 0x3e,
];

/// A console accepted by ROM intake.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum System {
    /// Nintendo Entertainment System.
    Nes,
    /// Original Game Boy.
    GameBoy,
    /// Game Boy Color.
    GameBoyColor,
    /// ZX Spectrum TAP image.
    ZxSpectrum,
    /// CHIP-8 program.
    Chip8,
}

impl System {
    /// Canonical catalog and directory shorthand.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nes => "nes",
            Self::GameBoy => "gb",
            Self::GameBoyColor => "gbc",
            Self::ZxSpectrum => "zx",
            Self::Chip8 => "chip8",
        }
    }

    /// Required lowercase ROM filename suffix.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Nes => ".nes",
            Self::GameBoy => ".gb",
            Self::GameBoyColor => ".gbc",
            Self::ZxSpectrum => ".tap",
            Self::Chip8 => ".ch8",
        }
    }

    /// Default dashboard accent retained from the deployed uploader.
    #[must_use]
    pub const fn color(self) -> &'static str {
        match self {
            Self::Nes => "#FF5F00",
            Self::GameBoy => "#87AF87",
            Self::GameBoyColor => "#5F87D7",
            Self::ZxSpectrum => "#AF87D7",
            Self::Chip8 => "#5FD7D7",
        }
    }

    /// Maximum accepted uncompressed ROM size.
    #[must_use]
    pub const fn maximum_bytes(self) -> usize {
        match self {
            Self::Chip8 => 65_024,
            Self::Nes | Self::GameBoy | Self::GameBoyColor | Self::ZxSpectrum => 8 * 1_024 * 1_024,
        }
    }
}

impl fmt::Display for System {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for System {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "nes" => Ok(Self::Nes),
            "gb" => Ok(Self::GameBoy),
            "gbc" => Ok(Self::GameBoyColor),
            "zx" => Ok(Self::ZxSpectrum),
            "chip8" => Ok(Self::Chip8),
            _ => Err(SystemError),
        }
    }
}

/// A string does not name one of the supported systems.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SystemError;

impl fmt::Display for SystemError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("choose a supported system")
    }
}

impl std::error::Error for SystemError {}

/// A validated user-facing title.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameTitle(String);

impl GameTitle {
    /// Validate a title without trimming or silently changing it.
    ///
    /// # Errors
    ///
    /// Returns [`TitleError`] for an empty, surrounding-whitespace,
    /// control-bearing, tab-bearing, or longer-than-64-character title.
    pub fn new(value: &str) -> Result<Self, TitleError> {
        if value.is_empty()
            || value.trim() != value
            || value.chars().count() > 64
            || value
                .chars()
                .any(|character| character.is_control() || character == '\t')
        {
            return Err(TitleError::Invalid);
        }
        if slugify(value).is_empty() {
            return Err(TitleError::NoAsciiFilename);
        }
        Ok(Self(value.to_owned()))
    }

    /// Original title text for catalog display.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "String slicing is not const on the pinned Rust toolchain"
    )]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Stable lowercase ASCII filename stem of at most 32 bytes.
    #[must_use]
    pub fn slug(&self) -> String {
        slugify(&self.0)
    }
}

impl fmt::Display for GameTitle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A title cannot be represented safely in the catalog or filesystem.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TitleError {
    /// Text violates the display-title contract.
    Invalid,
    /// Text has no ASCII letter or digit from which to form a filename.
    NoAsciiFilename,
}

impl fmt::Display for TitleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => {
                formatter.write_str("enter a title of 1 through 64 printable characters")
            }
            Self::NoAsciiFilename => formatter
                .write_str("the title needs at least one ASCII letter or number for its filename"),
        }
    }
}

impl std::error::Error for TitleError {}

/// Bytes whose system-specific ROM structure has been validated.
pub struct ValidatedRom {
    system: System,
    bytes: Vec<u8>,
}

impl ValidatedRom {
    /// Validate uncompressed ROM bytes for `system`.
    ///
    /// # Errors
    ///
    /// Returns [`RomError`] for an empty or excessive payload, or for a
    /// malformed system-specific header, checksum, or block structure.
    pub fn new(system: System, bytes: Vec<u8>) -> Result<Self, RomError> {
        if bytes.is_empty() || bytes.len() > system.maximum_bytes() {
            return Err(RomError::InvalidSize {
                maximum: system.maximum_bytes(),
            });
        }
        match system {
            System::Nes => validate_nes(&bytes)?,
            System::GameBoy => validate_game_boy(&bytes, false)?,
            System::GameBoyColor => validate_game_boy(&bytes, true)?,
            System::ZxSpectrum => validate_zx(&bytes)?,
            System::Chip8 => {}
        }
        Ok(Self { system, bytes })
    }

    /// Validated console identity.
    #[must_use]
    pub const fn system(&self) -> System {
        self.system
    }

    /// Borrow the validated bytes.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Vec slicing is not const on the pinned Rust toolchain"
    )]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume the wrapper and return its bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl fmt::Debug for ValidatedRom {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValidatedRom")
            .field("system", &self.system)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// Raw ROM validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RomError {
    /// Payload size is outside the selected system's contract.
    InvalidSize { maximum: usize },
    /// NES payload lacks the iNES magic header.
    MissingNesHeader,
    /// Game Boy payload lacks Nintendo's header logo.
    MissingGameBoyHeader,
    /// Game Boy header checksum does not match.
    InvalidGameBoyChecksum,
    /// A GBC upload does not advertise color support.
    NotGameBoyColor,
    /// TAP payload is too short to contain a block.
    ZxTooShort,
    /// TAP payload ends inside a block header.
    ZxTruncatedHeader,
    /// TAP block length exceeds the remaining payload or is too short.
    InvalidZxBlockLength,
    /// TAP block checksum does not XOR to zero.
    InvalidZxChecksum,
}

impl fmt::Display for RomError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSize { maximum } => {
                write!(formatter, "the ROM must contain 1 through {maximum} bytes")
            }
            Self::MissingNesHeader => formatter.write_str("the file has no iNES header"),
            Self::MissingGameBoyHeader => {
                formatter.write_str("the file has no valid Game Boy header")
            }
            Self::InvalidGameBoyChecksum => {
                formatter.write_str("the Game Boy header checksum is invalid")
            }
            Self::NotGameBoyColor => {
                formatter.write_str("the ROM does not advertise Game Boy Color support")
            }
            Self::ZxTooShort => formatter.write_str("the TAP file is too short"),
            Self::ZxTruncatedHeader => {
                formatter.write_str("the TAP file ends inside a block header")
            }
            Self::InvalidZxBlockLength => {
                formatter.write_str("the TAP file has an invalid block length")
            }
            Self::InvalidZxChecksum => {
                formatter.write_str("the TAP file has an invalid block checksum")
            }
        }
    }
}

impl std::error::Error for RomError {}

/// Raw-upload or single-ROM ZIP decoding failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadError {
    /// The request body could not be read completely.
    CouldNotRead,
    /// The compressed or raw request exceeds ten MiB.
    ArchiveTooLarge,
    /// The request filename has neither the selected ROM suffix nor `.zip`.
    WrongUploadExtension { required: &'static str },
    /// ZIP central-directory or member metadata is malformed.
    MalformedZip,
    /// ZIP has zero or multiple non-directory members.
    WrongMemberCount,
    /// ZIP member is encrypted, path-bearing, a link, or another special file.
    UnsafeOrEncryptedMember,
    /// The single ZIP member has the wrong suffix or advertised size.
    InvalidMember { required: &'static str },
    /// The ZIP member cannot be decompressed or opened.
    CouldNotOpenMember,
    /// Decompressed ZIP data fails its bounded read or integrity check.
    CouldNotReadMember,
    /// Uncompressed bytes fail their system-specific ROM validation.
    InvalidRom(RomError),
}

impl fmt::Display for UploadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CouldNotRead => formatter.write_str("the upload could not be read"),
            Self::ArchiveTooLarge => formatter.write_str("the upload exceeds 10 MiB"),
            Self::WrongUploadExtension { required } => {
                write!(
                    formatter,
                    "choose a {required} file or a ZIP containing one"
                )
            }
            Self::MalformedZip => formatter.write_str("the ZIP file is malformed"),
            Self::WrongMemberCount => formatter.write_str("the ZIP must contain exactly one ROM"),
            Self::UnsafeOrEncryptedMember => {
                formatter.write_str("the ZIP contains an unsafe or encrypted member")
            }
            Self::InvalidMember { required } => write!(
                formatter,
                "the ZIP must contain one {required} file of a valid size"
            ),
            Self::CouldNotOpenMember => formatter.write_str("the ZIP member could not be opened"),
            Self::CouldNotReadMember => {
                formatter.write_str("the ZIP member could not be read safely")
            }
            Self::InvalidRom(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for UploadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidRom(error) => Some(error),
            _ => None,
        }
    }
}

impl From<RomError> for UploadError {
    fn from(error: RomError) -> Self {
        Self::InvalidRom(error)
    }
}

/// Read and validate a raw ROM or a ZIP containing exactly one ROM.
///
/// # Errors
///
/// Returns [`UploadError`] when the input exceeds its compressed or
/// uncompressed bounds, has an unsafe archive structure, uses a wrong suffix,
/// cannot be decompressed, or fails system-specific ROM validation.
pub fn decode_upload(
    system: System,
    filename: &str,
    input: impl Read,
) -> Result<ValidatedRom, UploadError> {
    let archive = read_upload(input)?;
    if has_extension(filename, system.extension()) {
        return ValidatedRom::new(system, archive).map_err(Into::into);
    }
    if !has_extension(filename, ".zip") {
        return Err(UploadError::WrongUploadExtension {
            required: system.extension(),
        });
    }
    decode_zip(system, archive)
}

fn read_upload(input: impl Read) -> Result<Vec<u8>, UploadError> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAXIMUM_ARCHIVE_BYTES.saturating_add(1))
        .map_err(|_| UploadError::CouldNotRead)?;
    input
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| UploadError::CouldNotRead)?;
    if bytes.len() > MAXIMUM_ARCHIVE_BYTES {
        Err(UploadError::ArchiveTooLarge)
    } else {
        Ok(bytes)
    }
}

fn decode_zip(system: System, archive: Vec<u8>) -> Result<ValidatedRom, UploadError> {
    let mut archive =
        ZipArchive::new(Cursor::new(archive)).map_err(|_| UploadError::MalformedZip)?;
    let mut member_index = None;
    for index in 0..archive.len() {
        let member = archive
            .by_index_raw(index)
            .map_err(|_| UploadError::MalformedZip)?;
        if member.is_dir() {
            continue;
        }
        if member_index.replace(index).is_some() {
            return Err(UploadError::WrongMemberCount);
        }
        if member.encrypted() || !member.is_file() || !is_plain_member_name(member.name()) {
            return Err(UploadError::UnsafeOrEncryptedMember);
        }
        if !has_extension(member.name(), system.extension())
            || member.size()
                > u64::try_from(system.maximum_bytes())
                    .map_err(|_| UploadError::CouldNotReadMember)?
        {
            return Err(UploadError::InvalidMember {
                required: system.extension(),
            });
        }
    }
    let index = member_index.ok_or(UploadError::WrongMemberCount)?;
    let mut member = archive
        .by_index(index)
        .map_err(|_| UploadError::CouldNotOpenMember)?;
    let maximum = system.maximum_bytes();
    let limit =
        u64::try_from(maximum.saturating_add(1)).map_err(|_| UploadError::CouldNotReadMember)?;
    let mut bytes = Vec::new();
    member
        .by_ref()
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| UploadError::CouldNotReadMember)?;
    if bytes.len() > maximum {
        return Err(UploadError::CouldNotReadMember);
    }
    ValidatedRom::new(system, bytes).map_err(Into::into)
}

fn has_extension(filename: &str, extension: &str) -> bool {
    Path::new(filename)
        .extension()
        .and_then(OsStr::to_str)
        .zip(extension.strip_prefix('.'))
        .is_some_and(|(actual, required)| actual.eq_ignore_ascii_case(required))
}

fn is_plain_member_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\', '\0'])
        && Path::new(name).file_name() == Some(OsStr::new(name))
}

fn slugify(title: &str) -> String {
    let mut slug = String::with_capacity(32);
    let mut last_was_hyphen = false;
    for character in title.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_alphanumeric() {
            if slug.len() >= 32 {
                break;
            }
            slug.push(character);
            last_was_hyphen = false;
        } else if !slug.is_empty() && !last_was_hyphen && slug.len() < 32 {
            slug.push('-');
            last_was_hyphen = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

fn validate_nes(bytes: &[u8]) -> Result<(), RomError> {
    if bytes.len() < 16 || !bytes.starts_with(b"NES\x1a") {
        Err(RomError::MissingNesHeader)
    } else {
        Ok(())
    }
}

fn validate_game_boy(bytes: &[u8], color_only: bool) -> Result<(), RomError> {
    if bytes.len() < 0x150 || bytes.get(0x104..0x134) != Some(NINTENDO_LOGO.as_slice()) {
        return Err(RomError::MissingGameBoyHeader);
    }
    let checksum = bytes
        .get(0x134..=0x14c)
        .ok_or(RomError::MissingGameBoyHeader)?
        .iter()
        .fold(0_u8, |checksum, byte| {
            checksum.wrapping_sub(*byte).wrapping_sub(1)
        });
    if bytes.get(0x14d) != Some(&checksum) {
        return Err(RomError::InvalidGameBoyChecksum);
    }
    if color_only && !matches!(bytes.get(0x143), Some(0x80 | 0xc0)) {
        return Err(RomError::NotGameBoyColor);
    }
    Ok(())
}

fn validate_zx(bytes: &[u8]) -> Result<(), RomError> {
    if bytes.len() < 4 {
        return Err(RomError::ZxTooShort);
    }
    let mut offset = 0_usize;
    let mut blocks = 0_usize;
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset.saturating_add(2))
            .ok_or(RomError::ZxTruncatedHeader)?;
        let block_size = usize::from(u16::from_le_bytes(
            header.try_into().map_err(|_| RomError::ZxTruncatedHeader)?,
        ));
        offset = offset.saturating_add(2);
        let block = bytes
            .get(offset..offset.saturating_add(block_size))
            .ok_or(RomError::InvalidZxBlockLength)?;
        if block_size < 2 {
            return Err(RomError::InvalidZxBlockLength);
        }
        if block.iter().fold(0_u8, |checksum, byte| checksum ^ byte) != 0 {
            return Err(RomError::InvalidZxChecksum);
        }
        offset = offset.saturating_add(block_size);
        blocks = blocks.saturating_add(1);
    }
    if blocks == 0 {
        Err(RomError::ZxTooShort)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GameTitle, MAXIMUM_ARCHIVE_BYTES, NINTENDO_LOGO, RomError, System, UploadError,
        ValidatedRom, decode_upload,
    };
    use std::{
        io::{Cursor, Write as _},
        str::FromStr as _,
    };
    use zip::{CompressionMethod, ZipWriter, result::ZipError, write::SimpleFileOptions};

    fn nes_rom() -> Vec<u8> {
        let mut rom = vec![0_u8; 16 + 16_384];
        if let Some(header) = rom.get_mut(..4) {
            header.copy_from_slice(b"NES\x1a");
        }
        if let Some(prg_banks) = rom.get_mut(4) {
            *prg_banks = 1;
        }
        rom
    }

    fn zip_members(members: &[(&str, &[u8])]) -> Result<Vec<u8>, ZipError> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for &(name, contents) in members {
            writer.start_file(name, options)?;
            writer.write_all(contents)?;
        }
        Ok(writer.finish()?.into_inner())
    }

    fn zip_symlink(name: &str) -> Result<Vec<u8>, ZipError> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer.add_symlink(name, "target", SimpleFileOptions::default())?;
        Ok(writer.finish()?.into_inner())
    }

    fn mark_first_member_encrypted(archive: &mut [u8]) {
        for (signature, flag_offset) in [(b"PK\x03\x04", 6_usize), (b"PK\x01\x02", 8_usize)] {
            if let Some(header_offset) = archive
                .windows(signature.len())
                .position(|window| window == signature)
            {
                let flag_index = header_offset.saturating_add(flag_offset);
                if let Some(flag) = archive.get_mut(flag_index) {
                    *flag |= 1;
                }
            }
        }
    }

    fn game_boy_rom(color_flag: u8) -> Vec<u8> {
        let mut rom = vec![0_u8; 0x150];
        if let Some(logo) = rom.get_mut(0x104..0x134) {
            logo.copy_from_slice(&NINTENDO_LOGO);
        }
        if let Some(flag) = rom.get_mut(0x143) {
            *flag = color_flag;
        }
        let checksum = rom
            .get(0x134..=0x14c)
            .into_iter()
            .flatten()
            .fold(0_u8, |checksum, byte| {
                checksum.wrapping_sub(*byte).wrapping_sub(1)
            });
        if let Some(stored) = rom.get_mut(0x14d) {
            *stored = checksum;
        }
        rom
    }

    #[test]
    fn system_contracts_are_typed_and_exact() {
        for (name, system, extension, color) in [
            ("nes", System::Nes, ".nes", "#FF5F00"),
            ("gb", System::GameBoy, ".gb", "#87AF87"),
            ("gbc", System::GameBoyColor, ".gbc", "#5F87D7"),
            ("zx", System::ZxSpectrum, ".tap", "#AF87D7"),
            ("chip8", System::Chip8, ".ch8", "#5FD7D7"),
        ] {
            assert_eq!(System::from_str(name), Ok(system));
            assert_eq!(system.as_str(), name);
            assert_eq!(system.extension(), extension);
            assert_eq!(system.color(), color);
        }
        for rejected in ["", "GB", "gameboy", "chip-8", "deck"] {
            assert!(System::from_str(rejected).is_err());
        }
    }

    #[test]
    fn title_validation_and_slugging_match_deployed_intake() {
        let title = GameTitle::new("Žlutý HERO: Return!!!");
        assert!(matches!(title, Ok(title) if title.slug() == "lut-hero-return"));
        let long_ascii = GameTitle::new("A title whose filename is deliberately much longer");
        assert!(matches!(
            long_ascii,
            Ok(title) if title.slug() == "a-title-whose-filename-is-delibe"
        ));
        for rejected in ["", " padded", "padded ", "line\nbreak", "tab\tvalue", "♥♥♥"] {
            assert!(GameTitle::new(rejected).is_err(), "accepted {rejected:?}");
        }
        assert!(GameTitle::new(&"x".repeat(65)).is_err());
    }

    #[test]
    fn validates_nes_and_chip8_size_contracts() {
        let mut nes = vec![0_u8; 16];
        if let Some(header) = nes.get_mut(..4) {
            header.copy_from_slice(b"NES\x1a");
        }
        assert!(ValidatedRom::new(System::Nes, nes).is_ok());
        assert!(matches!(
            ValidatedRom::new(System::Nes, vec![0_u8; 16]),
            Err(RomError::MissingNesHeader)
        ));
        assert!(ValidatedRom::new(System::Chip8, vec![0; 65_024]).is_ok());
        assert!(matches!(
            ValidatedRom::new(System::Chip8, vec![0; 65_025]),
            Err(RomError::InvalidSize { maximum: 65_024 })
        ));
    }

    #[test]
    fn validates_game_boy_header_checksum_and_color_flag() {
        assert!(ValidatedRom::new(System::GameBoy, game_boy_rom(0)).is_ok());
        assert!(ValidatedRom::new(System::GameBoyColor, game_boy_rom(0x80)).is_ok());
        assert!(matches!(
            ValidatedRom::new(System::GameBoyColor, game_boy_rom(0)),
            Err(RomError::NotGameBoyColor)
        ));
        let mut corrupt = game_boy_rom(0x80);
        if let Some(byte) = corrupt.get_mut(0x134) {
            *byte ^= 1;
        }
        assert!(matches!(
            ValidatedRom::new(System::GameBoy, corrupt),
            Err(RomError::InvalidGameBoyChecksum)
        ));
    }

    #[test]
    fn validates_every_zx_tap_block() {
        assert!(
            ValidatedRom::new(
                System::ZxSpectrum,
                vec![3, 0, 0, 0x42, 0x42, 2, 0, 0xaa, 0xaa]
            )
            .is_ok()
        );
        for (bytes, expected) in [
            (vec![0, 0, 0, 0], RomError::InvalidZxBlockLength),
            (vec![3, 0, 0, 1, 1, 0], RomError::ZxTruncatedHeader),
            (vec![3, 0, 0, 1, 0], RomError::InvalidZxChecksum),
            (vec![4, 0, 0, 1], RomError::InvalidZxBlockLength),
        ] {
            assert!(matches!(
                ValidatedRom::new(System::ZxSpectrum, bytes),
                Err(error) if error == expected
            ));
        }
    }

    #[test]
    fn decodes_raw_uploads_with_both_case_variants() {
        let rom = nes_rom();
        for filename in ["game.nes", "GAME.NES"] {
            let decoded = decode_upload(System::Nes, filename, rom.as_slice());
            assert!(matches!(
                decoded,
                Ok(decoded) if decoded.as_bytes() == rom
            ));
        }
        assert!(matches!(
            decode_upload(System::Nes, "game.gb", rom.as_slice()),
            Err(UploadError::WrongUploadExtension { required: ".nes" })
        ));
        assert!(matches!(
            decode_upload(
                System::Nes,
                "large.nes",
                vec![0_u8; MAXIMUM_ARCHIVE_BYTES + 1].as_slice()
            ),
            Err(UploadError::ArchiveTooLarge)
        ));
    }

    #[test]
    fn decodes_one_compressed_root_rom() -> Result<(), ZipError> {
        let rom = nes_rom();
        let archive = zip_members(&[("game.nes", rom.as_slice())])?;
        let decoded = decode_upload(System::Nes, "collection.ZIP", archive.as_slice());
        assert!(matches!(
            decoded,
            Ok(decoded) if decoded.as_bytes() == rom
        ));
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_or_unsafe_zip_members() -> Result<(), ZipError> {
        let rom = nes_rom();
        let multiple = zip_members(&[("one.nes", rom.as_slice()), ("two.nes", rom.as_slice())])?;
        assert!(matches!(
            decode_upload(System::Nes, "games.zip", multiple.as_slice()),
            Err(UploadError::WrongMemberCount)
        ));
        for name in ["../game.nes", "folder/game.nes", "folder\\game.nes"] {
            let unsafe_archive = zip_members(&[(name, rom.as_slice())])?;
            assert!(matches!(
                decode_upload(System::Nes, "game.zip", unsafe_archive.as_slice()),
                Err(UploadError::UnsafeOrEncryptedMember)
            ));
        }
        let symlink = zip_symlink("game.nes")?;
        assert!(matches!(
            decode_upload(System::Nes, "game.zip", symlink.as_slice()),
            Err(UploadError::UnsafeOrEncryptedMember)
        ));
        let mut encrypted = zip_members(&[("game.nes", rom.as_slice())])?;
        mark_first_member_encrypted(&mut encrypted);
        assert!(matches!(
            decode_upload(System::Nes, "game.zip", encrypted.as_slice()),
            Err(UploadError::UnsafeOrEncryptedMember)
        ));
        Ok(())
    }

    #[test]
    fn rejects_malformed_wrong_type_and_oversized_zip_members() -> Result<(), ZipError> {
        assert!(matches!(
            decode_upload(System::Nes, "game.zip", b"not a ZIP".as_slice()),
            Err(UploadError::MalformedZip)
        ));
        let wrong_type = zip_members(&[("game.gb", nes_rom().as_slice())])?;
        assert!(matches!(
            decode_upload(System::Nes, "game.zip", wrong_type.as_slice()),
            Err(UploadError::InvalidMember { required: ".nes" })
        ));
        let oversized = vec![0_u8; System::Chip8.maximum_bytes() + 1];
        let archive = zip_members(&[("large.ch8", oversized.as_slice())])?;
        assert!(matches!(
            decode_upload(System::Chip8, "large.zip", archive.as_slice()),
            Err(UploadError::InvalidMember { required: ".ch8" })
        ));
        let empty = zip_members(&[])?;
        assert!(matches!(
            decode_upload(System::Nes, "empty.zip", empty.as_slice()),
            Err(UploadError::WrongMemberCount)
        ));
        Ok(())
    }
}
