//! Bounded, no-follow loading for a CHIP-8 ROM and optional sidecar.

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use rustix::fs::{Mode, OFlags, open};

use super::{ConfigError, Configuration, MAXIMUM_CONFIG_BYTES, MAXIMUM_ROM_BYTES};

/// Complete validated program input ready for core initialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    rom: Vec<u8>,
    configuration: Configuration,
}

impl Program {
    /// Load one regular ROM and its optional `<ROM>.cfg` sidecar.
    ///
    /// The final path component cannot be a symlink. Both inputs are read
    /// through already-open descriptors, bounded one byte beyond their limit,
    /// and rejected if their size changes during the read.
    ///
    /// # Errors
    ///
    /// Returns [`ProgramError`] for filesystem, type, size, concurrent-change,
    /// or sidecar schema failures.
    pub fn load(path: &Path) -> Result<Self, ProgramError> {
        let rom = read_bounded_regular(path, FileRole::Rom, 1, MAXIMUM_ROM_BYTES)?;
        let config_path = config_path(path);
        let configuration =
            match read_bounded_regular(&config_path, FileRole::Config, 0, MAXIMUM_CONFIG_BYTES) {
                Ok(bytes) => Configuration::parse(&bytes).map_err(|source| ProgramError {
                    path: config_path,
                    failure: ProgramFailure::InvalidConfig(source),
                })?,
                Err(error) if error.not_found() => Configuration::default(),
                Err(error) => return Err(error),
            };
        Ok(Self { rom, configuration })
    }

    /// Complete ROM bytes.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Vec slice access is not const on the supported Rust toolchain"
    )]
    pub fn rom(&self) -> &[u8] {
        &self.rom
    }

    /// Validated core and input configuration.
    #[must_use]
    pub const fn configuration(&self) -> Configuration {
        self.configuration
    }
}

fn config_path(rom: &Path) -> PathBuf {
    let mut name = rom.as_os_str().to_owned();
    name.push(".cfg");
    PathBuf::from(name)
}

fn read_bounded_regular(
    path: &Path,
    role: FileRole,
    minimum_bytes: usize,
    maximum_bytes: usize,
) -> Result<Vec<u8>, ProgramError> {
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|source| ProgramError {
        path: path.to_owned(),
        failure: ProgramFailure::Open {
            role,
            source: io::Error::from(source),
        },
    })?;
    let mut file = File::from(descriptor);
    let metadata = file.metadata().map_err(|source| ProgramError {
        path: path.to_owned(),
        failure: ProgramFailure::Read { role, source },
    })?;
    if !metadata.is_file() {
        return Err(ProgramError {
            path: path.to_owned(),
            failure: ProgramFailure::NotRegular { role },
        });
    }
    let expected = usize::try_from(metadata.len()).map_err(|_| ProgramError {
        path: path.to_owned(),
        failure: ProgramFailure::InvalidSize {
            role,
            bytes: usize::MAX,
            minimum: minimum_bytes,
            maximum: maximum_bytes,
        },
    })?;
    if expected < minimum_bytes || expected > maximum_bytes {
        return Err(ProgramError {
            path: path.to_owned(),
            failure: ProgramFailure::InvalidSize {
                role,
                bytes: expected,
                minimum: minimum_bytes,
                maximum: maximum_bytes,
            },
        });
    }

    let maximum_read = u64::try_from(maximum_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut bytes = Vec::with_capacity(expected);
    file.by_ref()
        .take(maximum_read)
        .read_to_end(&mut bytes)
        .map_err(|source| ProgramError {
            path: path.to_owned(),
            failure: ProgramFailure::Read { role, source },
        })?;
    if bytes.len() != expected {
        return Err(ProgramError {
            path: path.to_owned(),
            failure: ProgramFailure::Changed {
                role,
                expected,
                actual: bytes.len(),
            },
        });
    }
    Ok(bytes)
}

/// ROM or sidecar loading failure with its source path retained.
#[derive(Debug)]
pub struct ProgramError {
    path: PathBuf,
    failure: ProgramFailure,
}

impl ProgramError {
    fn not_found(&self) -> bool {
        matches!(
            &self.failure,
            ProgramFailure::Open { source, .. } if source.kind() == io::ErrorKind::NotFound
        )
    }
}

