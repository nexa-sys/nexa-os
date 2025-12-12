//! Syscall subsystem
//!
//! This module provides the system call interface for NexaOS.
//! It is organized into submodules by functionality:
//!
//! - `numbers`: Syscall number constants
//! - `types`: Shared types and data structures
//! - `exec`: Exec context for syscall/execve communication
//! - `file`: File I/O syscalls (read, write, open, close, stat, etc.)
//! - `process`: Process management syscalls (fork, execve, exit, wait4, etc.)
//! - `signal`: Signal handling syscalls (sigaction, sigprocmask)
//! - `fd`: File descriptor syscalls (dup, dup2, pipe)
//! - `ipc`: IPC syscalls (ipc_create, ipc_send, ipc_recv)
//! - `user`: User management syscalls (user_add, user_login, etc.)
//! - `network`: Network socket syscalls (socket, bind, connect, sendto, recvfrom)
//! - `time`: Time related syscalls (clock_gettime, nanosleep, sched_yield)
//! - `system`: System management syscalls (reboot, shutdown, runlevel, mount)
//! - `uefi`: UEFI compatibility syscalls
//! - `swap`: Swap management syscalls (swapon, swapoff)

use crate::kinfo;

mod exec;
mod fd;
mod file;
mod ioctl;
mod ipc;
mod kmod;
mod memory;
mod memory_advanced;
pub mod memory_vma;
mod network;
mod numbers;
mod process;
mod signal;
pub mod swap;
mod system;
mod thread;
mod time;
mod types;
mod uefi;
mod user;

// Re-export syscall numbers for external use
pub use numbers::*;

/// Kernel-internal helper: set CLOCK_REALTIME base (microseconds since Unix epoch).
///
/// This forwards to the time syscall implementation while keeping `time` private.
pub fn set_realtime_us(realtime_us: i64) {
    time::set_system_time_offset(realtime_us);
}

// Re-export exec context function for assembly
pub use exec::get_exec_context;

// Internal imports
use crate::posix;
use crate::process::ProcessState;
use crate::scheduler;
use crate::uefi_compat::{
    BlockDescriptor, CompatCounts, HidInputDescriptor, NetworkDescriptor, UsbHostDescriptor,
};
use core::arch::global_asm;
use nexa_boot_info::FramebufferInfo;

// Re-export types for syscall_dispatch
use types::*;

// Import all syscall implementations
use fd::{dup, dup2, pipe};
use file::{
    close, fcntl, fstat, get_errno, list_files, lseek, open, pread64, pwrite64, read, readlink,
    readlinkat, readv, stat, write, writev,
};
use ioctl::ioctl;
use ipc::{ipc_create, ipc_recv, ipc_send};
use kmod::{delete_module, init_module, query_module};
use memory::{mmap, mprotect, munmap};
use memory_advanced::{
    getrlimit, madvise, mincore, mlock, mlockall, mremap, msync, munlock, munlockall, prlimit64,
    setrlimit, RLimit,
};
use memory_vma::brk_vma as brk; // Use VMA-based brk for per-process heap tracking
use network::{
    bind, connect, get_dns_servers, recvfrom, sendto, set_dns_servers, setsockopt, socket,
    socketpair,
};
use process::{execve, exit, fork, getppid, kill, wait4};
use signal::{sigaction, sigprocmask};
use system::{chroot, mount, pivot_root, reboot, runlevel, shutdown, syslog, umount};
use thread::{arch_prctl, clone, futex, get_robust_list, gettid, set_robust_list, set_tid_address};
use time::{clock_gettime, clock_settime, nanosleep, sched_yield, sys_times, Tms};
use uefi::{
    uefi_get_block_info, uefi_get_counts, uefi_get_fb_info, uefi_get_hid_info, uefi_get_net_info,
    uefi_get_usb_info, uefi_map_net_mmio, uefi_map_usb_mmio,
};
use user::{user_add, user_info, user_list, user_login, user_logout};

// Re-export file descriptor tracking and cleanup functions
pub use file::{close_all_fds_for_process, mark_fd_closed, mark_fd_open};

