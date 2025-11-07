//! libc compatibility layer for std support
//! Provides necessary C ABI functions that std expects from libc
//! 
//! Note: Basic functions (read, write, open, close, exit, getpid, memcpy, etc.) 
//! are already defined in lib.rs. This module only adds additional functions 
//! needed by std that are not in lib.rs.

use core::ptr;
use crate::{c_int, c_void, size_t, ssize_t};

// ============================================================================
// Memory Allocation - Already defined in lib.rs
// ============================================================================

// Note: malloc, free, calloc, realloc are defined in lib.rs with bump allocator

#[no_mangle]
pub unsafe extern "C" fn posix_memalign(
    memptr: *mut *mut c_void,
    _alignment: size_t,
    size: size_t,
) -> c_int {
    let ptr = crate::malloc(size);
    if ptr.is_null() {
        return -1;
    }
    *memptr = ptr;
    0
}

// ============================================================================
// String/Memory Functions - Already defined in lib.rs
// ============================================================================

// Note: strlen, memcpy, memset, memmove, memcmp are defined in lib.rs

// ============================================================================
// I/O Functions - Already defined in lib.rs
// ============================================================================

// Note: read, write, open, close are defined in lib.rs

// Vector I/O
#[repr(C)]
pub struct iovec {
    pub iov_base: *mut c_void,
    pub iov_len: size_t,
}

#[no_mangle]
pub unsafe extern "C" fn readv(fd: c_int, iov: *const iovec, iovcnt: c_int) -> ssize_t {
    let mut total: ssize_t = 0;
    for i in 0..iovcnt as usize {
        let vec = &*iov.add(i);
        // Use read from lib.rs
        let n = crate::read(fd, vec.iov_base, vec.iov_len);
        if n < 0 {
            return if total > 0 { total } else { n };
        }
        total += n;
        if (n as size_t) < vec.iov_len {
            break; // Short read
        }
    }
    total
}

#[no_mangle]
pub unsafe extern "C" fn writev(fd: c_int, iov: *const iovec, iovcnt: c_int) -> ssize_t {
    let mut total: ssize_t = 0;
    for i in 0..iovcnt as usize {
        let vec = &*iov.add(i);
        // Use write from lib.rs
        let n = crate::write(fd, vec.iov_base, vec.iov_len);
        if n < 0 {
            return if total > 0 { total } else { n };
        }
        total += n;
        if (n as size_t) < vec.iov_len {
            break; // Short write
        }
    }
    total
}

// ============================================================================
// Process Control - Already defined in lib.rs
// ============================================================================

// Note: exit, _exit, getpid, getppid, abort are defined in lib.rs

#[no_mangle]
pub unsafe extern "C" fn pause() -> c_int {
    -1
}

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
pub unsafe extern "C" fn getcwd(buf: *mut i8, _size: size_t) -> *mut i8 {
    ptr::null_mut()
}

// ============================================================================
// Error Handling - Already defined in lib.rs
// ============================================================================

// Note: __errno_location is defined in lib.rs

// ============================================================================
// Thread-Local Storage - Already defined in lib.rs
// ============================================================================

// Note: pthread_key_create, pthread_key_delete, pthread_getspecific, 
// pthread_setspecific are all defined in lib.rs

// ============================================================================
// Unwind Stubs (for panic handling)
// ============================================================================

#[repr(C)]
pub struct UnwindContext {
    _private: [u8; 0],
}

pub type UnwindReasonCode = c_int;

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

pub type UnwindTraceFn =
    unsafe extern "C" fn(context: *mut UnwindContext, arg: *mut c_void) -> UnwindReasonCode;

#[no_mangle]
pub unsafe extern "C" fn _Unwind_Backtrace(
    _trace: UnwindTraceFn,
    _trace_argument: *mut c_void,
) -> UnwindReasonCode {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_RaiseException(_exception_object: *mut c_void) -> UnwindReasonCode {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_Resume(_exception_object: *mut c_void) {}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_DeleteException(_exception_object: *mut c_void) {}

// ============================================================================
// Dynamic Linker Stubs
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

// ============================================================================
// Memory Mapping Stubs
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn mmap(
    _addr: *mut c_void,
    _length: size_t,
    _prot: c_int,
    _flags: c_int,
    _fd: c_int,
    _offset: i64,
) -> *mut c_void {
    (-1isize) as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn munmap(_addr: *mut c_void, _length: size_t) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn mprotect(_addr: *mut c_void, _len: size_t, _prot: c_int) -> c_int {
    -1
}

// ============================================================================
// Signal Handling Stubs
// ============================================================================

#[repr(C)]
pub struct sigaction {
    _private: [u8; 0],
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
// Scheduling Stubs
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn sched_yield() -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn nanosleep(_req: *const c_void, _rem: *mut c_void) -> c_int {
    0
}

// ============================================================================
// Syscall Wrapper
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn syscall(_number: i64, ...) -> i64 {
    -1
}

// ============================================================================
// Auxiliary Vector
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn getauxval(_type: u64) -> u64 {
    0
}
