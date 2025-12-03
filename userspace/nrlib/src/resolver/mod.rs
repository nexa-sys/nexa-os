/// Resolver module - DNS and name resolution
///
/// Provides getaddrinfo/getnameinfo and /etc file parsing (/etc/hosts,
/// /etc/resolv.conf, /etc/nsswitch.conf).
///
/// ## Module Structure
///
/// - `constants`: AI/NI flags and EAI error codes
/// - `types`: AddrInfo, HostEntry, NssSource type definitions
/// - `dns_parser`: DNS response packet parsing
/// - `utils`: Helper functions for IP formatting and file I/O
/// - `core`: Main Resolver struct implementation
/// - `global`: Global resolver instance and initialization
/// - `posix`: POSIX-compatible getaddrinfo/getnameinfo functions

mod constants;
mod types;
mod dns_parser;
mod utils;
mod core;
mod global;
mod posix;

// Re-export public API

// Constants
pub use constants::{
    // AI flags
    AI_PASSIVE, AI_CANONNAME, AI_NUMERICHOST, AI_NUMERICSERV,
    AI_V4MAPPED, AI_ALL, AI_ADDRCONFIG,
    // NI flags
    NI_NUMERICHOST, NI_NUMERICSERV, NI_NOFQDN, NI_NAMEREQD, NI_DGRAM,
    // Error codes
    EAI_BADFLAGS, EAI_NONAME, EAI_AGAIN, EAI_FAIL, EAI_FAMILY,
    EAI_SOCKTYPE, EAI_SERVICE, EAI_MEMORY, EAI_SYSTEM, EAI_OVERFLOW,
    // Functions
    gai_strerror_str,
};

// Types
pub use types::{AddrInfo, HostEntry, NssSource};

// Core resolver
pub use self::core::Resolver;

// Global resolver
pub use global::{resolver_init, get_resolver};

// POSIX functions
pub use posix::{getaddrinfo, freeaddrinfo, getnameinfo};

// DNS parser (for advanced use cases)
pub use dns_parser::{parse_dns_response, DnsParseOutcome, dns_name_equals};

// Utilities
pub use utils::{format_ipv4_to_buffer, read_file_content};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::socket::parse_ipv4;

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
    fn test_gai_strerror() {
        assert_eq!(gai_strerror_str(0), "Success");
        assert_eq!(gai_strerror_str(EAI_NONAME), "Name or service not known");
        assert_eq!(gai_strerror_str(EAI_AGAIN), "Temporary failure in name resolution");
        assert_eq!(gai_strerror_str(-999), "Unknown error");
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
}
