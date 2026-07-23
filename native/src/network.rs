use crate::regular_file;
use std::ffi::CStr;
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

const IW_ESSID_MAX_SIZE: usize = 32;
const SIOCGIWESSID: libc::c_ulong = 0x8b1b;
const STATUS_UNAVAILABLE: &str = "STATUS UNAVAILABLE";
const STATUS_INVALID: &str = "STATUS INVALID";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkStatus {
    pub ssid: String,
    pub wlan_ipv4: String,
    pub wireguard_ipv4: String,
    pub selector: String,
}

struct IfAddrs(*mut libc::ifaddrs);

impl Drop for IfAddrs {
    fn drop(&mut self) {
        unsafe { libc::freeifaddrs(self.0) };
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IwPoint {
    pointer: *mut libc::c_void,
    length: u16,
    flags: u16,
}

#[repr(C)]
union IwreqData {
    essid: IwPoint,
    padding: [u8; 16],
}

#[repr(C)]
struct Iwreq {
    name: [libc::c_char; libc::IFNAMSIZ],
    data: IwreqData,
}

fn interface_ipv4(interface: &str) -> String {
    if interface.is_empty() || interface.as_bytes().contains(&0) {
        return String::new();
    }
    let mut addresses = ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut addresses) } != 0 || addresses.is_null() {
        return String::new();
    }
    let addresses = IfAddrs(addresses);
    let mut entry = addresses.0;
    while !entry.is_null() {
        let current = unsafe { &*entry };
        if !current.ifa_name.is_null()
            && !current.ifa_addr.is_null()
            && unsafe { CStr::from_ptr(current.ifa_name) }.to_bytes() == interface.as_bytes()
            && unsafe { (*current.ifa_addr).sa_family as libc::c_int } == libc::AF_INET
        {
            let address = unsafe { &*current.ifa_addr.cast::<libc::sockaddr_in>() };
            return Ipv4Addr::from(address.sin_addr.s_addr.to_ne_bytes()).to_string();
        }
        entry = current.ifa_next;
    }
    String::new()
}

fn wireless_ssid(interface: &str) -> String {
    let name = interface.as_bytes();
    if name.is_empty() || name.len() >= libc::IFNAMSIZ || name.contains(&0) {
        return String::new();
    }
    let descriptor =
        unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if descriptor < 0 {
        return String::new();
    }
    let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
    let mut value = [0u8; IW_ESSID_MAX_SIZE + 1];
    let mut request = Iwreq {
        name: [0; libc::IFNAMSIZ],
        data: IwreqData {
            essid: IwPoint {
                pointer: value.as_mut_ptr().cast(),
                length: IW_ESSID_MAX_SIZE as u16,
                flags: 0,
            },
        },
    };
    for (destination, source) in request.name.iter_mut().zip(name) {
        *destination = *source as libc::c_char;
    }
    if unsafe { libc::ioctl(descriptor.as_raw_fd(), SIOCGIWESSID, &mut request) } != 0 {
        return String::new();
    }
    let length = usize::from(unsafe { request.data.essid.length }).min(IW_ESSID_MAX_SIZE);
    let mut bytes = &value[..length];
    while bytes.last() == Some(&0) {
        bytes = &bytes[..bytes.len() - 1];
    }
    if valid_utf8_text(bytes, 32, true) {
        display_ascii(bytes)
    } else {
        "?".to_owned()
    }
}

fn selector_status(path: &Path) -> String {
    let path_bytes = path.as_os_str().as_bytes();
    if path_bytes.first() != Some(&b'/') || path_bytes.len() >= libc::PATH_MAX as usize {
        return STATUS_UNAVAILABLE.to_owned();
    }
    let Ok(Some(data)) = regular_file::read_regular(path, 1, 128, "Wi-Fi selector status") else {
        return STATUS_UNAVAILABLE.to_owned();
    };
    let mut line = &data[..data
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(data.len())];
    if line.last() == Some(&b'\r') {
        line = &line[..line.len() - 1];
    }
    if line.first().is_some_and(u8::is_ascii_whitespace)
        || line.last().is_some_and(u8::is_ascii_whitespace)
        || !valid_utf8_text(line, 64, false)
    {
        STATUS_INVALID.to_owned()
    } else {
        display_ascii(line)
    }
}

fn valid_utf8_text(bytes: &[u8], maximum_codepoints: usize, allow_empty: bool) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    if text.is_empty() {
        return allow_empty;
    }
    let mut count = 0;
    text.chars().all(|character| {
        count += 1;
        let codepoint = character as u32;
        codepoint >= 0x20 && codepoint != 0x7f && count <= maximum_codepoints
    })
}

fn display_ascii(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes)
        .expect("validated UTF-8")
        .chars()
        .map(|character| if character.is_ascii() { character } else { '?' })
        .collect()
}

pub fn read_network_status(selector_status_path: &Path) -> NetworkStatus {
    NetworkStatus {
        ssid: wireless_ssid("wlan0"),
        wlan_ipv4: interface_ipv4("wlan0"),
        wireguard_ipv4: interface_ipv4("wg0"),
        selector: selector_status(selector_status_path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn reads_selector_status_with_cpp_compatibility() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("retrodeck-network-{}-{suffix}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("status");
        let link = directory.join("status-link");

        std::fs::write(&path, b"CONNECTED TO NET1\n").unwrap();
        assert_eq!(selector_status(&path), "CONNECTED TO NET1");
        std::fs::write(&path, b"CONNECTED\r\n").unwrap();
        assert_eq!(selector_status(&path), "CONNECTED");
        std::fs::write(&path, b" LEADING SPACE\n").unwrap();
        assert_eq!(selector_status(&path), STATUS_INVALID);
        std::fs::write(&path, b"BAD \xc0\xaf\n").unwrap();
        assert_eq!(selector_status(&path), STATUS_INVALID);
        std::fs::write(&path, vec![b'X'; 129]).unwrap();
        assert_eq!(selector_status(&path), STATUS_UNAVAILABLE);
        symlink(&path, &link).unwrap();
        assert_eq!(selector_status(&link), STATUS_UNAVAILABLE);
        assert_eq!(
            selector_status(Path::new("relative/status")),
            STATUS_UNAVAILABLE
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn normalizes_valid_non_ascii_text_and_missing_interfaces() {
        assert_eq!(display_ascii("NETž".as_bytes()), "NET?");
        assert!(!valid_utf8_text(b"BAD\0", 64, false));
        assert!(interface_ipv4("retro-deck-missing").is_empty());
        assert!(wireless_ssid("retro-deck-missing").is_empty());
    }
}
