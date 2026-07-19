//! Versioned PBKDF2 password records compatible with the deployed Go service.

use std::{
    fmt,
    fs::Metadata,
    io::{self, Read as _},
    os::unix::fs::MetadataExt as _,
    path::Path,
};

use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use pbkdf2::pbkdf2_hmac;
use rustix::process::geteuid;
use sha2::Sha256;
use subtle::ConstantTimeEq as _;

use crate::file::{FileError, atomic_write, read_bounded_regular};

const PASSWORD_ITERATIONS: u32 = 210_000;
const MINIMUM_PASSWORD_BYTES: usize = 8;
const MAXIMUM_PASSWORD_BYTES: usize = 128;
const MINIMUM_CONFIG_ITERATIONS: u32 = 100_000;
const MAXIMUM_CONFIG_ITERATIONS: u32 = 1_000_000;
const MAXIMUM_CONFIG_BYTES: u64 = 1_024;
const SALT_BYTES: usize = 16;
const DIGEST_BYTES: usize = 32;

/// Parsed password verifier stored on a Deck.
#[derive(Clone)]
pub struct PasswordConfig {
    iterations: u32,
    salt: [u8; SALT_BYTES],
    digest: [u8; DIGEST_BYTES],
}

impl PasswordConfig {
    /// Derive a fresh password record using operating-system entropy.
    ///
    /// # Errors
    ///
    /// Returns [`PasswordError::InvalidPassword`] for a password outside the
    /// byte-length contract or [`PasswordError::Random`] if entropy fails.
    pub fn new(password: &str) -> Result<Self, PasswordError> {
        validate_password(password)?;
        let mut salt = [0_u8; SALT_BYTES];
        getrandom::getrandom(&mut salt)
            .map_err(|error| PasswordError::Random(error.to_string()))?;
        Ok(Self::derive(password.as_bytes(), salt, PASSWORD_ITERATIONS))
    }

    /// Parse the exact version 1 password-record schema.
    ///
    /// # Errors
    ///
    /// Returns [`PasswordError::InvalidConfig`] for malformed, excessive, or
    /// cryptographically invalid input.
    pub fn parse(contents: &[u8]) -> Result<Self, PasswordError> {
        if contents.is_empty()
            || u64::try_from(contents.len()).unwrap_or(u64::MAX) > MAXIMUM_CONFIG_BYTES
        {
            return Err(PasswordError::InvalidConfig("invalid size"));
        }
        let text =
            std::str::from_utf8(contents).map_err(|_| PasswordError::InvalidConfig("not UTF-8"))?;
        let lines = text.split('\n').collect::<Vec<_>>();
        if lines.len() != 5 || lines.first() != Some(&"version=1") || lines.last() != Some(&"") {
            return Err(PasswordError::InvalidConfig("invalid schema"));
        }

        let mut iterations = None;
        let mut salt = None;
        let mut digest = None;
        for line in lines.get(1..4).unwrap_or_default() {
            let Some((name, value)) = line.split_once('=') else {
                return Err(PasswordError::InvalidConfig("malformed field"));
            };
            match name {
                "iterations" if iterations.is_none() => iterations = Some(value),
                "salt" if salt.is_none() => salt = Some(value),
                "digest" if digest.is_none() => digest = Some(value),
                "iterations" | "salt" | "digest" => {
                    return Err(PasswordError::InvalidConfig("duplicate field"));
                }
                _ => return Err(PasswordError::InvalidConfig("unknown field")),
            }
        }

        let iterations = iterations
            .ok_or(PasswordError::InvalidConfig("missing iterations"))?
            .parse::<u32>()
            .map_err(|_| PasswordError::InvalidConfig("invalid iterations"))?;
        if !(MINIMUM_CONFIG_ITERATIONS..=MAXIMUM_CONFIG_ITERATIONS).contains(&iterations) {
            return Err(PasswordError::InvalidConfig("invalid iterations"));
        }
        let salt = decode_fixed::<SALT_BYTES>(
            salt.ok_or(PasswordError::InvalidConfig("missing salt"))?,
            "invalid salt",
        )?;
        let digest = decode_fixed::<DIGEST_BYTES>(
            digest.ok_or(PasswordError::InvalidConfig("missing digest"))?,
            "invalid digest",
        )?;
        Ok(Self {
            iterations,
            salt,
            digest,
        })
    }

    /// Open and parse a private regular password-record file without following
    /// a final symlink.
    ///
    /// # Errors
    ///
    /// Returns [`PasswordError::Io`] for open or read failures and
    /// [`PasswordError::UnsafeFile`] unless the file is regular, owned by the
    /// service's effective user, no more than 1024 bytes, and inaccessible to
    /// group and other users.
    pub fn load(path: &Path) -> Result<Self, PasswordError> {
        let file = read_bounded_regular(path, MAXIMUM_CONFIG_BYTES).map_err(map_file_error)?;
        validate_metadata(&file.metadata)?;
        Self::parse(&file.contents)
    }

