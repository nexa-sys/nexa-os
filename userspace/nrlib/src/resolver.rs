/// Resolver implementation compatible with musl libc
///
/// Provides getaddrinfo/getnameinfo and /etc file parsing (/etc/hosts,
/// /etc/resolv.conf, /etc/nsswitch.conf).

use super::dns::{DnsQuery, DnsResponse, QType, ResolverConfig};
use super::socket::{socket, sendto, recvfrom, SockAddr, SockAddrIn, AF_INET, AF_INET6, AF_UNSPEC, SOCK_STREAM, SOCK_DGRAM, parse_ipv4, format_ipv4};
use core::mem;

const MAX_HOSTNAME: usize = 256;
const DNS_PORT: u16 = 53;
const DNS_TIMEOUT_MS: u32 = 5000;

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

    /// Perform DNS query for A record (hostname -> IPv4)
    /// Returns the first IPv4 address found in the response
    pub fn query_dns(&self, hostname: &str, nameserver_ip: [u8; 4]) -> Option<[u8; 4]> {
        // Create UDP socket
        let sockfd = socket(AF_INET, SOCK_DGRAM, 0);
        if sockfd < 0 {
            return None;
        }

        // Build DNS query packet
        let mut query = DnsQuery::new();
        let query_packet = match query.build(12345, hostname, QType::A) {
            Ok(pkt) => pkt,
            Err(_) => return None,
        };

        // Create destination address (nameserver at port 53)
        let dest_addr = SockAddrIn::new(nameserver_ip, DNS_PORT);
        let dest_sockaddr = SockAddr::from(dest_addr);

        // Send DNS query
        let sent = sendto(
            sockfd,
            query_packet.as_ptr(),
            query_packet.len(),
            0,
            &dest_sockaddr,
            mem::size_of::<SockAddr>() as u32,
        );

        if sent < 0 || sent as usize != query_packet.len() {
            return None;
        }

        // Receive DNS response (allocate buffer on stack)
        let mut response_buf = [0u8; 512];
        let received = recvfrom(
            sockfd,
            response_buf.as_mut_ptr(),
            512,
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );

        if received <= 0 {
            return None;
        }

        // Parse DNS response
        let response_data = &response_buf[..received as usize];
        let mut response = DnsResponse::new(response_data);

        // Parse header
        let header = match response.parse_header() {
            Ok(h) => h,
            Err(_) => return None,
        };

        // Check if this is a response and has no errors
        if !header.is_response() || header.rcode() != 0 {
            return None;
        }

        // Skip questions
        let question_count = header.question_count();
        for _ in 0..question_count {
            if response.skip_question().is_err() {
                return None;
            }
        }

        // Parse answers
        let answer_count = header.answer_count();
        for _ in 0..answer_count {
            if let Ok(Some(ip)) = response.parse_a_record() {
                return Some(ip);
            }
        }

        None
    }

    /// Resolve hostname to IPv4 address using NSS sources
    /// First checks /etc/hosts (if Files is in nsswitch), then DNS (if Dns is in nsswitch)
    pub fn resolve(&self, hostname: &str) -> Option<[u8; 4]> {
        // Try each NSS source in order
        for source in self.nss_sources() {
            match source {
                NssSource::Files => {
                    if let Some(ip) = self.lookup_hosts(hostname) {
                        return Some(ip);
                    }
                }
                NssSource::Dns => {
                    // Try each configured nameserver
                    for i in 0..self.config.nameserver_count {
                        if let Some(ip) = self.query_dns(hostname, self.config.nameservers[i]) {
                            return Some(ip);
                        }
                    }
                }
                NssSource::Mdns => {
                    // TODO: Implement mDNS support
                }
                NssSource::Unknown => {
                    // Skip unknown sources
                }
            }
        }

        None
    }
}

/// Global resolver instance
static mut GLOBAL_RESOLVER: Option<Resolver> = None;
static RESOLVER_INIT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Helper to read file content into a temporary buffer
fn read_file_content(path: &str, buf: &mut [u8]) -> Option<usize> {
    use crate::os;
    use core::ffi::CStr;

    // Create null-terminated path string
    let mut path_buf = [0u8; 256];
    if path.len() >= path_buf.len() {
        return None;
    }
    path_buf[..path.len()].copy_from_slice(path.as_bytes());
    path_buf[path.len()] = 0;

    // Create CStr from bytes
    let cstr = CStr::from_bytes_with_nul(&path_buf[..path.len() + 1]).ok()?;

    // Open file
    let fd = os::open(cstr, 0).ok()?;

    // Read file content
    let read_result = os::read(fd, buf).ok()?;

    // Close file
    let _ = os::close(fd);
    
    Some(read_result)
}

