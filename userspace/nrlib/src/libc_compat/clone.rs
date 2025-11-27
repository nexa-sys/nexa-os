//! Clone and thread management functions
//!
//! Provides clone, gettid, set_tid_address, futex, and related functions.

use crate::{c_int, c_void, size_t};
use super::types::timespec;

// ============================================================================
// Clone Syscall
// ============================================================================

/// SYS_CLONE - Create a new process/thread
#[no_mangle]
pub unsafe extern "C" fn clone_syscall(
    flags: c_int,
    stack: *mut c_void,
    parent_tid: *mut c_int,
    child_tid: *mut c_int,
    tls: *mut c_void,
) -> c_int {
    let ret = crate::syscall5(
        crate::SYS_CLONE,
        flags as u64,
        stack as u64,
        parent_tid as u64,
        child_tid as u64,
        tls as u64,
    );
    
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        ret as c_int
    }
}

/// __clone - musl libc clone wrapper
#[no_mangle]
pub unsafe extern "C" fn __clone(
    func: extern "C" fn(*mut c_void) -> c_int,
    stack: *mut c_void,
    flags: c_int,
    arg: *mut c_void,
    parent_tid: *mut c_int,
    tls: *mut c_void,
    child_tid: *mut c_int,
) -> c_int {
    // In single-threaded environment, clone for threads is not fully supported
    let _ = (func, stack, flags, arg, parent_tid, tls, child_tid);
    crate::set_errno(crate::ENOSYS);
    -1
}

// ============================================================================
// Thread ID Functions
// ============================================================================

/// SYS_GETTID - Get thread ID
#[no_mangle]
pub unsafe extern "C" fn gettid() -> c_int {
    let ret = crate::syscall0(crate::SYS_GETTID);
    crate::set_errno(0);
    ret as c_int
}

/// SYS_SET_TID_ADDRESS - Set pointer to thread ID
#[no_mangle]
pub unsafe extern "C" fn set_tid_address(tidptr: *mut c_int) -> c_int {
    let ret = crate::syscall1(crate::SYS_SET_TID_ADDRESS, tidptr as u64);
    crate::set_errno(0);
    ret as c_int
}

// ============================================================================
// Robust List Functions
// ============================================================================

/// SYS_SET_ROBUST_LIST - Set robust futex list head
#[no_mangle]
pub unsafe extern "C" fn set_robust_list(head: *mut c_void, len: size_t) -> c_int {
    let ret = crate::syscall2(crate::SYS_SET_ROBUST_LIST, head as u64, len as u64);
    
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// SYS_GET_ROBUST_LIST - Get robust futex list
#[no_mangle]
pub unsafe extern "C" fn get_robust_list(
    pid: c_int,
    head_ptr: *mut *mut c_void,
    len_ptr: *mut size_t,
) -> c_int {
    let ret = crate::syscall3(
        crate::SYS_GET_ROBUST_LIST,
        pid as u64,
        head_ptr as u64,
        len_ptr as u64,
    );
    
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

// ============================================================================
// Futex Functions
// ============================================================================

/// SYS_FUTEX - Fast userspace mutex operations
#[no_mangle]
pub unsafe extern "C" fn futex(
    uaddr: *mut c_int,
    op: c_int,
    val: c_int,
    timeout: *const timespec,
    uaddr2: *mut c_int,
    val3: c_int,
) -> c_int {
    let ret = crate::syscall6(
        crate::SYS_FUTEX,
        uaddr as u64,
        op as u64,
        val as u64,
        timeout as u64,
        uaddr2 as u64,
        val3 as u64,
    );
    
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        ret as c_int
    }
}

// ============================================================================
// Scheduling Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn sched_yield() -> c_int {
    0
}
