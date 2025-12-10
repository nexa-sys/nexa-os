//! Memory management functions
//!
//! Provides mmap, munmap, brk, sbrk, and other memory-related functions.

use crate::{c_int, c_void, size_t};
use core::ptr;

use super::types::MAP_FAILED;

// Re-export constants
pub use super::types::{
    MAP_ANON, MAP_ANONYMOUS, MAP_FIXED, MAP_NORESERVE, MAP_POPULATE, MAP_PRIVATE, MAP_SHARED,
};

/// Trace function entry (logs to stderr)
/// Disabled by default for clean output
macro_rules! trace_fn {
    ($name:expr) => {
        // crate::debug_log_message(concat!("[nrlib] ", $name, "\n").as_bytes());
    };
}

// ============================================================================
// Memory Allocation
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn posix_memalign(
    memptr: *mut *mut c_void,
    alignment: size_t,
    size: size_t,
) -> c_int {
    trace_fn!("posix_memalign");
    if memptr.is_null() {
        return crate::EINVAL;
    }

    if alignment == 0
        || alignment < core::mem::size_of::<usize>()
        || (alignment & (alignment - 1)) != 0
    {
        return crate::EINVAL;
    }

    if size == 0 {
        *memptr = ptr::null_mut();
        return 0;
    }

    let ptr = crate::malloc_aligned(size, alignment);
    if ptr.is_null() {
        crate::ENOMEM
    } else {
        *memptr = ptr;
        0
    }
}

// ============================================================================
// Memory Mapping
// ============================================================================

/// SYS_MMAP - Memory map
/// Maps memory region with specified protection and flags
#[no_mangle]
pub unsafe extern "C" fn mmap(
    addr: *mut c_void,
    length: size_t,
    prot: c_int,
    flags: c_int,
    fd: c_int,
    offset: i64,
) -> *mut c_void {
    let ret = crate::syscall6(
        crate::SYS_MMAP,
        addr as u64,
        length as u64,
        prot as u64,
        flags as u64,
        fd as i64 as u64,
        offset as u64,
    );

    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        MAP_FAILED
    } else {
        crate::set_errno(0);
        ret as *mut c_void
    }
}

/// mmap64 - 64-bit version (same as mmap on 64-bit systems)
#[no_mangle]
pub unsafe extern "C" fn mmap64(
    addr: *mut c_void,
    length: size_t,
    prot: c_int,
    flags: c_int,
    fd: c_int,
    offset: i64,
) -> *mut c_void {
    mmap(addr, length, prot, flags, fd, offset)
}

/// SYS_MUNMAP - Unmap memory region
#[no_mangle]
pub unsafe extern "C" fn munmap(addr: *mut c_void, length: size_t) -> c_int {
    let ret = crate::syscall2(crate::SYS_MUNMAP, addr as u64, length as u64);

    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// SYS_MPROTECT - Change memory protection
#[no_mangle]
pub unsafe extern "C" fn mprotect(addr: *mut c_void, len: size_t, prot: c_int) -> c_int {
    let ret = crate::syscall3(crate::SYS_MPROTECT, addr as u64, len as u64, prot as u64);

    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// SYS_BRK - Change data segment size (heap management)
/// Returns new program break on success, current break on failure
#[no_mangle]
pub unsafe extern "C" fn brk(addr: *mut c_void) -> c_int {
    let ret = crate::syscall1(crate::SYS_BRK, addr as u64);

    if ret == addr as u64 {
        crate::set_errno(0);
        0
    } else {
        crate::set_errno(crate::ENOMEM);
        -1
    }
}

/// sbrk - Increment data space by increment bytes
/// Returns previous program break on success, (void*)-1 on failure
static mut CURRENT_BRK: *mut c_void = ptr::null_mut();

#[no_mangle]
pub unsafe extern "C" fn sbrk(increment: isize) -> *mut c_void {
    // Get current break if not yet initialized
    if CURRENT_BRK.is_null() {
        let ret = crate::syscall1(crate::SYS_BRK, 0);
        if ret == u64::MAX {
            crate::refresh_errno_from_kernel();
            return (-1isize) as *mut c_void;
        }
        CURRENT_BRK = ret as *mut c_void;
    }

    let old_brk = CURRENT_BRK;

    if increment == 0 {
        return old_brk;
    }

    // Calculate new break
    let new_brk = if increment > 0 {
        (old_brk as usize).checked_add(increment as usize)
    } else {
        (old_brk as usize).checked_sub((-increment) as usize)
    };

    let new_brk = match new_brk {
        Some(addr) => addr as *mut c_void,
        None => {
            crate::set_errno(crate::ENOMEM);
            return (-1isize) as *mut c_void;
        }
    };

    // Request new break
    let ret = crate::syscall1(crate::SYS_BRK, new_brk as u64);

    if ret == new_brk as u64 {
        CURRENT_BRK = new_brk;
        crate::set_errno(0);
        old_brk
    } else {
        crate::set_errno(crate::ENOMEM);
        (-1isize) as *mut c_void
    }
}
