//! Signal handling stubs
//!
//! Provides signal-related functions (stubs for now as signals aren't fully supported).

use crate::{c_int, c_void};
use super::types::{sigaction, sighandler_t};

// ============================================================================
// Signal Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn signal(_signum: c_int, _handler: sighandler_t) -> sighandler_t {
    None // Return NULL (signal not supported)
}

#[no_mangle]
pub unsafe extern "C" fn sigaction(
    _signum: c_int,
    _act: *const sigaction,
    _oldact: *mut sigaction,
) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn sigaltstack(_ss: *const c_void, _old_ss: *mut c_void) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn sigemptyset(_set: *mut c_void) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn sigaddset(_set: *mut c_void, _signum: c_int) -> c_int {
    0
}

// ============================================================================
// Kill Function (stub)
// ============================================================================

#[no_mangle]
pub extern "C" fn kill(_pid: crate::pid_t, _sig: c_int) -> c_int {
    // Stub: signals not fully implemented
    0
}