    /// Durably replace a password record with private permissions.
    ///
    /// # Errors
    ///
    /// Returns [`PasswordError::Io`] for filesystem failures or
    /// [`PasswordError::Random`] if a temporary name cannot be generated.
    pub fn store(&self, path: &Path) -> Result<(), PasswordError> {
        atomic_write(path, self.encode().as_bytes(), 0o600, 0o700).map_err(map_file_error)
    }

    /// Serialize this record exactly as the deployed version 1 format.
    #[must_use]
    pub fn encode(&self) -> String {
        format!(
            "version=1\niterations={}\nsalt={}\ndigest={}\n",
            self.iterations,
            STANDARD_NO_PAD.encode(self.salt),
            STANDARD_NO_PAD.encode(self.digest)
        )
    }

    /// Compare a candidate password in constant time after bounded derivation.
    #[must_use]
    pub fn matches(&self, password: &str) -> bool {
        if password.len() > MAXIMUM_PASSWORD_BYTES {
            return false;
        }
        let candidate = Self::derive(password.as_bytes(), self.salt, self.iterations);
        bool::from(candidate.digest.ct_eq(&self.digest))
    }

    fn derive(password: &[u8], salt: [u8; SALT_BYTES], iterations: u32) -> Self {
        let digest = derive_digest(password, &salt, iterations);
        Self {
            iterations,
            salt,
            digest,
        }
    }
}

impl fmt::Debug for PasswordConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PasswordConfig")
            .field("iterations", &self.iterations)
            .field("salt", &"[redacted]")
            .field("digest", &"[redacted]")
            .finish()
    }
}

/// Password input and record failure.
#[derive(Debug)]
pub enum PasswordError {
    /// A configured password violates the byte-length or control contract.
    InvalidPassword,
    /// A password record violates its exact schema.
    InvalidConfig(&'static str),
    /// The installed record is not a private file owned by this service user.
    UnsafeFile(&'static str),
    /// Operating-system entropy was unavailable.
    Random(String),
    /// Opening or reading the record failed.
    Io(io::Error),
}

impl fmt::Display for PasswordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPassword => write!(
                formatter,
                "password must contain {MINIMUM_PASSWORD_BYTES} through {MAXIMUM_PASSWORD_BYTES} bytes without CR, LF, or NUL"
            ),
            Self::InvalidConfig(reason) => {
                write!(formatter, "password configuration has {reason}")
            }
            Self::UnsafeFile(reason) => write!(formatter, "unsafe password file: {reason}"),
            Self::Random(error) => write!(formatter, "cannot generate password salt: {error}"),
            Self::Io(error) => write!(formatter, "password file I/O failed: {error}"),
        }
    }
}

impl std::error::Error for PasswordError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for PasswordError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Read one password from a bounded text stream, accepting LF, CRLF, or EOF.
///
/// # Errors
///
/// Returns [`PasswordError::InvalidPassword`] for an excessive, short, or
/// control-bearing password and [`PasswordError::Io`] for stream failures.
pub fn read_password(input: &mut impl io::Read) -> Result<String, PasswordError> {
    let maximum_input = u64::try_from(MAXIMUM_PASSWORD_BYTES + 2).unwrap_or(u64::MAX);
    let mut bytes = Vec::with_capacity(MAXIMUM_PASSWORD_BYTES + 1);
    input
        .take(maximum_input)
        .read_to_end(&mut bytes)
        .map_err(PasswordError::Io)?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    let password = String::from_utf8(bytes).map_err(|_| PasswordError::InvalidPassword)?;
    validate_password(&password)?;
    Ok(password)
}

fn validate_password(password: &str) -> Result<(), PasswordError> {
    if !(MINIMUM_PASSWORD_BYTES..=MAXIMUM_PASSWORD_BYTES).contains(&password.len())
        || password
            .bytes()
            .any(|byte| matches!(byte, b'\r' | b'\n' | 0))
    {
        Err(PasswordError::InvalidPassword)
    } else {
        Ok(())
    }
}

fn derive_digest(password: &[u8], salt: &[u8], iterations: u32) -> [u8; DIGEST_BYTES] {
    let mut digest = [0_u8; DIGEST_BYTES];
    pbkdf2_hmac::<Sha256>(password, salt, iterations, &mut digest);
    digest
}

