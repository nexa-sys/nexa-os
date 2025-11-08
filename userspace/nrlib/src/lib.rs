#![no_std]
#![feature(lang_items)]
#![feature(linkage)]
#![feature(thread_local)]
#![feature(c_variadic)]

use core::{arch::asm, cmp, ffi::c_void, mem, ptr};

// C Runtime support for std programs
pub mod crt;

// libc compatibility layer for std support
pub mod libc_compat;
// Minimal stdio support (unbuffered) implemented in stdio.rs
pub mod stdio;

// Re-export commonly used stdio helpers for convenience
pub use stdio::{
    fflush, fprintf, fread, fwrite, getchar, printf, putchar, puts, stderr_write_all,
    stderr_write_str, stdin_read_line, stdin_read_line_masked, stdin_read_line_noecho,
    stdout_flush, stdout_write_all, stdout_write_fmt, stdout_write_str,
};

// Libc-compatible type definitions for NexaOS
pub type c_char = i8;
pub type c_int = i32;
pub type c_uint = u32;
pub type c_long = i64;
pub type c_ulong = u64;
pub type size_t = usize;
pub type ssize_t = isize;
pub type time_t = i64;
pub type suseconds_t = i64;
pub type rlim_t = u64;
pub type pid_t = i32;
pub type uid_t = u32;
pub type gid_t = u32;
pub type mode_t = u32;
pub type off_t = i64;

// System call numbers mirror the kernel definitions
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_DUP: u64 = 32;
const SYS_PIPE: u64 = 22;
const SYS_DUP2: u64 = 33;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAIT4: u64 = 61;
const SYS_GETPID: u64 = 39;
const SYS_STAT: u64 = 4;
const SYS_FSTAT: u64 = 5;
const SYS_LSEEK: u64 = 8;

#[no_mangle]
pub extern "C" fn pipe(pipefd: *mut i32) -> i32 {
    if pipefd.is_null() {
        set_errno(EINVAL);
        return -1;
    }

    let mut fds = [0i32; 2];
    let ret = syscall1(SYS_PIPE, &mut fds as *mut [i32; 2] as u64);
    if ret == u64::MAX {
        refresh_errno_from_kernel();
        -1
    } else {
        unsafe {
            *pipefd.add(0) = fds[0];
            *pipefd.add(1) = fds[1];
        }
        set_errno(0);
        0
    }
}
const SYS_GETPPID: u64 = 110;
const SYS_RUNLEVEL: u64 = 231;
const SYS_USER_ADD: u64 = 220;
const SYS_USER_LOGIN: u64 = 221;
const SYS_GETERRNO: u64 = 201;

pub(crate) const EINVAL: i32 = 22;
pub(crate) const ENOENT: i32 = 2;
pub(crate) const EAGAIN: i32 = 11;
pub(crate) const ENOMEM: i32 = 12;
pub(crate) const ENOSYS: i32 = 38;

// Minimal syscall wrappers that match the userspace convention (int 0x81)
#[inline(always)]
pub fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rax") ret,
            clobber_abi("sysv64"),
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
pub fn syscall2(n: u64, a1: u64, a2: u64) -> u64 {
    syscall3(n, a1, a2, 0)
}

#[inline(always)]
pub fn syscall1(n: u64, a1: u64) -> u64 {
    syscall3(n, a1, 0, 0)
}

#[inline(always)]
pub fn syscall0(n: u64) -> u64 {
    syscall3(n, 0, 0, 0)
}

// errno support (global for now, single-process environment)
static mut ERRNO: i32 = 0;

#[inline(always)]
pub fn set_errno(value: i32) {
    unsafe {
        ERRNO = value;
    }
}

#[inline(always)]
pub fn get_errno() -> i32 {
    unsafe { ERRNO }
}

#[inline(always)]
fn refresh_errno_from_kernel() -> i32 {
    let err = syscall1(SYS_GETERRNO, 0) as i32;
    set_errno(err);
    err
}

