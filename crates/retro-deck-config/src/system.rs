//! Typed identity and file conventions for ROM-backed systems.

use std::{fmt, str::FromStr};

/// A console accepted by the catalog and ROM intake.
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

    /// Default dashboard accent retained from the deployed catalog.
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

#[cfg(test)]
mod tests {
    use super::System;
    use std::str::FromStr as _;

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
}
