//! Epoll and eventfd support for async I/O
//!
//! Provides epoll_create1, epoll_ctl, epoll_wait, and eventfd for tokio/mio support.

use crate::{c_int, c_uint, c_void, refresh_errno_from_kernel, set_errno, size_t, EINVAL, ENOSYS};
use core::arch::asm;

// ============================================================================
// Epoll Constants
// ============================================================================

/// Epoll events
pub const EPOLLIN: u32 = 0x001;
pub const EPOLLPRI: u32 = 0x002;
pub const EPOLLOUT: u32 = 0x004;
pub const EPOLLERR: u32 = 0x008;
pub const EPOLLHUP: u32 = 0x010;
pub const EPOLLNVAL: u32 = 0x020;
pub const EPOLLRDNORM: u32 = 0x040;
pub const EPOLLRDBAND: u32 = 0x080;
pub const EPOLLWRNORM: u32 = 0x100;
pub const EPOLLWRBAND: u32 = 0x200;
pub const EPOLLMSG: u32 = 0x400;
pub const EPOLLRDHUP: u32 = 0x2000;
pub const EPOLLEXCLUSIVE: u32 = 1 << 28;
pub const EPOLLWAKEUP: u32 = 1 << 29;
pub const EPOLLONESHOT: u32 = 1 << 30;
pub const EPOLLET: u32 = 1 << 31;

/// Epoll control operations
pub const EPOLL_CTL_ADD: c_int = 1;
pub const EPOLL_CTL_DEL: c_int = 2;
pub const EPOLL_CTL_MOD: c_int = 3;

/// Epoll create flags
pub const EPOLL_CLOEXEC: c_int = 0x80000;

/// Eventfd flags
pub const EFD_SEMAPHORE: c_int = 1;
pub const EFD_CLOEXEC: c_int = 0x80000;
pub const EFD_NONBLOCK: c_int = 0x800;

// Syscall numbers
const SYS_EPOLL_CREATE1: u64 = 291;
const SYS_EPOLL_CTL: u64 = 233;
const SYS_EPOLL_WAIT: u64 = 232;
const SYS_EPOLL_PWAIT: u64 = 281;
const SYS_EVENTFD2: u64 = 290;

// ============================================================================
// Epoll Data Structures
// ============================================================================

/// Epoll event structure (matches Linux)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct epoll_event {
    pub events: u32,
    pub data: epoll_data,
}

/// Epoll data union (using u64 representation)
#[repr(C)]
#[derive(Clone, Copy)]
pub union epoll_data {
    pub ptr: *mut c_void,
    pub fd: c_int,
    pub u32_val: u32,
    pub u64_val: u64,
}

// ============================================================================
// Epoll Functions
// ============================================================================

/// Create an epoll instance
///
/// # Arguments
/// * `flags` - EPOLL_CLOEXEC to set close-on-exec flag
///
/// # Returns
/// Epoll file descriptor on success, -1 on error
#[no_mangle]
pub extern "C" fn epoll_create1(flags: c_int) -> c_int {
    let ret: i64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") SYS_EPOLL_CREATE1 => ret,
            in("rdi") flags as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }

    if ret < 0 {
        set_errno((-ret) as c_int);
        -1
    } else {
        set_errno(0);
        ret as c_int
    }
}

/// Create an epoll instance (deprecated, use epoll_create1)
///
/// # Arguments
/// * `size` - Ignored since Linux 2.6.8
///
/// # Returns
/// Epoll file descriptor on success, -1 on error
#[no_mangle]
pub extern "C" fn epoll_create(size: c_int) -> c_int {
    if size <= 0 {
        set_errno(EINVAL);
        return -1;
    }
    epoll_create1(0)
}

