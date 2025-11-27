#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(all(not(feature = "std"), feature = "panic-handler"), feature(lang_items))]
#![feature(linkage)]
#![feature(thread_local)]
#![feature(c_variadic)]

#[cfg(feature = "std")]
extern crate std;

use core::{
    arch::asm,
    cmp,
    ffi::c_void,
    mem,
    ptr,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use nexa_boot_info::{BlockDeviceInfo, FramebufferInfo, NetworkDeviceInfo};

// Indicate to std that we're in single-threaded mode
// This may cause std to skip locking entirely for I/O
#[no_mangle]
pub static __libc_single_threaded: u8 = 1;  // 1 = single-threaded (skip locks)

// C Runtime support for std programs
pub mod crt;

// libc compatibility layer for std support
pub mod libc_compat;
// Minimal stdio support (unbuffered) implemented in stdio.rs
pub mod stdio;
// Timekeeping utilities for libc compatibility functions
pub mod time;

// DNS and resolver modules
pub mod dns;
pub mod resolver;

// Socket API module
pub mod socket;

// Re-export commonly used stdio helpers for convenience
pub use stdio::{
    fflush, fprintf, fread, fwrite, getchar, printf, putchar, puts, stderr_write_all,
    stderr_write_str, stdin_read_line, stdin_read_line_masked, stdin_read_line_noecho,
    stdout_flush, stdout_write_all, stdout_write_fmt, stdout_write_str,
};

// Re-export time functions
pub use time::{get_uptime, sleep};

// Re-export socket types and functions
pub use socket::{
    bind, connect, recvfrom, sendto, socket,
    format_ipv4, parse_ipv4, SockAddr, SockAddrIn,
    AF_INET, AF_INET6, AF_UNSPEC,
    SOCK_STREAM, SOCK_DGRAM, SOCK_RAW,
    IPPROTO_IP, IPPROTO_ICMP, IPPROTO_TCP, IPPROTO_UDP,
};

// Re-export process control functions and wait status macros
pub use libc_compat::{
    wexitstatus, wifexited, wifsignaled, wtermsig, wifstopped, wstopsig,
    WNOHANG, WUNTRACED, WCONTINUED,
};

// Re-export resolver types
pub use resolver::Resolver;

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
const SYS_FCNTL: u64 = 72;

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
const SYS_UEFI_GET_COUNTS: u64 = 240;
const SYS_UEFI_GET_FB_INFO: u64 = 241;
const SYS_UEFI_GET_NET_INFO: u64 = 242;
const SYS_UEFI_GET_BLOCK_INFO: u64 = 243;
const SYS_UEFI_MAP_NET_MMIO: u64 = 244;
const SYS_UEFI_GET_USB_INFO: u64 = 245;
const SYS_UEFI_GET_HID_INFO: u64 = 246;
const SYS_UEFI_MAP_USB_MMIO: u64 = 247;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct UefiCompatCounts {
    pub framebuffer: u8,
    pub network: u8,
    pub block: u8,
    pub usb_host: u8,
    pub hid_input: u8,
    pub _reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UefiNetworkDescriptor {
    pub info: NetworkDeviceInfo,
    pub mmio_base: u64,
    pub mmio_length: u64,
    pub bar_flags: u32,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
    pub _reserved: [u8; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UefiBlockDescriptor {
    pub info: BlockDeviceInfo,
    pub mmio_base: u64,
    pub mmio_length: u64,
    pub bar_flags: u32,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
    pub _reserved: [u8; 2],
}

impl Default for UefiNetworkDescriptor {
    fn default() -> Self {
        Self {
            info: NetworkDeviceInfo::empty(),
            mmio_base: 0,
            mmio_length: 0,
            bar_flags: 0,
            interrupt_line: 0,
            interrupt_pin: 0,
            _reserved: [0; 2],
        }
    }
}

impl Default for UefiBlockDescriptor {
    fn default() -> Self {
        Self {
            info: BlockDeviceInfo::empty(),
            mmio_base: 0,
            mmio_length: 0,
            bar_flags: 0,
            interrupt_line: 0,
            interrupt_pin: 0,
            _reserved: [0; 2],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UefiUsbHostDescriptor {
    pub info: nexa_boot_info::UsbHostInfo,
    pub mmio_base: u64,
    pub mmio_size: u64,
    pub interrupt_line: u8,
    pub _reserved: [u8; 7],
}

impl Default for UefiUsbHostDescriptor {
    fn default() -> Self {
        Self {
            info: nexa_boot_info::UsbHostInfo::empty(),
            mmio_base: 0,
            mmio_size: 0,
            interrupt_line: 0,
            _reserved: [0; 7],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UefiHidInputDescriptor {
    pub info: nexa_boot_info::HidInputInfo,
    pub _reserved: [u8; 16],
}

impl Default for UefiHidInputDescriptor {
    fn default() -> Self {
        Self {
            info: nexa_boot_info::HidInputInfo::empty(),
            _reserved: [0; 16],
        }
    }
}

pub(crate) const EINVAL: i32 = 22;
pub(crate) const ENOENT: i32 = 2;
pub(crate) const EAGAIN: i32 = 11;
pub(crate) const ENOMEM: i32 = 12;
pub(crate) const ENOSYS: i32 = 38;
pub(crate) const ENOTTY: i32 = 25;
pub(crate) const ENODEV: i32 = 19;
pub(crate) const ENOSPC: i32 = 28;

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

#[inline(always)]
pub fn uefi_get_counts(out: &mut UefiCompatCounts) -> i32 {
    translate_ret_i32(syscall1(SYS_UEFI_GET_COUNTS, out as *mut _ as u64))
}

#[inline(always)]
pub fn uefi_get_framebuffer(info: &mut FramebufferInfo) -> i32 {
    translate_ret_i32(syscall1(SYS_UEFI_GET_FB_INFO, info as *mut _ as u64))
}

#[inline(always)]
pub fn uefi_get_network(index: usize, info: &mut UefiNetworkDescriptor) -> i32 {
    translate_ret_i32(syscall2(
        SYS_UEFI_GET_NET_INFO,
        index as u64,
        info as *mut _ as u64,
    ))
}

#[inline(always)]
pub fn uefi_get_block(index: usize, info: &mut UefiBlockDescriptor) -> i32 {
    translate_ret_i32(syscall2(
        SYS_UEFI_GET_BLOCK_INFO,
        index as u64,
        info as *mut _ as u64,
    ))
}

#[inline(always)]
pub fn uefi_map_network_mmio(index: usize) -> *mut c_void {
    let ret = syscall1(SYS_UEFI_MAP_NET_MMIO, index as u64);
    if ret == u64::MAX {
        refresh_errno_from_kernel();
        ptr::null_mut()
    } else {
        ret as *mut c_void
    }
}

#[inline(always)]
pub fn uefi_get_usb_host(index: usize, info: &mut UefiUsbHostDescriptor) -> i32 {
    translate_ret_i32(syscall2(
        SYS_UEFI_GET_USB_INFO,
        index as u64,
        info as *mut _ as u64,
    ))
}

#[inline(always)]
pub fn uefi_get_hid_input(index: usize, info: &mut UefiHidInputDescriptor) -> i32 {
    translate_ret_i32(syscall2(
        SYS_UEFI_GET_HID_INFO,
        index as u64,
        info as *mut _ as u64,
    ))
}

#[inline(always)]
pub fn uefi_map_usb_mmio(index: usize) -> *mut c_void {
    let ret = syscall1(SYS_UEFI_MAP_USB_MMIO, index as u64);
    if ret == u64::MAX {
        refresh_errno_from_kernel();
        ptr::null_mut()
    } else {
        ret as *mut c_void
    }
}

// errno support (global for now, single-process environment)
static mut ERRNO: i32 = 0;

// Environment variables support (empty for now)
#[no_mangle]
pub static mut environ: *mut *mut c_char = ptr::null_mut();

#[no_mangle]
pub static mut __environ: *mut *mut c_char = ptr::null_mut();

static DEBUG_WRITE_LOGGING: AtomicBool = AtomicBool::new(false);
static PTHREAD_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
static ALLOC_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

const STDERR_FD: u64 = 2;

struct DebugWriteContext {
    active: bool,
    fd: i32,
    count: usize,
}

fn append_decimal(buf: &mut [u8], mut value: u64) -> usize {
    if value == 0 {
        if !buf.is_empty() {
            buf[0] = b'0';
            return 1;
        }
        return 0;
    }
    let mut tmp = [0u8; 20];
    let mut idx = 0usize;
    while value > 0 && idx < tmp.len() {
        tmp[idx] = b'0' + (value % 10) as u8;
        value /= 10;
        idx += 1;
    }
    let mut written = 0usize;
    for i in (0..idx).rev() {
        if written >= buf.len() {
            break;
        }
        buf[written] = tmp[i];
        written += 1;
    }
    written
}

fn append_signed(buf: &mut [u8], value: i64) -> usize {
    if value < 0 {
        if buf.is_empty() {
            return 0;
        }
        buf[0] = b'-';
        1 + append_decimal(&mut buf[1..], (-value) as u64)
    } else {
        append_decimal(buf, value as u64)
    }
}

fn debug_log_flush(mut buf: [u8; 80], len: usize) {
    if len == 0 {
        return;
    }
    let _ = syscall3(SYS_WRITE, STDERR_FD, buf.as_mut_ptr() as u64, len as u64);
}

// Debug logging - disabled by default for clean output
#[allow(dead_code)]
fn debug_log_message(_msg: &[u8]) {
    // Disabled: let _ = syscall3(SYS_WRITE, STDERR_FD, msg.as_ptr() as u64, msg.len() as u64);
}

fn debug_log_write_start(fd: i32, count: usize) -> DebugWriteContext {
    let active = DEBUG_WRITE_LOGGING
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_ok();
    if active {
        let mut buf = [0u8; 80];
        let mut cursor = 0usize;
        let prefix = b"[nrlib] write fd=";
        if prefix.len() < buf.len() {
            buf[..prefix.len()].copy_from_slice(prefix);
            cursor = prefix.len();
        }
        if cursor < buf.len() {
            cursor += append_signed(&mut buf[cursor..], fd as i64);
        }
        let middle = b", count=";
        if cursor + middle.len() < buf.len() {
            buf[cursor..cursor + middle.len()].copy_from_slice(middle);
            cursor += middle.len();
        }
        if cursor < buf.len() {
            cursor += append_decimal(&mut buf[cursor..], count as u64);
        }
        if cursor < buf.len() {
            buf[cursor] = b'\n';
            cursor += 1;
        }
        debug_log_flush(buf, cursor);
    }
    DebugWriteContext { active, fd, count }
}

fn debug_log_write_end(ctx: DebugWriteContext, ret: isize) {
    if !ctx.active {
        return;
    }

    let mut buf = [0u8; 80];
    let mut cursor = 0usize;
    let prefix = b"[nrlib] write fd=";
    if prefix.len() < buf.len() {
        buf[..prefix.len()].copy_from_slice(prefix);
        cursor = prefix.len();
    }
    if cursor < buf.len() {
        cursor += append_signed(&mut buf[cursor..], ctx.fd as i64);
    }
    let mid = b", ret=";
    if cursor + mid.len() < buf.len() {
        buf[cursor..cursor + mid.len()].copy_from_slice(mid);
        cursor += mid.len();
    }
    if cursor < buf.len() {
        cursor += append_signed(&mut buf[cursor..], ret as i64);
    }
    if cursor < buf.len() {
        buf[cursor] = b'\n';
        cursor += 1;
    }
    debug_log_flush(buf, cursor);

    DEBUG_WRITE_LOGGING.store(false, Ordering::Release);
}

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
pub(crate) fn refresh_errno_from_kernel() -> i32 {
    let err = syscall1(SYS_GETERRNO, 0) as i32;
    set_errno(err);
    err
}

#[inline(always)]
pub(crate) fn translate_ret_isize(ret: u64) -> isize {
    if ret == u64::MAX {
        refresh_errno_from_kernel();
        -1
    } else {
        set_errno(0);
        ret as isize
    }
}

#[inline(always)]
pub(crate) fn translate_ret_i32(ret: u64) -> i32 {
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
    // Temporarily disable all logging to debug the issue
    let ret_raw = syscall3(SYS_WRITE, fd as u64, buf as u64, count as u64);
    translate_ret_isize(ret_raw)
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
    dup_impl(fd)
}

#[no_mangle]
pub extern "C" fn __dup(fd: i32) -> i32 {
    dup_impl(fd)
}

#[inline(always)]
fn dup_impl(fd: i32) -> i32 {
    translate_ret_i32(syscall1(SYS_DUP, fd as u64))
}

#[no_mangle]
pub extern "C" fn dup2(oldfd: i32, newfd: i32) -> i32 {
    dup2_impl(oldfd, newfd)
}

#[no_mangle]
pub extern "C" fn __dup2(oldfd: i32, newfd: i32) -> i32 {
    dup2_impl(oldfd, newfd)
}

#[inline(always)]
fn dup2_impl(oldfd: i32, newfd: i32) -> i32 {
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
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let dest = dest as *mut u8;
    let src = src as *const u8;

    if n == 0 || (dest as *const u8) == src {
        return dest as *mut c_void;
    }

    let mut offset = 0usize;
    while offset < n {
        let byte = ptr::read_volatile(src.add(offset));
        ptr::write_volatile(dest.add(offset), byte);
        offset += 1;
    }
    dest as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let dest = dest as *mut u8;
    let src = src as *const u8;

    if n == 0 || (dest as *const u8) == src {
        return dest as *mut c_void;
    }

    let dest_addr = dest as usize;
    let src_addr = src as usize;

    if dest_addr < src_addr || dest_addr >= src_addr.saturating_add(n) {
        let mut offset = 0usize;
        while offset < n {
            let byte = ptr::read_volatile(src.add(offset));
            ptr::write_volatile(dest.add(offset), byte);
            offset += 1;
        }
    } else {
        let mut offset = n;
        while offset > 0 {
            offset -= 1;
            let byte = ptr::read_volatile(src.add(offset));
            ptr::write_volatile(dest.add(offset), byte);
        }
    }
    dest as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut c_void, c: i32, n: usize) -> *mut c_void {
    let s = s as *mut u8;

    if n == 0 {
        return s as *mut c_void;
    }

    let value = c as u8;
    let mut offset = 0usize;
    while offset < n {
        ptr::write_volatile(s.add(offset), value);
        offset += 1;
    }
    s as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let a = a as *const u8;
    let b = b as *const u8;

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

#[repr(C)]
struct MemIntrinsicRefs {
    memcpy: unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> *mut c_void,
    memmove: unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> *mut c_void,
    memset: unsafe extern "C" fn(*mut c_void, c_int, usize) -> *mut c_void,
    memcmp: unsafe extern "C" fn(*const c_void, *const c_void, usize) -> c_int,
}

#[used]
static MEM_INTRINSIC_REFS: MemIntrinsicRefs = MemIntrinsicRefs {
    memcpy,
    memmove,
    memset,
    memcmp,
};

#[no_mangle]
#[deprecated(note = "No longer required; memory intrinsics are retained automatically")]
pub extern "C" fn __nrlib_force_mem_link() {
    // Compatibility shim kept for older binaries; no work required now.
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

pub type pthread_key_t = c_uint;

type PthreadDestructor = Option<unsafe extern "C" fn(*mut c_void)>;

#[no_mangle]
pub unsafe extern "C" fn pthread_key_create(
    key: *mut pthread_key_t,
    _destructor: PthreadDestructor,
) -> i32 {
    // let slot = PTHREAD_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    // if slot < 32 {
    //     debug_log_message(b"[nrlib] pthread_key_create\n");
    // }
    if TLS_NEXT_KEY >= MAX_TLS_KEYS {
        set_errno(EINVAL);
        return -1;
    }
    let k = TLS_NEXT_KEY;
    TLS_NEXT_KEY += 1;
    TLS_KEYS[k] = None;
    *key = k as pthread_key_t;
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_key_delete(_key: pthread_key_t) -> i32 {
    let idx = _key as usize;
    if idx < MAX_TLS_KEYS {
        TLS_KEYS[idx] = None;
        0
    } else {
        set_errno(EINVAL);
        -1
    }
}

#[no_mangle]
pub unsafe extern "C" fn pthread_getspecific(key: pthread_key_t) -> *mut c_void {
    // let slot = PTHREAD_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    // if slot < 32 {
    //     debug_log_message(b"[nrlib] pthread_getspecific\n");
    // }
    let idx = key as usize;
    if idx < MAX_TLS_KEYS {
        TLS_KEYS[idx].unwrap_or(ptr::null_mut())
    } else {
        ptr::null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn pthread_setspecific(key: pthread_key_t, value: *const c_void) -> i32 {
    // let slot = PTHREAD_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    // if slot < 32 {
    //     debug_log_message(b"[nrlib] pthread_setspecific\n");
    // }
    let idx = key as usize;
    if idx < MAX_TLS_KEYS {
        TLS_KEYS[idx] = Some(value as *mut c_void);
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
    // CRITICAL: DO NOT log here - causes infinite recursion!
    // Logging from allocator can trigger more allocations.
    // let alloc_slot = ALLOC_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    // if alloc_slot < 64 { ... }
    
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
    // DO NOT log here - may cause recursion if logging allocates
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
    // DO NOT log here - may cause recursion if logging allocates
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
