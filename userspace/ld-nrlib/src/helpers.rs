//! Helper functions for the NexaOS dynamic linker

use crate::syscall::write;

// ============================================================================
// Printing Functions
// ============================================================================

#[inline(never)]
pub unsafe fn print(msg: &[u8]) {
    write(2, msg.as_ptr(), msg.len());
}

#[inline(never)]
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
// Memory Functions (internal use)
// ============================================================================

/// Copy memory (internal helper)
/// NOTE: We use volatile operations to prevent the compiler from "optimizing"
/// this into a call to memcpy (which would cause infinite recursion).
#[inline(never)]
pub unsafe fn memcpy_internal(dest: *mut u8, src: *const u8, n: usize) {
    let mut i = 0;
    while i < n {
        let byte = core::ptr::read_volatile(src.add(i));
        core::ptr::write_volatile(dest.add(i), byte);
        i += 1;
    }
}

/// Zero memory (internal helper)
/// NOTE: We use volatile writes to prevent the compiler from "optimizing"
/// this into a call to memset (which would cause infinite recursion).
#[inline(never)]
pub unsafe fn memset_internal(dest: *mut u8, val: u8, n: usize) {
    let mut i = 0;
    while i < n {
        core::ptr::write_volatile(dest.add(i), val);
        i += 1;
    }
}

// ============================================================================
// C ABI Memory Intrinsics (required by compiler/linker)
// These are exported as #[no_mangle] so the linker can find them
// ============================================================================

/// C ABI memcpy - required by compiler intrinsics
#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut core::ffi::c_void, src: *const core::ffi::c_void, n: usize) -> *mut core::ffi::c_void {
    memcpy_internal(dest as *mut u8, src as *const u8, n);
    dest
}

/// C ABI memset - required by compiler intrinsics
#[no_mangle]
pub unsafe extern "C" fn memset(dest: *mut core::ffi::c_void, val: i32, n: usize) -> *mut core::ffi::c_void {
    memset_internal(dest as *mut u8, val as u8, n);
    dest
}

/// C ABI memmove - handles overlapping regions
/// NOTE: We use volatile operations to prevent compiler optimization
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn memmove(dest: *mut core::ffi::c_void, src: *const core::ffi::c_void, n: usize) -> *mut core::ffi::c_void {
    let dest_ptr = dest as *mut u8;
    let src_ptr = src as *const u8;
    if (dest_ptr as usize) < (src_ptr as usize) {
        // Forward copy
        let mut i = 0;
        while i < n {
            let byte = core::ptr::read_volatile(src_ptr.add(i));
            core::ptr::write_volatile(dest_ptr.add(i), byte);
            i += 1;
        }
    } else {
        // Backward copy for overlapping regions
        let mut i = n;
        while i > 0 {
            i -= 1;
            let byte = core::ptr::read_volatile(src_ptr.add(i));
            core::ptr::write_volatile(dest_ptr.add(i), byte);
        }
    }
    dest
}

/// C ABI strlen
#[no_mangle]
pub unsafe extern "C" fn strlen(s: *const i8) -> usize {
    cstr_len(s as *const u8)
}

/// C ABI bcmp (BSD byte comparison, returns 0 if equal)
#[no_mangle]
pub unsafe extern "C" fn bcmp(a: *const core::ffi::c_void, b: *const core::ffi::c_void, n: usize) -> i32 {
    let a = a as *const u8;
    let b = b as *const u8;
    for i in 0..n {
        if *a.add(i) != *b.add(i) {
            return 1;
        }
    }
    0
}

/// C ABI memcmp
#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const core::ffi::c_void, b: *const core::ffi::c_void, n: usize) -> i32 {
    let a = a as *const u8;
    let b = b as *const u8;
    for i in 0..n {
        let va = *a.add(i);
        let vb = *b.add(i);
        if va != vb {
            return (va as i32) - (vb as i32);
        }
    }
    0
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
