//! libc compatibility layer for std support
//! Provides necessary C ABI functions that std expects from libc
//!
//! Note: Basic functions (read, write, open, close, exit, getpid, memcpy, etc.) 
//! are already defined in lib.rs. This module only adds additional functions
//! needed by std that are not in lib.rs.

use crate::{c_char, c_int, c_long, c_uint, c_ulong, c_void, size_t, ssize_t};
use core::{
    hint::spin_loop,
    mem,
    ptr,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};

/// Trace function entry (logs to stderr)
/// Disabled by default for clean output
macro_rules! trace_fn {
    ($name:expr) => {
        // crate::debug_log_message(concat!("[nrlib] ", $name, "\n").as_bytes());
    };
}

static PTHREAD_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
static PTHREAD_MUTEX_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
static PTHREAD_MUTEX_EXTRA_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

const SYS_WRITE_NR: u64 = 1;

const PTHREAD_MUTEX_NORMAL: c_int = 0;
const PTHREAD_MUTEX_RECURSIVE: c_int = 1;
const PTHREAD_MUTEX_DEFAULT: c_int = PTHREAD_MUTEX_NORMAL;

const EPERM: c_int = 1;
const EBUSY: c_int = 16;
const EDEADLK: c_int = 35;

const MUTEX_UNLOCKED: u32 = 0;
const MUTEX_LOCKED: u32 = 1;

const PTHREAD_MUTEX_WORDS: usize = 5; // Matches glibc pthread_mutex_t size (40 bytes on x86_64)
const MUTEX_MAGIC: usize = 0x4E584D5554585F4D; // "NXMUTX_M" sentinel, arbitrary unique value
const GLIBC_KIND_WORD: usize = 2; // Word index where glibc stores kind field (offset 16 bytes)

#[repr(C)]
pub struct pthread_mutex_t {
    data: [usize; PTHREAD_MUTEX_WORDS],
}

#[repr(C)]
pub struct pthread_mutexattr_t {
    data: [c_int; 7], // matches glibc size/layout (28 bytes)
}

impl pthread_mutexattr_t {
    fn set_kind(&mut self, kind: c_int) {
        self.data[0] = kind;
    }

    fn kind(&self) -> c_int {
        self.data[0]
    }
}

struct MutexInner {
    state: AtomicU32,
    owner: c_ulong,
    recursion: c_uint,
    kind: c_int,
}

impl MutexInner {
    const fn new(kind: c_int) -> Self {
        Self {
            state: AtomicU32::new(MUTEX_UNLOCKED),
            owner: 0,
            recursion: 0,
            kind,
        }
    }
}

const MAX_PTHREAD_MUTEXES: usize = 128;
const MUTEX_INNER_SIZE: usize = mem::size_of::<MutexInner>();

#[repr(align(16))]
#[derive(Copy, Clone)]
struct MutexSlot {
    bytes: [u8; MUTEX_INNER_SIZE],
}

impl MutexSlot {
    const fn new() -> Self {
        Self {
            bytes: [0; MUTEX_INNER_SIZE],
        }
    }

    fn as_mut_ptr(&mut self) -> *mut MutexInner {
        self.bytes.as_mut_ptr() as *mut MutexInner
    }

    fn as_ptr(&self) -> *const MutexInner {
        self.bytes.as_ptr() as *const MutexInner
    }

    fn reset(&mut self) {
        self.bytes = [0; MUTEX_INNER_SIZE];
    }
}

static mut MUTEX_POOL: [MutexSlot; MAX_PTHREAD_MUTEXES] = [MutexSlot::new(); MAX_PTHREAD_MUTEXES];
static mut MUTEX_POOL_USED: [bool; MAX_PTHREAD_MUTEXES] = [false; MAX_PTHREAD_MUTEXES];

unsafe fn mutex_word_ptr(mutex: *mut pthread_mutex_t, index: usize) -> *mut usize {
    (*mutex).data.as_mut_ptr().add(index)
}

unsafe fn mutex_get_inner(mutex: *mut pthread_mutex_t) -> Option<*mut MutexInner> {
    let word0 = *mutex_word_ptr(mutex, 0);
    if word0 == 0 {
        None
    } else {
        Some(word0 as *mut MutexInner)
    }
}