impl fmt::Display for ProgramError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.failure {
            ProgramFailure::Open { role, source } => {
                write!(
                    formatter,
                    "cannot open {role} {}: {source}",
                    self.path.display()
                )
            }
            ProgramFailure::Read { role, source } => {
                write!(
                    formatter,
                    "cannot read {role} {}: {source}",
                    self.path.display()
                )
            }
            ProgramFailure::NotRegular { role } => {
                write!(
                    formatter,
                    "{role} {} is not a regular file",
                    self.path.display()
                )
            }
            ProgramFailure::InvalidSize {
                role,
                bytes,
                minimum,
                maximum,
            } => write!(
                formatter,
                "{role} {} contains {bytes} bytes; expected {minimum} through {maximum}",
                self.path.display()
            ),
            ProgramFailure::Changed {
                role,
                expected,
                actual,
            } => write!(
                formatter,
                "{role} {} changed while reading: expected {expected} bytes, read {actual}",
                self.path.display()
            ),
            ProgramFailure::InvalidConfig(source) => {
                write!(formatter, "{}: {source}", self.path.display())
            }
        }
    }
}

impl Error for ProgramError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.failure {
            ProgramFailure::Open { source, .. } | ProgramFailure::Read { source, .. } => {
                Some(source)
            }
            ProgramFailure::InvalidConfig(source) => Some(source),
            ProgramFailure::NotRegular { .. }
            | ProgramFailure::InvalidSize { .. }
            | ProgramFailure::Changed { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileRole {
    Rom,
    Config,
}

impl fmt::Display for FileRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Rom => "CHIP-8 ROM",
            Self::Config => "CHIP-8 config",
        })
    }
}

#[derive(Debug)]
enum ProgramFailure {
    Open {
        role: FileRole,
        source: io::Error,
    },
    Read {
        role: FileRole,
        source: io::Error,
    },
    NotRegular {
        role: FileRole,
    },
    InvalidSize {
        role: FileRole,
        bytes: usize,
        minimum: usize,
        maximum: usize,
    },
    Changed {
        role: FileRole,
        expected: usize,
        actual: usize,
    },
    InvalidConfig(ConfigError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn program_loads_exact_rom_and_optional_configuration() {
        let Ok(directory) = tempfile::tempdir() else {
            return;
        };
        let rom_path = directory.path().join("program.ch8");
        assert!(fs::write(&rom_path, [0x00, 0xfd]).is_ok());

        let program = Program::load(&rom_path).expect("valid ROM without sidecar");
        assert_eq!(program.rom(), [0x00, 0xfd]);
        assert_eq!(program.configuration(), Configuration::default());

        assert!(fs::write(config_path(&rom_path), b"tickrate=12\n").is_ok());
        let configured = Program::load(&rom_path).expect("valid ROM and sidecar");
        assert_eq!(
            configured.configuration().core().instructions_per_frame(),
            12
        );
    }

    #[test]
    fn program_rejects_unsafe_types_sizes_and_sidecars() {
        let Ok(directory) = tempfile::tempdir() else {
            return;
        };
        let empty = directory.path().join("empty.ch8");
        assert!(fs::write(&empty, []).is_ok());
        assert!(Program::load(&empty).is_err());

        let oversized = directory.path().join("large.ch8");
        assert!(fs::write(&oversized, vec![0; MAXIMUM_ROM_BYTES + 1]).is_ok());
        assert!(Program::load(&oversized).is_err());

        let actual = directory.path().join("actual.ch8");
        let alias = directory.path().join("alias.ch8");
        assert!(fs::write(&actual, [0x00, 0xfd]).is_ok());
        assert!(symlink(&actual, &alias).is_ok());
        assert!(Program::load(&alias).is_err());

        let sidecar_link = config_path(&actual);
        let sidecar_actual = directory.path().join("actual.cfg");
        assert!(fs::write(&sidecar_actual, b"tickrate=12\n").is_ok());
        assert!(symlink(&sidecar_actual, &sidecar_link).is_ok());
        assert!(Program::load(&actual).is_err());

        assert!(fs::remove_file(&sidecar_link).is_ok());
        assert!(fs::write(&sidecar_link, b"tickrate=0\n").is_ok());
        assert!(Program::load(&actual).is_err());
    }
}