/// Control an epoll instance
///
/// # Arguments
/// * `epfd` - Epoll file descriptor
/// * `op` - Operation (EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD)
/// * `fd` - Target file descriptor
/// * `event` - Event configuration (can be NULL for EPOLL_CTL_DEL)
///
/// # Returns
/// 0 on success, -1 on error
#[no_mangle]
pub unsafe extern "C" fn epoll_ctl(
    epfd: c_int,
    op: c_int,
    fd: c_int,
    event: *mut epoll_event,
) -> c_int {
    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") SYS_EPOLL_CTL => ret,
        in("rdi") epfd as u64,
        in("rsi") op as u64,
        in("rdx") fd as u64,
        in("r10") event as u64,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );

    if ret < 0 {
        set_errno((-ret) as c_int);
        -1
    } else {
        set_errno(0);
        0
    }
}

/// Wait for events on an epoll instance
///
/// # Arguments
/// * `epfd` - Epoll file descriptor
/// * `events` - Buffer to receive events
/// * `maxevents` - Maximum number of events to return
/// * `timeout` - Timeout in milliseconds (-1 for infinite)
///
/// # Returns
/// Number of ready file descriptors, 0 on timeout, -1 on error
#[no_mangle]
pub unsafe extern "C" fn epoll_wait(
    epfd: c_int,
    events: *mut epoll_event,
    maxevents: c_int,
    timeout: c_int,
) -> c_int {
    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") SYS_EPOLL_WAIT => ret,
        in("rdi") epfd as u64,
        in("rsi") events as u64,
        in("rdx") maxevents as u64,
        in("r10") timeout as u64,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );

    if ret < 0 {
        set_errno((-ret) as c_int);
        -1
    } else {
        set_errno(0);
        ret as c_int
    }
}

/// Wait for events on an epoll instance with signal mask
///
/// # Arguments
/// * `epfd` - Epoll file descriptor
/// * `events` - Buffer to receive events
/// * `maxevents` - Maximum number of events to return
/// * `timeout` - Timeout in milliseconds (-1 for infinite)
/// * `sigmask` - Signal mask to apply during wait
///
/// # Returns
/// Number of ready file descriptors, 0 on timeout, -1 on error
#[no_mangle]
pub unsafe extern "C" fn epoll_pwait(
    epfd: c_int,
    events: *mut epoll_event,
    maxevents: c_int,
    timeout: c_int,
    sigmask: *const u64,
) -> c_int {
    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") SYS_EPOLL_PWAIT => ret,
        in("rdi") epfd as u64,
        in("rsi") events as u64,
        in("rdx") maxevents as u64,
        in("r10") timeout as u64,
        in("r8") sigmask as u64,
        in("r9") 8u64, // sigsetsize
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );

    if ret < 0 {
        set_errno((-ret) as c_int);
        -1
    } else {
        set_errno(0);
        ret as c_int
    }
}

// ============================================================================
// Eventfd Functions
// ============================================================================

/// Create an eventfd file descriptor
///
/// # Arguments
/// * `initval` - Initial value for the counter
/// * `flags` - EFD_CLOEXEC, EFD_NONBLOCK, EFD_SEMAPHORE
///
/// # Returns
/// File descriptor on success, -1 on error
#[no_mangle]
pub extern "C" fn eventfd(initval: c_uint, flags: c_int) -> c_int {
    let ret: i64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") SYS_EVENTFD2 => ret,
            in("rdi") initval as u64,
            in("rsi") flags as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }

    if ret < 0 {
        set_errno((-ret) as c_int);
        -1
    } else {
        set_errno(0);
        ret as c_int
    }
}

/// Read from an eventfd
#[no_mangle]
pub unsafe extern "C" fn eventfd_read(fd: c_int, value: *mut u64) -> c_int {
    if value.is_null() {
        set_errno(EINVAL);
        return -1;
    }

    let ret = crate::read(fd, value as *mut c_void, 8);
    if ret == 8 {
        0
    } else {
        -1
    }
}

/// Write to an eventfd
#[no_mangle]
pub unsafe extern "C" fn eventfd_write(fd: c_int, value: u64) -> c_int {
    let val = value;
    let ret = crate::write(fd, &val as *const u64 as *const c_void, 8);
    if ret == 8 {
        0
    } else {
        -1
    }
}