unsafe fn mutex_set_inner(mutex: *mut pthread_mutex_t, inner: *mut MutexInner) {
    *mutex_word_ptr(mutex, 0) = inner as usize;
    *mutex_word_ptr(mutex, 1) = MUTEX_MAGIC;
    *mutex_word_ptr(mutex, 2) = (*inner).kind as usize;
    *mutex_word_ptr(mutex, 3) = 0;
    *mutex_word_ptr(mutex, 4) = 0;
}

unsafe fn mutex_is_initialized(mutex: *mut pthread_mutex_t) -> bool {
    *mutex_word_ptr(mutex, 1) == MUTEX_MAGIC
}

unsafe fn detect_static_kind(mutex: *mut pthread_mutex_t) -> c_int {
    let word = (*mutex).data[GLIBC_KIND_WORD];
    let kind = (word & 0xFFFF_FFFF) as c_int;
    if kind == PTHREAD_MUTEX_RECURSIVE {
        PTHREAD_MUTEX_RECURSIVE
    } else {
        PTHREAD_MUTEX_DEFAULT
    }
}

unsafe fn alloc_mutex_inner(kind: c_int) -> Result<*mut MutexInner, c_int> {
    for idx in 0..MAX_PTHREAD_MUTEXES {
        if !MUTEX_POOL_USED[idx] {
            MUTEX_POOL_USED[idx] = true;
            let slot = &mut MUTEX_POOL[idx];
            let inner_ptr = slot.as_mut_ptr();
            ptr::write(inner_ptr, MutexInner::new(kind));
            debug_mutex_event(b"[nrlib] alloc_mutex_inner\n");
            return Ok(inner_ptr);
        }
    }

    crate::set_errno(crate::ENOMEM);
    Err(crate::ENOMEM)
}

unsafe fn free_mutex_inner(inner: *mut MutexInner) {
    if inner.is_null() {
        return;
    }

    for idx in 0..MAX_PTHREAD_MUTEXES {
        let slot_ptr = MUTEX_POOL[idx].as_ptr() as *const MutexInner;
        if slot_ptr == inner as *const MutexInner {
            MUTEX_POOL[idx].reset();
            MUTEX_POOL_USED[idx] = false;
            return;
        }
    }
}

unsafe fn ensure_mutex_inner(mutex: *mut pthread_mutex_t) -> Result<*mut MutexInner, c_int> {
    if mutex_is_initialized(mutex) {
        if let Some(inner) = mutex_get_inner(mutex) {
            return Ok(inner);
        }
        return Err(crate::EINVAL);
    }

    crate::debug_log_message(b"[nrlib] ensure_mutex_inner start\n");
    debug_mutex_event(b"[nrlib] ensure_mutex_inner allocating\n");
    log_mutex_state(b"[nrlib] mutex raw", &(*mutex).data);
    let kind = detect_static_kind(mutex);
    log_mutex_kind(b"[nrlib] mutex kind", kind);
    let inner = alloc_mutex_inner(kind)?;
    (*inner).kind = kind;
    mutex_set_inner(mutex, inner);
    crate::debug_log_message(b"[nrlib] ensure_mutex_inner done\n");
    Ok(inner)
}

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
    trace_fn!("writev");
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
    let mut buf = [0u8; 64];
    let msg = b"[nrlib] isatty called for fd=";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    let fd_str = simple_itoa(fd as u64, &mut buf);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, fd_str.as_ptr() as u64, fd_str.len() as u64);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);
    
    let result = if (0..=2).contains(&fd) { 1 } else { 0 };
    
    let result_msg = b"[nrlib] isatty returning: ";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, result_msg.as_ptr() as u64, result_msg.len() as u64);
    let result_str = simple_itoa(result as u64, &mut buf);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, result_str.as_ptr() as u64, result_str.len() as u64);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);
    
    result
}

