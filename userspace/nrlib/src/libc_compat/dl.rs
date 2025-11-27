//! Dynamic linker stubs
//!
//! Provides dlopen, dlsym, dladdr, dlclose, dlerror functions.

use crate::{c_int, c_void};
use core::ptr;

// ============================================================================
// Dynamic Linker Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn dladdr(_addr: *const c_void, _info: *mut c_void) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn dlopen(_filename: *const i8, _flags: c_int) -> *mut c_void {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn dlsym(_handle: *mut c_void, _symbol: *const i8) -> *mut c_void {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn dlclose(_handle: *mut c_void) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn dlerror() -> *mut i8 {
    ptr::null_mut()
}
