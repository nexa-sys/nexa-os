#![no_std]
#![feature(lang_items)]

use core::{arch::asm, ffi::c_void, ptr};

// System call numbers mirror the kernel definitions
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAIT4: u64 = 61;
const SYS_GETPID: u64 = 39;
const SYS_GETPPID: u64 = 110;
const SYS_RUNLEVEL: u64 = 231;
const SYS_USER_ADD: u64 = 220;
const SYS_USER_LOGIN: u64 = 221;
const SYS_GETERRNO: u64 = 201;

const EINVAL: i32 = 22;
const ENOENT: i32 = 2;

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
fn set_errno(value: i32) {
    unsafe { ERRNO = value; }
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
    let len = unsafe { strlen(path) };
    translate_ret_i32(syscall3(SYS_OPEN, path as u64, len as u64, flags as u64))
}

#[no_mangle]
pub extern "C" fn close(fd: i32) -> i32 {
    translate_ret_i32(syscall1(SYS_CLOSE, fd as u64))
}

#[no_mangle]
pub extern "C" fn fork() -> i32 {
    translate_ret_i32(syscall0(SYS_FORK))
}

#[no_mangle]
pub extern "C" fn execve(
    path: *const u8,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i32 {
    if path.is_null() {
        set_errno(ENOENT);
        return -1;
    }
    translate_ret_i32(syscall3(SYS_EXECVE, path as u64, argv as u64, envp as u64))
}

#[no_mangle]
pub extern "C" fn wait4(
    pid: i32,
    status: *mut i32,
    options: i32,
    _rusage: *mut c_void,
) -> i32 {
    translate_ret_i32(syscall3(
        SYS_WAIT4,
        pid as u64,
        status as u64,
        options as u64,
    ))
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

    use super::{c_void, __errno_location};

    #[inline]
    pub fn read(fd: i32, buf: &mut [u8]) -> Result<usize, i32> {
        let ret = unsafe { super::read(fd, buf.as_mut_ptr().cast::<c_void>(), buf.len()) };
        if ret < 0 {
            Err(unsafe { *__errno_location() })
        } else {
            Ok(ret as usize)
        }
    }

    #[inline]
    pub fn write(fd: i32, buf: &[u8]) -> Result<usize, i32> {
        let ret = unsafe { super::write(fd, buf.as_ptr().cast::<c_void>(), buf.len()) };
        if ret < 0 {
            Err(unsafe { *__errno_location() })
        } else {
            Ok(ret as usize)
        }
    }

    #[inline]
    pub fn open(path: &CStr, flags: i32) -> Result<i32, i32> {
        let ret = unsafe { super::open(path.as_ptr() as *const u8, flags, 0) };
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

// lang items ----------------------------------------------------------------

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    abort()
}