fn simple_itoa(mut n: u64, buf: &mut [u8]) -> &[u8] {
    if n == 0 {
        buf[0] = b'0';
        return &buf[0..1];
    }
    let mut i = 0;
    while n > 0 {
        buf[i] = (n % 10) as u8 + b'0';
        n /= 10;
        i += 1;
    }
    buf[..i].reverse();
    &buf[..i]
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
pub unsafe extern "C" fn getcwd(_buf: *mut i8, _size: size_t) -> *mut i8 {
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
    let mut buf = [0u8; 64];
    let msg = b"[nrlib] fcntl fd=";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    let fd_str = simple_itoa(fd as u64, &mut buf);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, fd_str.as_ptr() as u64, fd_str.len() as u64);
    let msg2 = b" cmd=";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg2.as_ptr() as u64, msg2.len() as u64);
    let cmd_str = simple_itoa(cmd as u64, &mut buf);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, cmd_str.as_ptr() as u64, cmd_str.len() as u64);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);
    
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
pub unsafe extern "C" fn ioctl(fd: c_int, request: c_ulong, _args: *mut c_void) -> c_int {
    // Log ioctl calls for debugging
    let mut buf = [0u8; 128];
    let msg = b"[nrlib] ioctl called fd=";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    let fd_str = simple_itoa(fd as u64, &mut buf);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, fd_str.as_ptr() as u64, fd_str.len() as u64);
    let msg2 = b" request=0x";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg2.as_ptr() as u64, msg2.len() as u64);
    // Print request in hex
    let mut hex_buf = [0u8; 16];
    let hex_str = u64_to_hex(request, &mut hex_buf);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, hex_str.as_ptr() as u64, hex_str.len() as u64);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);
    
    // Return error for all ioctl calls
    crate::set_errno(crate::ENOTTY);
    -1
}

fn u64_to_hex(mut n: u64, buf: &mut [u8]) -> &[u8] {
    if n == 0 {
        buf[0] = b'0';
        return &buf[0..1];
    }
    let mut i = 0;
    while n > 0 && i < buf.len() {
        let digit = (n & 0xF) as u8;
        buf[i] = if digit < 10 { b'0' + digit } else { b'a' + (digit - 10) };
        n >>= 4;
        i += 1;
    }
    buf[..i].reverse();
    &buf[..i]
}