#[inline(always)]
fn translate_ret_isize(ret: u64) -> isize {
    if ret == u64::MAX {
        refresh_errno_from_kernel();
        -1
    } else {
        set_errno(0);
        ret as isize
    }
}

#[inline(always)]
fn translate_ret_i32(ret: u64) -> i32 {
    translate_ret_isize(ret) as i32
}

#[no_mangle]
pub extern "C" fn __errno_location() -> *mut i32 {
    unsafe { &mut ERRNO }
}

#[no_mangle]
pub extern "C" fn __errno() -> *mut i32 {
    __errno_location()
}

// POSIX/C ABI surface ------------------------------------------------------

#[no_mangle]
pub extern "C" fn read(fd: i32, buf: *mut c_void, count: usize) -> isize {
    translate_ret_isize(syscall3(SYS_READ, fd as u64, buf as u64, count as u64))
}

#[no_mangle]
pub extern "C" fn write(fd: i32, buf: *const c_void, count: usize) -> isize {
    translate_ret_isize(syscall3(SYS_WRITE, fd as u64, buf as u64, count as u64))
}

#[no_mangle]
pub extern "C" fn open(path: *const u8, flags: i32, _mode: i32) -> i32 {
    if path.is_null() {
        set_errno(EINVAL);
        return -1;
    }
    let len = strlen(path);
    translate_ret_i32(syscall3(SYS_OPEN, path as u64, len as u64, flags as u64))
}

#[no_mangle]
pub extern "C" fn open64(path: *const u8, flags: i32, mode: i32) -> i32 {
    open(path, flags, mode)
}

#[no_mangle]
pub extern "C" fn close(fd: i32) -> i32 {
    translate_ret_i32(syscall1(SYS_CLOSE, fd as u64))
}

#[no_mangle]
pub extern "C" fn dup(fd: i32) -> i32 {
    translate_ret_i32(syscall1(SYS_DUP, fd as u64))
}

#[no_mangle]
pub extern "C" fn dup2(oldfd: i32, newfd: i32) -> i32 {
    translate_ret_i32(syscall2(SYS_DUP2, oldfd as u64, newfd as u64))
}

#[no_mangle]
pub extern "C" fn fork() -> i32 {
    translate_ret_i32(syscall0(SYS_FORK))
}

#[no_mangle]
pub extern "C" fn execve(path: *const u8, argv: *const *const u8, envp: *const *const u8) -> i32 {
    if path.is_null() {
        set_errno(ENOENT);
        return -1;
    }
    translate_ret_i32(syscall3(SYS_EXECVE, path as u64, argv as u64, envp as u64))
}

#[no_mangle]
pub extern "C" fn wait4(pid: i32, status: *mut i32, options: i32, _rusage: *mut c_void) -> i32 {
    translate_ret_i32(syscall3(
        SYS_WAIT4,
        pid as u64,
        status as u64,
        options as u64,
    ))
}

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_mode: u32,
    pub st_nlink: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: i64,
    pub st_atime_nsec: i64,
    pub st_mtime: i64,
    pub st_mtime_nsec: i64,
    pub st_ctime: i64,
    pub st_ctime_nsec: i64,
    pub st_reserved: [i64; 3],
}

#[no_mangle]
pub extern "C" fn stat(path: *const u8, buf: *mut stat) -> i32 {
    if path.is_null() || buf.is_null() {
        set_errno(EINVAL);
        return -1;
    }
    let len = strlen(path);
    translate_ret_i32(syscall3(SYS_STAT, path as u64, len as u64, buf as u64))
}

#[no_mangle]
pub extern "C" fn stat64(path: *const u8, buf: *mut stat) -> i32 {
    stat(path, buf)
}

#[no_mangle]
pub extern "C" fn fstat(fd: i32, buf: *mut stat) -> i32 {
    if buf.is_null() {
        set_errno(EINVAL);
        return -1;
    }
    translate_ret_i32(syscall3(SYS_FSTAT, fd as u64, buf as u64, 0))
}

#[no_mangle]
pub extern "C" fn fstat64(fd: i32, buf: *mut stat) -> i32 {
    fstat(fd, buf)
}

