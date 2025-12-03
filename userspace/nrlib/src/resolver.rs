/// Resolver implementation compatible with musl libc
///
/// Provides getaddrinfo/getnameinfo and /etc file parsing (/etc/hosts,
/// /etc/resolv.conf, /etc/nsswitch.conf).

use super::dns::{DnsQuery, QType, ResolverConfig};
use super::socket::{socket, sendto, recvfrom, SockAddr, SockAddrIn, AF_INET, SOCK_STREAM, SOCK_DGRAM, parse_ipv4};
use crate::get_system_dns_servers;
use core::hint::black_box;
use core::mem;
use core::sync::atomic::{AtomicU16, Ordering};

const MAX_HOSTNAME: usize = 256;
const DNS_PORT: u16 = 53;
const KERNEL_DNS_QUERY_CAP: usize = 3;
const MAX_CNAME_DEPTH: u8 = 8;
const MAX_DNS_RETRIES: u8 = 3;

/// Atomic counter for generating unique DNS transaction IDs
static DNS_TRANSACTION_COUNTER: AtomicU16 = AtomicU16::new(0x1337);

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
    /// Tries the hostname as-is first, then with search domains appended
    pub fn query_dns(&self, hostname: &str, nameserver_ip: [u8; 4]) -> Option<[u8; 4]> {
        // First try the hostname as given
        if let Some(ip) = self.query_dns_with_retry(hostname, nameserver_ip, 0) {
            return Some(ip);
        }

        // If hostname doesn't contain a dot, try appending search domains
        if !hostname.contains('.') {
            for i in 0..self.config.search_domain_count {
                let domain = &self.config.search_domains[i];
                // Find the actual length of the search domain
                let domain_len = domain.iter().position(|&b| b == 0).unwrap_or(domain.len());
                if domain_len == 0 {
                    continue;
                }

                // Build FQDN: hostname.searchdomain
                let mut fqdn_buf = [0u8; MAX_HOSTNAME];
                let hostname_bytes = hostname.as_bytes();
                if hostname_bytes.len() + 1 + domain_len >= MAX_HOSTNAME {
                    continue;
                }

                fqdn_buf[..hostname_bytes.len()].copy_from_slice(hostname_bytes);
                fqdn_buf[hostname_bytes.len()] = b'.';
                fqdn_buf[hostname_bytes.len() + 1..hostname_bytes.len() + 1 + domain_len]
                    .copy_from_slice(&domain[..domain_len]);

                if let Ok(fqdn) = core::str::from_utf8(&fqdn_buf[..hostname_bytes.len() + 1 + domain_len]) {
                    if let Some(ip) = self.query_dns_with_retry(fqdn, nameserver_ip, 0) {
                        return Some(ip);
                    }
                }
            }
        }

        None
    }

    /// Perform DNS query with retry logic
    fn query_dns_with_retry(&self, hostname: &str, nameserver_ip: [u8; 4], depth: u8) -> Option<[u8; 4]> {
        let max_attempts = self.config.attempts.max(1).min(MAX_DNS_RETRIES);
        for _attempt in 0..max_attempts {
            if let Some(ip) = self.query_dns_internal(hostname, nameserver_ip, depth) {
                return Some(ip);
            }
        }
        None
    }

    fn query_dns_internal(&self, hostname: &str, nameserver_ip: [u8; 4], depth: u8) -> Option<[u8; 4]> {
        use super::socket::bind;
        use crate::close;

        if depth >= MAX_CNAME_DEPTH {
            return None;
        }

        // Create UDP socket
        let sockfd = socket(AF_INET, SOCK_DGRAM, 0);
        if sockfd < 0 {
            return None;
        }

        // Helper to close socket on return
        struct SocketGuard(i32);
        impl Drop for SocketGuard {
            fn drop(&mut self) {
                let _ = close(self.0);
            }
        }
        let _guard = SocketGuard(sockfd);

        // Set receive timeout using setsockopt SO_RCVTIMEO
        // This prevents recvfrom from blocking indefinitely
        // timeval structure: tv_sec (8 bytes) + tv_usec (8 bytes) = 16 bytes
        let timeout_secs = self.config.timeout_ms as u64 / 1000;
        let timeout_usec = ((self.config.timeout_ms as u64 % 1000) * 1000) as u64;
        let timeval = [timeout_secs, timeout_usec];
        const SOL_SOCKET: i32 = 1;
        const SO_RCVTIMEO: i32 = 20;
        unsafe {
            crate::libc_compat::network::setsockopt(
                sockfd,
                SOL_SOCKET,
                SO_RCVTIMEO,
                timeval.as_ptr() as *const crate::c_void,
                16, // sizeof(timeval)
            );
        }

        // Bind to any local address and port (0.0.0.0:0)
        // This is essential for UDP sockets to work properly
        let local_addr = SockAddrIn::new([0, 0, 0, 0], 0);
        let local_sockaddr = SockAddr::from(local_addr);
        if bind(sockfd, &local_sockaddr, mem::size_of::<SockAddr>() as u32) < 0 {
            return None;
        }

        // Build DNS query packet with unique transaction ID
        let mut query = DnsQuery::new();
        // Generate unique transaction ID using atomic counter and some entropy
        let base_id = DNS_TRANSACTION_COUNTER.fetch_add(1, Ordering::Relaxed);
        // Mix in some pseudo-randomness from hostname hash and depth
        let hostname_hash = hostname.bytes().fold(0u16, |acc, b| acc.wrapping_add(b as u16).rotate_left(3));
        let query_id = base_id.wrapping_add(hostname_hash).wrapping_add((depth as u16) << 8);
        let query_packet = match query.build(query_id, hostname, QType::A) {
            Ok(pkt) => pkt,
            Err(_) => {
                return None;
            }
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

        // Prevent optimizer from eliding the syscall result check
        let sent = black_box(sent);

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

        // Prevent optimizer from eliding the syscall result check
        let received = black_box(received);

        if received <= 0 {
            return None;
        }
        
        // Minimum DNS response size check (header only is 12 bytes)
        if (received as usize) < 12 {
            return None;
        }

        // Parse DNS response
        let response_data = &response_buf[..received as usize];
        let mut cname_buf = [0u8; MAX_HOSTNAME];
        let mut scratch_buf = [0u8; MAX_HOSTNAME];
        let outcome = match parse_dns_response(response_data, query_id, &mut cname_buf, &mut scratch_buf) {
            Ok(result) => result,
            Err(_) => return None,
        };

        match outcome {
            DnsParseOutcome::Address(ip) => Some(ip),
            DnsParseOutcome::Cname(len) => {
                if len == 0 {
                    return None;
                }
                let cname = core::str::from_utf8(&cname_buf[..len]).ok()?;
                // Prevent CNAME loops by checking if we've seen this name
                if cname.eq_ignore_ascii_case(hostname) {
                    return None;
                }
                // Validate CNAME is a proper hostname
                if cname.is_empty() || cname.len() > 253 {
                    return None;
                }
                self.query_dns_internal(cname, nameserver_ip, depth + 1)
            }
            DnsParseOutcome::NoAnswer => None,
        }
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
                        let ns = self.config.nameservers[i];
                        if let Some(ip) = self.query_dns(hostname, ns) {
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
    }

    if resolver.config.nameserver_count == 0 {
        let mut kernel_dns = [0u32; KERNEL_DNS_QUERY_CAP];
        if let Ok(count) = get_system_dns_servers(&mut kernel_dns) {
            for idx in 0..count {
                let ip_bytes = kernel_dns[idx].to_be_bytes();
                let _ = resolver.config.add_nameserver(ip_bytes);
            }
        }
    }

    if resolver.config.nameserver_count == 0 {
        let _ = resolver.config.add_nameserver([10, 0, 2, 3]);
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

/// Simple IP address formatting (without allocation)
#[allow(dead_code)]
fn format_ip_simple(ip: &[u8; 4], buf: &mut [u8; 16]) -> usize {
    let mut pos = 0;
    for (i, &octet) in ip.iter().enumerate() {
        if i > 0 {
            buf[pos] = b'.';
            pos += 1;
        }
        // Convert number to string
        let mut n = octet as usize;
        if n == 0 {
            buf[pos] = b'0';
            pos += 1;
        } else {
            let mut digits = [0u8; 3];
            let mut digit_count = 0;
            while n > 0 {
                digits[digit_count] = (n % 10) as u8 + b'0';
                n /= 10;
                digit_count += 1;
            }
            for j in (0..digit_count).rev() {
                buf[pos] = digits[j];
                pos += 1;
            }
        }
    }
    pos
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
            Err(_) => {
                return EAI_NONAME;
            }
        };

        // Parse service (port number) if provided
        let mut port: u16 = 0;
        if !service.is_null() {
            let mut service_buf = [0u8; 16];
            let mut slen = 0;
            while slen < 15 {
                let byte = *service.add(slen);
                if byte == 0 {
                    break;
                }
                service_buf[slen] = byte;
                slen += 1;
            }
            
            if slen > 0 {
                if let Ok(service_str) = core::str::from_utf8(&service_buf[..slen]) {
                    // Try to parse as numeric port
                    if let Ok(p) = service_str.parse::<u16>() {
                        port = p;
                    } else {
                        // TODO: Look up service name in /etc/services
                        // For now, common services:
                        port = match service_str {
                            "http" => 80,
                            "https" => 443,
                            "ftp" => 21,
                            "ssh" => 22,
                            "telnet" => 23,
                            "smtp" => 25,
                            "dns" => 53,
                            _ => return EAI_SERVICE,
                        };
                    }
                }
            }
        }

        // First, try to parse hostname as a numeric IP address
        // This avoids unnecessary DNS queries for addresses like "192.168.1.1"
        let ip = if let Some(numeric_ip) = parse_ipv4(hostname) {
            numeric_ip
        } else {
            // Get resolver and ensure it's initialized
            let resolver = match get_resolver() {
                Some(r) => r,
                None => {
                    return EAI_FAIL;
                }
            };

            // Try to resolve hostname via DNS
            match resolver.resolve(hostname) {
                Some(ip) => ip,
                None => {
                    return EAI_NONAME;
                }
            }
        };

        // Determine socket type and protocol from hints
        let (socktype, protocol) = if !hints.is_null() {
            let hints_ref = &*hints;
            let st = if hints_ref.ai_socktype != 0 {
                hints_ref.ai_socktype
            } else {
                SOCK_STREAM // Default to TCP
            };
            let proto = if hints_ref.ai_protocol != 0 {
                hints_ref.ai_protocol
            } else if st == SOCK_STREAM {
                6 // IPPROTO_TCP
            } else {
                17 // IPPROTO_UDP
            };
            (st, proto)
        } else {
            (SOCK_STREAM, 6) // Default to TCP
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

        // Initialize SockAddrIn with resolved IP and port
        *addr_ptr = SockAddrIn::new(ip, port);

        // Initialize AddrInfo
        let addrinfo = &mut *addrinfo_ptr;
        addrinfo.ai_flags = 0;
        addrinfo.ai_family = AF_INET;
        addrinfo.ai_socktype = socktype;
        addrinfo.ai_protocol = protocol;
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

/// Convert error code to static string slice (Rust-friendly version)
/// Note: C-compatible gai_strerror is provided in libc_compat/network.rs
pub fn gai_strerror_str(errcode: i32) -> &'static str {
    match errcode {
        0 => "Success",
        EAI_BADFLAGS => "Invalid flags",
        EAI_NONAME => "Name or service not known",
        EAI_AGAIN => "Temporary failure in name resolution",
        EAI_FAIL => "Non-recoverable failure in name resolution",
        EAI_FAMILY => "Address family not supported",
        EAI_SOCKTYPE => "Socket type not supported",
        EAI_SERVICE => "Service not supported for socket type",
        EAI_MEMORY => "Memory allocation failure",
        EAI_SYSTEM => "System error",
        EAI_OVERFLOW => "Buffer overflow",
        _ => "Unknown error",
    }
}

#[derive(Debug, Clone, Copy)]
enum DnsParseOutcome {
    Address([u8; 4]),
    Cname(usize),
    NoAnswer,
}

fn parse_dns_response(
    data: &[u8],
    expected_id: u16,
    cname_buf: &mut [u8],
    scratch_buf: &mut [u8],
) -> Result<DnsParseOutcome, &'static str> {
    // Minimum DNS response: 12 byte header + at least minimal question
    if data.len() < 12 {
        return Err("dns response too short");
    }
    
    // Maximum reasonable DNS response size (RFC 6891 recommends 4096 for EDNS)
    // But we only support 512 byte UDP responses without EDNS
    if data.len() > 512 {
        return Err("dns response too large");
    }

    // Validate transaction ID matches our query
    let transaction_id = u16::from_be_bytes([data[0], data[1]]);
    if transaction_id != expected_id {
        return Err("dns transaction id mismatch");
    }

    let flags = u16::from_be_bytes([data[2], data[3]]);
    
    // QR bit (bit 15) must be 1 for response
    if flags & 0x8000 == 0 {
        return Err("dns packet is not a response");
    }
    
    // TC bit (bit 9) indicates truncation - response may be incomplete
    // For UDP we cannot recover from this, but we can try to parse what we have
    let truncated = flags & 0x0200 != 0;
    
    // RCODE (bits 0-3) should be 0 for success
    let rcode = flags & 0x000F;
    match rcode {
        0 => {} // NOERROR - success
        1 => return Err("dns format error"),
        2 => return Err("dns server failure"),
        3 => return Err("dns name does not exist"), // NXDOMAIN
        4 => return Err("dns not implemented"),
        5 => return Err("dns query refused"),
        _ => return Err("dns server returned error"),
    }

    let question_count = u16::from_be_bytes([data[4], data[5]]) as usize;
    let answer_count = u16::from_be_bytes([data[6], data[7]]) as usize;
    let authority_count = u16::from_be_bytes([data[8], data[9]]) as usize;
    let additional_count = u16::from_be_bytes([data[10], data[11]]) as usize;

    // Sanity check: prevent excessive iteration
    if question_count > 64 || answer_count > 256 || authority_count > 256 || additional_count > 256 {
        return Err("dns response has too many records");
    }
    
    // If truncated and no answers, we can't proceed
    if truncated && answer_count == 0 {
        return Err("dns response truncated with no answers");
    }

    let mut offset = 12;
    
    // Skip question section
    for _ in 0..question_count {
        skip_name(data, &mut offset)?;
        if offset + 4 > data.len() {
            return Err("dns question overflow");
        }
        offset += 4;
    }

    let mut last_cname_len: Option<usize> = None;

    for _ in 0..answer_count {
        skip_name(data, &mut offset)?;
        if offset + 10 > data.len() {
            return Err("dns answer header overflow");
        }
        let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]) as usize;
        offset += 10;
        if offset + rdlength > data.len() {
            return Err("dns answer data overflow");
        }

        // Save rdata start position for CNAME parsing
        let rdata_start = offset;

        match rtype {
            1 => {
                // A record (IPv4 address)
                if rdlength != 4 {
                    return Err("invalid A record length");
                }
                let mut ip = [0u8; 4];
                ip.copy_from_slice(&data[offset..offset + 4]);
                return Ok(DnsParseOutcome::Address(ip));
            }
            5 => {
                // CNAME record - read the canonical name
                // Note: read_name_into updates offset, but we need to ensure
                // we advance by exactly rdlength for proper packet parsing
                let mut temp_offset = rdata_start;
                let len = read_name_into(data, &mut temp_offset, cname_buf)?;
                last_cname_len = Some(len);
                // Skip past the entire rdata section
                offset = rdata_start + rdlength;
                continue;
            }
            28 => {
                // AAAA record (IPv6) - skip for now, we only support IPv4
                offset += rdlength;
                continue;
            }
            _ => {
                // Unknown record type - skip
            }
        }
        offset += rdlength;
    }

    for _ in 0..authority_count {
        skip_resource_record(data, &mut offset)?;
    }

    if let Some(len) = last_cname_len {
        let cname = &cname_buf[..len];
        // Try to find a matching A record in additional section
        // This optimization avoids an extra DNS query for the CNAME target
        for _ in 0..additional_count {
            let name_len = match read_name_into(data, &mut offset, scratch_buf) {
                Ok(l) => l,
                Err(_) => {
                    // If we can't parse additional records, just return CNAME
                    // The caller will do a followup query
                    return Ok(DnsParseOutcome::Cname(len));
                }
            };
            if offset + 10 > data.len() {
                // Truncated additional section, return CNAME for followup
                return Ok(DnsParseOutcome::Cname(len));
            }
            let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]) as usize;
            offset += 10;
            if offset + rdlength > data.len() {
                // Truncated data, return CNAME for followup
                return Ok(DnsParseOutcome::Cname(len));
            }
            if rtype == 1
                && rdlength == 4
                && name_len == len
                && dns_name_equals(&scratch_buf[..name_len], cname)
            {
                let mut ip = [0u8; 4];
                ip.copy_from_slice(&data[offset..offset + 4]);
                return Ok(DnsParseOutcome::Address(ip));
            }
            offset += rdlength;
        }
        return Ok(DnsParseOutcome::Cname(len));
    }
    
    // Skip additional section if we didn't find any answers
    // (already skipped if we had a CNAME)

    for _ in 0..additional_count {
        skip_resource_record(data, &mut offset)?;
    }

    Ok(DnsParseOutcome::NoAnswer)
}

