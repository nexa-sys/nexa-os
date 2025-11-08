//! libc compatibility layer for std support
//! Provides necessary C ABI functions that std expects from libc
//!
//! Note: Basic functions (read, write, open, close, exit, getpid, memcpy, etc.)
//! are already defined in lib.rs. This module only adds additional functions
//! needed by std that are not in lib.rs.

use crate::{c_char, c_int, c_long, c_uint, c_ulong, c_void, size_t, ssize_t};
use core::{hint::spin_loop, ptr};

// ============================================================================
// Memory Allocation - Already defined in lib.rs
// ============================================================================

// Note: malloc, free, calloc, realloc are defined in lib.rs with bump allocator

#[no_mangle]
pub unsafe extern "C" fn posix_memalign(
    memptr: *mut *mut c_void,
    alignment: size_t,
    size: size_t,
) -> c_int {
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
// String/Memory Functions - Already defined in lib.rs
// ============================================================================

// Note: strlen, memcpy, memset, memmove, memcmp are defined in lib.rs

// ============================================================================
// I/O Functions - Already defined in lib.rs
// ============================================================================

// Note: read, write, open, close are defined in lib.rs

// Versioned stat functions for glibc compatibility
// std expects these for File::open and metadata operations
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

// fstatat and newfstatat - used by std::fs for relative path operations
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
pub unsafe extern "C" fn isatty(fd: c_int) -> c_int {
    if (0..=2).contains(&fd) {
        1
    } else {
        0
    }
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
pub unsafe extern "C" fn _Unwind_RaiseException(
    _exception_object: *mut c_void,
) -> UnwindReasonCode {
    0
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_Resume(_exception_object: *mut c_void) {}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_DeleteException(_exception_object: *mut c_void) {}

// ============================================================================
// POSIX System Configuration
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn sysconf(_name: c_int) -> c_long {
    -1 // Not supported
}

// File access check (F_OK, R_OK, W_OK, X_OK)
const F_OK: c_int = 0;
const R_OK: c_int = 4;
const W_OK: c_int = 2;
const X_OK: c_int = 1;

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
        // stat failed, file doesn't exist or error
        return -1;
    }

    // File exists, check permissions if needed
    // For now, we allow all access (no permission checking)
    if mode == F_OK {
        return 0; // File exists
    }

    // Simplified: assume all files are readable/writable/executable
    0
}

// openat - open file relative to directory fd
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

const F_DUPFD: c_int = 0;
const F_GETFL: c_int = 3;
const F_SETFL: c_int = 4;

#[no_mangle]
pub unsafe extern "C" fn fcntl(fd: c_int, cmd: c_int, arg: c_int) -> c_int {
    match cmd {
        F_DUPFD => {
            if arg <= fd {
                crate::dup(fd) as c_int
            } else {
                crate::dup2(fd, arg) as c_int
            }
        }
        F_GETFL | F_SETFL => 0,
        _ => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn ioctl(_fd: c_int, _request: c_ulong, _args: *mut c_void) -> c_int {
    // Minimal ioctl implementation
    // std uses this to check if fd is a terminal
    -1 // Return error for all ioctl calls
}

#[no_mangle]
pub unsafe extern "C" fn readlink(
    path: *const c_char,
    buf: *mut c_char,
    bufsiz: size_t,
) -> ssize_t {
    // Symbolic links not supported - return error
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
    // Symbolic links not supported - return error
    let _ = dirfd;
    let _ = pathname;
    let _ = buf;
    let _ = bufsiz;
    crate::set_errno(22); // EINVAL
    -1
}

#[no_mangle]
pub unsafe extern "C" fn poll(_fds: *mut c_void, _nfds: c_ulong, _timeout: c_int) -> c_int {
    0 // No events
}

#[no_mangle]
pub unsafe extern "C" fn getdents64(fd: c_int, dirp: *mut c_void, count: c_uint) -> c_int {
    // Directory reading not yet supported
    let _ = fd;
    let _ = dirp;
    let _ = count;
    crate::set_errno(38); // ENOSYS - Function not implemented
    -1
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
// Thread Attribute Functions
// ============================================================================

#[repr(C)]
pub struct pthread_attr_t {
    __size: [u64; 7],
}

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_init(_attr: *mut pthread_attr_t) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_destroy(_attr: *mut pthread_attr_t) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_setstacksize(
    _attr: *mut pthread_attr_t,
    _stacksize: size_t,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_setguardsize(
    _attr: *mut pthread_attr_t,
    _guardsize: size_t,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_getguardsize(
    _attr: *const pthread_attr_t,
    guardsize: *mut size_t,
) -> c_int {
    if !guardsize.is_null() {
        *guardsize = 0;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_getstack(
    _attr: *const pthread_attr_t,
    stackaddr: *mut *mut c_void,
    stacksize: *mut size_t,
) -> c_int {
    if !stackaddr.is_null() {
        *stackaddr = ptr::null_mut();
    }
    if !stacksize.is_null() {
        *stacksize = 0;
    }
    0
}

pub type pthread_t = c_ulong;

#[no_mangle]
pub unsafe extern "C" fn pthread_self() -> pthread_t {
    1 // Always return 1 for single-threaded
}

#[no_mangle]
pub unsafe extern "C" fn pthread_getattr_np(
    _thread: pthread_t,
    _attr: *mut pthread_attr_t,
) -> c_int {
    0
}

// ============================================================================
// File Descriptor Operations
// ============================================================================

// ============================================================================
// Signal Handling Stubs
// ============================================================================

pub type sighandler_t = Option<unsafe extern "C" fn(c_int)>;

#[no_mangle]
pub unsafe extern "C" fn signal(_signum: c_int, _handler: sighandler_t) -> sighandler_t {
    None // Return NULL (signal not supported)
}

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

const SYS_SCHED_YIELD: i64 = 24;
const SYS_NANOSLEEP: i64 = 35;
const SYS_GETPID: i64 = 39;
const SYS_GETTID: i64 = 186;
const SYS_FUTEX: i64 = 202;
const SYS_GETRANDOM: i64 = 318;

const FUTEX_WAIT: i32 = 0;
const FUTEX_WAKE: i32 = 1;
const FUTEX_CMD_MASK: i32 = 0x7;
const FUTEX_PRIVATE_FLAG: i32 = 128;

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[no_mangle]
pub unsafe extern "C" fn syscall(number: i64, mut args: ...) -> i64 {
    use core::ffi::VaListImpl;

    match number {
        SYS_GETPID => {
            crate::set_errno(0);
            crate::getpid() as i64
        }
        SYS_GETTID => {
            crate::set_errno(0);
            crate::getpid() as i64
        }
        SYS_SCHED_YIELD => {
            // Single-threaded for now – nothing to schedule.
            crate::set_errno(0);
            0
        }
        SYS_NANOSLEEP => {
            let _req: *const Timespec = args.arg();
            let _rem: *mut Timespec = args.arg();
            let _ = (_req, _rem);
            crate::set_errno(0);
            0
        }
        SYS_GETRANDOM => {
            let buf: *mut c_void = args.arg();
            let len: usize = args.arg();
            let flags: u32 = args.arg();
            let res = crate::getrandom(buf, len, flags);
            if res < 0 {
                res as i64
            } else {
                crate::set_errno(0);
                res as i64
            }
        }
        SYS_FUTEX => {
            let uaddr: *mut i32 = args.arg();
            let mut op: i32 = args.arg();
            let val: i32 = args.arg();
            let _timeout: *const Timespec = args.arg();
            let _uaddr2: *mut i32 = args.arg();
            let _val3: i32 = args.arg();

            if uaddr.is_null() {
                crate::set_errno(crate::EINVAL);
                return -1;
            }

            op &= !(FUTEX_PRIVATE_FLAG);
            let cmd = op & FUTEX_CMD_MASK;

            match cmd {
                FUTEX_WAIT => {
                    let current = core::ptr::read_volatile(uaddr);
                    if current != val {
                        crate::set_errno(crate::EAGAIN);
                        return -1;
                    }

                    for _ in 0..1024 {
                        spin_loop();
                        if core::ptr::read_volatile(uaddr) != val {
                            crate::set_errno(0);
                            return 0;
                        }
                    }

                    crate::set_errno(crate::EAGAIN);
                    -1
                }
                FUTEX_WAKE => {
                    crate::set_errno(0);
                    if val > 0 {
                        1
                    } else {
                        0
                    }
                }
                _ => {
                    crate::set_errno(crate::ENOSYS);
                    -1
                }
            }
        }
        _ => {
            crate::set_errno(crate::ENOSYS);
            -1
        }
    }
}

// ============================================================================
// Auxiliary Vector
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn getauxval(_type: u64) -> u64 {
    0
}