#[no_mangle]
pub extern "C" fn lseek(fd: c_int, offset: c_long, whence: c_int) -> c_long {
    let raw_offset = offset as i64 as u64;
    let ret = syscall3(SYS_LSEEK, fd as u64, raw_offset, whence as u64);
    if ret == u64::MAX {
        refresh_errno_from_kernel();
        -1
    } else {
        set_errno(0);
        ret as i64
    }
}

#[no_mangle]
pub extern "C" fn lseek64(fd: c_int, offset: i64, whence: c_int) -> i64 {
    lseek(fd, offset as c_long, whence)
}

#[no_mangle]
pub extern "C" fn _exit(code: i32) -> ! {
    unsafe {
        syscall1(SYS_EXIT, code as u64);
        loop {
            asm!("hlt");
        }
    }
}

#[no_mangle]
pub extern "C" fn exit(code: i32) -> ! {
    _exit(code)
}

#[no_mangle]
pub extern "C" fn getpid() -> i32 {
    translate_ret_i32(syscall0(SYS_GETPID))
}

#[no_mangle]
pub extern "C" fn getppid() -> i32 {
    translate_ret_i32(syscall0(SYS_GETPPID))
}

#[no_mangle]
pub extern "C" fn nexa_runlevel() -> i32 {
    translate_ret_i32(syscall1(SYS_RUNLEVEL, u64::MAX))
}

#[repr(C)]
pub struct UserRequest {
    pub username_ptr: u64,
    pub username_len: u64,
    pub password_ptr: u64,
    pub password_len: u64,
    pub flags: u64,
}

#[no_mangle]
pub extern "C" fn nexa_user_add(req: *const UserRequest) -> i32 {
    if req.is_null() {
        set_errno(EINVAL);
        return -1;
    }
    translate_ret_i32(syscall1(SYS_USER_ADD, req as u64))
}

#[no_mangle]
pub extern "C" fn nexa_user_login(req: *const UserRequest) -> i32 {
    if req.is_null() {
        set_errno(EINVAL);
        return -1;
    }
    translate_ret_i32(syscall1(SYS_USER_LOGIN, req as u64))
}

// Safe Rust wrappers --------------------------------------------------------

pub mod os {
    use core::{ffi::CStr, result::Result};

    use super::{__errno_location, c_void};

    #[inline]
    pub fn read(fd: i32, buf: &mut [u8]) -> Result<usize, i32> {
        let ret = super::read(fd, buf.as_mut_ptr().cast::<c_void>(), buf.len());
        if ret < 0 {
            Err(unsafe { *__errno_location() })
        } else {
            Ok(ret as usize)
        }
    }

    #[inline]
    pub fn write(fd: i32, buf: &[u8]) -> Result<usize, i32> {
        let ret = super::write(fd, buf.as_ptr().cast::<c_void>(), buf.len());
        if ret < 0 {
            Err(unsafe { *__errno_location() })
        } else {
            Ok(ret as usize)
        }
    }

    #[inline]
    pub fn open(path: &CStr, flags: i32) -> Result<i32, i32> {
        let ret = super::open(path.as_ptr() as *const u8, flags, 0);
        if ret < 0 {
            Err(unsafe { *__errno_location() })
        } else {
            Ok(ret)
        }
    }

    #[inline]
    pub fn close(fd: i32) -> Result<(), i32> {
        let ret = super::close(fd);
        if ret < 0 {
            Err(unsafe { *__errno_location() })
        } else {
            Ok(())
        }
    }
}

// Minimal C runtime helpers -------------------------------------------------

#[no_mangle]
pub extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        ptr::copy_nonoverlapping(src, dest, n);
        dest
    }
}

#[no_mangle]
pub extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        ptr::copy(src, dest, n);
        dest
    }
}

#[no_mangle]
pub extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    unsafe {
        ptr::write_bytes(s, c as u8, n);
        s
    }
}