fn skip_resource_record(data: &[u8], offset: &mut usize) -> Result<(), &'static str> {
    skip_name(data, offset)?;
    if *offset + 10 > data.len() {
        return Err("dns rr header overflow");
    }
    let rdlength = u16::from_be_bytes([data[*offset + 8], data[*offset + 9]]) as usize;
    *offset += 10;
    if *offset + rdlength > data.len() {
        return Err("dns rr data overflow");
    }
    *offset += rdlength;
    Ok(())
}

fn skip_name(data: &[u8], offset: &mut usize) -> Result<(), &'static str> {
    let mut pos = *offset;
    let mut jumped = false;
    let mut steps = 0;
    let initial_offset = *offset;

    loop {
        if pos >= data.len() {
            return Err("dns name exceeds packet");
        }
        let len = data[pos];
        
        // Check for compression pointer (top 2 bits = 11)
        if len & 0xC0 == 0xC0 {
            if pos + 1 >= data.len() {
                return Err("dns name pointer overflow");
            }
            let ptr = (((len & 0x3F) as usize) << 8) | (data[pos + 1] as usize);
            // Pointer must point to earlier in the packet (forward references not allowed)
            // Also prevent self-referencing pointers
            if ptr >= data.len() || ptr >= initial_offset || ptr == pos {
                return Err("dns name pointer out of bounds");
            }
            if !jumped {
                *offset = pos + 2;
            }
            pos = ptr;
            jumped = true;
            steps += 1;
            // Limit pointer hops to prevent infinite loops
            if steps > 128 {
                return Err("dns name pointer loop");
            }
            continue;
        }
        
        // Check for reserved label type (top 2 bits = 01 or 10)
        if len & 0xC0 != 0 {
            return Err("dns invalid label type");
        }

        if len == 0 {
            if !jumped {
                *offset = pos + 1;
            }
            return Ok(());
        }

        pos += 1;
        if pos + len as usize > data.len() {
            return Err("dns label exceeds packet");
        }
        pos += len as usize;
        if !jumped {
            *offset = pos;
        }
    }
}

