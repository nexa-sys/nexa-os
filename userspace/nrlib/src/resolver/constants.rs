/// Resolver constants compatible with musl libc
///
/// Contains AI flags for getaddrinfo, NI flags for getnameinfo,
/// and EAI error codes.

// ============================================================================
// Internal constants
// ============================================================================

pub(crate) const MAX_HOSTNAME: usize = 256;
pub(crate) const DNS_PORT: u16 = 53;
pub(crate) const KERNEL_DNS_QUERY_CAP: usize = 3;
pub(crate) const MAX_CNAME_DEPTH: u8 = 8;
pub(crate) const MAX_DNS_RETRIES: u8 = 3;

// ============================================================================
// AI flags for getaddrinfo
// ============================================================================

/// Socket address will be used in bind(2)
pub const AI_PASSIVE: i32 = 0x01;
/// Request canonical name
pub const AI_CANONNAME: i32 = 0x02;
/// Don't use name resolution
pub const AI_NUMERICHOST: i32 = 0x04;
/// Service name is numeric
pub const AI_NUMERICSERV: i32 = 0x08;
/// IPv4-mapped addresses are acceptable
pub const AI_V4MAPPED: i32 = 0x10;
/// Return all addresses
pub const AI_ALL: i32 = 0x20;
/// Use configuration of this host
pub const AI_ADDRCONFIG: i32 = 0x40;

// ============================================================================
// NI flags for getnameinfo
// ============================================================================

/// Return numeric host address
pub const NI_NUMERICHOST: i32 = 0x01;
/// Return numeric port number
pub const NI_NUMERICSERV: i32 = 0x02;
/// Don't return fully qualified domain name
pub const NI_NOFQDN: i32 = 0x04;
/// Require host name
pub const NI_NAMEREQD: i32 = 0x08;
/// Datagram service (for UDP)
pub const NI_DGRAM: i32 = 0x10;

// ============================================================================
// Error codes (compatible with musl)
// ============================================================================

/// Invalid value for ai_flags
pub const EAI_BADFLAGS: i32 = -1;
/// Name or service is unknown
pub const EAI_NONAME: i32 = -2;
/// Temporary failure in name resolution
pub const EAI_AGAIN: i32 = -3;
/// Non-recoverable failure in name resolution
pub const EAI_FAIL: i32 = -4;
/// Address family not supported
pub const EAI_FAMILY: i32 = -6;
/// Socket type not supported
pub const EAI_SOCKTYPE: i32 = -7;
/// Service not supported for socket type
pub const EAI_SERVICE: i32 = -8;
/// Memory allocation failure
pub const EAI_MEMORY: i32 = -10;
/// System error
pub const EAI_SYSTEM: i32 = -11;
/// Buffer overflow
pub const EAI_OVERFLOW: i32 = -12;

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
