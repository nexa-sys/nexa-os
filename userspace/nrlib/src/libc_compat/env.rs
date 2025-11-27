//! Environment and string functions
//!
//! Provides getenv, setenv, getcwd, strerror, and related functions.

use crate::{c_int, size_t};
use core::ptr;

// ============================================================================
// Environment Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn getenv(_name: *const i8) -> *mut i8 {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn setenv(_name: *const i8, _value: *const i8, _overwrite: c_int) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn unsetenv(_name: *const i8) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn getcwd(_buf: *mut i8, _size: size_t) -> *mut i8 {
    ptr::null_mut()
}

// ============================================================================
// String/Error Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn __xpg_strerror_r(_errnum: c_int, buf: *mut i8, buflen: size_t) -> c_int {
    // Write a generic error message
    if buflen > 0 {
        *buf = 0; // Empty string
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn strerror_r(errnum: c_int, buf: *mut i8, buflen: size_t) -> *mut i8 {
    __xpg_strerror_r(errnum, buf, buflen);
    buf
}

// ============================================================================
// Auxiliary Vector
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn getauxval(_type: u64) -> u64 {
    0
}

// ============================================================================
// Helper Functions
// ============================================================================

pub fn simple_itoa(mut n: u64, buf: &mut [u8]) -> &[u8] {
    if n == 0 {
        buf[0] = b'0';
        return &buf[0..1];
    }
    let mut i = 0;
    while n > 0 {
        buf[i] = (n % 10) as u8 + b'0';
        n /= 10;
        i += 1;
    }
    buf[..i].reverse();
    &buf[..i]
}