fn decode_fixed<const SIZE: usize>(
    encoded: &str,
    reason: &'static str,
) -> Result<[u8; SIZE], PasswordError> {
    let decoded = STANDARD_NO_PAD
        .decode(encoded)
        .map_err(|_| PasswordError::InvalidConfig(reason))?;
    decoded
        .try_into()
        .map_err(|_| PasswordError::InvalidConfig(reason))
}

fn validate_metadata(metadata: &Metadata) -> Result<(), PasswordError> {
    if !metadata.is_file() {
        return Err(PasswordError::UnsafeFile("not a regular file"));
    }
    if metadata.len() == 0 || metadata.len() > MAXIMUM_CONFIG_BYTES {
        return Err(PasswordError::UnsafeFile("invalid size"));
    }
    if metadata.mode() & 0o077 != 0 {
        return Err(PasswordError::UnsafeFile(
            "group or other access is enabled",
        ));
    }
    if metadata.uid() != geteuid().as_raw() {
        return Err(PasswordError::UnsafeFile("wrong owner"));
    }
    Ok(())
}

fn map_file_error(error: FileError) -> PasswordError {
    match error {
        FileError::Io(error) => PasswordError::Io(error),
        FileError::Unsafe(reason) => PasswordError::UnsafeFile(reason),
        FileError::Random(error) => PasswordError::Random(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{PasswordConfig, PasswordError, derive_digest, read_password};
    use std::{
        fs,
        io::Cursor,
        os::unix::fs::{PermissionsExt as _, symlink},
    };

    #[test]
    fn pbkdf2_matches_the_standard_sha256_vector() {
        assert_eq!(
            derive_digest(b"password", b"salt", 1),
            [
                0x12, 0x0f, 0xb6, 0xcf, 0xfc, 0xf8, 0xb3, 0x2c, 0x43, 0xe7, 0x22, 0x52, 0x56, 0xc4,
                0xf8, 0x37, 0xa8, 0x65, 0x48, 0xc9, 0x2c, 0xcc, 0x35, 0x48, 0x08, 0x05, 0x98, 0x7c,
                0xb7, 0x0b, 0xe1, 0x7b,
            ]
        );
    }

    #[test]
    fn deployed_record_schema_round_trips_and_verifies() {
        let config = PasswordConfig::derive(b"configured-password", *b"0123456789abcdef", 100_000);
        let parsed = PasswordConfig::parse(config.encode().as_bytes());
        assert!(matches!(
            parsed,
            Ok(parsed)
                if parsed.matches("configured-password")
                    && !parsed.matches("incorrect-password")
        ));
    }

    #[test]
    fn malformed_records_are_rejected() {
        for contents in [
            "",
            "version=2\niterations=100000\nsalt=MDEyMzQ1Njc4OWFiY2RlZg\ndigest=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
            "version=1\niterations=1\nsalt=MDEyMzQ1Njc4OWFiY2RlZg\ndigest=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
            "version=1\niterations=100000\nsalt=short\ndigest=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
            "version=1\niterations=100000\nsalt=MDEyMzQ1Njc4OWFiY2RlZg\nsalt=MDEyMzQ1Njc4OWFiY2RlZg\n",
        ] {
            assert!(PasswordConfig::parse(contents.as_bytes()).is_err());
        }
    }

    #[test]
    fn password_input_is_bounded_and_control_free() {
        for accepted in [
            "configured-password",
            "configured-password\n",
            "configured-password\r\n",
        ] {
            let mut input = Cursor::new(accepted.as_bytes());
            assert!(matches!(
                read_password(&mut input),
                Ok(password) if password == "configured-password"
            ));
        }
        for rejected in [
            "short\n".to_owned(),
            format!("{}\n", "x".repeat(129)),
            "valid-password\rjunk\n".to_owned(),
            "valid-password\0junk\n".to_owned(),
        ] {
            let mut input = Cursor::new(rejected.into_bytes());
            assert!(matches!(
                read_password(&mut input),
                Err(PasswordError::InvalidPassword)
            ));
        }
    }

    #[test]
    fn installed_record_must_be_private_regular_and_not_a_symlink() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let record = directory.path().join("password.conf");
        let config = PasswordConfig::derive(b"configured-password", *b"0123456789abcdef", 100_000);
        assert!(config.store(&record).is_ok());
        assert!(PasswordConfig::load(&record).is_ok());

        assert!(fs::set_permissions(&record, fs::Permissions::from_mode(0o644)).is_ok());
        assert!(matches!(
            PasswordConfig::load(&record),
            Err(PasswordError::UnsafeFile(_))
        ));
        assert!(fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).is_ok());

        let link = directory.path().join("password-link.conf");
        assert!(symlink(&record, &link).is_ok());
        assert!(PasswordConfig::load(&link).is_err());
    }
}