fn read_name_into(data: &[u8], offset: &mut usize, out: &mut [u8]) -> Result<usize, &'static str> {
    let mut pos = *offset;
    let mut jumped = false;
    let mut steps = 0;
    let mut buf_pos = 0;
    let mut total_len = 0usize; // Track total name length
    let initial_offset = *offset;

    loop {
        if pos >= data.len() {
            return Err("dns name exceeds packet");
        }
        let len = data[pos];
        
        // Check for compression pointer
        if len & 0xC0 == 0xC0 {
            if pos + 1 >= data.len() {
                return Err("dns name pointer overflow");
            }
            let ptr = (((len & 0x3F) as usize) << 8) | (data[pos + 1] as usize);
            // Pointer must point to earlier in the packet (prevent forward and self references)
            if ptr >= data.len() || ptr >= initial_offset || ptr == pos {
                return Err("dns name pointer out of bounds");
            }
            if !jumped {
                *offset = pos + 2;
            }
            pos = ptr;
            jumped = true;
            steps += 1;
            if steps > 128 {
                return Err("dns name pointer loop");
            }
            continue;
        }
        
        // Check for reserved label type
        if len & 0xC0 != 0 {
            return Err("dns invalid label type");
        }

        if len == 0 {
            if !jumped {
                *offset = pos + 1;
            }
            if buf_pos < out.len() {
                out[buf_pos] = 0;
            }
            return Ok(buf_pos);
        }
        
        // Validate total name length won't exceed DNS limit (253 chars)
        total_len += len as usize + 1; // +1 for dot separator
        if total_len > 254 {
            return Err("dns name too long");
        }

        pos += 1;
        if pos + len as usize > data.len() {
            return Err("dns label exceeds packet");
        }

        if buf_pos != 0 {
            if buf_pos >= out.len() {
                return Err("dns name too long");
            }
            out[buf_pos] = b'.';
            buf_pos += 1;
        }

        if buf_pos + len as usize > out.len() {
            return Err("dns name too long");
        }
        out[buf_pos..buf_pos + len as usize].copy_from_slice(&data[pos..pos + len as usize]);
        buf_pos += len as usize;
        pos += len as usize;

        if !jumped {
            *offset = pos;
        }
    }
}

