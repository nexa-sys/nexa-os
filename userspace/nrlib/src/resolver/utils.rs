/// Resolver utility functions
///
/// Contains helper functions for IP formatting, file reading, etc.
use crate::os;
use core::ffi::CStr;

/// Format IPv4 address to string in buffer
/// Returns the length written (excluding null terminator), or 0 on overflow
#[inline]
pub fn format_ipv4_to_buffer(ip: [u8; 4], buf: *mut u8, buf_len: usize) -> usize {
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

/// Simple IP address formatting (without allocation)
#[allow(dead_code)]
pub fn format_ip_simple(ip: &[u8; 4], buf: &mut [u8; 16]) -> usize {
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

/// Helper to read file content into a temporary buffer
pub fn read_file_content(path: &str, buf: &mut [u8]) -> Option<usize> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