/// Initialize global resolver (call once at program startup)
#[no_mangle]
pub extern "C" fn resolver_init() -> i32 {
    if RESOLVER_INIT.load(core::sync::atomic::Ordering::Acquire) {
        return 0; // Already initialized
    }

    let mut resolver = Resolver::new();

    // Try to load /etc/resolv.conf
    let mut resolv_buf = [0u8; 2048];
    if let Some(len) = read_file_content("/etc/resolv.conf", &mut resolv_buf) {
        if let Ok(content) = core::str::from_utf8(&resolv_buf[..len]) {
            resolver.parse_resolv_conf(content);
        }
    } else {
        // Set default nameserver if no resolv.conf
        let _ = resolver.config.add_nameserver([8, 8, 8, 8]);
    }

    // Try to load /etc/hosts
    let mut hosts_buf = [0u8; 4096];
    if let Some(len) = read_file_content("/etc/hosts", &mut hosts_buf) {
        if let Ok(content) = core::str::from_utf8(&hosts_buf[..len]) {
            resolver.parse_hosts(content);
        }
    }

    // Try to load /etc/nsswitch.conf
    let mut nsswitch_buf = [0u8; 1024];
    if let Some(len) = read_file_content("/etc/nsswitch.conf", &mut nsswitch_buf) {
        if let Ok(content) = core::str::from_utf8(&nsswitch_buf[..len]) {
            resolver.parse_nsswitch(content);
        }
    }

    unsafe {
        GLOBAL_RESOLVER = Some(resolver);
    }

    RESOLVER_INIT.store(true, core::sync::atomic::Ordering::Release);
    0
}

/// Get the global resolver instance
fn get_resolver() -> Option<&'static Resolver> {
    if !RESOLVER_INIT.load(core::sync::atomic::Ordering::Acquire) {
        let _ = resolver_init();
    }
    unsafe { GLOBAL_RESOLVER.as_ref() }
}

/// Musl-compatible getaddrinfo implementation
/// 
/// Parameters match POSIX getaddrinfo signature
#[no_mangle]
pub extern "C" fn getaddrinfo(
    node: *const u8,
    service: *const u8,
    hints: *const AddrInfo,
    res: *mut *mut AddrInfo,
) -> i32 {
    if res.is_null() {
        return EAI_FAIL;
    }

    if node.is_null() {
        return EAI_NONAME;
    }

    unsafe {
        // Read null-terminated hostname string
        let mut hostname_buf = [0u8; 256];
        let mut len = 0;
        while len < 255 {
            let byte = *node.add(len);
            if byte == 0 {
                break;
            }
            hostname_buf[len] = byte;
            len += 1;
        }
        hostname_buf[len] = 0;

        let hostname = match core::str::from_utf8(&hostname_buf[..len]) {
            Ok(s) => s,
            Err(_) => return EAI_NONAME,
        };

        // Get resolver and ensure it's initialized
        let resolver = match get_resolver() {
            Some(r) => r,
            None => return EAI_FAIL,
        };

        // Try to resolve hostname
        let ip = match resolver.resolve(hostname) {
            Some(ip) => ip,
            None => return EAI_NONAME,
        };

        // Allocate AddrInfo structure manually (malloc)
        let addrinfo_ptr = crate::malloc(core::mem::size_of::<AddrInfo>()) as *mut AddrInfo;
        if addrinfo_ptr.is_null() {
            return EAI_FAIL;
        }

        // Allocate SockAddrIn
        let addr_ptr = crate::malloc(core::mem::size_of::<SockAddrIn>()) as *mut SockAddrIn;
        if addr_ptr.is_null() {
            crate::free(addrinfo_ptr as *mut crate::c_void);
            return EAI_FAIL;
        }

        // Initialize SockAddrIn
        *addr_ptr = SockAddrIn::new(ip, 0);

        // Initialize AddrInfo
        let addrinfo = &mut *addrinfo_ptr;
        addrinfo.ai_flags = 0;
        addrinfo.ai_family = AF_INET;
        addrinfo.ai_socktype = SOCK_DGRAM;
        addrinfo.ai_protocol = 17; // IPPROTO_UDP
        addrinfo.ai_addrlen = core::mem::size_of::<SockAddrIn>() as u32;
        addrinfo.ai_addr = addr_ptr;
        addrinfo.ai_canonname = core::ptr::null_mut();
        addrinfo.ai_next = core::ptr::null_mut();

        *res = addrinfo_ptr;
    }

    0
}

/// Musl-compatible freeaddrinfo implementation
#[no_mangle]
pub extern "C" fn freeaddrinfo(res: *mut AddrInfo) {
    if !res.is_null() {
        unsafe {
            let addrinfo = &mut *res;
            if !addrinfo.ai_addr.is_null() {
                crate::free(addrinfo.ai_addr as *mut crate::c_void);
            }
            if !addrinfo.ai_canonname.is_null() {
                crate::free(addrinfo.ai_canonname as *mut crate::c_void);
            }
            if !addrinfo.ai_next.is_null() {
                freeaddrinfo(addrinfo.ai_next);
            }
            crate::free(res as *mut crate::c_void);
        }
    }
}