// Re-export internal file APIs for kernel subsystems (loop devices, etc.)
pub use file::{get_file_path, get_file_size, pread_internal, pwrite_internal};

// Re-export user buffer validation for kernel subsystems
pub use types::user_buffer_in_range;

// Re-export thread-related functions for internal kernel use
pub use thread::{futex_wake_internal, FUTEX_WAKE};

/// Main syscall dispatcher
#[no_mangle]
pub extern "C" fn syscall_dispatch(
    nr: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    syscall_return_addr: u64,
) -> u64 {
    // Debug: log CR3 at syscall entry for execve
    if nr == SYS_EXECVE {
        let current_cr3: u64;
        let kernel_cr3 = crate::paging::kernel_pml4_phys();
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) current_cr3, options(nomem, nostack));
        }
        kinfo!(
            "[syscall_dispatch] ENTRY: nr={}, CR3={:#x}, kernel_CR3={:#x}",
            nr,
            current_cr3,
            kernel_cr3
        );
    }

    let (user_rsp, user_rflags) = unsafe {
        let mut rsp_out: u64;
        let mut rflags_out: u64;
        core::arch::asm!(
            "mov {0}, gs:[0]",
            "mov {1}, gs:[64]",
            out(reg) rsp_out,
            out(reg) rflags_out,
            options(nostack, preserves_flags)
        );
        (rsp_out, rflags_out)
    };

    scheduler::update_current_user_context(syscall_return_addr, user_rsp, user_rflags);

    let result = match nr {
        SYS_WRITE => write(arg1, arg2, arg3),
        SYS_READ => read(arg1, arg2 as *mut u8, arg3 as usize),
        SYS_OPEN => open(arg1 as *const u8, arg2 as u64, arg3 as u64),
        SYS_CLOSE => close(arg1),
        SYS_IOCTL => ioctl(arg1, arg2, arg3),
        SYS_STAT => stat(arg1 as *const u8, arg2 as usize, arg3 as *mut posix::Stat),
        SYS_FSTAT => fstat(arg1, arg2 as *mut posix::Stat),
        SYS_READLINK => readlink(arg1 as *const u8, arg2 as *mut u8, arg3 as usize),
        SYS_READLINKAT => {
            // readlinkat needs 4 args: dirfd, pathname, buf, bufsiz
            let arg4 = unsafe {
                let mut r10_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    out(reg) r10_val,
                    options(nostack, preserves_flags)
                );
                r10_val
            };
            readlinkat(arg1 as i32, arg2 as *const u8, arg3 as *mut u8, arg4 as usize)
        }
        SYS_LSEEK => lseek(arg1, arg2 as i64, arg3),
        SYS_PREAD64 => {
            // pread64 needs 4 args: fd, buf, count, offset
            let arg4 = unsafe {
                let mut r10_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    out(reg) r10_val,
                    options(nostack, preserves_flags)
                );
                r10_val
            };
            pread64(arg1, arg2 as *mut u8, arg3 as usize, arg4 as i64)
        }
        SYS_PWRITE64 => {
            // pwrite64 needs 4 args: fd, buf, count, offset
            let arg4 = unsafe {
                let mut r10_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    out(reg) r10_val,
                    options(nostack, preserves_flags)
                );
                r10_val
            };
            pwrite64(arg1, arg2, arg3, arg4 as i64)
        }
        SYS_READV => readv(arg1, arg2 as *const IoVec, arg3 as i32),
        SYS_WRITEV => writev(arg1, arg2 as *const IoVec, arg3 as i32),
        SYS_MMAP => {
            // mmap needs 6 args: addr, length, prot, flags, fd, offset
            let (arg4, arg5, arg6) = unsafe {
                let mut r10_val: u64;
                let mut r8_val: u64;
                let mut r9_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    "mov {1}, gs:[40]",
                    "mov {2}, gs:[48]",
                    out(reg) r10_val,
                    out(reg) r8_val,
                    out(reg) r9_val,
                    options(nostack, preserves_flags)
                );
                (r10_val, r8_val, r9_val)
            };
            mmap(arg1, arg2, arg3, arg4, arg5 as i64, arg6)
        }
        SYS_MPROTECT => mprotect(arg1, arg2, arg3),
        SYS_MUNMAP => munmap(arg1, arg2),
        SYS_BRK => brk(arg1),
        SYS_FCNTL => fcntl(arg1, arg2, arg3),
        SYS_PIPE => pipe(arg1 as *mut [i32; 2]),
        SYS_DUP => dup(arg1),
        SYS_DUP2 => dup2(arg1, arg2),
        SYS_CLONE => {
            // clone needs 5 args: flags, stack, parent_tid, child_tid, tls
            let (arg4, arg5) = unsafe {
                let mut r10_val: u64;
                let mut r8_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    "mov {1}, gs:[40]",
                    out(reg) r10_val,
                    out(reg) r8_val,
                    options(nostack, preserves_flags)
                );
                (r10_val, r8_val)
            };
            clone(arg1, arg2, arg3, arg4, arg5, syscall_return_addr)
        }
        SYS_FORK => fork(syscall_return_addr),
        SYS_EXECVE => {
            // Debug: log current CR3
            let current_cr3: u64;
            unsafe {
                core::arch::asm!("mov {}, cr3", out(reg) current_cr3, options(nomem, nostack));
            }
            kinfo!(
                "[syscall_dispatch] SYS_EXECVE called, arg1={:#x}, CR3={:#x}",
                arg1,
                current_cr3
            );
            execve(
                arg1 as *const u8,
                arg2 as *const *const u8,
                arg3 as *const *const u8,
            )
        }
        SYS_EXIT => exit(arg1 as i32),
        SYS_WAIT4 => wait4(arg1 as i64, arg2 as *mut i32, arg3 as i32, 0 as *mut u8),
        SYS_KILL => kill(arg1 as i64, arg2),
        SYS_SIGACTION => sigaction(arg1, arg2 as *const u8, arg3 as *mut u8),
        SYS_SIGPROCMASK => sigprocmask(arg1 as i32, arg2 as *const u64, arg3 as *mut u64),
        SYS_GETPID => crate::scheduler::get_current_pid().unwrap_or(0),
        SYS_GETPPID => getppid(),
        SYS_GETTID => gettid(),
        SYS_FUTEX => {
            // futex needs 6 args: uaddr, op, val, timeout, uaddr2, val3
            let (arg4, arg5, arg6) = unsafe {
                let mut r10_val: u64;
                let mut r8_val: u64;
                let mut r9_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    "mov {1}, gs:[40]",
                    "mov {2}, gs:[48]",
                    out(reg) r10_val,
                    out(reg) r8_val,
                    out(reg) r9_val,
                    options(nostack, preserves_flags)
                );
                (r10_val, r8_val, r9_val)
            };
            futex(arg1, arg2 as i32, arg3 as i32, arg4, arg5, arg6 as i32)
        }
        SYS_SET_TID_ADDRESS => set_tid_address(arg1),
        SYS_SET_ROBUST_LIST => set_robust_list(arg1, arg2 as usize),
        SYS_GET_ROBUST_LIST => get_robust_list(arg1, arg2, arg3),
        SYS_ARCH_PRCTL => arch_prctl(arg1 as i32, arg2),
        SYS_SCHED_YIELD => sched_yield(),
        SYS_CLOCK_GETTIME => clock_gettime(arg1 as i32, arg2 as *mut TimeSpec),
        SYS_CLOCK_SETTIME => clock_settime(arg1 as i32, arg2 as *const TimeSpec),
        SYS_NANOSLEEP => nanosleep(arg1 as *const TimeSpec, arg2 as *mut TimeSpec),
        SYS_TIMES => sys_times(arg1 as *mut Tms),
        SYS_LIST_FILES => list_files(
            arg1 as *mut u8,
            arg2 as usize,
            arg3 as *const ListDirRequest,
        ),
        SYS_GETERRNO => get_errno(),
        SYS_IPC_CREATE => ipc_create(),
        SYS_IPC_SEND => ipc_send(arg1 as *const IpcTransferRequest),
        SYS_IPC_RECV => ipc_recv(arg1 as *const IpcTransferRequest),
        SYS_USER_ADD => user_add(arg1 as *const UserRequest),
        SYS_USER_LOGIN => user_login(arg1 as *const UserRequest),
        SYS_USER_INFO => user_info(arg1 as *mut UserInfoReply),
        SYS_USER_LIST => user_list(arg1 as *mut u8, arg2 as usize),
        SYS_USER_LOGOUT => user_logout(),
        SYS_SOCKET => socket(arg1 as i32, arg2 as i32, arg3 as i32),
        SYS_SOCKETPAIR => {
            // socketpair needs 4 args: domain, type, protocol, sv
            let arg4 = unsafe {
                let mut r10_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    out(reg) r10_val,
                    options(nostack, preserves_flags)
                );
                r10_val
            };
            socketpair(arg1 as i32, arg2 as i32, arg3 as i32, arg4 as *mut [i32; 2])
        }
        SYS_BIND => bind(arg1, arg2 as *const SockAddr, arg3 as u32),
        SYS_NET_SET_DNS => set_dns_servers(arg1 as *const u32, arg2 as u32),
        SYS_NET_GET_DNS => get_dns_servers(arg1 as *mut u32, arg2 as u32),
        SYS_SENDTO => {
            // sendto needs 6 args: sockfd, buf, len, flags, dest_addr, addrlen
            let (arg4, arg5, arg6) = unsafe {
                let mut r10_val: u64;
                let mut r8_val: u64;
                let mut r9_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    "mov {1}, gs:[40]",
                    "mov {2}, gs:[48]",
                    out(reg) r10_val,
                    out(reg) r8_val,
                    out(reg) r9_val,
                    options(nostack, preserves_flags)
                );
                (r10_val, r8_val, r9_val)
            };
            sendto(
                arg1,
                arg2 as *const u8,
                arg3 as usize,
                arg4 as i32,
                arg5 as *const SockAddr,
                arg6 as u32,
            )
        }
        SYS_RECVFROM => {
            kinfo!("[syscall] SYS_RECVFROM called: arg1={}", arg1);
            // recvfrom needs 6 args: sockfd, buf, len, flags, src_addr, addrlen
            let (arg4, arg5, arg6) = unsafe {
                let mut r10_val: u64;
                let mut r8_val: u64;
                let mut r9_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    "mov {1}, gs:[40]",
                    "mov {2}, gs:[48]",
                    out(reg) r10_val,
                    out(reg) r8_val,
                    out(reg) r9_val,
                    options(nostack, preserves_flags)
                );
                (r10_val, r8_val, r9_val)
            };
            recvfrom(
                arg1,
                arg2 as *mut u8,
                arg3 as usize,
                arg4 as i32,
                arg5 as *mut SockAddr,
                arg6 as *mut u32,
            )
        }
        SYS_CONNECT => connect(arg1, arg2 as *const SockAddr, arg3 as u32),
        SYS_SETSOCKOPT => {
            // setsockopt needs 5 args: sockfd, level, optname, optval, optlen
            let (arg4, arg5) = unsafe {
                let mut r10_val: u64;
                let mut r8_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    "mov {1}, gs:[40]",
                    out(reg) r10_val,
                    out(reg) r8_val,
                    options(nostack, preserves_flags)
                );
                (r10_val, r8_val)
            };
            setsockopt(
                arg1,
                arg2 as i32,
                arg3 as i32,
                arg4 as *const u8,
                arg5 as u32,
            )
        }
        SYS_REBOOT => reboot(arg1 as i32),
        SYS_SHUTDOWN => shutdown(),
        SYS_RUNLEVEL => runlevel(arg1 as i32),
        // Advanced memory management syscalls
        SYS_MREMAP => {
            // mremap needs 5 args: old_addr, old_size, new_size, flags, new_addr
            let (arg4, arg5) = unsafe {
                let mut r10_val: u64;
                let mut r8_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    "mov {1}, gs:[40]",
                    out(reg) r10_val,
                    out(reg) r8_val,
                    options(nostack, preserves_flags)
                );
                (r10_val, r8_val)
            };
            mremap(arg1, arg2, arg3, arg4, arg5)
        }
        SYS_MSYNC => msync(arg1, arg2, arg3 as i32),
        SYS_MINCORE => mincore(arg1, arg2, arg3 as *mut u8),
        SYS_MADVISE => madvise(arg1, arg2, arg3 as i32),
        SYS_MLOCK => mlock(arg1, arg2),
        SYS_MUNLOCK => munlock(arg1, arg2),
        SYS_MLOCKALL => mlockall(arg1 as i32),
        SYS_MUNLOCKALL => munlockall(),
        SYS_GETRLIMIT => getrlimit(arg1 as i32, arg2 as *mut RLimit),
        SYS_SETRLIMIT => setrlimit(arg1 as i32, arg2 as *const RLimit),
        SYS_PRLIMIT64 => {
            // prlimit64 needs 4 args: pid, resource, new_rlim, old_rlim
            let arg4 = unsafe {
                let mut r10_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    out(reg) r10_val,
                    options(nostack, preserves_flags)
                );
                r10_val
            };
            prlimit64(
                arg1 as i64,
                arg2 as i32,
                arg3 as *const RLimit,
                arg4 as *mut RLimit,
            )
        }
        SYS_MOUNT => mount(arg1 as *const MountRequest),
        SYS_UMOUNT => umount(arg1 as *const u8, arg2 as usize),
        SYS_CHROOT => chroot(arg1 as *const u8, arg2 as usize),
        SYS_PIVOT_ROOT => pivot_root(arg1 as *const PivotRootRequest),
        SYS_SYSLOG => syslog(arg1 as i32, arg2 as *mut u8, arg3 as usize),
        SYS_UEFI_GET_COUNTS => uefi_get_counts(arg1 as *mut CompatCounts),
        SYS_UEFI_GET_FB_INFO => uefi_get_fb_info(arg1 as *mut FramebufferInfo),
        SYS_UEFI_GET_NET_INFO => uefi_get_net_info(arg1 as usize, arg2 as *mut NetworkDescriptor),
        SYS_UEFI_GET_BLOCK_INFO => uefi_get_block_info(arg1 as usize, arg2 as *mut BlockDescriptor),
        SYS_UEFI_MAP_NET_MMIO => uefi_map_net_mmio(arg1 as usize),
        SYS_UEFI_GET_USB_INFO => uefi_get_usb_info(arg1 as usize, arg2 as *mut UsbHostDescriptor),
        SYS_UEFI_GET_HID_INFO => uefi_get_hid_info(arg1 as usize, arg2 as *mut HidInputDescriptor),
        SYS_UEFI_MAP_USB_MMIO => uefi_map_usb_mmio(arg1 as usize),
        // Swap management syscalls
        SYS_SWAPON => swap::swapon(arg1 as *const u8, arg2 as u32),
        SYS_SWAPOFF => swap::swapoff(arg1 as *const u8),
        // Kernel module management syscalls
        SYS_INIT_MODULE => init_module(arg1 as *const u8, arg2 as usize, arg3 as *const u8),
        SYS_DELETE_MODULE => delete_module(arg1 as *const u8, arg2 as u32),
        SYS_QUERY_MODULE => {
            // query_module needs 4 args: operation, name_ptr, buf_ptr, buf_size
            let arg4 = unsafe {
                let mut r10_val: u64;
                core::arch::asm!(
                    "mov {0}, gs:[32]",
                    out(reg) r10_val,
                    options(nostack, preserves_flags)
                );
                r10_val
            };
            query_module(
                arg1 as u32,
                arg2 as *const u8,
                arg3 as *mut u8,
                arg4 as usize,
            )
        }
        SYS_GETRANDOM => {
            let result = crate::drivers::sys_getrandom(arg1 as *mut u8, arg2 as usize, arg3 as u32);
            if result < 0 {
                posix::set_errno((-result) as i32);
                (-1i64) as u64
            } else {
                result as u64
            }
        }
        _ => {
            crate::kwarn!("Unknown syscall: {}", nr);
            posix::set_errno(posix::errno::ENOSYS);
            0
        }
    };
    result
}

