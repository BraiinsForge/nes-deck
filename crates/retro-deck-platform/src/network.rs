//! Read-only Linux network-interface status for dashboard presentation.

use std::error::Error;
use std::ffi::CStr;
use std::fmt;
use std::io;
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::ptr;

/// One bounded read-only snapshot of the Deck network interfaces.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkInterfaces {
    ssid: String,
    wireless_ipv4: Option<Ipv4Addr>,
    wireguard_ipv4: Option<Ipv4Addr>,
}

impl NetworkInterfaces {
    /// Read active SSID and IPv4 addresses without changing interface state.
    ///
    /// A disconnected or wireless-extension-incompatible station has an empty
    /// SSID. An absent address is represented by `None`.
    ///
    /// # Errors
    ///
    /// Returns [`NetworkReadError`] for invalid fixed interface names or when
    /// Linux cannot enumerate interface addresses at all.
    pub fn read(wireless: &str, wireguard: &str) -> Result<Self, NetworkReadError> {
        let wireless = InterfaceName::new(wireless)?;
        let wireguard = InterfaceName::new(wireguard)?;
        let (wireless_ipv4, wireguard_ipv4) =
            interface_ipv4(&wireless, &wireguard).map_err(NetworkReadError::InterfaceAddresses)?;
        let ssid = wireless_ssid(&wireless)
            .map(|bytes| display_ssid(&bytes))
            .unwrap_or_default();
        Ok(Self {
            ssid,
            wireless_ipv4,
            wireguard_ipv4,
        })
    }

    /// Active SSID normalized to printable ASCII, or empty when disconnected.
    #[must_use]
    pub fn ssid(&self) -> &str {
        self.ssid.as_str()
    }

    /// Active wireless IPv4 address.
    #[must_use]
    pub const fn wireless_ipv4(&self) -> Option<Ipv4Addr> {
        self.wireless_ipv4
    }

    /// Active `WireGuard` IPv4 address.
    #[must_use]
    pub const fn wireguard_ipv4(&self) -> Option<Ipv4Addr> {
        self.wireguard_ipv4
    }
}

/// Read-only network status query failure.
#[derive(Debug)]
pub enum NetworkReadError {
    /// An interface name was empty, excessive, or outside a conservative
    /// ASCII device-name alphabet.
    InvalidInterfaceName(String),
    /// Linux could not enumerate interface addresses.
    InterfaceAddresses(io::Error),
}

impl fmt::Display for NetworkReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInterfaceName(name) => {
                write!(formatter, "invalid network interface name {name:?}")
            }
            Self::InterfaceAddresses(error) => {
                write!(
                    formatter,
                    "cannot read network interface addresses: {error}"
                )
            }
        }
    }
}

impl Error for NetworkReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InterfaceAddresses(error) => Some(error),
            Self::InvalidInterfaceName(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InterfaceName {
    bytes: [u8; libc::IFNAMSIZ],
    len: usize,
}

impl InterfaceName {
    fn new(name: &str) -> Result<Self, NetworkReadError> {
        let source = name.as_bytes();
        if source.is_empty()
            || source.len() >= libc::IFNAMSIZ
            || !source
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(NetworkReadError::InvalidInterfaceName(name.to_owned()));
        }
        let mut bytes = [0; libc::IFNAMSIZ];
        let Some(destination) = bytes.get_mut(..source.len()) else {
            return Err(NetworkReadError::InvalidInterfaceName(name.to_owned()));
        };
        destination.copy_from_slice(source);
        Ok(Self {
            bytes,
            len: source.len(),
        })
    }

    fn matches(self, name: &CStr) -> bool {
        self.bytes.get(..self.len) == Some(name.to_bytes())
    }

    fn copy_to(self, destination: &mut [libc::c_char; libc::IFNAMSIZ]) {
        for (output, input) in destination.iter_mut().zip(self.bytes.iter().copied()) {
            *output = libc::c_char::try_from(input).unwrap_or_default();
        }
    }
}

#[derive(Debug)]
struct InterfaceAddresses(*mut libc::ifaddrs);

impl InterfaceAddresses {
    fn read() -> io::Result<Self> {
        let mut addresses = ptr::null_mut();
        // SAFETY: `addresses` is a valid out-pointer. A successful call owns
        // the returned list until `freeifaddrs`, which the guard's Drop calls.
        if unsafe { libc::getifaddrs(&raw mut addresses) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(addresses))
    }
}

impl Drop for InterfaceAddresses {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: the pointer came from one successful `getifaddrs` call and
        // this guard calls `freeifaddrs` exactly once.
        unsafe { libc::freeifaddrs(self.0) };
    }
}

