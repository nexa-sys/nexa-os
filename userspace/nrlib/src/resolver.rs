/// Resolver implementation compatible with musl libc
///
/// Provides getaddrinfo/getnameinfo and /etc file parsing (/etc/hosts,
/// /etc/resolv.conf, /etc/nsswitch.conf).

use super::dns::{DnsQuery, DnsResponse, QType, ResolverConfig};
use super::socket::{SockAddrIn, AF_INET, AF_INET6, AF_UNSPEC, SOCK_STREAM, SOCK_DGRAM, parse_ipv4, format_ipv4};

const MAX_HOSTNAME: usize = 256;

/// AI flags for getaddrinfo
pub const AI_PASSIVE: i32 = 0x01;
pub const AI_CANONNAME: i32 = 0x02;
pub const AI_NUMERICHOST: i32 = 0x04;
pub const AI_NUMERICSERV: i32 = 0x08;
pub const AI_V4MAPPED: i32 = 0x10;
pub const AI_ALL: i32 = 0x20;
pub const AI_ADDRCONFIG: i32 = 0x40;

/// NI flags for getnameinfo
pub const NI_NUMERICHOST: i32 = 0x01;
pub const NI_NUMERICSERV: i32 = 0x02;
pub const NI_NOFQDN: i32 = 0x04;
pub const NI_NAMEREQD: i32 = 0x08;
pub const NI_DGRAM: i32 = 0x10;

/// Error codes (compatible with musl)
pub const EAI_BADFLAGS: i32 = -1;
pub const EAI_NONAME: i32 = -2;
pub const EAI_AGAIN: i32 = -3;
pub const EAI_FAIL: i32 = -4;
pub const EAI_FAMILY: i32 = -6;
pub const EAI_SOCKTYPE: i32 = -7;
pub const EAI_SERVICE: i32 = -8;
pub const EAI_MEMORY: i32 = -10;
pub const EAI_SYSTEM: i32 = -11;
pub const EAI_OVERFLOW: i32 = -12;

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

    /// Check if hostname matches
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

/// Global resolver state
pub struct Resolver {
    config: ResolverConfig,
    hosts: [HostEntry; 32],
    hosts_count: usize,
    nsswitch_hosts: [NssSource; 4],
    nsswitch_count: usize,
}

/// NSS source type
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NssSource {
    Files,  // /etc/hosts
    Dns,    // DNS query
    Mdns,   // Multicast DNS
    Unknown,
}

impl Resolver {
    pub const fn new() -> Self {
        Self {
            config: ResolverConfig::new(),
            hosts: [HostEntry::empty(); 32],
            hosts_count: 0,
            nsswitch_hosts: [NssSource::Files, NssSource::Dns, NssSource::Unknown, NssSource::Unknown],
            nsswitch_count: 2,
        }
    }

    /// Parse /etc/resolv.conf
    pub fn parse_resolv_conf(&mut self, content: &str) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut parts = line.split_whitespace();
            let first = match parts.next() {
                Some(s) => s,
                None => continue,
            };

            match first {
                "nameserver" => {
                    if let Some(ip_str) = parts.next() {
                        if let Some(ip) = parse_ipv4(ip_str) {
                            let _ = self.config.add_nameserver(ip);
                        }
                    }
                }
                "search" | "domain" => {
                    for domain in parts {
                        let _ = self.config.add_search_domain(domain);
                    }
                }
                "options" => {
                    for opt in parts {
                        if opt.starts_with("timeout:") {
                            if let Some(val) = opt.strip_prefix("timeout:") {
                                if let Ok(timeout) = val.parse::<u32>() {
                                    self.config.timeout_ms = timeout * 1000;
                                }
                            }
                        } else if opt.starts_with("attempts:") {
                            if let Some(val) = opt.strip_prefix("attempts:") {
                                if let Ok(attempts) = val.parse::<u8>() {
                                    self.config.attempts = attempts;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Parse /etc/hosts
    pub fn parse_hosts(&mut self, content: &str) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut parts = line.split_whitespace();
            let ip_str = match parts.next() {
                Some(s) => s,
                None => continue,
            };

            if let Some(ip) = parse_ipv4(ip_str) {
                for hostname in parts {
                    if hostname.is_empty() || self.hosts_count >= 32 {
                        break;
                    }

                    let mut entry = HostEntry::empty();
                    entry.ip = ip;
                    entry.name_len = hostname.len().min(255);
                    entry.name[..entry.name_len].copy_from_slice(&hostname.as_bytes()[..entry.name_len]);

                    self.hosts[self.hosts_count] = entry;
                    self.hosts_count += 1;
                }
            }
        }
    }

    /// Parse /etc/nsswitch.conf
    pub fn parse_nsswitch(&mut self, content: &str) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(hosts_line) = line.strip_prefix("hosts:") {
                self.nsswitch_count = 0;

                for source in hosts_line.split_whitespace() {
                    if self.nsswitch_count >= 4 {
                        break;
                    }

                    let nss_source = match source {
                        "files" => NssSource::Files,
                        "dns" => NssSource::Dns,
                        "mdns" | "mdns4" | "mdns6" => NssSource::Mdns,
                        _ => continue,
                    };

                    self.nsswitch_hosts[self.nsswitch_count] = nss_source;
                    self.nsswitch_count += 1;
                }
                break;
            }
        }
    }

    /// Lookup hostname in /etc/hosts
    pub fn lookup_hosts(&self, hostname: &str) -> Option<[u8; 4]> {
        for i in 0..self.hosts_count {
            if self.hosts[i].matches(hostname) {
                return Some(self.hosts[i].ip);
            }
        }
        None
    }

    /// Reverse lookup in /etc/hosts
    pub fn reverse_lookup_hosts(&self, ip: [u8; 4]) -> Option<&str> {
        for i in 0..self.hosts_count {
            if self.hosts[i].ip == ip {
                return Some(self.hosts[i].name_str());
            }
        }
        None
    }

    /// Get resolver configuration
    pub fn config(&self) -> &ResolverConfig {
        &self.config
    }

    /// Get NSS sources for host resolution
    pub fn nss_sources(&self) -> &[NssSource] {
        &self.nsswitch_hosts[..self.nsswitch_count]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ipv4() {
        assert_eq!(parse_ipv4("192.168.1.1"), Some([192, 168, 1, 1]));
        assert_eq!(parse_ipv4("8.8.8.8"), Some([8, 8, 8, 8]));
        assert_eq!(parse_ipv4("invalid"), None);
        assert_eq!(parse_ipv4("256.0.0.1"), None);
    }

    #[test]
    fn test_parse_resolv_conf() {
        let mut resolver = Resolver::new();
        let content = "nameserver 8.8.8.8\nnameserver 1.1.1.1\nsearch example.com\n";
        resolver.parse_resolv_conf(content);

        assert_eq!(resolver.config.nameserver_count, 2);
        assert_eq!(resolver.config.nameservers[0], [8, 8, 8, 8]);
        assert_eq!(resolver.config.nameservers[1], [1, 1, 1, 1]);
    }

    #[test]
    fn test_parse_hosts() {
        let mut resolver = Resolver::new();
        let content = "127.0.0.1 localhost\n192.168.1.100 myhost\n";
        resolver.parse_hosts(content);

        assert_eq!(resolver.hosts_count, 2);
        assert_eq!(resolver.lookup_hosts("localhost"), Some([127, 0, 0, 1]));
        assert_eq!(resolver.lookup_hosts("myhost"), Some([192, 168, 1, 100]));
    }
}