#[no_mangle]
pub extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    unsafe {
        let mut i = 0usize;
        while i < n {
            let va = ptr::read(a.add(i));
            let vb = ptr::read(b.add(i));
            if va != vb {
                return (va as i32) - (vb as i32);
            }
            i += 1;
        }
        0
    }
}

#[no_mangle]
pub extern "C" fn strlen(s: *const u8) -> usize {
    unsafe {
        let mut i = 0usize;
        loop {
            if ptr::read(s.add(i)) == 0 {
                return i;
            }
            i += 1;
        }
    }
}

// Minimal abort -> call exit via syscall 60
#[no_mangle]
pub extern "C" fn abort() -> ! {
    unsafe {
        syscall1(SYS_EXIT, 1);
        loop {
            asm!("hlt");
        }
    }
}

// Thread-local storage (TLS) support ----------------------------------------
// std expects pthread_key_create/delete/setspecific/getspecific
// We provide a minimal fake implementation (single-threaded for now)

const MAX_TLS_KEYS: usize = 128;
static mut TLS_KEYS: [Option<*mut c_void>; MAX_TLS_KEYS] = [None; MAX_TLS_KEYS];
static mut TLS_NEXT_KEY: usize = 0;

#[repr(C)]
pub struct pthread_key_t {
    key: usize,
}

type pthread_destructor = Option<unsafe extern "C" fn(*mut c_void)>;