fn dns_name_equals(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for i in 0..a.len() {
        if to_ascii_lower(a[i]) != to_ascii_lower(b[i]) {
            return false;
        }
    }
    true
}

fn to_ascii_lower(byte: u8) -> u8 {
    if byte >= b'A' && byte <= b'Z' {
        byte + 32
    } else {
        byte
    }
}

/// Format IPv4 address to string in buffer
/// Returns the length written (excluding null terminator), or 0 on overflow
#[inline]
fn format_ipv4_to_buffer(ip: [u8; 4], buf: *mut u8, buf_len: usize) -> usize {
    if buf_len < 8 {
        // Minimum: "0.0.0.0\0" = 8 bytes
        return 0;
    }

    let mut pos = 0;

    for (i, octet) in ip.iter().enumerate() {
        let mut n = *octet as usize;
        let mut digits = [0u8; 3];
        let mut digit_count = 0;

        // Convert number to digits (reversed)
        if n == 0 {
            digits[0] = b'0';
            digit_count = 1;
        } else {
            while n > 0 {
                digits[digit_count] = (n % 10) as u8 + b'0';
                n /= 10;
                digit_count += 1;
            }
        }

        // Write digits in correct order
        for j in 0..digit_count {
            if pos >= buf_len - 1 {
                return 0;
            }
            unsafe {
                *buf.add(pos) = digits[digit_count - 1 - j];
            }
            pos += 1;
        }

        // Add dot separator (except after last octet)
        if i < 3 {
            if pos >= buf_len - 1 {
                return 0;
            }
            unsafe {
                *buf.add(pos) = b'.';
            }
            pos += 1;
        }
    }

    // Null terminate
    if pos >= buf_len {
        return 0;
    }
    unsafe {
        *buf.add(pos) = 0;
    }

    pos
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
            // Convert IP to string format using helper function
            let len = format_ipv4_to_buffer(ip, host, hostlen as usize);
            if len == 0 {
                return EAI_OVERFLOW;
            }
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
                // NI_NAMEREQD requires a name to be found
                if (flags & NI_NAMEREQD) != 0 {
                    return EAI_NONAME;
                }
                // No reverse entry found, use numeric form
                let len = format_ipv4_to_buffer(ip, host, hostlen as usize);
                if len == 0 {
                    return EAI_OVERFLOW;
                }
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
            }
            // Write digits in correct order
            for j in 0..digit_count {
                port_buf[port_pos] = digits[digit_count - 1 - j];
                port_pos += 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ipv4() {
        assert_eq!(parse_ipv4("192.168.1.1"), Some([192, 168, 1, 1]));
        assert_eq!(parse_ipv4("8.8.8.8"), Some([8, 8, 8, 8]));
        assert_eq!(parse_ipv4("0.0.0.0"), Some([0, 0, 0, 0]));
        assert_eq!(parse_ipv4("255.255.255.255"), Some([255, 255, 255, 255]));
        assert_eq!(parse_ipv4("invalid"), None);
        assert_eq!(parse_ipv4("256.0.0.1"), None);
        assert_eq!(parse_ipv4("1.2.3"), None);
        assert_eq!(parse_ipv4("1.2.3.4.5"), None);
        assert_eq!(parse_ipv4(""), None);
        assert_eq!(parse_ipv4("1.2.3."), None);
        assert_eq!(parse_ipv4(".1.2.3"), None);
    }

    #[test]
    fn test_parse_resolv_conf() {
        let mut resolver = Resolver::new();
        let content = "nameserver 8.8.8.8\nnameserver 1.1.1.1\nsearch example.com\n";
        resolver.parse_resolv_conf(content);

        assert_eq!(resolver.config.nameserver_count, 2);
        assert_eq!(resolver.config.nameservers[0], [8, 8, 8, 8]);
        assert_eq!(resolver.config.nameservers[1], [1, 1, 1, 1]);
        assert_eq!(resolver.config.search_domain_count, 1);
    }

    #[test]
    fn test_parse_resolv_conf_with_options() {
        let mut resolver = Resolver::new();
        let content = "nameserver 8.8.8.8\noptions timeout:3 attempts:5\n";
        resolver.parse_resolv_conf(content);

        assert_eq!(resolver.config.nameserver_count, 1);
        assert_eq!(resolver.config.timeout_ms, 3000);
        assert_eq!(resolver.config.attempts, 5);
    }

    #[test]
    fn test_parse_resolv_conf_comments() {
        let mut resolver = Resolver::new();
        let content = "# This is a comment\nnameserver 8.8.8.8\n# Another comment\n";
        resolver.parse_resolv_conf(content);

        assert_eq!(resolver.config.nameserver_count, 1);
        assert_eq!(resolver.config.nameservers[0], [8, 8, 8, 8]);
    }

    #[test]
    fn test_parse_hosts() {
        let mut resolver = Resolver::new();
        let content = "127.0.0.1 localhost\n192.168.1.100 myhost\n";
        resolver.parse_hosts(content);

        assert_eq!(resolver.hosts_count, 2);
        assert_eq!(resolver.lookup_hosts("localhost"), Some([127, 0, 0, 1]));
        assert_eq!(resolver.lookup_hosts("myhost"), Some([192, 168, 1, 100]));
        // Case insensitive matching
        assert_eq!(resolver.lookup_hosts("LOCALHOST"), Some([127, 0, 0, 1]));
        assert_eq!(resolver.lookup_hosts("MyHost"), Some([192, 168, 1, 100]));
    }

    #[test]
    fn test_parse_hosts_multiple_aliases() {
        let mut resolver = Resolver::new();
        let content = "127.0.0.1 localhost loopback\n";
        resolver.parse_hosts(content);

        assert_eq!(resolver.hosts_count, 2);
        assert_eq!(resolver.lookup_hosts("localhost"), Some([127, 0, 0, 1]));
        assert_eq!(resolver.lookup_hosts("loopback"), Some([127, 0, 0, 1]));
    }

    #[test]
    fn test_parse_nsswitch() {
        let mut resolver = Resolver::new();
        let content = "hosts: files dns\n";
        resolver.parse_nsswitch(content);

        assert_eq!(resolver.nsswitch_count, 2);
        assert_eq!(resolver.nsswitch_hosts[0], NssSource::Files);
        assert_eq!(resolver.nsswitch_hosts[1], NssSource::Dns);
    }

    #[test]
    fn test_gai_strerror() {
        assert_eq!(gai_strerror_str(0), "Success");
        assert_eq!(gai_strerror_str(EAI_NONAME), "Name or service not known");
        assert_eq!(gai_strerror_str(EAI_AGAIN), "Temporary failure in name resolution");
        assert_eq!(gai_strerror_str(-999), "Unknown error");
    }

    #[test]
    fn test_format_ipv4_to_buffer() {
        let mut buf = [0u8; 16];
        let len = format_ipv4_to_buffer([192, 168, 1, 1], buf.as_mut_ptr(), 16);
        assert!(len > 0);
        assert_eq!(&buf[..len], b"192.168.1.1");
        
        let len = format_ipv4_to_buffer([0, 0, 0, 0], buf.as_mut_ptr(), 16);
        assert!(len > 0);
        assert_eq!(&buf[..len], b"0.0.0.0");
        
        let len = format_ipv4_to_buffer([255, 255, 255, 255], buf.as_mut_ptr(), 16);
        assert!(len > 0);
        assert_eq!(&buf[..len], b"255.255.255.255");
    }

    #[test]
    fn test_format_ipv4_buffer_too_small() {
        let mut buf = [0u8; 4];
        let len = format_ipv4_to_buffer([192, 168, 1, 1], buf.as_mut_ptr(), 4);
        assert_eq!(len, 0); // Should fail due to buffer too small
    }

    #[test]
    fn test_parse_dns_response_a_record() {
        let response: [u8; 45] = [
            0x30, 0x30, 0x85, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, // header
            0x05, b'b', b'a', b'i', b'd', b'u', 0x03, b'c', b'o', b'm', 0x00, // question name
            0x00, 0x01, 0x00, 0x01, // qtype, qclass
            0xC0, 0x0C, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04,
            0xC6, 0x12, 0x00, 0x65, // answer A record 198.18.0.101
        ];

        let mut cname = [0u8; MAX_HOSTNAME];
        let mut scratch = [0u8; MAX_HOSTNAME];
        match parse_dns_response(&response, 0x3030, &mut cname, &mut scratch).unwrap() {
            DnsParseOutcome::Address(ip) => assert_eq!(ip, [0xC6, 0x12, 0x00, 0x65]),
            other => panic!("unexpected parse result: {:?}", other),
        }
    }

    #[test]
    fn test_parse_dns_response_wrong_id() {
        let response: [u8; 45] = [
            0x30, 0x30, 0x85, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
            0x05, b'b', b'a', b'i', b'd', b'u', 0x03, b'c', b'o', b'm', 0x00,
            0x00, 0x01, 0x00, 0x01,
            0xC0, 0x0C, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04,
            0xC6, 0x12, 0x00, 0x65,
        ];

        let mut cname = [0u8; MAX_HOSTNAME];
        let mut scratch = [0u8; MAX_HOSTNAME];
        let result = parse_dns_response(&response, 0x1234, &mut cname, &mut scratch);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_dns_response_nxdomain() {
        // NXDOMAIN response (RCODE = 3)
        let response: [u8; 25] = [
            0x12, 0x34, 0x81, 0x83, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x04, b't', b'e', b's', b't', 0x03, b'c', b'o', b'm', 0x00,
            0x00, 0x01, 0x00, 0x01,
        ];

        let mut cname = [0u8; MAX_HOSTNAME];
        let mut scratch = [0u8; MAX_HOSTNAME];
        let result = parse_dns_response(&response, 0x1234, &mut cname, &mut scratch);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_dns_response_too_short() {
        let response: [u8; 8] = [0x12, 0x34, 0x85, 0x80, 0x00, 0x01, 0x00, 0x01];
        let mut cname = [0u8; MAX_HOSTNAME];
        let mut scratch = [0u8; MAX_HOSTNAME];
        let result = parse_dns_response(&response, 0x1234, &mut cname, &mut scratch);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_dns_response_cname_with_glue() {
        let response: [u8; 87] = [
            0x11, 0x11, 0x85, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // header
            0x05, b'a', b'l', b'i', b'a', b's', 0x04, b't', b'e', b's', b't', 0x03, b'c', b'o', b'm', 0x00, // question
            0x00, 0x01, 0x00, 0x01,
            0xC0, 0x0C, 0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11, // answer header
            0x06, b't', b'a', b'r', b'g', b'e', b't', 0x04, b't', b'e', b's', b't', 0x03, b'c', b'o', b'm', 0x00, // canonical name
            0x06, b't', b'a', b'r', b'g', b'e', b't', 0x04, b't', b'e', b's', b't', 0x03, b'c', b'o', b'm', 0x00, // additional name
            0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04,
            0x01, 0x02, 0x03, 0x04, // glue A record
        ];

        let mut cname = [0u8; MAX_HOSTNAME];
        let mut scratch = [0u8; MAX_HOSTNAME];
        match parse_dns_response(&response, 0x1111, &mut cname, &mut scratch).unwrap() {
            DnsParseOutcome::Address(ip) => assert_eq!(ip, [1, 2, 3, 4]),
            other => panic!("unexpected parse result: {:?}", other),
        }
    }

    #[test]
    fn test_dns_name_equals() {
        assert!(dns_name_equals(b"example.com", b"example.com"));
        assert!(dns_name_equals(b"EXAMPLE.COM", b"example.com"));
        assert!(dns_name_equals(b"Example.Com", b"EXAMPLE.COM"));
        assert!(!dns_name_equals(b"example.com", b"example.org"));
        assert!(!dns_name_equals(b"example.com", b"example.co"));
    }

    #[test]
    fn test_parse_ipv4_edge_cases() {
        // Empty octets should be rejected
        assert_eq!(parse_ipv4("1..2.3"), None);
        assert_eq!(parse_ipv4("1.2..3"), None);
        // Leading/trailing dots
        assert_eq!(parse_ipv4(".1.2.3.4"), None);
        assert_eq!(parse_ipv4("1.2.3.4."), None);
        // Too many octets
        assert_eq!(parse_ipv4("1.2.3.4.5"), None);
        // Valid cases
        assert_eq!(parse_ipv4("10.0.2.3"), Some([10, 0, 2, 3]));
        assert_eq!(parse_ipv4("127.0.0.1"), Some([127, 0, 0, 1]));
    }

    #[test]
    fn test_host_entry_matches() {
        let mut entry = HostEntry::empty();
        entry.name[..9].copy_from_slice(b"localhost");
        entry.name_len = 9;
        entry.ip = [127, 0, 0, 1];

        assert!(entry.matches("localhost"));
        assert!(entry.matches("LOCALHOST"));
        assert!(entry.matches("LocalHost"));
        assert!(!entry.matches("localhosts"));
        assert!(!entry.matches("localhos"));
    }

    #[test]
    fn test_resolver_nss_sources() {
        let mut resolver = Resolver::new();
        // Default should be files, dns
        assert_eq!(resolver.nsswitch_count, 2);
        assert_eq!(resolver.nsswitch_hosts[0], NssSource::Files);
        assert_eq!(resolver.nsswitch_hosts[1], NssSource::Dns);

        // Parse custom nsswitch
        resolver.parse_nsswitch("hosts: dns files\n");
        assert_eq!(resolver.nsswitch_count, 2);
        assert_eq!(resolver.nsswitch_hosts[0], NssSource::Dns);
        assert_eq!(resolver.nsswitch_hosts[1], NssSource::Files);
    }

    #[test]
    fn test_format_ipv4_with_zeros() {
        let mut buf = [0u8; 16];
        let len = format_ipv4_to_buffer([0, 0, 0, 0], buf.as_mut_ptr(), 16);
        assert!(len > 0);
        assert_eq!(&buf[..len], b"0.0.0.0");

        let len = format_ipv4_to_buffer([10, 0, 0, 1], buf.as_mut_ptr(), 16);
        assert!(len > 0);
        assert_eq!(&buf[..len], b"10.0.0.1");
    }
}
