#![no_std]
#![no_builtins]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

// Minimal libc types for NexaOS to satisfy std's unix backend
// This is compiled with -Z build-std so core is available

// Basic integer types
pub type c_char = i8;
pub type c_schar = i8;
pub type c_uchar = u8;
pub type c_short = i16;
pub type c_ushort = u16;
pub type c_int = i32;
pub type c_uint = u32;
pub type c_long = i64;
pub type c_ulong = u64;
pub type c_longlong = i64;
pub type c_ulonglong = u64;
pub type c_float = f32;
pub type c_double = f64;

// System types  
pub type size_t = usize;
pub type ssize_t = isize;
pub type ptrdiff_t = isize;
pub type intptr_t = isize;
pub type uintptr_t = usize;

// POSIX types
pub type pid_t = i32;
pub type uid_t = u32;
pub type gid_t = u32;
pub type mode_t = u32;
pub type off_t = i64;
pub type time_t = i64;
pub type clock_t = i64;
pub type suseconds_t = i64;
pub type rlim_t = u64;
pub type dev_t = u64;
pub type ino_t = u64;
pub type nlink_t = u64;
pub type blksize_t = i64;
pub type blkcnt_t = i64;

// File descriptor constants
pub const STDIN_FILENO: c_int = 0;
pub const STDOUT_FILENO: c_int = 1;
pub const STDERR_FILENO: c_int = 2;

// File mode bits
pub const S_IFMT: mode_t = 0o170000;
pub const S_IFDIR: mode_t = 0o040000;
pub const S_IFCHR: mode_t = 0o020000;
pub const S_IFBLK: mode_t = 0o060000;
pub const S_IFREG: mode_t = 0o100000;
pub const S_IFIFO: mode_t = 0o010000;
pub const S_IFLNK: mode_t = 0o120000;
pub const S_IFSOCK: mode_t = 0o140000;

pub const S_ISUID: mode_t = 0o4000;
pub const S_ISGID: mode_t = 0o2000;
pub const S_ISVTX: mode_t = 0o1000;

pub const S_IRUSR: mode_t = 0o400;
pub const S_IWUSR: mode_t = 0o200;
pub const S_IXUSR: mode_t = 0o100;
pub const S_IRGRP: mode_t = 0o040;
pub const S_IWGRP: mode_t = 0o020;
pub const S_IXGRP: mode_t = 0o010;
pub const S_IROTH: mode_t = 0o004;
pub const S_IWOTH: mode_t = 0o002;
pub const S_IXOTH: mode_t = 0o001;

// Open flags
pub const O_RDONLY: c_int = 0;
pub const O_WRONLY: c_int = 1;
pub const O_RDWR: c_int = 2;
pub const O_CREAT: c_int = 64;
pub const O_EXCL: c_int = 128;
pub const O_TRUNC: c_int = 512;
pub const O_APPEND: c_int = 1024;

// Error codes
pub const EPERM: c_int = 1;
pub const ENOENT: c_int = 2;
pub const ESRCH: c_int = 3;
pub const EINTR: c_int = 4;
pub const EIO: c_int = 5;
pub const ENXIO: c_int = 6;
pub const E2BIG: c_int = 7;
pub const ENOEXEC: c_int = 8;
pub const EBADF: c_int = 9;
pub const ECHILD: c_int = 10;
pub const EAGAIN: c_int = 11;
pub const ENOMEM: c_int = 12;
pub const EACCES: c_int = 13;
pub const EFAULT: c_int = 14;
pub const EBUSY: c_int = 16;
pub const EEXIST: c_int = 17;
pub const EXDEV: c_int = 18;
pub const ENODEV: c_int = 19;
pub const ENOTDIR: c_int = 20;
pub const EISDIR: c_int = 21;
pub const EINVAL: c_int = 22;
pub const ENFILE: c_int = 23;
pub const EMFILE: c_int = 24;
pub const ENOTTY: c_int = 25;
pub const ETXTBSY: c_int = 26;
pub const EFBIG: c_int = 27;
pub const ENOSPC: c_int = 28;
pub const ESPIPE: c_int = 29;
pub const EROFS: c_int = 30;
pub const EMLINK: c_int = 31;
pub const EPIPE: c_int = 32;
pub const EDOM: c_int = 33;
pub const ERANGE: c_int = 34;
pub const EWOULDBLOCK: c_int = EAGAIN;

// Structures
#[repr(C)]
pub struct timeval {
    pub tv_sec: time_t,
    pub tv_usec: suseconds_t,
}

#[repr(C)]
pub struct timespec {
    pub tv_sec: time_t,
    pub tv_nsec: c_long,
}

#[repr(C)]
pub struct utimbuf {
    pub actime: time_t,
    pub modtime: time_t,
}

#[repr(C)]
pub struct rlimit {
    pub rlim_cur: rlim_t,
    pub rlim_max: rlim_t,
}

#[repr(C)]
pub struct tms {
    pub tms_utime: clock_t,
    pub tms_stime: clock_t,
    pub tms_cutime: clock_t,
    pub tms_cstime: clock_t,
}

// These functions will be provided by nrlib at link time
// For now, provide stub implementations that will be replaced during linking

use core::ptr::null_mut;

#[no_mangle]
pub unsafe extern "C" fn malloc(_size: size_t) -> *mut core::ffi::c_void {
    null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn free(_ptr: *mut core::ffi::c_void) {}

#[no_mangle]
pub unsafe extern "C" fn realloc(_ptr: *mut core::ffi::c_void, _size: size_t) -> *mut core::ffi::c_void {
    null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn calloc(_nmemb: size_t, _size: size_t) -> *mut core::ffi::c_void {
    null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn read(_fd: c_int, _buf: *mut core::ffi::c_void, _count: size_t) -> ssize_t {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn write(_fd: c_int, _buf: *const core::ffi::c_void, _count: size_t) -> ssize_t {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn close(_fd: c_int) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn pthread_key_create(
    _key: *mut c_uint,
    _destructor: Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn pthread_key_delete(_key: c_uint) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn pthread_getspecific(_key: c_uint) -> *mut core::ffi::c_void {
    null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn pthread_setspecific(_key: c_uint, _value: *const core::ffi::c_void) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn arc4random_buf(_buf: *mut core::ffi::c_void, _nbytes: size_t) {}

// Additional stubs that might be needed by std
#[no_mangle]
pub unsafe extern "C" fn getenv(_name: *const c_char) -> *mut c_char {
    null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn strlen(_s: *const c_char) -> size_t {
    0
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut core::ffi::c_void, src: *const core::ffi::c_void, n: size_t) -> *mut core::ffi::c_void {
    let dest = dest as *mut u8;
    let src = src as *const u8;
    for i in 0..n {
        *dest.add(i) = *src.add(i);
    }
    dest as *mut core::ffi::c_void
}

#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut core::ffi::c_void, c: c_int, n: size_t) -> *mut core::ffi::c_void {
    let s = s as *mut u8;
    for i in 0..n {
        *s.add(i) = c as u8;
    }
    s as *mut core::ffi::c_void
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(s1: *const core::ffi::c_void, s2: *const core::ffi::c_void, n: size_t) -> c_int {
    let s1 = s1 as *const u8;
    let s2 = s2 as *const u8;
    for i in 0..n {
        let a = *s1.add(i);
        let b = *s2.add(i);
        if a != b {
            return if a < b { -1 } else { 1 };
        }
    }
    0
}
