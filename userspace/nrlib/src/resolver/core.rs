/// Core Resolver implementation
///
/// Handles configuration parsing, hosts lookup, and DNS queries.
use core::hint::black_box;
use core::mem;
use core::sync::atomic::{AtomicU16, Ordering};

use crate::close;
use crate::dns::{DnsQuery, QType, ResolverConfig};
use crate::socket::{
    bind, parse_ipv4, recvfrom, sendto, socket, SockAddr, SockAddrIn, AF_INET, SOCK_DGRAM,
};

use super::constants::{DNS_PORT, MAX_CNAME_DEPTH, MAX_DNS_RETRIES, MAX_HOSTNAME};
use super::dns_parser::{parse_dns_response, DnsParseOutcome};
use super::types::{HostEntry, NssSource};

/// Atomic counter for generating unique DNS transaction IDs
static DNS_TRANSACTION_COUNTER: AtomicU16 = AtomicU16::new(0x1337);

/// Global resolver state
pub struct Resolver {
    pub(crate) config: ResolverConfig,
    pub(crate) hosts: [HostEntry; 32],
    pub(crate) hosts_count: usize,
    pub(crate) nsswitch_hosts: [NssSource; 4],
    pub(crate) nsswitch_count: usize,
}

impl Resolver {
    pub const fn new() -> Self {
        Self {
            config: ResolverConfig::new(),
            hosts: [HostEntry::empty(); 32],
            hosts_count: 0,
            nsswitch_hosts: [
                NssSource::Files,
                NssSource::Dns,
                NssSource::Unknown,
                NssSource::Unknown,
            ],
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
                    entry.name[..entry.name_len]
                        .copy_from_slice(&hostname.as_bytes()[..entry.name_len]);

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

                if let Ok(fqdn) =
                    core::str::from_utf8(&fqdn_buf[..hostname_bytes.len() + 1 + domain_len])
                {
                    if let Some(ip) = self.query_dns_with_retry(fqdn, nameserver_ip, 0) {
                        return Some(ip);
                    }
                }
            }
        }

        None
    }

    /// Perform DNS query with retry logic
    fn query_dns_with_retry(
        &self,
        hostname: &str,
        nameserver_ip: [u8; 4],
        depth: u8,
    ) -> Option<[u8; 4]> {
        let max_attempts = self.config.attempts.max(1).min(MAX_DNS_RETRIES);
        for _attempt in 0..max_attempts {
            if let Some(ip) = self.query_dns_internal(hostname, nameserver_ip, depth) {
                return Some(ip);
            }
        }
        None
    }

    fn query_dns_internal(
        &self,
        hostname: &str,
        nameserver_ip: [u8; 4],
        depth: u8,
    ) -> Option<[u8; 4]> {
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
        let hostname_hash = hostname
            .bytes()
            .fold(0u16, |acc, b| acc.wrapping_add(b as u16).rotate_left(3));
        let query_id = base_id
            .wrapping_add(hostname_hash)
            .wrapping_add((depth as u16) << 8);
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
        let outcome =
            match parse_dns_response(response_data, query_id, &mut cname_buf, &mut scratch_buf) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
