//! OpenSSL ERR_* Compatibility Functions
//!
//! Error queue management compatible with OpenSSL error handling.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::vec::Vec;

/// Thread-local error queue (simplified with global)
static ERROR_QUEUE: Mutex<VecDeque<ErrorEntry>> = Mutex::new(VecDeque::new());

/// Error entry
#[derive(Clone)]
struct ErrorEntry {
    /// Packed error code
    code: u64,
    /// File name
    file: &'static str,
    /// Line number
    line: u32,
    /// Extra data string
    data: Option<String>,
}

/// Library codes
pub mod lib_code {
    pub const ERR_LIB_NONE: i32 = 1;
    pub const ERR_LIB_SYS: i32 = 2;
    pub const ERR_LIB_BN: i32 = 3;
    pub const ERR_LIB_RSA: i32 = 4;
    pub const ERR_LIB_DH: i32 = 5;
    pub const ERR_LIB_EVP: i32 = 6;
    pub const ERR_LIB_BUF: i32 = 7;
    pub const ERR_LIB_OBJ: i32 = 8;
    pub const ERR_LIB_PEM: i32 = 9;
    pub const ERR_LIB_DSA: i32 = 10;
    pub const ERR_LIB_X509: i32 = 11;
    pub const ERR_LIB_ASN1: i32 = 13;
    pub const ERR_LIB_CONF: i32 = 14;
    pub const ERR_LIB_CRYPTO: i32 = 15;
    pub const ERR_LIB_EC: i32 = 16;
    pub const ERR_LIB_SSL: i32 = 20;
    pub const ERR_LIB_BIO: i32 = 32;
    pub const ERR_LIB_RAND: i32 = 36;
    pub const ERR_LIB_ENGINE: i32 = 38;
    pub const ERR_LIB_OCSP: i32 = 39;
    pub const ERR_LIB_UI: i32 = 40;
    pub const ERR_LIB_FIPS: i32 = 41;
    pub const ERR_LIB_CMS: i32 = 46;
    pub const ERR_LIB_CT: i32 = 50;
    pub const ERR_LIB_ASYNC: i32 = 51;
    pub const ERR_LIB_USER: i32 = 128;
}

/// Pack error into OpenSSL-compatible format
fn pack_error(lib: i32, reason: i32) -> u64 {
    ((lib as u64) << 24) | (reason as u64 & 0xFFFFFF)
}

/// Unpack error code
pub fn unpack_error(code: u64) -> (i32, i32) {
    let lib = ((code >> 24) & 0xFF) as i32;
    let reason = (code & 0xFFFFFF) as i32;
    (lib, reason)
}

/// Push error to queue
pub fn push_error(lib: i32, reason: i32, file: &'static str, line: u32) {
    let entry = ErrorEntry {
        code: pack_error(lib, reason),
        file,
        line,
        data: None,
    };

    if let Ok(mut queue) = ERROR_QUEUE.lock() {
        queue.push_back(entry);
    }
}

