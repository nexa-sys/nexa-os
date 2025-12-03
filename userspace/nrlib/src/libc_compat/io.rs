//! I/O operations compatibility layer
//!
//! Provides stat variants, vectored I/O, fcntl, ioctl, and related functions.

use crate::{c_char, c_int, c_uint, c_ulong, c_void, size_t, ssize_t};

use super::types::{iovec, F_DUPFD, F_GETFL, F_SETFL};

/// Trace function entry (disabled by default)
macro_rules! trace_fn {
    ($name:expr) => {
        // crate::debug_log_message(concat!("[nrlib] ", $name, "\n").as_bytes());
    };
}

// ============================================================================
// Versioned stat Functions (glibc compatibility)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn __xstat(_ver: c_int, path: *const u8, buf: *mut crate::stat) -> c_int {
    crate::stat(path, buf)
}

#[no_mangle]
pub unsafe extern "C" fn __xstat64(_ver: c_int, path: *const u8, buf: *mut crate::stat) -> c_int {
    crate::stat(path, buf)
}

#[no_mangle]
pub unsafe extern "C" fn __fxstat(_ver: c_int, fd: c_int, buf: *mut crate::stat) -> c_int {
    crate::fstat(fd, buf)
}

#[no_mangle]
pub unsafe extern "C" fn __fxstat64(_ver: c_int, fd: c_int, buf: *mut crate::stat) -> c_int {
    crate::fstat(fd, buf)
}

#[no_mangle]
pub unsafe extern "C" fn __lxstat(_ver: c_int, path: *const u8, buf: *mut crate::stat) -> c_int {
    // lstat is the same as stat for us (no symlinks)
    crate::stat(path, buf)
}

#[no_mangle]
pub unsafe extern "C" fn __lxstat64(_ver: c_int, path: *const u8, buf: *mut crate::stat) -> c_int {
    crate::stat(path, buf)
}

// ============================================================================
// fstatat Variants
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn fstatat(
    dirfd: c_int,
    pathname: *const c_char,
    buf: *mut crate::stat,
    flags: c_int,
) -> c_int {
    // We don't support dirfd, just treat as normal stat
    let _ = dirfd;
    let _ = flags;
    crate::stat(pathname as *const u8, buf)
}

#[no_mangle]
pub unsafe extern "C" fn newfstatat(
    dirfd: c_int,
    pathname: *const c_char,
    buf: *mut crate::stat,
    flags: c_int,
) -> c_int {
    fstatat(dirfd, pathname, buf, flags)
}

#[no_mangle]
pub unsafe extern "C" fn fstatat64(
    dirfd: c_int,
    pathname: *const c_char,
    buf: *mut crate::stat,
    flags: c_int,
) -> c_int {
    fstatat(dirfd, pathname, buf, flags)
}

// ============================================================================
// Vector I/O
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn readv(fd: c_int, iov: *const iovec, iovcnt: c_int) -> ssize_t {
    let mut total: ssize_t = 0;
    for i in 0..iovcnt as usize {
        let vec = &*iov.add(i);
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
    trace_fn!("writev");
    let mut total: ssize_t = 0;
    for i in 0..iovcnt as usize {
        let vec = &*iov.add(i);
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
// File Access Check
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn access(path: *const u8, mode: c_int) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    // Use stat to check if file exists
    let mut statbuf: crate::stat = core::mem::zeroed();
    let ret = crate::stat(path, &mut statbuf);

    if ret < 0 {
        return -1;
    }

    // File exists, check permissions if needed
    // F_OK = 0
    if mode == 0 {
        return 0;
    }

    // Simplified: assume all files are readable/writable/executable
    0
}

// ============================================================================
// openat Variants
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn openat(
    dirfd: c_int,
    pathname: *const c_char,
    flags: c_int,
    mode: c_int,
) -> c_int {
    // We don't support dirfd, just treat as normal open
    let _ = dirfd;
    crate::open(pathname as *const u8, flags, mode)
}

#[no_mangle]
pub unsafe extern "C" fn openat64(
    dirfd: c_int,
    pathname: *const c_char,
    flags: c_int,
    mode: c_int,
) -> c_int {
    openat(dirfd, pathname, flags, mode)
}

// ============================================================================
// File Control Operations
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn fcntl(fd: c_int, cmd: c_int, arg: c_int) -> c_int {
    match cmd {
        F_DUPFD => {
            crate::translate_ret_i32(crate::syscall3(
                crate::SYS_FCNTL,
                fd as u64,
                cmd as u64,
                (arg as i64) as u64,
            ))
        }
        F_GETFL | F_SETFL => 0,
        _ => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn ioctl(fd: c_int, request: c_ulong, _args: *mut c_void) -> c_int {
    let _ = fd;
    let _ = request;
    crate::set_errno(crate::ENOTTY);
    -1
}

// ============================================================================
// Symlink Functions (not supported)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn readlink(
    _path: *const c_char,
    _buf: *mut c_char,
    _bufsiz: size_t,
) -> ssize_t {
    crate::set_errno(22); // EINVAL
    -1
}

#[no_mangle]
pub unsafe extern "C" fn readlinkat(
    dirfd: c_int,
    pathname: *const c_char,
    buf: *mut c_char,
    bufsiz: size_t,
) -> ssize_t {
    let _ = dirfd;
    let _ = pathname;
    let _ = buf;
    let _ = bufsiz;
    crate::set_errno(22); // EINVAL
    -1
}

// ============================================================================
// Directory Reading - moved to fs.rs
// ============================================================================

// ============================================================================
// Poll
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn poll(_fds: *mut c_void, _nfds: c_ulong, _timeout: c_int) -> c_int {
    0 // No events
}

// ============================================================================
// Terminal Check
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn isatty(fd: c_int) -> c_int {
    // Return 1 (true) for stdin/stdout/stderr, 0 (false) otherwise
    if (0..=2).contains(&fd) { 1 } else { 0 }
}
