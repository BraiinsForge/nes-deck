//! Canonical all-interface listener configuration.

use std::{fmt, io, net::SocketAddrV4, path::Path};

use crate::file::{FileError, read_bounded_regular};

const CANONICAL_ADDRESS: &str = "0.0.0.0:8080";
const CANONICAL_CONFIG: &[u8] = b"0.0.0.0:8080\n";
const MAXIMUM_CONFIG_BYTES: u64 = 64;

/// The uploader's intentionally fixed IPv4 listener address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceAddress(SocketAddrV4);

impl ServiceAddress {
    /// Parse the exact service address accepted by the deployed uploader.
    ///
    /// # Errors
    ///
    /// Returns [`AddressError`] unless `value` is exactly `0.0.0.0:8080`.
    pub fn parse(value: &str) -> Result<Self, AddressError> {
        if value != CANONICAL_ADDRESS {
            return Err(AddressError::Invalid);
        }
        let address = value
            .parse::<SocketAddrV4>()
            .map_err(|_| AddressError::Invalid)?;
        Ok(Self(address))
    }

    /// Parse the exact newline-terminated installed configuration.
    ///
    /// # Errors
    ///
    /// Returns [`AddressError`] for missing or additional bytes, another
    /// address, another port, IPv6, or non-UTF-8 input.
    pub fn parse_config(contents: &[u8]) -> Result<Self, AddressError> {
        if contents != CANONICAL_CONFIG {
            return Err(AddressError::Invalid);
        }
        Self::parse(CANONICAL_ADDRESS)
    }

    /// Load the canonical listener from a bounded regular file without
    /// following a final symlink.
    ///
    /// # Errors
    ///
    /// Returns [`AddressError::Invalid`] for noncanonical contents,
    /// [`AddressError::UnsafeFile`] for a non-file or excessive file, and
    /// [`AddressError::Io`] for filesystem failures.
    pub fn load(path: &Path) -> Result<Self, AddressError> {
        let file = read_bounded_regular(path, MAXIMUM_CONFIG_BYTES).map_err(map_file_error)?;
        Self::parse_config(&file.contents)
    }

    /// Return the standard-library socket address used for `tcp4` binding.
    #[must_use]
    pub const fn socket_addr(self) -> SocketAddrV4 {
        self.0
    }

    /// Return the canonical installed file contents.
    #[must_use]
    pub const fn encode() -> &'static [u8] {
        CANONICAL_CONFIG
    }
}

impl fmt::Display for ServiceAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The service address is not the one deliberately exposed on every IPv4
/// interface at port 8080.
#[derive(Debug)]
pub enum AddressError {
    /// The value is not exactly the supported listener address.
    Invalid,
    /// The installed path is not a bounded regular file.
    UnsafeFile(&'static str),
    /// Opening or reading the installed path failed.
    Io(io::Error),
}

impl fmt::Display for AddressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => formatter.write_str("service address must be 0.0.0.0:8080"),
            Self::UnsafeFile(reason) => write!(formatter, "unsafe address file: {reason}"),
            Self::Io(error) => write!(formatter, "address file I/O failed: {error}"),
        }
    }
}

impl std::error::Error for AddressError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Invalid | Self::UnsafeFile(_) => None,
        }
    }
}

fn map_file_error(error: FileError) -> AddressError {
    match error {
        FileError::Io(error) => AddressError::Io(error),
        FileError::Unsafe(reason) => AddressError::UnsafeFile(reason),
        FileError::Random(error) => AddressError::Io(io::Error::other(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::{AddressError, CANONICAL_ADDRESS, ServiceAddress};
    use std::{fs, os::unix::fs::symlink};

    #[test]
    fn accepts_only_the_canonical_listener() {
        let address = ServiceAddress::parse(CANONICAL_ADDRESS).expect("canonical address");
        assert_eq!(address.to_string(), CANONICAL_ADDRESS);
        assert_eq!(address.socket_addr().ip().octets(), [0, 0, 0, 0]);
        assert_eq!(address.socket_addr().port(), 8080);

        for rejected in [
            "",
            "0.0.0.0:08080",
            "0.0.0.0:80",
            "127.0.0.1:8080",
            "10.0.0.15:8080",
            "[::]:8080",
            " 0.0.0.0:8080",
            "0.0.0.0:8080\n",
        ] {
            assert!(
                ServiceAddress::parse(rejected).is_err(),
                "accepted {rejected:?}"
            );
        }
    }

    #[test]
    fn requires_one_canonical_config_line() {
        let address = ServiceAddress::parse_config(ServiceAddress::encode())
            .expect("canonical configuration");
        assert_eq!(address.to_string(), CANONICAL_ADDRESS);

        for rejected in [
            b"".as_slice(),
            b"0.0.0.0:8080".as_slice(),
            b"0.0.0.0:8080\r\n".as_slice(),
            b"0.0.0.0:8080\nextra\n".as_slice(),
            b"127.0.0.1:8080\n".as_slice(),
            b"0.0.0.0:8081\n".as_slice(),
            b"\xff\n".as_slice(),
        ] {
            assert!(
                ServiceAddress::parse_config(rejected).is_err(),
                "accepted {rejected:?}"
            );
        }
    }

    #[test]
    fn loads_only_a_bounded_regular_config() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let config = directory.path().join("address.conf");
        assert!(fs::write(&config, ServiceAddress::encode()).is_ok());
        assert!(ServiceAddress::load(&config).is_ok());

        assert!(fs::write(&config, b"127.0.0.1:8080\n").is_ok());
        assert!(matches!(
            ServiceAddress::load(&config),
            Err(AddressError::Invalid)
        ));

        assert!(fs::write(&config, vec![b'x'; 65]).is_ok());
        assert!(matches!(
            ServiceAddress::load(&config),
            Err(AddressError::UnsafeFile(_))
        ));

        let link = directory.path().join("address-link.conf");
        assert!(symlink(&config, &link).is_ok());
        assert!(ServiceAddress::load(&link).is_err());
    }
}
