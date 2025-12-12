//! I/O operations compatibility layer
//!
//! Provides stat variants, vectored I/O, fcntl, ioctl, termios, and related functions.

use crate::{c_char, c_int, c_ulong, c_void, size_t, ssize_t};

use super::types::{iovec, F_DUPFD, F_GETFL, F_SETFL};

// ============================================================================
// Termios Constants
// ============================================================================

const TCGETS: c_ulong = 0x5401;
const TCSETS: c_ulong = 0x5402;
const TCSETSW: c_ulong = 0x5403;
const TCSETSF: c_ulong = 0x5404;
const TIOCGWINSZ: c_ulong = 0x5413;
const TIOCSWINSZ: c_ulong = 0x5414;
const TIOCGPGRP: c_ulong = 0x540F;
const TIOCSPGRP: c_ulong = 0x5410;
const FIONREAD: c_ulong = 0x541B;
const FIONBIO: c_ulong = 0x5421;

// Local mode flags
const ECHO: u32 = 0o000010;
const ECHONL: u32 = 0o000100;
const ICANON: u32 = 0o000002;
const ISIG: u32 = 0o000001;
const IEXTEN: u32 = 0o100000;

// Input mode flags
const IGNBRK: u32 = 0o000001;
const BRKINT: u32 = 0o000002;
const PARMRK: u32 = 0o000010;
const ISTRIP: u32 = 0o000040;
const INLCR: u32 = 0o000100;
const IGNCR: u32 = 0o000200;
const ICRNL: u32 = 0o000400;
const IXON: u32 = 0o002000;

// Output mode flags
const OPOST: u32 = 0o000001;

// NCCS - number of control characters
const NCCS: usize = 32;

/// Termios structure for terminal control
#[repr(C)]
#[derive(Clone, Copy)]
pub struct termios {
    pub c_iflag: u32,     // Input mode flags
    pub c_oflag: u32,     // Output mode flags
    pub c_cflag: u32,     // Control mode flags
    pub c_lflag: u32,     // Local mode flags
    pub c_line: u8,       // Line discipline
    pub c_cc: [u8; NCCS], // Control characters
    pub c_ispeed: u32,    // Input speed
    pub c_ospeed: u32,    // Output speed
}

/// Winsize structure for terminal size
#[repr(C)]
#[derive(Clone, Copy)]
pub struct winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

// Global default termios - starts in "raw" mode (non-canonical, no echo)
static mut DEFAULT_TERMIOS: termios = termios {
    c_iflag: 0,
    c_oflag: OPOST,
    c_cflag: 0o000060, // CS8
    c_lflag: 0,        // Raw mode: no ICANON, no ECHO
    c_line: 0,
    c_cc: [0; NCCS],
    c_ispeed: 38400,
    c_ospeed: 38400,
};

// Default window size
static mut DEFAULT_WINSIZE: winsize = winsize {
    ws_row: 25,
    ws_col: 80,
    ws_xpixel: 0,
    ws_ypixel: 0,
};

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

/// lstat - get file status (same as stat, we don't support symlinks)
#[no_mangle]
pub unsafe extern "C" fn lstat(path: *const c_char, buf: *mut crate::stat) -> c_int {
    crate::stat(path as *const u8, buf)
}

