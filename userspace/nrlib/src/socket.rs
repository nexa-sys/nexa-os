/// Socket API for NexaOS (POSIX-compatible)
/// Provides standard socket functions for networking

use core::arch::asm;

// Socket system call numbers (must match kernel)
const SYS_SOCKET: usize = 41;
const SYS_BIND: usize = 49;
const SYS_SENDTO: usize = 44;
const SYS_RECVFROM: usize = 45;
const SYS_CONNECT: usize = 42;
const SYS_SETSOCKOPT: usize = 54;
const SYS_GETSOCKNAME: usize = 51;
const SYS_GETPEERNAME: usize = 52;

// Socket domain constants (POSIX)
pub const AF_INET: i32 = 2;       // IPv4
pub const AF_INET6: i32 = 10;     // IPv6
pub const AF_NETLINK: i32 = 16;   // Netlink
pub const AF_UNSPEC: i32 = 0;     // Unspecified

// Socket type constants (POSIX)
pub const SOCK_STREAM: i32 = 1;   // TCP
pub const SOCK_DGRAM: i32 = 2;    // UDP
pub const SOCK_RAW: i32 = 3;      // Raw sockets

// Socket protocol constants (POSIX)
pub const IPPROTO_IP: i32 = 0;    // Dummy protocol for TCP
pub const IPPROTO_ICMP: i32 = 1;  // ICMP
pub const IPPROTO_TCP: i32 = 6;   // TCP
pub const IPPROTO_UDP: i32 = 17;  // UDP

/// Netlink socket address
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SockAddrNl {
    pub nl_family: u16,     // AF_NETLINK
    pub nl_pad: u16,        // Zero
    pub nl_pid: u32,        // Port ID
    pub nl_groups: u32,     // Multicast groups mask
}

impl SockAddrNl {
    pub fn new(pid: u32, groups: u32) -> Self {
        Self {
            nl_family: AF_NETLINK as u16,
            nl_pad: 0,
            nl_pid: pid,
            nl_groups: groups,
        }
    }
}

impl From<SockAddrNl> for SockAddr {
    fn from(addr: SockAddrNl) -> Self {
        let mut sa_data = [0u8; 14];
        // nl_pid (4 bytes)
        sa_data[0..4].copy_from_slice(&addr.nl_pid.to_ne_bytes());
        // nl_groups (4 bytes)
        sa_data[4..8].copy_from_slice(&addr.nl_groups.to_ne_bytes());
        // Rest is zero padding
        Self {
            sa_family: addr.nl_family,
            sa_data,
        }
    }
}

/// sockaddr_in structure (POSIX-compatible)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SockAddrIn {
    pub sin_family: u16,    // AF_INET
    pub sin_port: u16,      // Port number (network byte order)
    pub sin_addr: u32,      // IPv4 address (network byte order)
    pub sin_zero: [u8; 8],  // Padding to match sockaddr size
}

impl SockAddrIn {
    /// Create a new sockaddr_in for IPv4
    pub fn new(ip: [u8; 4], port: u16) -> Self {
        Self {
            sin_family: AF_INET as u16,
            sin_port: port.to_be(),  // Host to network byte order
            sin_addr: u32::from_be_bytes(ip),
            sin_zero: [0; 8],
        }
    }

    /// Get the IP address as bytes
    pub fn ip(&self) -> [u8; 4] {
        self.sin_addr.to_be_bytes()
    }

    /// Get the port number in host byte order
    pub fn port(&self) -> u16 {
        u16::from_be(self.sin_port)
    }
}

/// Generic sockaddr structure (POSIX-compatible)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockAddr {
    pub sa_family: u16,     // Address family
    pub sa_data: [u8; 14],  // Address data
}

impl From<SockAddrIn> for SockAddr {
    fn from(addr: SockAddrIn) -> Self {
        let mut sa_data = [0u8; 14];
        // Port (2 bytes)
        sa_data[0..2].copy_from_slice(&addr.sin_port.to_be_bytes());
        // IPv4 address (4 bytes)
        sa_data[2..6].copy_from_slice(&addr.sin_addr.to_be_bytes());
        // Rest is zero padding (matches sin_zero)
        Self {
            sa_family: addr.sin_family,
            sa_data,
        }
    }
}

/// Create a socket
/// 
/// # Arguments
/// * `domain` - Protocol family (AF_INET, AF_INET6)
/// * `type_` - Socket type (SOCK_STREAM, SOCK_DGRAM)
/// * `protocol` - Protocol number (0 for default)
/// 
/// # Returns
/// Socket file descriptor on success, -1 on error
#[no_mangle]
pub extern "C" fn socket(domain: i32, type_: i32, protocol: i32) -> i32 {
    let result: i64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") SYS_SOCKET => result,
            in("rdi") domain as u64,
            in("rsi") type_ as u64,
            in("rdx") protocol as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    if result == u64::MAX as i64 {
        -1
    } else {
        result as i32
    }
}