fn interface_ipv4(
    wireless: &InterfaceName,
    wireguard: &InterfaceName,
) -> io::Result<(Option<Ipv4Addr>, Option<Ipv4Addr>)> {
    let addresses = InterfaceAddresses::read()?;
    let mut wireless_ipv4 = None;
    let mut wireguard_ipv4 = None;
    let mut current = addresses.0;
    while !current.is_null() {
        // SAFETY: each non-null node belongs to the live list guarded by
        // `addresses`; `ifa_next` is read before the guard can be dropped.
        let entry = unsafe { &*current };
        current = entry.ifa_next;
        if entry.ifa_name.is_null() || entry.ifa_addr.is_null() {
            continue;
        }
        // SAFETY: Linux supplies a NUL-terminated interface name for every
        // `ifaddrs` entry with a non-null `ifa_name`.
        let name = unsafe { CStr::from_ptr(entry.ifa_name) };
        // SAFETY: `ifa_addr` points to at least a `sockaddr`, whose family can
        // be read before deciding whether a larger IPv4 cast is valid.
        let family = unsafe { (*entry.ifa_addr).sa_family };
        if i32::from(family) != libc::AF_INET {
            continue;
        }
        // SAFETY: an AF_INET `ifa_addr` contains a complete live
        // `sockaddr_in`. Reading unaligned avoids assuming more alignment than
        // the base `sockaddr` pointer type advertises.
        let address = unsafe { ptr::read_unaligned(entry.ifa_addr.cast::<libc::sockaddr_in>()) };
        let address = Ipv4Addr::from(address.sin_addr.s_addr.to_ne_bytes());
        if wireless.matches(name) && wireless_ipv4.is_none() {
            wireless_ipv4 = Some(address);
        }
        if wireguard.matches(name) && wireguard_ipv4.is_none() {
            wireguard_ipv4 = Some(address);
        }
    }
    Ok((wireless_ipv4, wireguard_ipv4))
}

fn wireless_ssid(interface: &InterfaceName) -> io::Result<Vec<u8>> {
    // SAFETY: arguments describe a standard close-on-exec IPv4 datagram
    // socket. A nonnegative descriptor is immediately wrapped for one close.
    let descriptor =
        unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `descriptor` is newly returned and uniquely owned here.
    let socket = unsafe { OwnedFd::from_raw_fd(descriptor) };
    let mut value = [0_u8; libc::IW_ESSID_MAX_SIZE + 1];
    // SAFETY: `iwreq` is a C plain-data request whose all-zero state is the
    // documented initialization before setting name and ESSID pointer fields.
    let mut request = unsafe { std::mem::zeroed::<libc::iwreq>() };
    // Writing one union field does not read an inactive field.
    interface.copy_to(unsafe { &mut request.ifr_ifrn.ifrn_name });
    request.u.essid = libc::iw_point {
        pointer: value.as_mut_ptr().cast(),
        length: u16::try_from(libc::IW_ESSID_MAX_SIZE).unwrap_or(u16::MAX),
        flags: 0,
    };
    // SAFETY: the request points to a live writable buffer of the advertised
    // size, and the socket descriptor remains owned across the ioctl call.
    if unsafe { libc::ioctl(socket.as_raw_fd(), libc::SIOCGIWESSID, &mut request) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: SIOCGIWESSID initializes the active `essid` union member.
    let length = usize::from(unsafe { request.u.essid.length }).min(libc::IW_ESSID_MAX_SIZE);
    let mut bytes = value.get(..length).unwrap_or_default().to_vec();
    while bytes.last() == Some(&0) {
        let _ = bytes.pop();
    }
    Ok(bytes)
}

fn display_ssid(bytes: &[u8]) -> String {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return "?".to_owned();
    };
    let mut output = String::with_capacity(bytes.len().min(libc::IW_ESSID_MAX_SIZE));
    for character in text.chars().take(libc::IW_ESSID_MAX_SIZE) {
        if character.is_ascii() && !character.is_ascii_control() {
            output.push(character);
        } else {
            output.push('?');
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::{InterfaceName, NetworkInterfaces, NetworkReadError, display_ssid};

    #[test]
    fn interface_names_are_short_conservative_kernel_names() {
        assert!(InterfaceName::new("wlan0").is_ok());
        assert!(InterfaceName::new("wg-deck.1").is_ok());
        for invalid in ["", "bad/name", "not allowed", "é", "abcdefghijklmnop"] {
            assert!(matches!(
                InterfaceName::new(invalid),
                Err(NetworkReadError::InvalidInterfaceName(_))
            ));
        }
    }

    #[test]
    fn ssid_display_is_bounded_ascii_without_case_loss() {
        assert_eq!(display_ssid(b"Mixed Case 5G"), "Mixed Case 5G");
        assert_eq!(display_ssid("Síť".as_bytes()), "S??");
        assert_eq!(display_ssid(b"bad\nname"), "bad?name");
        assert_eq!(display_ssid(&[0xff, 0xfe]), "?");
    }

    #[test]
    fn loopback_address_query_uses_read_only_kernel_enumeration() {
        let Some(snapshot) = NetworkInterfaces::read("lo", "lo").ok() else {
            return;
        };
        assert_eq!(snapshot.wireless_ipv4(), Some(Ipv4Addr::LOCALHOST));
        assert_eq!(snapshot.wireguard_ipv4(), Some(Ipv4Addr::LOCALHOST));
        assert!(snapshot.ssid().is_empty());
    }
}