#[no_mangle]
pub unsafe extern "C" fn readlink(
    _path: *const c_char,
    _buf: *mut c_char,
    _bufsiz: size_t,
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

#[inline(always)]
fn log_mutex(msg: &[u8]) {
    let slot = PTHREAD_MUTEX_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if slot < 0 {  // Disabled: was 256
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    }
}

#[inline(always)]
fn debug_mutex_event(msg: &[u8]) {
    let slot = PTHREAD_MUTEX_EXTRA_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if slot < 0 {  // Disabled: was 128
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    }
}

fn log_mutex_state(tag: &[u8], words: &[usize; PTHREAD_MUTEX_WORDS]) {
    let mut buf = [0u8; 192];
    let mut pos = 0usize;

    let copy_tag = core::cmp::min(tag.len(), buf.len());
    buf[..copy_tag].copy_from_slice(&tag[..copy_tag]);
    pos += copy_tag;

    for (idx, word) in words.iter().enumerate() {
        if pos + 5 >= buf.len() {
            break;
        }
        buf[pos] = b' ';
        buf[pos + 1] = b'w';
        buf[pos + 2] = b'0' + (idx as u8);
        buf[pos + 3] = b'=';
        buf[pos + 4] = b'0';
        buf[pos + 5] = b'x';
        pos += 6;

        if pos >= buf.len() {
            break;
        }

        let mut tmp = [0u8; 16];
        let hex = u64_to_hex(*word as u64, &mut tmp);
        let available = core::cmp::min(hex.len(), buf.len() - pos);
        buf[pos..pos + available].copy_from_slice(&hex[..available]);
        pos += available;
    }

    if pos < buf.len() {
        buf[pos] = b'\n';
        pos += 1;
    }

    let _ = crate::syscall3(SYS_WRITE_NR, 2, buf.as_ptr() as u64, pos as u64);
}

fn log_mutex_kind(tag: &[u8], kind: c_int) {
    let mut buf = [0u8; 64];
    let mut pos = 0usize;

    let copy_tag = core::cmp::min(tag.len(), buf.len());
    buf[..copy_tag].copy_from_slice(&tag[..copy_tag]);
    pos += copy_tag;

    if pos + 4 >= buf.len() {
        let _ = crate::syscall3(SYS_WRITE_NR, 2, buf.as_ptr() as u64, pos as u64);
        return;
    }

    buf[pos] = b' ';
    buf[pos + 1] = b'k';
    buf[pos + 2] = b'i';
    buf[pos + 3] = b'n';
    buf[pos + 4] = b'd';
    buf[pos + 5] = b'=';
    pos += 6;

    if pos < buf.len() {
        let mut tmp = [0u8; 16];
        let hex = u64_to_hex(kind as u64, &mut tmp);
        let available = core::cmp::min(hex.len(), buf.len() - pos);
        buf[pos..pos + available].copy_from_slice(&hex[..available]);
        pos += available;
    }

    if pos < buf.len() {
        buf[pos] = b'\n';
        pos += 1;
    }

    let _ = crate::syscall3(SYS_WRITE_NR, 2, buf.as_ptr() as u64, pos as u64);
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutexattr_init(attr: *mut pthread_mutexattr_t) -> c_int {
    if attr.is_null() {
        return crate::EINVAL;
    }
    debug_mutex_event(b"[nrlib] pthread_mutexattr_init enter\n");
    (*attr).set_kind(PTHREAD_MUTEX_DEFAULT);
    for slot in 1..(*attr).data.len() {
        (*attr).data[slot] = 0;
    }
    log_mutex_kind(b"[nrlib] pthread_mutexattr_init", PTHREAD_MUTEX_DEFAULT);
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutexattr_destroy(attr: *mut pthread_mutexattr_t) -> c_int {
    if attr.is_null() {
        return crate::EINVAL;
    }
    debug_mutex_event(b"[nrlib] pthread_mutexattr_destroy enter\n");
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutexattr_settype(
    attr: *mut pthread_mutexattr_t,
    kind: c_int,
) -> c_int {
    if attr.is_null() {
        return crate::EINVAL;
    }

    // TEMPORARILY DISABLED FOR DEBUGGING
    // log_mutex_kind(b"[nrlib] pthread_mutexattr_settype", kind);
    crate::debug_log_message(b"[nrlib] pthread_mutexattr_settype called\n");

    match kind {
        PTHREAD_MUTEX_NORMAL | PTHREAD_MUTEX_RECURSIVE => {
            crate::debug_log_message(b"[nrlib] pthread_mutexattr_settype setting kind\n");
            (*attr).set_kind(kind);
            crate::debug_log_message(b"[nrlib] pthread_mutexattr_settype returning 0\n");
            0
        }
        _ => {
            crate::debug_log_message(b"[nrlib] pthread_mutexattr_settype returning EINVAL\n");
            crate::EINVAL
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutexattr_gettype(
    attr: *const pthread_mutexattr_t,
    kind_out: *mut c_int,
) -> c_int {
    if attr.is_null() || kind_out.is_null() {
        return crate::EINVAL;
    }
    debug_mutex_event(b"[nrlib] pthread_mutexattr_gettype enter\n");

    *kind_out = (*attr).kind();
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutex_init(
    mutex: *mut pthread_mutex_t,
    attr: *const pthread_mutexattr_t,
) -> c_int {
    if mutex.is_null() {
        return crate::EINVAL;
    }

    crate::debug_log_message(b"[nrlib] pthread_mutex_init enter\n");
    debug_mutex_event(b"[nrlib] pthread_mutex_init enter\n");
    let kind = if attr.is_null() {
        PTHREAD_MUTEX_DEFAULT
    } else {
        (*attr).kind()
    };

    log_mutex_kind(b"[nrlib] pthread_mutex_init", kind);

    let inner = match alloc_mutex_inner(kind) {
        Ok(inner) => inner,
        Err(err) => return err,
    };
    (*inner).kind = kind;
    mutex_set_inner(mutex, inner);
    log_mutex(b"[nrlib] pthread_mutex_init\n");
    crate::debug_log_message(b"[nrlib] pthread_mutex_init done\n");
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutex_destroy(mutex: *mut pthread_mutex_t) -> c_int {
    if mutex.is_null() {
        return crate::EINVAL;
    }

    if let Some(inner) = mutex_get_inner(mutex) {
        if (*inner).state.load(Ordering::Acquire) == MUTEX_LOCKED {
            return EBUSY;
        }

        free_mutex_inner(inner);
    }

    (*mutex).data = [0; PTHREAD_MUTEX_WORDS];
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutex_lock(mutex: *mut pthread_mutex_t) -> c_int {
    if mutex.is_null() {
        return crate::EINVAL;
    }

    crate::debug_log_message(b"[nrlib] pthread_mutex_lock enter\n");
    debug_mutex_event(b"[nrlib] pthread_mutex_lock enter\n");
    log_mutex(b"[nrlib] pthread_mutex_lock\n");
    log_mutex_state(b"[nrlib] lock raw", &(*mutex).data);

    let inner = match ensure_mutex_inner(mutex) {
        Ok(inner) => inner,
        Err(err) => return err,
    };

    crate::debug_log_message(b"[nrlib] pthread_mutex_lock after ensure\n");

    let tid = crate::getpid() as c_ulong;
    let kind = (*inner).kind;

    if kind == PTHREAD_MUTEX_RECURSIVE && (*inner).owner == tid {
        (*inner).recursion = (*inner).recursion.saturating_add(1);
        return 0;
    }

    if kind != PTHREAD_MUTEX_RECURSIVE
        && (*inner).owner == tid
        && (*inner).state.load(Ordering::Acquire) == MUTEX_LOCKED
    {
        return EDEADLK;
    }

    let mut spins = 0usize;
    const MAX_SPINS: usize = 1_000_000;  // Safety limit for single-threaded
    while (*inner)
        .state
        .compare_exchange(MUTEX_UNLOCKED, MUTEX_LOCKED, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spins += 1;
        if spins > MAX_SPINS {
            // In single-threaded environment, if we've spun this much,
            // something is wrong (likely deadlock or incorrect usage).
            // Log and return EBUSY to prevent infinite loop.
            log_mutex(b"[nrlib] pthread_mutex_lock DEADLOCK detected\n");
            return EBUSY;
        }
        if spins % 10_000 == 0 {
            log_mutex(b"[nrlib] pthread_mutex_lock spinning\n");
        }
        spin_loop();
    }

    (*inner).owner = tid;
    (*inner).recursion = 1;
    crate::debug_log_message(b"[nrlib] pthread_mutex_lock acquired\n");
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutex_trylock(mutex: *mut pthread_mutex_t) -> c_int {
    if mutex.is_null() {
        return crate::EINVAL;
    }

    debug_mutex_event(b"[nrlib] pthread_mutex_trylock enter\n");
    log_mutex(b"[nrlib] pthread_mutex_trylock\n");

    let inner = match ensure_mutex_inner(mutex) {
        Ok(inner) => inner,
        Err(err) => return err,
    };

    let tid = crate::getpid() as c_ulong;
    let kind = (*inner).kind;

    if kind == PTHREAD_MUTEX_RECURSIVE && (*inner).owner == tid {
        (*inner).recursion = (*inner).recursion.saturating_add(1);
        return 0;
    }

    match (*inner).state.compare_exchange(
        MUTEX_UNLOCKED,
        MUTEX_LOCKED,
        Ordering::Acquire,
        Ordering::Relaxed,
    ) {
        Ok(_) => {
            (*inner).owner = tid;
            (*inner).recursion = 1;
            0
        }
        Err(_) => EBUSY,
    }
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutex_unlock(mutex: *mut pthread_mutex_t) -> c_int {
    if mutex.is_null() {
        return crate::EINVAL;
    }

    debug_mutex_event(b"[nrlib] pthread_mutex_unlock enter\n");
    log_mutex(b"[nrlib] pthread_mutex_unlock\n");
    log_mutex_state(b"[nrlib] unlock raw", &(*mutex).data);

    let inner = match ensure_mutex_inner(mutex) {
        Ok(inner) => inner,
        Err(err) => return err,
    };

    if (*inner).state.load(Ordering::Acquire) == MUTEX_UNLOCKED {
        return crate::EINVAL;
    }

    let tid = crate::getpid() as c_ulong;
    if (*inner).owner != tid {
        return EPERM;
    }

    if (*inner).kind == PTHREAD_MUTEX_RECURSIVE {
        if (*inner).recursion > 1 {
            (*inner).recursion -= 1;
            return 0;
        }
    }

    (*inner).owner = 0;
    (*inner).recursion = 0;
    (*inner).state.store(MUTEX_UNLOCKED, Ordering::Release);
    0
}

#[no_mangle]
pub unsafe extern "C" fn __pthread_mutex_init(
    mutex: *mut pthread_mutex_t,
    attr: *const pthread_mutexattr_t,
) -> c_int {
    pthread_mutex_init(mutex, attr)
}

#[no_mangle]
pub unsafe extern "C" fn __pthread_mutex_destroy(mutex: *mut pthread_mutex_t) -> c_int {
    pthread_mutex_destroy(mutex)
}

#[no_mangle]
pub unsafe extern "C" fn __pthread_mutex_lock(mutex: *mut pthread_mutex_t) -> c_int {
    pthread_mutex_lock(mutex)
}

#[no_mangle]
pub unsafe extern "C" fn __pthread_mutex_trylock(mutex: *mut pthread_mutex_t) -> c_int {
    pthread_mutex_trylock(mutex)
}

#[no_mangle]
pub unsafe extern "C" fn __pthread_mutex_unlock(mutex: *mut pthread_mutex_t) -> c_int {
    pthread_mutex_unlock(mutex)
}

pub type pthread_t = c_ulong;

#[no_mangle]
pub unsafe extern "C" fn pthread_self() -> pthread_t {
    let slot = PTHREAD_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if slot < 32 {
        let msg = b"[nrlib] pthread_self\n";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    }
    1 // Always return 1 for single-threaded
}

#[no_mangle]
pub unsafe extern "C" fn pthread_getattr_np(
    _thread: pthread_t,
    _attr: *mut pthread_attr_t,
) -> c_int {
    trace_fn!("pthread_getattr_np");
    0
}

// pthread_once support for std::sync::Once
#[repr(C)]
pub struct pthread_once_t {
    state: AtomicU32,
}

const PTHREAD_ONCE_INIT_VALUE: u32 = 0;
const PTHREAD_ONCE_IN_PROGRESS: u32 = 1;
const PTHREAD_ONCE_DONE: u32 = 2;

#[no_mangle]
pub static PTHREAD_ONCE_INIT: pthread_once_t = pthread_once_t {
    state: AtomicU32::new(PTHREAD_ONCE_INIT_VALUE),
};

#[no_mangle]
pub unsafe extern "C" fn pthread_once(
    once_control: *mut pthread_once_t,
    init_routine: Option<unsafe extern "C" fn()>,
) -> c_int {
    trace_fn!("pthread_once");
    
    // CRITICAL DIAGNOSTIC: Log every call to pthread_once with the function pointer
    let routine_addr = if let Some(f) = init_routine {
        f as *const () as u64
    } else {
        0
    };
    
    let mut buf = [0u8; 64];
    let diag_msg = b"[nrlib] pthread_once called with routine @ 0x";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, diag_msg.as_ptr() as u64, diag_msg.len() as u64);
    
    // Format routine address as hex
    for i in 0..16 {
        let shift = (15 - i) * 4;
        let nibble = ((routine_addr >> shift) & 0xF) as u8;
        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
        let _ = crate::syscall3(SYS_WRITE_NR, 2, &ch as *const u8 as u64, 1);
    }
    let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);
    
    if once_control.is_null() {
        return crate::EINVAL;
    }
    
    let init = match init_routine {
        Some(f) => f,
        None => return crate::EINVAL,
    };
    
    let control = &*once_control;
    
    // Fast path: already initialized
    if control.state.load(Ordering::Acquire) == PTHREAD_ONCE_DONE {
        return 0;
    }
    
    // Try to be the thread that initializes
    match control.state.compare_exchange(
        PTHREAD_ONCE_INIT_VALUE,
        PTHREAD_ONCE_IN_PROGRESS,
        Ordering::Acquire,
        Ordering::Acquire,
    ) {
        Ok(_) => {
            // We won the race, do the initialization
            // DIAGNOSTIC: Log when we're about to call the init routine
            let diag_msg = b"[nrlib] pthread_once: Calling init routine\n";
            let _ = crate::syscall3(SYS_WRITE_NR, 2, diag_msg.as_ptr() as u64, diag_msg.len() as u64);
            
            init();
            
            // DIAGNOSTIC: Log when init completed
            let diag_msg = b"[nrlib] pthread_once: Init routine completed\n";
            let _ = crate::syscall3(SYS_WRITE_NR, 2, diag_msg.as_ptr() as u64, diag_msg.len() as u64);
            
            control.state.store(PTHREAD_ONCE_DONE, Ordering::Release);
            0
        }
        Err(PTHREAD_ONCE_DONE) => {
            // Someone else finished while we were checking
            0
        }
        Err(_) => {
            // Someone else is initializing, spin-wait
            let diag_msg = b"[nrlib] pthread_once: Waiting for init from another thread\n";
            let _ = crate::syscall3(SYS_WRITE_NR, 2, diag_msg.as_ptr() as u64, diag_msg.len() as u64);
            
            // IMPORTANT: Add timeout to detect hangs
            let mut spin_count = 0u32;
            loop {
                if control.state.load(Ordering::Acquire) != PTHREAD_ONCE_IN_PROGRESS {
                    break;
                }
                spin_count += 1;
                if spin_count > 100000 {
                    let hang_msg = b"[nrlib] WARNING: pthread_once init timeout - possible hang\n";
                    let _ = crate::syscall3(SYS_WRITE_NR, 2, hang_msg.as_ptr() as u64, hang_msg.len() as u64);
                    break;
                }
                spin_loop();
            }
            
            let diag_msg = b"[nrlib] pthread_once: Init completed by other thread\n";
            let _ = crate::syscall3(SYS_WRITE_NR, 2, diag_msg.as_ptr() as u64, diag_msg.len() as u64);
            
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn __pthread_once(
    once_control: *mut pthread_once_t,
    init_routine: Option<unsafe extern "C" fn()>,
) -> c_int {
    pthread_once(once_control, init_routine)
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

static FUTEX_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

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

            let log_slot = FUTEX_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
            if log_slot < 999 {  // Increased from 128 to see more logs
                let mut buf = [0u8; 128];
                let prefix = if cmd == FUTEX_WAIT {
                    b"[nrlib] futex wait op\n"
                } else {
                    b"[nrlib] futex wake op\n"
                };
                let len = prefix.len().min(buf.len());
                buf[..len].copy_from_slice(&prefix[..len]);
                let _ = crate::syscall3(SYS_WRITE_NR, 2, buf.as_ptr() as u64, len as u64);
            }

            match cmd {
                FUTEX_WAIT => {
                    // Check if the value at uaddr matches the expected value
                    let current = core::ptr::read_volatile(uaddr);
                    if current != val {
                        // Value already changed, no need to wait
                        crate::set_errno(crate::EAGAIN);
                        return -1;
                    }

                    // CRITICAL FIX for single-threaded environment:
                    // In single-threaded, we can't block because no other thread can wake us.
                    // std's Once/OnceLock pattern expects:
                    // - If state != COMPLETE, try to acquire initialization lock
                    // - If can't acquire (another thread initializing), WAIT
                    // - When initialization done, state = COMPLETE, WAKE all
                    //
                    // In single-threaded:
                    // - Only ONE "thread" (execution flow) exists
                    // - If we reach FUTEX_WAIT, it means:
                    //   a) State was checked and found incomplete
                    //   b) Lock acquisition attempted and "failed"
                    //   c) Caller wants to wait for completion
                    //
                    // Since single-threaded, if state is incomplete when we check,
                    // it means WE are the initializer! No other thread exists.
                    // So FUTEX_WAIT should never happen in correct single-threaded Once.
                    //
                    // However, if we DO get here (bug in caller or racy check),
                    // returning EAGAIN causes infinite retry loop!
                    //
                    // Solution: Return 0 (success) immediately.
                    // This makes the caller think the wait completed and re-check state.
                    // If state is now COMPLETE (initialization finished), good!
                    // If state still incomplete, caller will retry initialization.
                    //
                    // This breaks deadlock while maintaining Once semantics.
                    crate::set_errno(0);
                    0  // Return success to break wait loop
                }
                FUTEX_WAKE => {
                    // In single-threaded environment, just return success
                    // In multi-threaded, this would wake up waiting threads
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
