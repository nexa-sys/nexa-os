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

/// Socket type constants (POSIX)
pub const SOCK_STREAM: i32 = 1;   // TCP
pub const SOCK_DGRAM: i32 = 2;    // UDP
pub const SOCK_RAW: i32 = 3;      // Raw sockets

// Socket type flags (Linux-specific)
pub const SOCK_NONBLOCK: i32 = 0x800;      // Non-blocking mode
pub const SOCK_CLOEXEC: i32 = 0x80000;     // Close-on-exec flag
const SOCK_TYPE_MASK: i32 = 0xf;           // Mask to extract base type

// Message flag constants used by libc send/recv wrappers
const MSG_NOSIGNAL: i32 = 0x4000;

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
    /// ip: IPv4 address as [a, b, c, d] for a.b.c.d
    /// port: Port number in HOST byte order
    pub fn new(ip: [u8; 4], port: u16) -> Self {
        // Store port in network byte order by converting to bytes then back to u16
        // This ensures the bytes are in the right order for the wire format
        let port_bytes = port.to_be_bytes();
        let port_ne = u16::from_ne_bytes(port_bytes);
        
        Self {
            sin_family: AF_INET as u16,
            sin_port: port_ne,
            sin_addr: u32::from_ne_bytes(ip),
            sin_zero: [0; 8],
        }
    }

    /// Get the IP address as bytes in network order
    pub fn ip(&self) -> [u8; 4] {
        self.sin_addr.to_ne_bytes()
    }

    /// Get the port number in host byte order
    pub fn port(&self) -> u16 {
        let port_bytes = self.sin_port.to_ne_bytes();
        u16::from_be_bytes(port_bytes)
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
        // Port: extract as native endian bytes (which are in network byte order)
        sa_data[0..2].copy_from_slice(&addr.sin_port.to_ne_bytes());
        // IPv4 address: extract as native endian bytes
        sa_data[2..6].copy_from_slice(&addr.sin_addr.to_ne_bytes());
        // Rest is zero padding
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
/// Socket file descriptor on success, -1 on error (errno set)
#[no_mangle]
pub extern "C" fn socket(domain: i32, type_: i32, protocol: i32) -> i32 {
    // Strip flags and pass only base type to kernel
    // Linux socket() accepts flags in type parameter, but NexaOS kernel expects clean type
    let base_type = type_ & SOCK_TYPE_MASK;
    
    let ret = crate::syscall3(
        SYS_SOCKET as u64,
        domain as u64,
        base_type as u64,  // Pass stripped type to kernel
        protocol as u64,
    );
    
    crate::translate_ret_i32(ret)
}

/// Bind socket to local address
/// 
/// # Arguments
/// * `sockfd` - Socket file descriptor
/// * `addr` - Local address to bind to
/// * `addrlen` - Size of address structure
/// 
/// # Returns
/// 0 on success, -1 on error (errno set)
#[no_mangle]
pub extern "C" fn bind(sockfd: i32, addr: *const SockAddr, addrlen: u32) -> i32 {
    let ret = crate::syscall3(
        SYS_BIND as u64,
        sockfd as u64,
        addr as u64,
        addrlen as u64,
    );
    crate::translate_ret_i32(ret)
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
    if result == -1 {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
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
    if result == -1 {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
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
/// 0 on success, -1 on error (errno set)
#[no_mangle]
pub extern "C" fn connect(sockfd: i32, addr: *const SockAddr, addrlen: u32) -> i32 {
    let ret = crate::syscall3(
        SYS_CONNECT as u64,
        sockfd as u64,
        addr as u64,
        addrlen as u64,
    );
    crate::translate_ret_i32(ret)
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

/// Send data on a connected socket
/// 
/// # Arguments
/// * `sockfd` - Socket file descriptor
/// * `buf` - Buffer containing data to send
/// * `len` - Length of data
/// * `flags` - Send flags (usually 0)
/// 
/// # Returns
/// Number of bytes sent on success, -1 on error
#[no_mangle]
pub extern "C" fn send(sockfd: i32, buf: *const u8, len: usize, flags: i32) -> isize {
    if buf.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    if len == 0 {
        return 0;
    }

    // Linux libc uses MSG_NOSIGNAL for TCP streams; treat it as advisory
    let remaining_flags = flags & !MSG_NOSIGNAL;
    if remaining_flags == 0 {
        return crate::write(sockfd, buf as *const crate::c_void, len);
    }

    // Fallback to kernel sendto path when unsupported flags are requested
    sendto(sockfd, buf, len, flags, core::ptr::null(), 0)
}

/// Receive data from a connected socket
/// 
/// # Arguments
/// * `sockfd` - Socket file descriptor
/// * `buf` - Buffer to receive data
/// * `len` - Maximum length to receive
/// * `flags` - Receive flags (usually 0)
/// 
/// # Returns
/// Number of bytes received on success, -1 on error
#[no_mangle]
pub extern "C" fn recv(sockfd: i32, buf: *mut u8, len: usize, flags: i32) -> isize {
    if buf.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    if len == 0 {
        return 0;
    }

    // For blocking TCP streams, libc passes MSG_NOSIGNAL-equivalent flags (which we ignore)
    if flags == 0 {
        return crate::read(sockfd, buf as *mut crate::c_void, len);
    }

    // Fallback to kernel recvfrom path when flags demand special handling
    recvfrom(
        sockfd,
        buf,
        len,
        flags,
        core::ptr::null_mut(),
        core::ptr::null_mut(),
    )
}

/// Get socket options
/// 
/// # Arguments
/// * `sockfd` - Socket file descriptor
/// * `level` - Protocol level (SOL_SOCKET, IPPROTO_TCP, etc.)
/// * `optname` - Option name (SO_ERROR, SO_REUSEADDR, etc.)
/// * `optval` - Pointer to buffer to receive option value
/// * `optlen` - Pointer to size of buffer (in/out parameter)
/// 
/// # Returns
/// 0 on success, -1 on error
#[no_mangle]
pub extern "C" fn getsockopt(
    sockfd: i32,
    level: i32,
    optname: i32,
    optval: *mut u8,
    optlen: *mut u32,
) -> i32 {
    // For now, we'll implement a minimal version that handles common cases
    // In a full implementation, this would make a syscall to the kernel
    
    const SOL_SOCKET: i32 = 1;
    const SO_ERROR: i32 = 4;
    const SO_TYPE: i32 = 3;
    
    unsafe {
        if optval.is_null() || optlen.is_null() {
            return -1;
        }
        
        match (level, optname) {
            (SOL_SOCKET, SO_ERROR) => {
                // Return 0 (no error) for now
                if *optlen >= 4 {
                    *(optval as *mut i32) = 0;
                    *optlen = 4;
                    0
                } else {
                    -1
                }
            }
            (SOL_SOCKET, SO_TYPE) => {
                // Return socket type - we don't track this yet, so return DGRAM
                if *optlen >= 4 {
                    *(optval as *mut i32) = SOCK_DGRAM;
                    *optlen = 4;
                    0
                } else {
                    -1
                }
            }
            _ => {
                // Unsupported option
                -1
            }
        }
    }
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
