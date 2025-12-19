//! Helper functions for the NexaOS dynamic linker

use crate::syscall::write;

// ============================================================================
// Printing Functions
// ============================================================================

pub unsafe fn print(msg: &[u8]) {
    write(2, msg.as_ptr(), msg.len());
}

pub unsafe fn print_str(s: &str) {
    print(s.as_bytes());
}

#[allow(dead_code)]
pub unsafe fn print_hex(val: u64) {
    let mut buf = [0u8; 18]; // "0x" + 16 hex digits
    buf[0] = b'0';
    buf[1] = b'x';
    let hex_chars = b"0123456789abcdef";
    for i in 0..16 {
        let nibble = ((val >> (60 - i * 4)) & 0xf) as usize;
        buf[2 + i] = hex_chars[nibble];
    }
    print(&buf);
}

/// Print a decimal number
#[allow(dead_code)]
pub unsafe fn print_num(val: u64) {
    if val == 0 {
        print(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = val;
    let mut i = 19;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i -= 1;
    }
    print(&buf[i + 1..20]);
}

// ============================================================================
// Memory Functions
// ============================================================================

/// Copy memory
pub unsafe fn memcpy(dest: *mut u8, src: *const u8, n: usize) {
    for i in 0..n {
        *dest.add(i) = *src.add(i);
    }
}

/// Zero memory
pub unsafe fn memset(dest: *mut u8, val: u8, n: usize) {
    for i in 0..n {
        *dest.add(i) = val;
    }
}

// ============================================================================
// String Functions
// ============================================================================

/// Get length of C string (not including null terminator)
pub unsafe fn cstr_len(s: *const u8) -> usize {
    let mut len = 0;
    while *s.add(len) != 0 {
        len += 1;
        if len > 256 {
            break;
        }
    }
    len
}

/// Compare two library names (handles libc.so -> libnrlib.so mapping)
pub unsafe fn is_same_library_name(a: *const u8, b: *const u8) -> bool {
    let mut i = 0;
    loop {
        let ca = *a.add(i);
        let cb = *b.add(i);
        if ca == 0 && cb == 0 {
            return true;
        }
        if ca != cb {
            return false;
        }
        i += 1;
        if i > 256 {
            return false;
        }
    }
}

/// Check if name starts with prefix
pub unsafe fn starts_with(name: *const u8, prefix: &[u8]) -> bool {
    for (i, &c) in prefix.iter().enumerate() {
        if c == 0 {
            return true;
        }
        if *name.add(i) != c {
            return false;
        }
    }
    true
}

// ============================================================================
// Library Name Mapping
// ============================================================================

/// Map library name to NexaOS equivalent
/// Returns the original name if no mapping exists
pub fn map_library_name(name: &[u8]) -> [u8; 64] {
    let mut result = [0u8; 64];

    // Default: copy original name
    for (i, &c) in name.iter().enumerate() {
        if i >= 63 || c == 0 {
            break;
        }
        result[i] = c;
    }

    result
}

// ============================================================================
// Page Alignment
// ============================================================================

use crate::constants::PAGE_SIZE;

/// Align down to page boundary
pub fn page_align_down(addr: u64) -> u64 {
    addr & !(PAGE_SIZE - 1)
}

/// Align up to page boundary
pub fn page_align_up(addr: u64) -> u64 {
    (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}