/// Check if current process should be rescheduled (called from syscall handler)
#[no_mangle]
extern "C" fn should_reschedule() -> bool {
    if let Some(pid) = scheduler::get_current_pid() {
        if let Some(process) = scheduler::get_process(pid) {
            return process.state == ProcessState::Sleeping;
        }
    }
    false
}

/// Trigger rescheduling from syscall return path
#[no_mangle]
extern "C" fn do_schedule_from_syscall() {
    // Wake up the process before scheduling (it will be picked up again if still runnable)
    if let Some(pid) = scheduler::get_current_pid() {
        let _ = scheduler::set_process_state(pid, ProcessState::Ready);
    }
    scheduler::do_schedule();
}

global_asm!(
    ".global syscall_handler",
    "syscall_handler:",
    "swapgs",
    "mov gs:[0], rsp", // Save user RSP to GS_SLOT_USER_RSP
    // CRITICAL: Save user RCX (return address) and R11 (rflags) to GS_DATA
    // This is required for context switching during syscall handling (e.g., timer interrupt)
    // GS_SLOT_SAVED_RCX = 7, offset = 7 * 8 = 56
    // GS_SLOT_SAVED_RFLAGS = 8, offset = 8 * 8 = 64
    "mov gs:[56], rcx", // Save user RIP to GS_SLOT_SAVED_RCX for context switch
    "mov gs:[64], r11", // Save user RFLAGS to GS_SLOT_SAVED_RFLAGS for context switch
    // Save more user registers to GS_DATA for fork() to access
    // GS_SLOT_SAVED_RDI = 11, offset = 11 * 8 = 88
    // GS_SLOT_SAVED_RSI = 12, offset = 12 * 8 = 96
    // GS_SLOT_SAVED_RDX = 13, offset = 13 * 8 = 104
    // GS_SLOT_SAVED_RBX = 14, offset = 14 * 8 = 112
    // GS_SLOT_SAVED_RBP = 15, offset = 15 * 8 = 120
    // GS_SLOT_SAVED_R8 = 16, offset = 16 * 8 = 128
    // GS_SLOT_SAVED_R9 = 17, offset = 17 * 8 = 136
    // GS_SLOT_SAVED_R10 = 18, offset = 18 * 8 = 144
    // GS_SLOT_SAVED_R12 = 19, offset = 19 * 8 = 152
    "mov gs:[88], rdi",  // Save user RDI (syscall arg1)
    "mov gs:[96], rsi",  // Save user RSI (syscall arg2)
    "mov gs:[104], rdx", // Save user RDX (syscall arg3)
    "mov gs:[112], rbx", // Save user RBX (callee-saved)
    "mov gs:[120], rbp", // Save user RBP (callee-saved)
    "mov gs:[128], r8",  // Save user R8 (syscall arg5)
    "mov gs:[136], r9",  // Save user R9 (syscall arg6)
    "mov gs:[144], r10", // Save user R10
    "mov gs:[152], r12", // Save user R12 (callee-saved)
    "mov rsp, gs:[8]",   // Load kernel RSP
    // Also push to stack for sysretq restore
    "push r11", // save user rflags
    "push rcx", // save user return address
    "push rbx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "sub rsp, 8", // maintain 16-byte stack alignment before calling into Rust
    // Stack layout after sub rsp,8:
    // [rsp+0] = alignment padding
    // [rsp+8] = r15 (最后push的)
    // [rsp+16] = r14
    // [rsp+24] = r13
    // [rsp+32] = r12
    // [rsp+40] = r10
    // [rsp+48] = r9
    // [rsp+56] = r8
    // [rsp+64] = rbp
    // [rsp+72] = rdi
    // [rsp+80] = rsi
    // [rsp+88] = rdx
    // [rsp+96] = rbx
    // [rsp+104] = rcx (user return address, DO NOT OVERWRITE!) <-- THIS ONE
    // [rsp+112] = r11 (user rflags, 第一个push的)
    "mov r8, [rsp + 104]", // Get original RCX (syscall return address) -> r8 (param 5)
    // WARNING: RCX register will be used for param 4, but we must NOT overwrite [rsp+104]!
    // Prepare arguments for syscall_dispatch:
    // RDI = nr, RSI = arg1, RDX = arg2, RCX = arg3, R8 = syscall_return_addr
    "mov rcx, rdx", // arg3 -> rcx (param 4) - THIS OVERWRITES RCX REGISTER BUT NOT STACK
    "mov rdx, rsi", // arg2 -> rdx (param 3)
    "mov rsi, rdi", // arg1 -> rsi (param 2)
    "mov rdi, rax", // nr -> rdi (param 1)
    "call syscall_dispatch",
    // Return value is in rax
    // Check if this is exec returning (magic value 0x4558454300000000 = "EXEC")
    "movabs rbx, 0x4558454300000000",
    "cmp rax, rbx",
    "jne .Lnormal_return", // Not exec, normal return
    // Exec return: call get_exec_context to get entry/stack
    ".Lexec_return:",
    "sub rsp, 32",         // Keep 16-byte alignment: 32 is divisible by 16
    "lea rdi, [rsp + 24]", // entry_out = rsp+24 (first parameter)
    "lea rsi, [rsp + 16]", // stack_out = rsp+16 (second parameter)
    "lea rdx, [rsp + 8]",  // user_data_sel_out = rsp+8 (third parameter)
    "call get_exec_context",
    "test al, al",                   // Check if exec was pending
    "jz .Lnormal_return_after_exec", // Not exec, treat as normal
    // Exec successful: load new entry, stack, and user_data_sel
    "mov rcx, [rsp + 24]", // Load entry -> RCX (for sysretq)
    "mov r8, [rsp + 16]",  // Load stack -> R8 (temp, we'll move to RSP)
    "mov r9, [rsp + 8]",   // Load user_data_sel -> R9 (temp for segment selector)
    "mov rsp, r8",         // Switch to new user stack
    "mov r11, 0x202",      // User rflags (IF=1, reserved bit=1)
    "xor rax, rax",        // Clear return value for exec
    // Set user segment selectors from user_data_sel (in R9)
    "mov ax, r9w", // user data segment selector
    "mov ds, ax",
    "mov es, ax",
    "mov fs, ax",
    "swapgs",
    "mov gs, ax",
    // sysretq: rcx=rip, r11=rflags, rsp already set to user stack
    "sysretq",
    ".Lnormal_return_after_exec:",
    "add rsp, 32", // Clean up the 32 bytes we allocated
    ".Lnormal_return:",
    // Check if current process needs to be rescheduled (Sleeping state)
    "call should_reschedule",
    "test al, al",
    "jz .Lno_reschedule",
    // Process needs to be rescheduled - save context and switch
    "call do_schedule_from_syscall",
    // After returning from schedule, continue with normal return
    ".Lno_reschedule:",
    // Normal syscall return path
    "add rsp, 8", // remove alignment padding
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rbx",
    // Restore RCX and R11 for sysretq
    "pop rcx", // user return address
    "pop r11", // user rflags
    // Restore user segment registers before sysretq
    // Note: user data segment is now entry 3 (0x18 | 3 = 0x1B)
    "mov r8w, 0x1B", // user data segment selector (0x18 | 3)
    "mov ds, r8w",
    "mov es, r8w",
    "mov fs, r8w",
    "mov rsp, gs:[0]", // Restore user RSP
    "swapgs",          // Restore user GS base
    // Return to user mode via sysretq (RCX=rip, R11=rflags, RAX=return value)
    "sysretq"
);
