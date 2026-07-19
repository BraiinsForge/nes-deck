//! Provisioning and validation commands shared by the uploader executable.

use std::{
    ffi::OsString,
    fmt, io,
    path::{Path, PathBuf},
};

use crate::{
    address::{AddressError, ServiceAddress},
    password::{PasswordConfig, PasswordError, read_password},
};

/// A currently implemented uploader command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    /// Read a password from standard input and durably replace its record.
    SetPassword(PathBuf),
    /// Validate an installed password record.
    CheckPasswordConfig(PathBuf),
    /// Validate the exact all-interface listener configuration.
    CheckAddress(PathBuf),
}

/// Parse the uploader's strict two-argument management commands.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for an unsupported command or argument count.
pub fn parse_args(args: &[OsString]) -> Result<Command, CliError> {
    match args {
        [flag, path] if flag == "--set-password" => Ok(Command::SetPassword(PathBuf::from(path))),
        [flag, path] if flag == "--check-password-config" => {
            Ok(Command::CheckPasswordConfig(PathBuf::from(path)))
        }
        [flag, path] if flag == "--check-address" => Ok(Command::CheckAddress(PathBuf::from(path))),
        _ => Err(CliError::Usage),
    }
}

/// Execute one management command with bounded input.
///
/// # Errors
///
/// Returns [`CliError::Password`] or [`CliError::Address`] when validation,
/// cryptography, entropy, or filesystem operations fail.
pub fn execute(command: &Command, input: &mut impl io::Read) -> Result<(), CliError> {
    match command {
        Command::SetPassword(path) => set_password(path, input),
        Command::CheckPasswordConfig(path) => {
            PasswordConfig::load(path)?;
            Ok(())
        }
        Command::CheckAddress(path) => {
            ServiceAddress::load(path)?;
            Ok(())
        }
    }
}

/// Human-readable command syntax for the incomplete replacement binary.
pub const USAGE: &str = "Usage:\n  rom-uploader --set-password PATH\n  rom-uploader --check-password-config PATH\n  rom-uploader --check-address PATH";

fn set_password(path: &Path, input: &mut impl io::Read) -> Result<(), CliError> {
    let password = read_password(input)?;
    PasswordConfig::new(&password)?.store(path)?;
    Ok(())
}

/// Uploader command-line failure.
#[derive(Debug)]
pub enum CliError {
    /// The command or argument count is unsupported.
    Usage,
    /// Password parsing, derivation, or file access failed.
    Password(PasswordError),
    /// Listener configuration access or validation failed.
    Address(AddressError),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => formatter.write_str("invalid command line"),
            Self::Password(error) => error.fmt(formatter),
            Self::Address(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Usage => None,
            Self::Password(error) => Some(error),
            Self::Address(error) => Some(error),
        }
    }
}

impl From<PasswordError> for CliError {
    fn from(error: PasswordError) -> Self {
        Self::Password(error)
    }
}

impl From<AddressError> for CliError {
    fn from(error: AddressError) -> Self {
        Self::Address(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{CliError, Command, execute, parse_args};
    use crate::{address::ServiceAddress, password::PasswordConfig};
    use std::{ffi::OsString, fs, io::Cursor};

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn command_lines_are_exact() {
        for (values, expected) in [
            (
                ["--set-password", "/tmp/password.conf"],
                Command::SetPassword("/tmp/password.conf".into()),
            ),
            (
                ["--check-password-config", "/tmp/password.conf"],
                Command::CheckPasswordConfig("/tmp/password.conf".into()),
            ),
            (
                ["--check-address", "/tmp/address.conf"],
                Command::CheckAddress("/tmp/address.conf".into()),
            ),
        ] {
            assert!(matches!(
                parse_args(&arguments(&values)),
                Ok(command) if command == expected
            ));
        }
        for values in [
            Vec::new(),
            arguments(&["--set-password"]),
            arguments(&["--install-bmc-scene", "/tmp/bmc.json"]),
            arguments(&["--check-address", "/tmp/address", "extra"]),
        ] {
            assert!(matches!(parse_args(&values), Err(CliError::Usage)));
        }
    }

    #[test]
    fn management_commands_use_the_real_file_formats() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let password_path = directory.path().join("private/password.conf");
        let mut password = Cursor::new(b"configured-password\n");
        assert!(execute(&Command::SetPassword(password_path.clone()), &mut password).is_ok());
        assert!(matches!(
            PasswordConfig::load(&password_path),
            Ok(config) if config.matches("configured-password")
        ));
        assert!(
            execute(
                &Command::CheckPasswordConfig(password_path),
                &mut Cursor::new([])
            )
            .is_ok()
        );

        let address_path = directory.path().join("address.conf");
        assert!(fs::write(&address_path, ServiceAddress::encode()).is_ok());
        assert!(
            execute(
                &Command::CheckAddress(address_path.clone()),
                &mut Cursor::new([])
            )
            .is_ok()
        );
        assert!(fs::write(&address_path, b"127.0.0.1:8080\n").is_ok());
        assert!(matches!(
            execute(&Command::CheckAddress(address_path), &mut Cursor::new([])),
            Err(CliError::Address(_))
        ));
    }
}
