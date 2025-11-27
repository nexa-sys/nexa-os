//! Network-related functions
//!
//! Provides byte order conversion, inet_*, setsockopt, and related functions.

use crate::{c_char, c_int, c_uint, c_void};
use core::{arch::asm, ptr};

// ============================================================================
// Byte Order Conversion Functions
// ============================================================================

/// Convert 16-bit host byte order to network byte order (big-endian)
#[no_mangle]
pub extern "C" fn htons(hostshort: u16) -> u16 {
    hostshort.to_be()
}

/// Convert 16-bit network byte order (big-endian) to host byte order
#[no_mangle]
pub extern "C" fn ntohs(netshort: u16) -> u16 {
    u16::from_be(netshort)
}

/// Convert 32-bit host byte order to network byte order (big-endian)
#[no_mangle]
pub extern "C" fn htonl(hostlong: u32) -> u32 {
    hostlong.to_be()
}

/// Convert 32-bit network byte order (big-endian) to host byte order
#[no_mangle]
pub extern "C" fn ntohl(netlong: u32) -> u32 {
    u32::from_be(netlong)
}

// ============================================================================
// inet_* Functions
// ============================================================================

/// Convert IPv4 dotted-decimal string to binary network byte order
/// Returns 1 on success, 0 on error
#[no_mangle]
pub unsafe extern "C" fn inet_aton(cp: *const c_char, inp: *mut u32) -> c_int {
    if cp.is_null() || inp.is_null() {
        return 0;
    }

    let mut octets = [0u8; 4];
    let mut idx = 0;
    let mut current = 0u16;
    let mut has_digit = false;

    let mut ptr = cp;
    loop {
        let ch = *ptr as u8;
        if ch == 0 {
            break;
        }

        if ch == b'.' {
            if !has_digit || idx >= 4 || current > 255 {
                return 0;
            }
            octets[idx] = current as u8;
            idx += 1;
            current = 0;
            has_digit = false;
        } else if ch >= b'0' && ch <= b'9' {
            current = current * 10 + (ch - b'0') as u16;
            if current > 255 {
                return 0;
            }
            has_digit = true;
        } else {
            return 0;
        }

        ptr = ptr.add(1);
    }

    if !has_digit || idx != 3 || current > 255 {
        return 0;
    }
    octets[3] = current as u8;

    *inp = u32::from_be_bytes(octets);
    1
}

/// Convert IPv4 address from binary to dotted-decimal string
/// Returns pointer to static buffer
#[no_mangle]
pub unsafe extern "C" fn inet_ntoa(inp: u32) -> *const c_char {
    static mut BUFFER: [u8; 16] = [0; 16];
    
    let octets = inp.to_be_bytes();
    let mut pos = 0;
    
    for (i, octet) in octets.iter().enumerate() {
        let mut n = *octet as usize;
        if n == 0 {
            BUFFER[pos] = b'0';
            pos += 1;
        } else {
            let mut digits = [0u8; 3];
            let mut digit_count = 0;
            
            while n > 0 && digit_count < 3 {
                digits[digit_count] = (n % 10) as u8 + b'0';
                n /= 10;
                digit_count += 1;
            }
            
            for j in (0..digit_count).rev() {
                BUFFER[pos] = digits[j];
                pos += 1;
            }
        }
        
        if i < 3 {
            BUFFER[pos] = b'.';
            pos += 1;
        }
    }
    
    BUFFER[pos] = 0; // Null terminator
    BUFFER.as_ptr() as *const c_char
}

/// Convert IPv4 address from presentation (string) to network format
/// Returns 1 on success, 0 on error, -1 on invalid family
#[no_mangle]
pub unsafe extern "C" fn inet_pton(af: c_int, src: *const c_char, dst: *mut c_void) -> c_int {
    if src.is_null() || dst.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    match af {
        2 => {  // AF_INET
            let result = inet_aton(src, dst as *mut u32);
            if result == 1 {
                1
            } else {
                0
            }
        }
        _ => {
            crate::set_errno(crate::ENOSYS);  // AF_INET6 not supported yet
            -1
        }
    }
}

/// Convert IPv4 address from network format to presentation (string)
/// Returns pointer to dst on success, NULL on error
#[no_mangle]
pub unsafe extern "C" fn inet_ntop(
    af: c_int,
    src: *const c_void,
    dst: *mut c_char,
    size: u32,
) -> *const c_char {
    if src.is_null() || dst.is_null() || size < 16 {
        crate::set_errno(crate::EINVAL);
        return ptr::null();
    }

    match af {
        2 => {  // AF_INET
            let addr = *(src as *const u32);
            let octets = addr.to_be_bytes();
            let mut pos = 0;
            
            for (i, octet) in octets.iter().enumerate() {
                let mut n = *octet as usize;
                if n == 0 {
                    if pos >= size as usize {
                        crate::set_errno(crate::ENOSPC);
                        return ptr::null();
                    }
                    *dst.add(pos) = b'0' as c_char;
                    pos += 1;
                } else {
                    let mut digits = [0u8; 3];
                    let mut digit_count = 0;
                    
                    while n > 0 && digit_count < 3 {
                        digits[digit_count] = (n % 10) as u8 + b'0';
                        n /= 10;
                        digit_count += 1;
                    }
                    
                    for j in (0..digit_count).rev() {
                        if pos >= size as usize {
                            crate::set_errno(crate::ENOSPC);
                            return ptr::null();
                        }
                        *dst.add(pos) = digits[j] as c_char;
                        pos += 1;
                    }
                }
                
                if i < 3 {
                    if pos >= size as usize {
                        crate::set_errno(crate::ENOSPC);
                        return ptr::null();
                    }
                    *dst.add(pos) = b'.' as c_char;
                    pos += 1;
                }
            }
            
            if pos >= size as usize {
                crate::set_errno(crate::ENOSPC);
                return ptr::null();
            }
            *dst.add(pos) = 0; // Null terminator
            dst
        }
        _ => {
            crate::set_errno(crate::ENOSYS);  // AF_INET6 not supported yet
            ptr::null()
        }
    }
}

// ============================================================================
// Socket Options
// ============================================================================

/// Set socket options
#[no_mangle]
pub unsafe extern "C" fn setsockopt(
    sockfd: c_int,
    level: c_int,
    optname: c_int,
    optval: *const c_void,
    optlen: c_uint,
) -> c_int {
    const SYS_SETSOCKOPT: usize = 54;
    let result: i64;
    asm!(
        "syscall",
        inlateout("rax") SYS_SETSOCKOPT => result,
        in("rdi") sockfd as u64,
        in("rsi") level as u64,
        in("rdx") optname as u64,
        in("r10") optval as u64,
        in("r8") optlen as u64,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack),
    );
    if result == -1 {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        result as i32
    }
}

// ============================================================================
// getaddrinfo Error Messages
// ============================================================================

/// Get error message for getaddrinfo errors
#[no_mangle]
pub unsafe extern "C" fn gai_strerror(ecode: c_int) -> *const c_char {
    let msg = match ecode {
        -1 => "Bad flags\0",
        -2 => "Name or service not known\0",
        -3 => "Temporary failure in name resolution\0",
        -4 => "Non-recoverable failure in name resolution\0",
        -6 => "Address family not supported\0",
        -7 => "Socket type not supported\0",
        -8 => "Service not available\0",
        -10 => "Out of memory\0",
        -11 => "System error\0",
        -12 => "Argument buffer overflow\0",
        0 => "Success\0",
        _ => "Unknown error\0",
    };
    msg.as_ptr() as *const c_char
}
