//! Additional OpenSSL C ABI FFI Exports
//!
//! This module provides additional OpenSSL libssl.so compatible C ABI
//! function exports that are NOT exported from lib.rs or compat.rs.
//!
//! Most SSL_* functions are already exported from lib.rs and compat.rs.
//! This module only contains:
//! - Legacy initialization functions (OpenSSL 1.0 compatibility)
//! - Error functions (from libcrypto)
//! - Version functions

use crate::{c_char, c_int, c_ulong, size_t};

// ============================================================================
// Legacy Library Initialization Functions (OpenSSL 1.0 compatibility)
// ============================================================================

/// Add all algorithms (OpenSSL 1.0 style)
///
/// This initializes all cryptographic algorithms.
/// In nssl, this calls SSL_library_init() internally.
#[no_mangle]
pub extern "C" fn OpenSSL_add_all_algorithms() {
    let _ = crate::SSL_library_init();
}

/// Add SSL algorithms (legacy)
#[no_mangle]
pub extern "C" fn OpenSSL_add_ssl_algorithms() -> c_int {
    crate::SSL_library_init()
}

// ============================================================================
// Error Functions (forwarded from ncryptolib)
// ============================================================================

/// Get last error from error queue
#[no_mangle]
pub extern "C" fn ERR_get_error() -> c_ulong {
    unsafe { crate::ncryptolib::ERR_get_error() }
}

/// Peek at last error without removing from queue
#[no_mangle]
pub extern "C" fn ERR_peek_error() -> c_ulong {
    unsafe { crate::ncryptolib::ERR_peek_error() }
}

/// Clear error queue
#[no_mangle]
pub extern "C" fn ERR_clear_error() {
    unsafe { crate::ncryptolib::ERR_clear_error() }
}

/// Get error string
#[no_mangle]
pub extern "C" fn ERR_error_string(e: c_ulong, buf: *mut c_char) -> *const c_char {
    unsafe { crate::ncryptolib::ERR_error_string(e, buf) }
}

/// Get error string (safer version)
#[no_mangle]
pub extern "C" fn ERR_error_string_n(e: c_ulong, buf: *mut c_char, len: size_t) {
    unsafe { crate::ncryptolib::ERR_error_string_n(e, buf, len) }
}

/// Print error queue to stderr
#[no_mangle]
pub extern "C" fn ERR_print_errors_fp(fp: *mut core::ffi::c_void) {
    unsafe { crate::ncryptolib::ERR_print_errors_fp(fp) }
}

// ============================================================================
// Version Functions
// ============================================================================

/// Get OpenSSL version number (for compatibility checks)
#[no_mangle]
pub extern "C" fn OpenSSL_version_num() -> c_ulong {
    crate::OPENSSL_VERSION_NUMBER
}

/// Get OpenSSL version string
#[no_mangle]
pub extern "C" fn OpenSSL_version(type_: c_int) -> *const c_char {
    // Type 0 = OPENSSL_VERSION
    if type_ == 0 {
        b"nssl 1.0.0 (OpenSSL 3.0.0 compatible)\0".as_ptr() as *const c_char
    } else {
        b"unknown\0".as_ptr() as *const c_char
    }
}