#[no_mangle]
pub unsafe extern "C" fn pthread_key_create(
    key: *mut pthread_key_t,
    _destructor: pthread_destructor,
) -> i32 {
    if TLS_NEXT_KEY >= MAX_TLS_KEYS {
        set_errno(EINVAL);
        return -1;
    }
    let k = TLS_NEXT_KEY;
    TLS_NEXT_KEY += 1;
    (*key).key = k;
    TLS_KEYS[k] = None;
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_key_delete(_key: pthread_key_t) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_getspecific(key: pthread_key_t) -> *mut c_void {
    if key.key < MAX_TLS_KEYS {
        TLS_KEYS[key.key].unwrap_or(ptr::null_mut())
    } else {
        ptr::null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn pthread_setspecific(key: pthread_key_t, value: *const c_void) -> i32 {
    if key.key < MAX_TLS_KEYS {
        TLS_KEYS[key.key] = Some(value as *mut c_void);
        0
    } else {
        set_errno(EINVAL);
        -1
    }
}

// Allocator support for std::alloc::System ----------------------------------
// std expects malloc/free/realloc/calloc

const HEAP_SIZE: usize = 2 * 1024 * 1024; // 2MB heap
const DEFAULT_ALIGNMENT: usize = 16;
const HEADER_SIZE: usize = core::mem::size_of::<usize>();

static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
static mut HEAP_POS: usize = 0;

#[inline(always)]
fn is_power_of_two(value: usize) -> bool {
    value != 0 && (value & (value - 1)) == 0
}

#[inline(always)]
fn align_up(value: usize, align: usize) -> Option<usize> {
    if !is_power_of_two(align) {
        return None;
    }
    let mask = align - 1;
    value.checked_add(mask).map(|aligned| aligned & !mask)
}

unsafe fn alloc_internal(size: usize, align: usize) -> *mut c_void {
    if size == 0 {
        set_errno(0);
        return ptr::null_mut();
    }

    if align == 0 {
        set_errno(EINVAL);
        return ptr::null_mut();
    }

    let requested_align = align.max(mem::size_of::<usize>());
    if !is_power_of_two(requested_align) {
        set_errno(EINVAL);
        return ptr::null_mut();
    }

    let header_align = mem::align_of::<usize>();
    let mut current = HEAP_POS;
    current = match align_up(current, header_align) {
        Some(val) => val,
        None => {
            set_errno(EINVAL);
            return ptr::null_mut();
        }
    };

    let after_header = match current.checked_add(HEADER_SIZE) {
        Some(val) => val,
        None => {
            set_errno(ENOMEM);
            return ptr::null_mut();
        }
    };

    let aligned_data = match align_up(after_header, requested_align) {
        Some(val) => val,
        None => {
            set_errno(EINVAL);
            return ptr::null_mut();
        }
    };

    let end = match aligned_data.checked_add(size) {
        Some(val) => val,
        None => {
            set_errno(ENOMEM);
            return ptr::null_mut();
        }
    };

    if end > HEAP_SIZE {
        set_errno(ENOMEM);
        return ptr::null_mut();
    }

    let header_offset = aligned_data - HEADER_SIZE;
    let header_ptr = HEAP.as_mut_ptr().add(header_offset) as *mut usize;
    header_ptr.write(size);

    HEAP_POS = end;
    set_errno(0);
    HEAP.as_mut_ptr().add(aligned_data) as *mut c_void
}

unsafe fn allocation_size(ptr: *mut c_void) -> Option<usize> {
    if ptr.is_null() {
        return None;
    }

    let base = HEAP.as_ptr() as *mut u8;
    let data_ptr = ptr as *mut u8;

    if data_ptr < base.add(HEADER_SIZE) || data_ptr >= base.add(HEAP_SIZE) {
        return None;
    }

    let header_ptr = data_ptr.sub(HEADER_SIZE) as *mut usize;
    Some(header_ptr.read())
}

pub(crate) unsafe fn malloc_aligned(size: usize, alignment: usize) -> *mut c_void {
    alloc_internal(size, alignment)
}

#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    alloc_internal(size, DEFAULT_ALIGNMENT)
}

#[no_mangle]
pub unsafe extern "C" fn free(_ptr: *mut c_void) {
    // Bump allocator doesn't support free
}

#[no_mangle]
pub unsafe extern "C" fn realloc(ptr: *mut c_void, new_size: usize) -> *mut c_void {
    if ptr.is_null() {
        return alloc_internal(new_size, DEFAULT_ALIGNMENT);
    }

    if new_size == 0 {
        free(ptr);
        set_errno(0);
        return ptr::null_mut();
    }

    let old_size = allocation_size(ptr).unwrap_or(0);

    if old_size != 0 && new_size <= old_size {
        let header_ptr = (ptr as *mut u8).sub(HEADER_SIZE) as *mut usize;
        header_ptr.write(new_size);
        set_errno(0);
        return ptr;
    }

    let new_ptr = alloc_internal(new_size, DEFAULT_ALIGNMENT);
    if new_ptr.is_null() {
        return ptr::null_mut();
    }

    if old_size != 0 {
        let copy_len = cmp::min(old_size, new_size);
        ptr::copy_nonoverlapping(ptr as *const u8, new_ptr as *mut u8, copy_len);
    }

    new_ptr
}

#[no_mangle]
pub unsafe extern "C" fn calloc(nmemb: usize, size: usize) -> *mut c_void {
    match nmemb.checked_mul(size) {
        Some(total) if total > 0 => {
            let ptr = alloc_internal(total, DEFAULT_ALIGNMENT);
            if !ptr.is_null() {
                ptr::write_bytes(ptr as *mut u8, 0, total);
            }
            ptr
        }
        _ => {
            set_errno(0);
            ptr::null_mut()
        }
    }
}

// Random number generation (for std::random) --------------------------------
#[no_mangle]
pub unsafe extern "C" fn arc4random_buf(buf: *mut c_void, nbytes: usize) {
    // Simple pseudo-random (not cryptographically secure)
    static mut SEED: u64 = 0x123456789abcdef0;

    let bytes = core::slice::from_raw_parts_mut(buf as *mut u8, nbytes);
    for byte in bytes {
        SEED = SEED.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = (SEED >> 56) as u8;
    }
}

#[no_mangle]
pub unsafe extern "C" fn getrandom(buf: *mut c_void, buflen: usize, _flags: u32) -> isize {
    arc4random_buf(buf, buflen);
    buflen as isize
}

// lang items ----------------------------------------------------------------

#[cfg(feature = "panic-handler")]
#[lang = "eh_personality"]
#[linkage = "weak"]
extern "C" fn eh_personality() {}

#[cfg(feature = "panic-handler")]
#[panic_handler]
#[linkage = "weak"]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    abort()
}
