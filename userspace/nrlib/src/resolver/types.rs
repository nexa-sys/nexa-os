/// Resolver type definitions
///
/// Contains AddrInfo, HostEntry, and NssSource types.

use crate::socket::SockAddrIn;

/// addrinfo structure (compatible with POSIX)
#[repr(C)]
pub struct AddrInfo {
    pub ai_flags: i32,
    pub ai_family: i32,
    pub ai_socktype: i32,
    pub ai_protocol: i32,
    pub ai_addrlen: u32,
    pub ai_addr: *mut SockAddrIn,
    pub ai_canonname: *mut u8,
    pub ai_next: *mut AddrInfo,
}

/// Host entry from /etc/hosts
#[derive(Clone, Copy)]
pub struct HostEntry {
    pub ip: [u8; 4],
    pub name: [u8; 256],
    pub name_len: usize,
}

impl HostEntry {
    pub const fn empty() -> Self {
        Self {
            ip: [0; 4],
            name: [0; 256],
            name_len: 0,
        }
    }

    /// Check if hostname matches (case-insensitive)
    pub fn matches(&self, hostname: &str) -> bool {
        if hostname.len() != self.name_len {
            return false;
        }
        let name_bytes = &self.name[..self.name_len];
        hostname.as_bytes().eq_ignore_ascii_case(name_bytes)
    }

    /// Get name as str
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
}

/// NSS source type for name resolution order
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NssSource {
    /// /etc/hosts file
    Files,
    /// DNS query
    Dns,
    /// Multicast DNS (mDNS)
    Mdns,
    /// Unknown source
    Unknown,
}
