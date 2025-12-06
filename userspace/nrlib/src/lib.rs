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
    slice,
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
    stderr_write_str, stderr_write_usize, stderr_write_isize, stderr_write_i32,
    stdin_read_line, stdin_read_line_masked, stdin_read_line_noecho,
    stdout_flush, stdout_write_all, stdout_write_fmt, stdout_write_str,
};

// Re-export time functions
pub use time::{get_uptime, sleep};

// Re-export socket types and functions
pub use socket::{
    bind, connect, recvfrom, sendto, socket, socketpair,
    format_ipv4, parse_ipv4, SockAddr, SockAddrIn,
    AF_INET, AF_INET6, AF_UNSPEC, AF_UNIX, AF_LOCAL,
    SOCK_STREAM, SOCK_DGRAM, SOCK_RAW,
    IPPROTO_IP, IPPROTO_ICMP, IPPROTO_TCP, IPPROTO_UDP,
};

// Re-export process control functions and wait status macros
pub use libc_compat::{
    wexitstatus, wifexited, wifsignaled, wtermsig, wifstopped, wstopsig,
    WNOHANG, WUNTRACED, WCONTINUED,
};

// Re-export memory management functions
pub use libc_compat::{
    mmap, mmap64, munmap, mprotect, brk, sbrk,
    PROT_NONE, PROT_READ, PROT_WRITE, PROT_EXEC,
    MAP_SHARED, MAP_PRIVATE, MAP_FIXED, MAP_ANONYMOUS, MAP_ANON,
    MAP_NORESERVE, MAP_POPULATE, MAP_FAILED,
};

// Re-export thread management functions
pub use libc_compat::{
    clone_syscall, gettid, set_tid_address, set_robust_list, get_robust_list, futex,
    CLONE_VM, CLONE_FS, CLONE_FILES, CLONE_SIGHAND, CLONE_THREAD,
    CLONE_SETTLS, CLONE_PARENT_SETTID, CLONE_CHILD_CLEARTID, CLONE_CHILD_SETTID, CLONE_VFORK,
    FUTEX_WAIT_OP, FUTEX_WAKE_OP, FUTEX_PRIVATE, FUTEX_CLOCK_REALTIME_FLAG,
};

// Re-export dynamic linker types and functions
pub use libc_compat::rtld::{
    RTLD_LAZY, RTLD_NOW, RTLD_GLOBAL, RTLD_LOCAL, RTLD_NODELETE,
    RTLD_NOLOAD, RTLD_DEEPBIND, RTLD_DEFAULT, RTLD_NEXT,
    DlInfo, DlError, rtld_init, rtld_is_initialized,
};

// Re-export directory operations
pub use libc_compat::dirent::{
    opendir, readdir, readdir64, readdir_r, closedir, rewinddir,
    seekdir, telldir, dirfd, fdopendir, alphasort, versionsort,
    dirent, dirent64, DIR,
    DT_UNKNOWN, DT_FIFO, DT_CHR, DT_DIR, DT_BLK, DT_REG, DT_LNK, DT_SOCK, DT_WHT,
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
const SYS_STAT: u64 = 4;
const SYS_FSTAT: u64 = 5;
const SYS_LSEEK: u64 = 8;

// Memory management syscalls (Linux-compatible)
const SYS_MMAP: u64 = 9;
const SYS_MPROTECT: u64 = 10;
const SYS_MUNMAP: u64 = 11;
const SYS_BRK: u64 = 12;

const SYS_PIPE: u64 = 22;
const SYS_DUP: u64 = 32;
const SYS_DUP2: u64 = 33;
const SYS_GETPID: u64 = 39;

// Thread management syscalls (Linux-compatible)
const SYS_CLONE: u64 = 56;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAIT4: u64 = 61;
const SYS_FCNTL: u64 = 72;
const SYS_FUTEX: u64 = 98;
const SYS_ARCH_PRCTL: u64 = 158;
const SYS_GETTID: u64 = 186;
const SYS_SET_TID_ADDRESS: u64 = 218;
const SYS_SET_ROBUST_LIST: u64 = 273;
const SYS_GET_ROBUST_LIST: u64 = 274;

// arch_prctl codes
pub const ARCH_SET_GS: i32 = 0x1001;
pub const ARCH_SET_FS: i32 = 0x1002;
pub const ARCH_GET_FS: i32 = 0x1003;
pub const ARCH_GET_GS: i32 = 0x1004;

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
const SYS_NET_SET_DNS: u64 = 260;
const SYS_NET_GET_DNS: u64 = 261;
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
pub(crate) const ESRCH: i32 = 3;
pub(crate) const EAGAIN: i32 = 11;
pub(crate) const ENOMEM: i32 = 12;
pub(crate) const EFAULT: i32 = 14;
pub(crate) const ENOSYS: i32 = 38;
pub(crate) const ENOTTY: i32 = 25;
pub(crate) const ENODEV: i32 = 19;
pub(crate) const ENOSPC: i32 = 28;
pub(crate) const EPERM: i32 = 1;

const MAX_KERNEL_DNS_SERVERS: usize = 3;

// Minimal syscall wrappers that match the userspace convention (int 0x81)
#[inline(always)]
pub fn syscall4(n: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            lateout("rax") ret,
            clobber_abi("sysv64"),
            options(nostack)
        );
    }
    ret
}

