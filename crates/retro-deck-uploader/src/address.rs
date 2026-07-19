//! Canonical all-interface listener configuration.

use std::{fmt, net::SocketAddrV4};

const CANONICAL_ADDRESS: &str = "0.0.0.0:8080";
const CANONICAL_CONFIG: &[u8] = b"0.0.0.0:8080\n";

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
            return Err(AddressError);
        }
        let address = value.parse::<SocketAddrV4>().map_err(|_| AddressError)?;
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
            return Err(AddressError);
        }
        Self::parse(CANONICAL_ADDRESS)
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddressError;

impl fmt::Display for AddressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("service address must be 0.0.0.0:8080")
    }
}

impl std::error::Error for AddressError {}

#[cfg(test)]
mod tests {
    use super::{CANONICAL_ADDRESS, ServiceAddress};

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
}
