//! Unwind stubs for panic handling
//!
//! Provides _Unwind_* functions required by the Rust panic infrastructure.

use super::types::{UnwindContext, UnwindReasonCode, UnwindTraceFn};
use crate::{c_int, c_void};

// ============================================================================
// Unwind Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetIP(_context: *mut UnwindContext) -> u64 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetIPInfo(
    _context: *mut UnwindContext,
    ip_before_insn: *mut c_int,
) -> u64 {
    if !ip_before_insn.is_null() {
        *ip_before_insn = 0;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetCFA(_context: *mut UnwindContext) -> u64 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetGR(_context: *mut UnwindContext, _index: c_int) -> u64 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_SetGR(_context: *mut UnwindContext, _index: c_int, _value: u64) {}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_SetIP(_context: *mut UnwindContext, _value: u64) {}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetDataRelBase(_context: *mut UnwindContext) -> u64 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetTextRelBase(_context: *mut UnwindContext) -> u64 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetRegionStart(_context: *mut UnwindContext) -> u64 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetLanguageSpecificData(_context: *mut UnwindContext) -> u64 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_Backtrace(
    _trace: UnwindTraceFn,
    _trace_argument: *mut c_void,
) -> UnwindReasonCode {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_RaiseException(
    _exception_object: *mut c_void,
) -> UnwindReasonCode {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_Resume(_exception_object: *mut c_void) {}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_DeleteException(_exception_object: *mut c_void) {}
