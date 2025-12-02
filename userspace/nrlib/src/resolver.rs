/// Resolver implementation compatible with musl libc
///
/// Provides getaddrinfo/getnameinfo and /etc file parsing (/etc/hosts,
/// /etc/resolv.conf, /etc/nsswitch.conf).

use super::dns::{DnsQuery, QType, ResolverConfig};
use super::socket::{socket, sendto, recvfrom, SockAddr, SockAddrIn, AF_INET, SOCK_STREAM, SOCK_DGRAM, parse_ipv4};
use crate::get_system_dns_servers;
use core::mem;

const MAX_HOSTNAME: usize = 256;
const DNS_PORT: u16 = 53;
const KERNEL_DNS_QUERY_CAP: usize = 3;
const MAX_CNAME_DEPTH: u8 = 5;

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
        self.query_dns_recursive(hostname, nameserver_ip, 0)
    }

    fn query_dns_recursive(&self, hostname: &str, nameserver_ip: [u8; 4], depth: u8) -> Option<[u8; 4]> {
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

        // Build DNS query packet
        let mut query = DnsQuery::new();
        let query_id = 0x1337u16.wrapping_add(depth as u16);
        let query_packet = match query.build(query_id, hostname, QType::A) {
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
                if cname.eq_ignore_ascii_case(hostname) {
                    return None;
                }
                self.query_dns_recursive(cname, nameserver_ip, depth + 1)
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
                None => return EAI_FAIL,
            };

            // Try to resolve hostname via DNS
            match resolver.resolve(hostname) {
                Some(ip) => ip,
                None => return EAI_NONAME,
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
    if data.len() < 12 {
        return Err("dns response too short");
    }

    let transaction_id = u16::from_be_bytes([data[0], data[1]]);
    if transaction_id != expected_id {
        return Err("dns transaction id mismatch");
    }

    let flags = u16::from_be_bytes([data[2], data[3]]);
    if flags & 0x8000 == 0 {
        return Err("dns packet is not a response");
    }
    if flags & 0x0200 != 0 {
        return Err("dns response truncated");
    }
    if flags & 0x000F != 0 {
        return Err("dns server returned error");
    }

    let question_count = u16::from_be_bytes([data[4], data[5]]) as usize;
    let answer_count = u16::from_be_bytes([data[6], data[7]]) as usize;
    let authority_count = u16::from_be_bytes([data[8], data[9]]) as usize;
    let additional_count = u16::from_be_bytes([data[10], data[11]]) as usize;

    let mut offset = 12;
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

        match rtype {
            1 => {
                if rdlength != 4 {
                    return Err("invalid A record length");
                }
                let mut ip = [0u8; 4];
                ip.copy_from_slice(&data[offset..offset + 4]);
                return Ok(DnsParseOutcome::Address(ip));
            }
            5 => {
                let len = read_name_into(data, &mut offset, cname_buf)?;
                last_cname_len = Some(len);
                continue;
            }
            _ => {}
        }
        offset += rdlength;
    }

    for _ in 0..authority_count {
        skip_resource_record(data, &mut offset)?;
    }

    if let Some(len) = last_cname_len {
        let cname = &cname_buf[..len];
        for _ in 0..additional_count {
            let name_len = read_name_into(data, &mut offset, scratch_buf)?;
            if offset + 10 > data.len() {
                return Err("dns additional header overflow");
            }
            let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]) as usize;
            offset += 10;
            if offset + rdlength > data.len() {
                return Err("dns additional data overflow");
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

    loop {
        if pos >= data.len() {
            return Err("dns name exceeds packet");
        }
        let len = data[pos];
        if len & 0xC0 == 0xC0 {
            if pos + 1 >= data.len() {
                return Err("dns name pointer overflow");
            }
            let ptr = (((len & 0x3F) as usize) << 8) | (data[pos + 1] as usize);
            if ptr >= data.len() {
                return Err("dns name pointer out of bounds");
            }
            if !jumped {
                *offset = pos + 2;
            }
            pos = ptr;
            jumped = true;
            steps += 1;
            if steps > data.len() {
                return Err("dns name pointer loop");
            }
            continue;
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

    loop {
        if pos >= data.len() {
            return Err("dns name exceeds packet");
        }
        let len = data[pos];
        if len & 0xC0 == 0xC0 {
            if pos + 1 >= data.len() {
                return Err("dns name pointer overflow");
            }
            let ptr = (((len & 0x3F) as usize) << 8) | (data[pos + 1] as usize);
            if ptr >= data.len() {
                return Err("dns name pointer out of bounds");
            }
            if !jumped {
                *offset = pos + 2;
            }
            pos = ptr;
            jumped = true;
            steps += 1;
            if steps > data.len() {
                return Err("dns name pointer loop");
            }
            continue;
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
}