fn kernel_net_set_dns_servers(servers: &[u32]) -> Result<(), i32> {
    if servers.len() > MAX_KERNEL_DNS_SERVERS {
        return Err(EINVAL);
    }

    let ptr = if servers.is_empty() {
        core::ptr::null()
    } else {
        servers.as_ptr()
    };

    let ret = syscall3(SYS_NET_SET_DNS, ptr as u64, servers.len() as u64, 0);
    if ret == u64::MAX {
        Err(refresh_errno_from_kernel())
    } else {
        set_errno(0);
        Ok(())
    }
}

fn kernel_net_get_dns_servers(buffer: &mut [u32]) -> Result<usize, i32> {
    let cap = buffer.len().min(MAX_KERNEL_DNS_SERVERS);
    let ptr = if cap == 0 {
        core::ptr::null_mut()
    } else {
        buffer.as_mut_ptr()
    };

    let ret = syscall3(SYS_NET_GET_DNS, ptr as u64, cap as u64, 0);
    if ret == u64::MAX {
        Err(refresh_errno_from_kernel())
    } else {
        set_errno(0);
        Ok(ret as usize)
    }
}

pub(crate) fn get_system_dns_servers(buffer: &mut [u32]) -> Result<usize, i32> {
    kernel_net_get_dns_servers(buffer)
}

pub fn publish_system_dns_servers(servers: &[u32]) -> Result<(), i32> {
    kernel_net_set_dns_servers(servers)
}