/// Bind socket to local address
/// 
/// # Arguments
/// * `sockfd` - Socket file descriptor
/// * `addr` - Local address to bind to
/// * `addrlen` - Size of address structure
/// 
/// # Returns
/// 0 on success, -1 on error
#[no_mangle]
pub extern "C" fn bind(sockfd: i32, addr: *const SockAddr, addrlen: u32) -> i32 {
    let result: i64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") SYS_BIND => result,
            in("rdi") sockfd as u64,
            in("rsi") addr as u64,
            in("rdx") addrlen as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    if result == u64::MAX as i64 {
        -1
    } else {
        result as i32
    }
}

/// Send datagram to specified address
/// 
/// # Arguments
/// * `sockfd` - Socket file descriptor
/// * `buf` - Buffer containing data to send
/// * `len` - Length of data
/// * `flags` - Send flags (usually 0)
/// * `dest_addr` - Destination address
/// * `addrlen` - Size of destination address
/// 
/// # Returns
/// Number of bytes sent on success, -1 on error
#[no_mangle]
pub extern "C" fn sendto(
    sockfd: i32,
    buf: *const u8,
    len: usize,
    flags: i32,
    dest_addr: *const SockAddr,
    addrlen: u32,
) -> isize {
    let result: i64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") SYS_SENDTO => result,
            in("rdi") sockfd as u64,
            in("rsi") buf as u64,
            in("rdx") len as u64,
            in("r10") flags as u64,
            in("r8") dest_addr as u64,
            in("r9") addrlen as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    if result == u64::MAX as i64 {
        -1
    } else {
        result as isize
    }
}

/// Receive datagram and source address
/// 
/// # Arguments
/// * `sockfd` - Socket file descriptor
/// * `buf` - Buffer to receive data
/// * `len` - Size of buffer
/// * `flags` - Receive flags (usually 0)
/// * `src_addr` - Buffer to receive source address (may be null)
/// * `addrlen` - Pointer to size of address buffer (may be null)
/// 
/// # Returns
/// Number of bytes received on success, -1 on error
#[no_mangle]
pub extern "C" fn recvfrom(
    sockfd: i32,
    buf: *mut u8,
    len: usize,
    flags: i32,
    src_addr: *mut SockAddr,
    addrlen: *mut u32,
) -> isize {
    let result: i64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") SYS_RECVFROM => result,
            in("rdi") sockfd as u64,
            in("rsi") buf as u64,
            in("rdx") len as u64,
            in("r10") flags as u64,
            in("r8") src_addr as u64,
            in("r9") addrlen as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    if result == u64::MAX as i64 {
        -1
    } else {
        result as isize
    }
}

/// Connect socket to remote address
/// 
/// # Arguments
/// * `sockfd` - Socket file descriptor
/// * `addr` - Remote address to connect to
/// * `addrlen` - Size of address structure
/// 
/// # Returns
/// 0 on success, -1 on error
#[no_mangle]
pub extern "C" fn connect(sockfd: i32, addr: *const SockAddr, addrlen: u32) -> i32 {
    let result: i64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") SYS_CONNECT => result,
            in("rdi") sockfd as u64,
            in("rsi") addr as u64,
            in("rdx") addrlen as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    if result == u64::MAX as i64 {
        -1
    } else {
        result as i32
    }
}

/// Helper: Convert IPv4 address string to binary
/// Format: "192.168.1.1" -> [192, 168, 1, 1]
pub fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut idx = 0;
    let mut current = 0u16;
    
    for ch in s.chars() {
        if ch == '.' {
            if idx >= 4 || current > 255 {
                return None;
            }
            octets[idx] = current as u8;
            idx += 1;
            current = 0;
        } else if ch.is_ascii_digit() {
            current = current * 10 + (ch as u16 - '0' as u16);
            if current > 255 {
                return None;
            }
        } else {
            return None;
        }
    }
    
    if idx != 3 || current > 255 {
        return None;
    }
    octets[3] = current as u8;
    Some(octets)
}

/// Helper: Convert binary IPv4 address to string
/// Format: [192, 168, 1, 1] -> "192.168.1.1"
pub fn format_ipv4(ip: [u8; 4]) -> [u8; 16] {
    let mut buf = [0u8; 16];
    let mut pos = 0;
    
    for (i, octet) in ip.iter().enumerate() {
        // Convert u8 to decimal string
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
                buf[pos] = digits[digit_count - 1 - j];
                pos += 1;
            }
        }
        
        if i < 3 {
            buf[pos] = b'.';
            pos += 1;
        }
    }
    
    buf
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
        assert_eq!(parse_ipv4("256.1.1.1"), None);
        assert_eq!(parse_ipv4("1.2.3"), None);
        assert_eq!(parse_ipv4("1.2.3.4.5"), None);
    }

    #[test]
    fn test_sockaddr_in() {
        let addr = SockAddrIn::new([192, 168, 1, 1], 8080);
        assert_eq!(addr.sin_family, AF_INET as u16);
        assert_eq!(addr.ip(), [192, 168, 1, 1]);
        assert_eq!(addr.port(), 8080);
    }
}
