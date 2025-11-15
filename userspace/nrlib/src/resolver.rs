/// Resolver implementation compatible with musl libc
///
/// Provides getaddrinfo/getnameinfo and /etc file parsing (/etc/hosts,
/// /etc/resolv.conf, /etc/nsswitch.conf).

use super::dns::{DnsQuery, DnsResponse, QType, ResolverConfig};

const MAX_HOSTNAME: usize = 256;

/// Address family constants (compatible with POSIX)
pub const AF_UNSPEC: i32 = 0;
pub const AF_INET: i32 = 2;
pub const AF_INET6: i32 = 10;

/// Socket type constants
pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;

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

/// sockaddr_in structure (IPv4)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockAddrIn {
    pub sin_family: u16,      // AF_INET
    pub sin_port: u16,        // Port (network byte order)
    pub sin_addr: [u8; 4],    // IPv4 address
    pub sin_zero: [u8; 8],    // Padding
}

impl SockAddrIn {
    pub fn new(ip: [u8; 4], port: u16) -> Self {
        Self {
            sin_family: AF_INET as u16,
            sin_port: port.to_be(),
            sin_addr: ip,
            sin_zero: [0; 8],
        }
    }
}

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

/// Parse IPv4 address string
pub fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut index = 0;

    for part in s.split('.') {
        if index >= 4 {
            return None;
        }
        octets[index] = part.parse::<u8>().ok()?;
        index += 1;
    }

    if index != 4 {
        return None;
    }

    Some(octets)
}

/// Format IPv4 address to string
pub fn format_ipv4(ip: [u8; 4], buffer: &mut [u8]) -> usize {
    let mut pos = 0;
    for (i, octet) in ip.iter().enumerate() {
        if i > 0 {
            if pos < buffer.len() {
                buffer[pos] = b'.';
                pos += 1;
            }
        }

        let s = format_u8(*octet);
        let len = s.len().min(buffer.len() - pos);
        buffer[pos..pos + len].copy_from_slice(&s.as_bytes()[..len]);
        pos += len;
    }
    pos
}

/// Format u8 as decimal string
fn format_u8(n: u8) -> &'static str {
    const LOOKUP: [&str; 256] = [
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
        "10", "11", "12", "13", "14", "15", "16", "17", "18", "19",
        "20", "21", "22", "23", "24", "25", "26", "27", "28", "29",
        "30", "31", "32", "33", "34", "35", "36", "37", "38", "39",
        "40", "41", "42", "43", "44", "45", "46", "47", "48", "49",
        "50", "51", "52", "53", "54", "55", "56", "57", "58", "59",
        "60", "61", "62", "63", "64", "65", "66", "67", "68", "69",
        "70", "71", "72", "73", "74", "75", "76", "77", "78", "79",
        "80", "81", "82", "83", "84", "85", "86", "87", "88", "89",
        "90", "91", "92", "93", "94", "95", "96", "97", "98", "99",
        "100", "101", "102", "103", "104", "105", "106", "107", "108", "109",
        "110", "111", "112", "113", "114", "115", "116", "117", "118", "119",
        "120", "121", "122", "123", "124", "125", "126", "127", "128", "129",
        "130", "131", "132", "133", "134", "135", "136", "137", "138", "139",
        "140", "141", "142", "143", "144", "145", "146", "147", "148", "149",
        "150", "151", "152", "153", "154", "155", "156", "157", "158", "159",
        "160", "161", "162", "163", "164", "165", "166", "167", "168", "169",
        "170", "171", "172", "173", "174", "175", "176", "177", "178", "179",
        "180", "181", "182", "183", "184", "185", "186", "187", "188", "189",
        "190", "191", "192", "193", "194", "195", "196", "197", "198", "199",
        "200", "201", "202", "203", "204", "205", "206", "207", "208", "209",
        "210", "211", "212", "213", "214", "215", "216", "217", "218", "219",
        "220", "221", "222", "223", "224", "225", "226", "227", "228", "229",
        "230", "231", "232", "233", "234", "235", "236", "237", "238", "239",
        "240", "241", "242", "243", "244", "245", "246", "247", "248", "249",
        "250", "251", "252", "253", "254", "255",
    ];
    LOOKUP[n as usize]
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