#[no_mangle]
pub extern "C" fn net_set_dns_servers(servers: *const u32, count: usize) -> i32 {
    if count > MAX_KERNEL_DNS_SERVERS {
        set_errno(EINVAL);
        return -1;
    }

    if count == 0 {
        return match kernel_net_set_dns_servers(&[]) {
            Ok(()) => 0,
            Err(errno) => {
                set_errno(errno);
                -1
            }
        };
    }

    if servers.is_null() {
        set_errno(EFAULT);
        return -1;
    }

    let slice = unsafe { slice::from_raw_parts(servers, count) };
    match kernel_net_set_dns_servers(slice) {
        Ok(()) => 0,
        Err(errno) => {
            set_errno(errno);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn net_get_dns_servers(out: *mut u32, capacity: usize) -> isize {
    if capacity > MAX_KERNEL_DNS_SERVERS {
        set_errno(EINVAL);
        return -1;
    }

    if capacity == 0 {
        let mut empty: [u32; 0] = [];
        return match kernel_net_get_dns_servers(&mut empty) {
            Ok(_) => 0,
            Err(errno) => {
                set_errno(errno);
                -1
            }
        };
    }

    if out.is_null() {
        set_errno(EFAULT);
        return -1;
    }

    let slice = unsafe { slice::from_raw_parts_mut(out, capacity) };
    match kernel_net_get_dns_servers(slice) {
        Ok(written) => written as isize,
        Err(errno) => {
            set_errno(errno);
            -1
        }
    }
}

#[inline(always)]
pub fn syscall5(n: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            lateout("rax") ret,
            clobber_abi("sysv64"),
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
pub fn syscall6(n: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            in("r9") a6,
            lateout("rax") ret,
            clobber_abi("sysv64"),
            options(nostack)
        );
    }
    ret
}

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

// Debug logging - ENABLED for debugging posix_spawn issue
#[allow(dead_code)]
pub fn debug_log_message(msg: &[u8]) {
    let _ = syscall3(SYS_WRITE, STDERR_FD, msg.as_ptr() as u64, msg.len() as u64);
}

// Debug: output a u64 as hex
#[allow(dead_code)]
pub fn debug_log_hex(value: u64) {
    let hex_chars = b"0123456789abcdef";
    let mut buf = [0u8; 16];
    let mut v = value;
    for i in (0..16).rev() {
        buf[i] = hex_chars[(v & 0xf) as usize];
        v >>= 4;
    }
    let _ = syscall3(SYS_WRITE, STDERR_FD, buf.as_ptr() as u64, 16);
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

/// Matches Linux x86_64 struct stat layout exactly.
/// See: glibc/sysdeps/unix/sysv/linux/x86_64/bits/stat.h
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct stat {
    pub st_dev: u64,      // offset 0
    pub st_ino: u64,      // offset 8
    pub st_nlink: u64,    // offset 16 (u64 on x86_64!)
    pub st_mode: u32,     // offset 24
    pub st_uid: u32,      // offset 28
    pub st_gid: u32,      // offset 32
    pub __pad0: u32,      // offset 36 (padding)
    pub st_rdev: u64,     // offset 40
    pub st_size: i64,     // offset 48
    pub st_blksize: i64,  // offset 56
    pub st_blocks: i64,   // offset 64
    pub st_atime: i64,    // offset 72
    pub st_atime_nsec: i64, // offset 80
    pub st_mtime: i64,    // offset 88
    pub st_mtime_nsec: i64, // offset 96
    pub st_ctime: i64,    // offset 104
    pub st_ctime_nsec: i64, // offset 112
    pub st_reserved: [i64; 3], // offset 120
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

/// arch_prctl - Set/get architecture-specific thread state (TLS)
#[no_mangle]
pub unsafe extern "C" fn arch_prctl(code: i32, addr: u64) -> i32 {
    translate_ret_i32(syscall2(SYS_ARCH_PRCTL, code as u64, addr))
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
// Now uses per-thread TCB storage for proper multi-threading support

const MAX_TLS_KEYS: usize = 128;
// Global key allocation tracking (which keys are in use)
static mut TLS_KEY_USED: [bool; MAX_TLS_KEYS] = [false; MAX_TLS_KEYS];
static mut TLS_NEXT_KEY: usize = 0;
static mut TLS_DESTRUCTORS: [Option<unsafe extern "C" fn(*mut c_void)>; MAX_TLS_KEYS] = [None; MAX_TLS_KEYS];

// Fallback global storage for when TCB is not available (early init)
static mut TLS_FALLBACK: [*mut c_void; MAX_TLS_KEYS] = [ptr::null_mut(); MAX_TLS_KEYS];

pub type pthread_key_t = c_uint;

type PthreadDestructor = Option<unsafe extern "C" fn(*mut c_void)>;

#[no_mangle]
pub unsafe extern "C" fn pthread_key_create(
    key: *mut pthread_key_t,
    destructor: PthreadDestructor,
) -> i32 {
    // Find a free key slot
    if TLS_NEXT_KEY >= MAX_TLS_KEYS {
        // Try to find a deleted slot
        for i in 0..MAX_TLS_KEYS {
            if !TLS_KEY_USED[i] {
                TLS_KEY_USED[i] = true;
                TLS_DESTRUCTORS[i] = destructor;
                *key = i as pthread_key_t;
                return 0;
            }
        }
        set_errno(EAGAIN);
        return EAGAIN;
    }
    let k = TLS_NEXT_KEY;
    TLS_NEXT_KEY += 1;
    TLS_KEY_USED[k] = true;
    TLS_DESTRUCTORS[k] = destructor;
    *key = k as pthread_key_t;
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_key_delete(key: pthread_key_t) -> i32 {
    let idx = key as usize;
    if idx < MAX_TLS_KEYS && TLS_KEY_USED[idx] {
        TLS_KEY_USED[idx] = false;
        TLS_DESTRUCTORS[idx] = None;
        0
    } else {
        set_errno(EINVAL);
        EINVAL
    }
}

#[no_mangle]
pub unsafe extern "C" fn pthread_getspecific(key: pthread_key_t) -> *mut c_void {
    let idx = key as usize;
    if idx >= MAX_TLS_KEYS {
        return ptr::null_mut();
    }
    
    // Try to get from per-thread TCB first
    let value = libc_compat::pthread::get_thread_tls_data(idx);
    if !value.is_null() {
        return value;
    }
    
    // Fallback to global storage (for early init or if TCB not set)
    TLS_FALLBACK[idx]
}

#[no_mangle]
pub unsafe extern "C" fn pthread_setspecific(key: pthread_key_t, value: *const c_void) -> i32 {
    let idx = key as usize;
    if idx >= MAX_TLS_KEYS {
        set_errno(EINVAL);
        return EINVAL;
    }
    
    // Try to set in per-thread TCB first
    if libc_compat::pthread::set_thread_tls_data(idx, value as *mut c_void) {
        return 0;
    }
    
    // Fallback to global storage (for early init or if TCB not set)
    TLS_FALLBACK[idx] = value as *mut c_void;
    0
}

// Allocator support for std::alloc::System ----------------------------------
// std expects malloc/free/realloc/calloc
//
// This allocator uses sbrk() to dynamically expand the heap from the kernel.
// It implements a free list allocator with block coalescing for efficient
// memory reuse, similar to dlmalloc/glibc malloc.

const DEFAULT_ALIGNMENT: usize = 16;
const MIN_BLOCK_SIZE: usize = 32; // Minimum block size (header + at least 16 bytes)
const SBRK_INCREMENT: usize = 64 * 1024; // Request 64KB at a time

/// Block header for free list allocator
/// When allocated: stores size and flags
/// When free: stores size, flags, and free list pointers
#[repr(C)]
struct BlockHeader {
    /// Size of the block (including header) | flags in low bits
    /// Bit 0: is_allocated (1 = allocated, 0 = free)
    /// Bit 1: prev_allocated (1 = previous block is allocated)
    size_flags: usize,
}

/// Free block structure (only valid when block is free)
#[repr(C)]
struct FreeBlock {
    header: BlockHeader,
    /// Next free block in the free list
    next_free: *mut FreeBlock,
    /// Previous free block in the free list
    prev_free: *mut FreeBlock,
}

/// Footer for free blocks (stores size for backward coalescing)
#[repr(C)]
struct BlockFooter {
    size: usize,
}

const HEADER_SIZE: usize = core::mem::size_of::<BlockHeader>();
const FOOTER_SIZE: usize = core::mem::size_of::<BlockFooter>();
const FREE_BLOCK_MIN: usize = core::mem::size_of::<FreeBlock>() + FOOTER_SIZE;

// Flag bits
const FLAG_ALLOCATED: usize = 0x1;
const FLAG_PREV_ALLOCATED: usize = 0x2;
const SIZE_MASK: usize = !0x3;

/// Heap state
static mut HEAP_START: usize = 0;
static mut HEAP_END: usize = 0;
/// Head of the free list (sorted by address for coalescing)
static mut FREE_LIST_HEAD: *mut FreeBlock = ptr::null_mut();
/// Total allocated bytes (for statistics)
static mut TOTAL_ALLOCATED: usize = 0;
/// Total free bytes
static mut TOTAL_FREE: usize = 0;

impl BlockHeader {
    #[inline(always)]
    fn size(&self) -> usize {
        self.size_flags & SIZE_MASK
    }

    #[inline(always)]
    fn is_allocated(&self) -> bool {
        (self.size_flags & FLAG_ALLOCATED) != 0
    }

    #[inline(always)]
    fn is_prev_allocated(&self) -> bool {
        (self.size_flags & FLAG_PREV_ALLOCATED) != 0
    }

    #[inline(always)]
    fn set_size_flags(&mut self, size: usize, allocated: bool, prev_allocated: bool) {
        self.size_flags = (size & SIZE_MASK)
            | (if allocated { FLAG_ALLOCATED } else { 0 })
            | (if prev_allocated { FLAG_PREV_ALLOCATED } else { 0 });
    }
}

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

/// Expand heap using sbrk() syscall
unsafe fn expand_heap(min_size: usize) -> bool {
    // Calculate how much to request (round up to SBRK_INCREMENT)
    let increment = ((min_size + SBRK_INCREMENT - 1) / SBRK_INCREMENT) * SBRK_INCREMENT;
    
    let result = libc_compat::sbrk(increment as isize);
    if result == (-1isize) as *mut c_void {
        return false;
    }
    
    let new_block_start = result as usize;
    
    // If this is first allocation, initialize HEAP_START
    if HEAP_START == 0 {
        HEAP_START = new_block_start;
        HEAP_END = new_block_start;
    }
    
    // Create a new free block from the expanded region
    let block = new_block_start as *mut FreeBlock;
    let block_size = increment;
    
    // Set up the block header
    (*block).header.set_size_flags(block_size, false, true); // Free, prev allocated (boundary)
    
    // Set up the footer
    let footer = (new_block_start + block_size - FOOTER_SIZE) as *mut BlockFooter;
    (*footer).size = block_size;
    
    // Initialize free list pointers
    (*block).next_free = ptr::null_mut();
    (*block).prev_free = ptr::null_mut();
    
    // Add to free list
    add_to_free_list(block);
    
    HEAP_END = new_block_start + increment;
    TOTAL_FREE += block_size;
    
    true
}

/// Add a free block to the free list (address-ordered for coalescing)
unsafe fn add_to_free_list(block: *mut FreeBlock) {
    if FREE_LIST_HEAD.is_null() {
        FREE_LIST_HEAD = block;
        (*block).next_free = ptr::null_mut();
        (*block).prev_free = ptr::null_mut();
        return;
    }
    
    // Find insertion point (address-ordered)
    let mut prev: *mut FreeBlock = ptr::null_mut();
    let mut curr = FREE_LIST_HEAD;
    
    while !curr.is_null() && (curr as usize) < (block as usize) {
        prev = curr;
        curr = (*curr).next_free;
    }
    
    // Insert between prev and curr
    (*block).next_free = curr;
    (*block).prev_free = prev;
    
    if !curr.is_null() {
        (*curr).prev_free = block;
    }
    
    if prev.is_null() {
        FREE_LIST_HEAD = block;
    } else {
        (*prev).next_free = block;
    }
}

/// Remove a block from the free list
unsafe fn remove_from_free_list(block: *mut FreeBlock) {
    let prev = (*block).prev_free;
    let next = (*block).next_free;
    
    if !prev.is_null() {
        (*prev).next_free = next;
    } else {
        FREE_LIST_HEAD = next;
    }
    
    if !next.is_null() {
        (*next).prev_free = prev;
    }
}

/// Try to coalesce a free block with adjacent free blocks
unsafe fn coalesce(block: *mut FreeBlock) -> *mut FreeBlock {
    let header = &mut (*block).header;
    let mut current_size = header.size();
    let mut result = block;
    
    // Try to coalesce with next block
    let next_block = (block as usize + current_size) as *mut BlockHeader;
    if (next_block as usize) < HEAP_END {
        if !(*next_block).is_allocated() {
            // Next block is free - merge
            let next_free = next_block as *mut FreeBlock;
            let next_size = (*next_block).size();
            
            // Remove next block from free list
            remove_from_free_list(next_free);
            
            // Extend current block
            current_size += next_size;
            header.set_size_flags(current_size, false, header.is_prev_allocated());
            
            // Update footer
            let footer = (result as usize + current_size - FOOTER_SIZE) as *mut BlockFooter;
            (*footer).size = current_size;
        }
    }
    
    // Try to coalesce with previous block
    if !header.is_prev_allocated() && (block as usize) > HEAP_START {
        // Read previous block's footer to get its size
        let prev_footer = (block as usize - FOOTER_SIZE) as *mut BlockFooter;
        let prev_size = (*prev_footer).size;
        let prev_block = (block as usize - prev_size) as *mut FreeBlock;
        
        // Remove current block from free list (we'll re-add the merged block)
        remove_from_free_list(result);
        
        // Extend previous block
        let new_size = prev_size + current_size;
        (*prev_block).header.set_size_flags(new_size, false, (*prev_block).header.is_prev_allocated());
        
        // Update footer
        let footer = (prev_block as usize + new_size - FOOTER_SIZE) as *mut BlockFooter;
        (*footer).size = new_size;
        
        result = prev_block;
        // Note: prev_block is already in the free list
    }
    
    result
}

/// Find a free block that fits the requested size using first-fit
unsafe fn find_free_block(size: usize) -> *mut FreeBlock {
    let mut curr = FREE_LIST_HEAD;
    
    while !curr.is_null() {
        if (*curr).header.size() >= size {
            return curr;
        }
        curr = (*curr).next_free;
    }
    
    ptr::null_mut()
}

/// Split a block if it's larger than needed
unsafe fn split_block(block: *mut FreeBlock, needed_size: usize) {
    let block_size = (*block).header.size();
    let remaining = block_size - needed_size;
    
    // Only split if remaining is large enough for a free block
    if remaining >= FREE_BLOCK_MIN {
        // Create a new free block from the remainder
        let new_block = (block as usize + needed_size) as *mut FreeBlock;
        (*new_block).header.set_size_flags(remaining, false, true); // Free, prev allocated
        
        // Set up footer
        let footer = (new_block as usize + remaining - FOOTER_SIZE) as *mut BlockFooter;
        (*footer).size = remaining;
        
        // Update original block size
        (*block).header.set_size_flags(
            needed_size,
            (*block).header.is_allocated(),
            (*block).header.is_prev_allocated(),
        );
        
        // Add new block to free list
        add_to_free_list(new_block);
        
        TOTAL_FREE += remaining;
    }
}

unsafe fn alloc_internal(size: usize, align: usize) -> *mut c_void {
    // CRITICAL: DO NOT log here - causes infinite recursion!
    
    if size == 0 {
        set_errno(0);
        return ptr::null_mut();
    }

    if align == 0 {
        set_errno(EINVAL);
        return ptr::null_mut();
    }

    let requested_align = align.max(DEFAULT_ALIGNMENT);
    if !is_power_of_two(requested_align) {
        set_errno(EINVAL);
        return ptr::null_mut();
    }

    // Calculate total size needed
    // Size = header + data (aligned) + possible padding
    let data_size = match align_up(size, requested_align) {
        Some(s) => s,
        None => {
            set_errno(ENOMEM);
            return ptr::null_mut();
        }
    };
    
    let total_size = match data_size.checked_add(HEADER_SIZE) {
        Some(s) => s.max(FREE_BLOCK_MIN), // Minimum size for free block
        None => {
            set_errno(ENOMEM);
            return ptr::null_mut();
        }
    };

    // Initialize heap on first allocation
    if HEAP_START == 0 {
        if !expand_heap(SBRK_INCREMENT.max(total_size)) {
            // Print error to stderr (safe - doesn't allocate)
            let msg = b"memory allocation of ";
            let _ = syscall3(SYS_WRITE, 2, msg.as_ptr() as u64, msg.len() as u64);
            let mut buf = [0u8; 20];
            let len = format_usize(size, &mut buf);
            let _ = syscall3(SYS_WRITE, 2, buf.as_ptr() as u64, len as u64);
            let msg2 = b" bytes failed\n";
            let _ = syscall3(SYS_WRITE, 2, msg2.as_ptr() as u64, msg2.len() as u64);
            set_errno(ENOMEM);
            return ptr::null_mut();
        }
    }

    // Find a suitable free block
    let mut block = find_free_block(total_size);
    
    // If no suitable block found, expand heap
    if block.is_null() {
        if !expand_heap(total_size) {
            set_errno(ENOMEM);
            return ptr::null_mut();
        }
        block = find_free_block(total_size);
        if block.is_null() {
            set_errno(ENOMEM);
            return ptr::null_mut();
        }
    }
    
    let block_size = (*block).header.size();
    
    // Remove from free list
    remove_from_free_list(block);
    TOTAL_FREE -= block_size;
    
    // Split if necessary
    split_block(block, total_size);
    
    // Mark as allocated
    let final_size = (*block).header.size();
    (*block).header.set_size_flags(final_size, true, (*block).header.is_prev_allocated());
    
    // Mark next block's prev_allocated flag
    let next_header = (block as usize + final_size) as *mut BlockHeader;
    if (next_header as usize) < HEAP_END {
        let next_flags = (*next_header).size_flags;
        (*next_header).size_flags = next_flags | FLAG_PREV_ALLOCATED;
    }
    
    TOTAL_ALLOCATED += final_size;
    
    // Return pointer to data (after header)
    let data_ptr = (block as usize + HEADER_SIZE) as *mut c_void;
    set_errno(0);
    data_ptr
}

/// Format usize to decimal string (no allocation)
fn format_usize(mut n: usize, buf: &mut [u8; 20]) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut i = 20;
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let len = 20 - i;
    buf.copy_within(i..20, 0);
    len
}

unsafe fn allocation_size(ptr: *mut c_void) -> Option<usize> {
    if ptr.is_null() {
        return None;
    }

    let data_ptr = ptr as usize;

    // Check if pointer is within our heap
    if HEAP_START == 0 || data_ptr < HEAP_START + HEADER_SIZE || data_ptr >= HEAP_END {
        return None;
    }

    let header = (data_ptr - HEADER_SIZE) as *mut BlockHeader;
    if (*header).is_allocated() {
        Some((*header).size() - HEADER_SIZE)
    } else {
        None
    }
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
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    // DO NOT log here - may cause recursion
    if ptr.is_null() {
        return;
    }
    
    let data_ptr = ptr as usize;
    
    // Validate pointer is within our heap
    if HEAP_START == 0 || data_ptr < HEAP_START + HEADER_SIZE || data_ptr >= HEAP_END {
        return; // Invalid pointer, silently ignore (like glibc)
    }
    
    // Get block header
    let header = (data_ptr - HEADER_SIZE) as *mut BlockHeader;
    
    // Check if block is actually allocated
    if !(*header).is_allocated() {
        return; // Double free, silently ignore
    }
    
    let block_size = (*header).size();
    
    // Mark as free
    (*header).set_size_flags(block_size, false, (*header).is_prev_allocated());
    
    // Set up footer for backward coalescing
    let footer = (header as usize + block_size - FOOTER_SIZE) as *mut BlockFooter;
    (*footer).size = block_size;
    
    // Update next block's prev_allocated flag
    let next_header = (header as usize + block_size) as *mut BlockHeader;
    if (next_header as usize) < HEAP_END {
        let next_flags = (*next_header).size_flags;
        (*next_header).size_flags = next_flags & !FLAG_PREV_ALLOCATED;
    }
    
    TOTAL_ALLOCATED -= block_size;
    TOTAL_FREE += block_size;
    
    // Add to free list
    let block = header as *mut FreeBlock;
    add_to_free_list(block);
    
    // Try to coalesce with adjacent free blocks
    coalesce(block);
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
    
    if old_size == 0 {
        // Invalid pointer
        set_errno(EINVAL);
        return ptr::null_mut();
    }

    // If shrinking significantly, we could split the block
    // For now, just keep the same block if new_size <= old_size
    if new_size <= old_size {
        set_errno(0);
        return ptr;
    }
    
    // Try to expand in place by checking if next block is free
    let header = (ptr as usize - HEADER_SIZE) as *mut BlockHeader;
    let block_size = (*header).size();
    let next_header = (header as usize + block_size) as *mut BlockHeader;
    
    if (next_header as usize) < HEAP_END && !(*next_header).is_allocated() {
        let next_size = (*next_header).size();
        let combined_size = block_size + next_size;
        let needed_size = new_size + HEADER_SIZE;
        
        if combined_size >= needed_size {
            // Can expand in place!
            let next_block = next_header as *mut FreeBlock;
            remove_from_free_list(next_block);
            TOTAL_FREE -= next_size;
            
            // Update header with new size
            (*header).set_size_flags(combined_size, true, (*header).is_prev_allocated());
            TOTAL_ALLOCATED += next_size;
            
            // Update next block's prev_allocated flag
            let new_next = (header as usize + combined_size) as *mut BlockHeader;
            if (new_next as usize) < HEAP_END {
                let flags = (*new_next).size_flags;
                (*new_next).size_flags = flags | FLAG_PREV_ALLOCATED;
            }
            
            // Optionally split if there's excess
            // (Skip for simplicity - could add later)
            
            set_errno(0);
            return ptr;
        }
    }

    // Fall back to allocate + copy + free
    let new_ptr = alloc_internal(new_size, DEFAULT_ALIGNMENT);
    if new_ptr.is_null() {
        return ptr::null_mut();
    }

    let copy_len = cmp::min(old_size, new_size);
    ptr::copy_nonoverlapping(ptr as *const u8, new_ptr as *mut u8, copy_len);
    
    free(ptr);
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

/// getrandom syscall number (Linux-compatible)
const SYS_GETRANDOM: usize = 318;

/// getrandom flags
pub const GRND_NONBLOCK: u32 = 0x0001;
pub const GRND_RANDOM: u32 = 0x0002;
pub const GRND_INSECURE: u32 = 0x0004;

/// Get random bytes from the kernel via getrandom syscall
#[no_mangle]
pub unsafe extern "C" fn getrandom(buf: *mut c_void, buflen: usize, flags: u32) -> isize {
    if buf.is_null() || buflen == 0 {
        set_errno(EINVAL);
        return -1;
    }

    let ret: isize;
    core::arch::asm!(
        "syscall",
        inout("rax") SYS_GETRANDOM => ret,
        in("rdi") buf,
        in("rsi") buflen,
        in("rdx") flags,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );

    if ret < 0 {
        set_errno((-ret) as i32);
        -1
    } else {
        set_errno(0);
        ret
    }
}

/// Fill buffer with random bytes (arc4random_buf compatible)
/// This is a cryptographically secure random number generator
#[no_mangle]
pub unsafe extern "C" fn arc4random_buf(buf: *mut c_void, nbytes: usize) {
    if buf.is_null() || nbytes == 0 {
        return;
    }

    // Use getrandom syscall with blocking mode
    let mut filled = 0usize;
    let bytes = buf as *mut u8;
    
    while filled < nbytes {
        let to_fill = nbytes - filled;
        let result = getrandom(bytes.add(filled) as *mut c_void, to_fill, 0);
        
        if result > 0 {
            filled += result as usize;
        } else {
            // If syscall fails, fall back to software PRNG for remaining bytes
            // This should be rare if ever happens
            fallback_random(bytes.add(filled), nbytes - filled);
            break;
        }
    }
}

/// arc4random - return a random 32-bit value
#[no_mangle]
pub unsafe extern "C" fn arc4random() -> u32 {
    let mut buf = [0u8; 4];
    arc4random_buf(buf.as_mut_ptr() as *mut c_void, 4);
    u32::from_ne_bytes(buf)
}

/// arc4random_uniform - return a random value in [0, upper_bound)
#[no_mangle]
pub unsafe extern "C" fn arc4random_uniform(upper_bound: u32) -> u32 {
    if upper_bound < 2 {
        return 0;
    }
    
    // Rejection sampling to avoid modulo bias
    let min = (-(upper_bound as i32) as u32) % upper_bound;
    
    loop {
        let r = arc4random();
        if r >= min {
            return r % upper_bound;
        }
    }
}

/// Fallback software PRNG (used only if kernel syscall fails)
unsafe fn fallback_random(buf: *mut u8, len: usize) {
    static mut SEED: u64 = 0x123456789abcdef0;
    
    // Mix in some entropy from TSC
    let tsc: u64;
    core::arch::asm!(
        "rdtsc",
        "shl rdx, 32",
        "or rax, rdx",
        out("rax") tsc,
        out("rdx") _,
        options(nomem, nostack)
    );
    SEED ^= tsc;
    
    let bytes = core::slice::from_raw_parts_mut(buf, len);
    for byte in bytes {
        SEED = SEED.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = (SEED >> 56) as u8;
    }
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