/// Musl-compatible getnameinfo implementation (reverse DNS lookup)
#[no_mangle]
pub extern "C" fn getnameinfo(
    addr: *const SockAddr,
    addrlen: u32,
    host: *mut u8,
    hostlen: u32,
    serv: *mut u8,
    servlen: u32,
    flags: i32,
) -> i32 {
    if addr.is_null() || host.is_null() || hostlen == 0 {
        return EAI_FAIL;
    }

    unsafe {
        let sockaddr = &*addr;
        
        // Only support AF_INET for now
        if sockaddr.sa_family != AF_INET as u16 {
            return EAI_FAMILY;
        }

        // Extract IP address from sa_data (port at [0:2], IP at [2:6])
        let ip = [
            sockaddr.sa_data[2],
            sockaddr.sa_data[3],
            sockaddr.sa_data[4],
            sockaddr.sa_data[5],
        ];

        // Handle NI_NUMERICHOST flag
        if (flags & NI_NUMERICHOST) != 0 {
            // Convert IP to string format
            let mut buf = [0u8; 16];
            let mut pos = 0;
            
            for (i, octet) in ip.iter().enumerate() {
                // Convert octet to string
                let mut n = *octet as usize;
                let mut digits = [0u8; 3];
                let mut digit_count = 0;
                
                if n == 0 {
                    digits[0] = b'0';
                    digit_count = 1;
                } else {
                    while n > 0 && digit_count < 3 {
                        digits[digit_count] = (n % 10) as u8 + b'0';
                        n /= 10;
                        digit_count += 1;
                    }
                    // Reverse digits
                    for j in 0..digit_count {
                        if pos >= hostlen as usize {
                            return EAI_OVERFLOW;
                        }
                        *host.add(pos) = digits[digit_count - 1 - j];
                        pos += 1;
                    }
                }
                
                if i < 3 {
                    if pos >= hostlen as usize {
                        return EAI_OVERFLOW;
                    }
                    *host.add(pos) = b'.';
                    pos += 1;
                }
            }
            
            if pos >= hostlen as usize {
                return EAI_OVERFLOW;
            }
            *host.add(pos) = 0;
        } else {
            // Try reverse DNS lookup
            let resolver = match get_resolver() {
                Some(r) => r,
                None => return EAI_FAIL,
            };

            if let Some(hostname) = resolver.reverse_lookup_hosts(ip) {
                let hostname_bytes = hostname.as_bytes();
                if hostname_bytes.len() + 1 > hostlen as usize {
                    return EAI_OVERFLOW;
                }
                core::ptr::copy_nonoverlapping(hostname_bytes.as_ptr(), host, hostname_bytes.len());
                *host.add(hostname_bytes.len()) = 0;
            } else {
                // No reverse entry found, use numeric form
                let mut buf = [0u8; 16];
                let mut pos = 0;
                
                for (i, octet) in ip.iter().enumerate() {
                    let mut n = *octet as usize;
                    let mut digits = [0u8; 3];
                    let mut digit_count = 0;
                    
                    if n == 0 {
                        digits[0] = b'0';
                        digit_count = 1;
                    } else {
                        while n > 0 && digit_count < 3 {
                            digits[digit_count] = (n % 10) as u8 + b'0';
                            n /= 10;
                            digit_count += 1;
                        }
                        for j in 0..digit_count {
                            if pos >= hostlen as usize {
                                return EAI_OVERFLOW;
                            }
                            buf[pos] = digits[digit_count - 1 - j];
                            pos += 1;
                        }
                    }
                    
                    if i < 3 {
                        if pos >= hostlen as usize {
                            return EAI_OVERFLOW;
                        }
                        buf[pos] = b'.';
                        pos += 1;
                    }
                }
                
                if pos + 1 > hostlen as usize {
                    return EAI_OVERFLOW;
                }
                buf[pos] = 0;
                core::ptr::copy_nonoverlapping(buf.as_ptr(), host, pos + 1);
            }
        }

        // Handle service (port) if requested
        if !serv.is_null() && servlen > 0 {
            // Extract port from sa_data[0:2]
            let port = u16::from_be_bytes([sockaddr.sa_data[0], sockaddr.sa_data[1]]);
            let mut port_buf = [0u8; 6];
            let mut port_pos = 0;
            
            let mut p = port as usize;
            let mut digits = [0u8; 5];
            let mut digit_count = 0;
            
            if p == 0 {
                digits[0] = b'0';
                digit_count = 1;
            } else {
                while p > 0 && digit_count < 5 {
                    digits[digit_count] = (p % 10) as u8 + b'0';
                    p /= 10;
                    digit_count += 1;
                }
                for j in 0..digit_count {
                    port_buf[port_pos] = digits[digit_count - 1 - j];
                    port_pos += 1;
                }
            }
            
            if port_pos + 1 > servlen as usize {
                return EAI_OVERFLOW;
            }
            port_buf[port_pos] = 0;
            core::ptr::copy_nonoverlapping(port_buf.as_ptr(), serv, port_pos + 1);
        }
    }

    0
}
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