/// Push error with data
pub fn push_error_data(lib: i32, reason: i32, file: &'static str, line: u32, data: &str) {
    let entry = ErrorEntry {
        code: pack_error(lib, reason),
        file,
        line,
        data: Some(data.to_string()),
    };

    if let Ok(mut queue) = ERROR_QUEUE.lock() {
        queue.push_back(entry);
    }
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// ERR_get_error - Get and remove first error
#[no_mangle]
pub extern "C" fn ERR_get_error() -> u64 {
    if let Ok(mut queue) = ERROR_QUEUE.lock() {
        queue.pop_front().map(|e| e.code).unwrap_or(0)
    } else {
        0
    }
}

/// ERR_peek_error - Peek at first error
#[no_mangle]
pub extern "C" fn ERR_peek_error() -> u64 {
    if let Ok(queue) = ERROR_QUEUE.lock() {
        queue.front().map(|e| e.code).unwrap_or(0)
    } else {
        0
    }
}

/// ERR_peek_last_error - Peek at last error
#[no_mangle]
pub extern "C" fn ERR_peek_last_error() -> u64 {
    if let Ok(queue) = ERROR_QUEUE.lock() {
        queue.back().map(|e| e.code).unwrap_or(0)
    } else {
        0
    }
}

/// ERR_clear_error - Clear error queue
#[no_mangle]
pub extern "C" fn ERR_clear_error() {
    if let Ok(mut queue) = ERROR_QUEUE.lock() {
        queue.clear();
    }
}

/// ERR_get_error_line - Get error with file/line
#[no_mangle]
pub extern "C" fn ERR_get_error_line(file: *mut *const i8, line: *mut i32) -> u64 {
    if let Ok(mut queue) = ERROR_QUEUE.lock() {
        if let Some(entry) = queue.pop_front() {
            if !file.is_null() {
                unsafe {
                    *file = entry.file.as_ptr() as *const i8;
                }
            }
            if !line.is_null() {
                unsafe {
                    *line = entry.line as i32;
                }
            }
            return entry.code;
        }
    }
    0
}

/// ERR_get_error_line_data - Get error with file/line/data
#[no_mangle]
pub extern "C" fn ERR_get_error_line_data(
    file: *mut *const i8,
    line: *mut i32,
    data: *mut *const i8,
    flags: *mut i32,
) -> u64 {
    let code = ERR_get_error_line(file, line);
    if !data.is_null() {
        unsafe {
            *data = core::ptr::null();
        }
    }
    if !flags.is_null() {
        unsafe {
            *flags = 0;
        }
    }
    code
}

/// ERR_peek_error_line - Peek error with file/line
#[no_mangle]
pub extern "C" fn ERR_peek_error_line(file: *mut *const i8, line: *mut i32) -> u64 {
    if let Ok(queue) = ERROR_QUEUE.lock() {
        if let Some(entry) = queue.front() {
            if !file.is_null() {
                unsafe {
                    *file = entry.file.as_ptr() as *const i8;
                }
            }
            if !line.is_null() {
                unsafe {
                    *line = entry.line as i32;
                }
            }
            return entry.code;
        }
    }
    0
}

/// ERR_error_string - Get error string
#[no_mangle]
pub extern "C" fn ERR_error_string(e: u64, buf: *mut i8) -> *mut i8 {
    let (lib, reason) = unpack_error(e);

    let lib_name = match lib {
        lib_code::ERR_LIB_SSL => "SSL",
        lib_code::ERR_LIB_RSA => "RSA",
        lib_code::ERR_LIB_EVP => "EVP",
        lib_code::ERR_LIB_BIO => "BIO",
        lib_code::ERR_LIB_X509 => "X509",
        lib_code::ERR_LIB_PEM => "PEM",
        lib_code::ERR_LIB_CRYPTO => "CRYPTO",
        lib_code::ERR_LIB_EC => "EC",
        lib_code::ERR_LIB_RAND => "RAND",
        _ => "lib",
    };

    // Format: "error:XXXXXXXX:lib:func:reason"
    let msg = format!("error:{:08X}:{}:func:reason({})\0", e, lib_name, reason);

    if buf.is_null() {
        // Use static buffer (simplified)
        static BUFFER: Mutex<[u8; 256]> = Mutex::new([0u8; 256]);
        if let Ok(mut buffer) = BUFFER.lock() {
            let len = msg.len().min(255);
            buffer[..len].copy_from_slice(&msg.as_bytes()[..len]);
            return buffer.as_ptr() as *mut i8;
        }
        return core::ptr::null_mut();
    }

    unsafe {
        let len = msg.len().min(255);
        core::ptr::copy_nonoverlapping(msg.as_ptr(), buf as *mut u8, len);
        *(buf as *mut u8).add(len) = 0;
    }

    buf
}

/// ERR_error_string_n - Get error string with length
#[no_mangle]
pub extern "C" fn ERR_error_string_n(e: u64, buf: *mut i8, len: usize) {
    if buf.is_null() || len == 0 {
        return;
    }

    let (lib, reason) = unpack_error(e);
    let msg = format!("error:{:08X}:lib({}):func:reason({})", e, lib, reason);

    let copy_len = msg.len().min(len - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(msg.as_ptr(), buf as *mut u8, copy_len);
        *(buf as *mut u8).add(copy_len) = 0;
    }
}

/// ERR_lib_error_string - Get library name
#[no_mangle]
pub extern "C" fn ERR_lib_error_string(e: u64) -> *const i8 {
    let (lib, _) = unpack_error(e);

    match lib {
        lib_code::ERR_LIB_SSL => b"SSL routines\0".as_ptr() as *const i8,
        lib_code::ERR_LIB_RSA => b"RSA routines\0".as_ptr() as *const i8,
        lib_code::ERR_LIB_EVP => b"digital envelope routines\0".as_ptr() as *const i8,
        lib_code::ERR_LIB_BIO => b"BIO routines\0".as_ptr() as *const i8,
        lib_code::ERR_LIB_X509 => b"x509 certificate routines\0".as_ptr() as *const i8,
        lib_code::ERR_LIB_PEM => b"PEM routines\0".as_ptr() as *const i8,
        lib_code::ERR_LIB_CRYPTO => b"crypto library\0".as_ptr() as *const i8,
        lib_code::ERR_LIB_EC => b"EC routines\0".as_ptr() as *const i8,
        lib_code::ERR_LIB_RAND => b"random number generator\0".as_ptr() as *const i8,
        _ => b"unknown library\0".as_ptr() as *const i8,
    }
}

/// ERR_reason_error_string - Get reason string
#[no_mangle]
pub extern "C" fn ERR_reason_error_string(e: u64) -> *const i8 {
    let (_, reason) = unpack_error(e);

    // Common reason codes
    match reason {
        0 => b"no error\0".as_ptr() as *const i8,
        100 => b"malloc failure\0".as_ptr() as *const i8,
        101 => b"passed null parameter\0".as_ptr() as *const i8,
        102 => b"internal error\0".as_ptr() as *const i8,
        _ => b"unknown error\0".as_ptr() as *const i8,
    }
}

/// ERR_print_errors_fp - Print errors to file
#[no_mangle]
pub extern "C" fn ERR_print_errors_fp(_fp: *mut core::ffi::c_void) {
    // Print all errors (simplified: just clear queue)
    ERR_clear_error();
}

/// ERR_print_errors - Print errors to BIO
#[no_mangle]
pub extern "C" fn ERR_print_errors(_bio: *mut core::ffi::c_void) {
    ERR_clear_error();
}

/// ERR_put_error - Put error (deprecated)
#[no_mangle]
pub extern "C" fn ERR_put_error(lib: i32, _func: i32, reason: i32, file: *const i8, line: i32) {
    let file_str = if file.is_null() {
        ""
    } else {
        // We need a static string, so use a placeholder
        "unknown"
    };
    push_error(lib, reason, file_str, line as u32);
}

/// ERR_set_error - Set error (OpenSSL 3.0+)
#[no_mangle]
pub extern "C" fn ERR_set_error(
    lib: i32,
    reason: i32,
    _fmt: *const i8,
    // ... varargs not supported
) {
    push_error(lib, reason, "unknown", 0);
}

/// ERR_load_crypto_strings - Load error strings (no-op)
#[no_mangle]
pub extern "C" fn ERR_load_crypto_strings() {
    // No-op: strings always available
}

/// ERR_free_strings - Free error strings (no-op)
#[no_mangle]
pub extern "C" fn ERR_free_strings() {
    // No-op
}

/// ERR_remove_state - Remove thread state (deprecated)
#[no_mangle]
pub extern "C" fn ERR_remove_state(_pid: u64) {
    ERR_clear_error();
}

/// ERR_remove_thread_state - Remove thread state
#[no_mangle]
pub extern "C" fn ERR_remove_thread_state(_tid: *const core::ffi::c_void) {
    ERR_clear_error();
}

/// OPENSSL_cleanse - Secure memory wipe
#[no_mangle]
pub extern "C" fn OPENSSL_cleanse(ptr: *mut core::ffi::c_void, len: usize) {
    if ptr.is_null() {
        return;
    }
    crate::constant_time::secure_zero(unsafe {
        core::slice::from_raw_parts_mut(ptr as *mut u8, len)
    });
}

/// OPENSSL_malloc - Allocate memory
#[no_mangle]
pub extern "C" fn OPENSSL_malloc(size: usize) -> *mut core::ffi::c_void {
    if size == 0 {
        return core::ptr::null_mut();
    }
    let layout = std::alloc::Layout::from_size_align(size, 8).unwrap();
    unsafe { std::alloc::alloc(layout) as *mut core::ffi::c_void }
}

/// OPENSSL_free - Free memory
#[no_mangle]
pub extern "C" fn OPENSSL_free(ptr: *mut core::ffi::c_void) {
    // Note: This is simplified and may not work correctly for all allocations
    // In practice, Rust's memory management handles this differently
}

/// OPENSSL_realloc - Reallocate memory
#[no_mangle]
pub extern "C" fn OPENSSL_realloc(
    ptr: *mut core::ffi::c_void,
    size: usize,
) -> *mut core::ffi::c_void {
    if ptr.is_null() {
        return OPENSSL_malloc(size);
    }
    if size == 0 {
        OPENSSL_free(ptr);
        return core::ptr::null_mut();
    }
    // Simplified: allocate new, return (no copy)
    OPENSSL_malloc(size)
}

/// OPENSSL_hexchar2int - Convert hex char to int
#[no_mangle]
pub extern "C" fn OPENSSL_hexchar2int(c: u8) -> i32 {
    match c {
        b'0'..=b'9' => (c - b'0') as i32,
        b'a'..=b'f' => (c - b'a' + 10) as i32,
        b'A'..=b'F' => (c - b'A' + 10) as i32,
        _ => -1,
    }
}

/// OPENSSL_hexstr2buf - Convert hex string to buffer
#[no_mangle]
pub extern "C" fn OPENSSL_hexstr2buf(str: *const i8, len: *mut i64) -> *mut u8 {
    if str.is_null() {
        return core::ptr::null_mut();
    }

    let hex_str = unsafe { core::ffi::CStr::from_ptr(str) };
    let hex_str = match hex_str.to_str() {
        Ok(s) => s,
        Err(_) => return core::ptr::null_mut(),
    };

    match crate::encoding::hex_decode(hex_str) {
        Ok(bytes) => {
            if !len.is_null() {
                unsafe {
                    *len = bytes.len() as i64;
                }
            }
            let boxed = bytes.into_boxed_slice();
            let ptr = Box::into_raw(boxed) as *mut u8;
            ptr
        }
        Err(_) => core::ptr::null_mut(),
    }
}

/// OPENSSL_buf2hexstr - Convert buffer to hex string
#[no_mangle]
pub extern "C" fn OPENSSL_buf2hexstr(buf: *const u8, len: i64) -> *mut i8 {
    if buf.is_null() || len < 0 {
        return core::ptr::null_mut();
    }

    let data = unsafe { core::slice::from_raw_parts(buf, len as usize) };
    let hex = crate::encoding::hex_encode(data).to_uppercase();

    // Add colons between bytes
    let mut result = String::with_capacity(hex.len() * 3 / 2);
    for (i, c) in hex.chars().enumerate() {
        if i > 0 && i % 2 == 0 {
            result.push(':');
        }
        result.push(c);
    }
    result.push('\0');

    let boxed = result.into_bytes().into_boxed_slice();
    Box::into_raw(boxed) as *mut i8
}