/// lstat64 - get file status (64-bit version)
#[no_mangle]
pub unsafe extern "C" fn lstat64(path: *const c_char, buf: *mut crate::stat) -> c_int {
    crate::stat(path as *const u8, buf)
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

/// readv - read data into multiple buffers using native kernel syscall
#[no_mangle]
pub unsafe extern "C" fn readv(fd: c_int, iov: *const iovec, iovcnt: c_int) -> ssize_t {
    crate::readv_impl(fd, iov as *const crate::c_void, iovcnt)
}

/// writev - write data from multiple buffers using native kernel syscall
#[no_mangle]
pub unsafe extern "C" fn writev(fd: c_int, iov: *const iovec, iovcnt: c_int) -> ssize_t {
    trace_fn!("writev");
    crate::writev_impl(fd, iov as *const crate::c_void, iovcnt)
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
        F_DUPFD => crate::translate_ret_i32(crate::syscall3(
            crate::SYS_FCNTL,
            fd as u64,
            cmd as u64,
            (arg as i64) as u64,
        )),
        F_GETFL | F_SETFL => 0,
        _ => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn ioctl(fd: c_int, request: c_ulong, arg: *mut c_void) -> c_int {
    // Handle terminal control requests
    match request {
        TCGETS => {
            // Get terminal attributes
            if !arg.is_null() && (0..=2).contains(&fd) {
                let termios_ptr = arg as *mut termios;
                *termios_ptr = DEFAULT_TERMIOS;
                return 0;
            }
        }
        TCSETS | TCSETSW | TCSETSF => {
            // Set terminal attributes
            if !arg.is_null() && (0..=2).contains(&fd) {
                let termios_ptr = arg as *const termios;
                DEFAULT_TERMIOS = *termios_ptr;
                return 0;
            }
        }
        TIOCGWINSZ => {
            // Get window size
            if !arg.is_null() && (0..=2).contains(&fd) {
                let ws_ptr = arg as *mut winsize;
                *ws_ptr = DEFAULT_WINSIZE;
                return 0;
            }
        }
        TIOCSWINSZ => {
            // Set window size (ignore but succeed)
            if !arg.is_null() && (0..=2).contains(&fd) {
                let ws_ptr = arg as *const winsize;
                DEFAULT_WINSIZE = *ws_ptr;
                return 0;
            }
        }
        TIOCGPGRP => {
            // Get foreground process group
            if !arg.is_null() && (0..=2).contains(&fd) {
                let pgrp_ptr = arg as *mut c_int;
                *pgrp_ptr = crate::getpid();
                return 0;
            }
        }
        TIOCSPGRP => {
            // Set foreground process group (ignore but succeed)
            if (0..=2).contains(&fd) {
                return 0;
            }
        }
        FIONREAD => {
            // Bytes available to read - return 0 (unknown)
            if !arg.is_null() {
                let count_ptr = arg as *mut c_int;
                *count_ptr = 0;
                return 0;
            }
        }
        FIONBIO => {
            // Set non-blocking mode (ignore but succeed)
            return 0;
        }
        _ => {}
    }

    // Unknown ioctl
    crate::set_errno(crate::ENOTTY);
    -1
}

// ============================================================================
// Termios Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn tcgetattr(fd: c_int, termios_p: *mut termios) -> c_int {
    if termios_p.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }
    if !(0..=2).contains(&fd) {
        crate::set_errno(crate::ENOTTY);
        return -1;
    }
    *termios_p = DEFAULT_TERMIOS;
    0
}

#[no_mangle]
pub unsafe extern "C" fn tcsetattr(
    fd: c_int,
    _optional_actions: c_int,
    termios_p: *const termios,
) -> c_int {
    if termios_p.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }
    if !(0..=2).contains(&fd) {
        crate::set_errno(crate::ENOTTY);
        return -1;
    }
    DEFAULT_TERMIOS = *termios_p;
    0
}

#[no_mangle]
pub unsafe extern "C" fn cfgetispeed(_termios_p: *const termios) -> u32 {
    38400 // Default baud rate
}

#[no_mangle]
pub unsafe extern "C" fn cfgetospeed(_termios_p: *const termios) -> u32 {
    38400 // Default baud rate
}

#[no_mangle]
pub unsafe extern "C" fn cfsetispeed(termios_p: *mut termios, speed: u32) -> c_int {
    if !termios_p.is_null() {
        (*termios_p).c_ispeed = speed;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn cfsetospeed(termios_p: *mut termios, speed: u32) -> c_int {
    if !termios_p.is_null() {
        (*termios_p).c_ospeed = speed;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn cfmakeraw(termios_p: *mut termios) {
    if termios_p.is_null() {
        return;
    }
    let t = &mut *termios_p;
    t.c_iflag &= !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON);
    t.c_oflag &= !OPOST;
    t.c_lflag &= !(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
    t.c_cflag &= !(0o000060 | 0o000400); // Clear CSIZE, PARENB
    t.c_cflag |= 0o000060; // CS8
}

// ============================================================================
// Symlink Functions
// ============================================================================

const SYS_READLINK: u64 = 89;
const SYS_READLINKAT: u64 = 267;

#[no_mangle]
pub unsafe extern "C" fn readlink(
    path: *const c_char,
    buf: *mut c_char,
    bufsiz: size_t,
) -> ssize_t {
    crate::translate_ret_isize(crate::syscall3(
        SYS_READLINK,
        path as u64,
        buf as u64,
        bufsiz as u64,
    )) as ssize_t
}

#[no_mangle]
pub unsafe extern "C" fn readlinkat(
    dirfd: c_int,
    pathname: *const c_char,
    buf: *mut c_char,
    bufsiz: size_t,
) -> ssize_t {
    crate::translate_ret_isize(crate::syscall6(
        SYS_READLINKAT,
        dirfd as u64,
        pathname as u64,
        buf as u64,
        bufsiz as u64,
        0,
        0,
    )) as ssize_t
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
    if (0..=2).contains(&fd) {
        1
    } else {
        0
    }
}
