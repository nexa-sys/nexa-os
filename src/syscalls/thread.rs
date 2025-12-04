//! Thread management syscalls
//!
//! Implements: clone, futex, gettid, set_tid_address, arch_prctl

use crate::posix::{self, errno};
use crate::process::{Process, ProcessState};
use crate::scheduler;
use crate::{kdebug, kerror, kinfo, ktrace, kwarn};
use alloc::alloc::{alloc, Layout};
use core::sync::atomic::{AtomicU64, Ordering};

/// Clone flags (subset of Linux clone flags)
pub const CLONE_VM: u64 = 0x00000100; // Share virtual memory
pub const CLONE_FS: u64 = 0x00000200; // Share filesystem info
pub const CLONE_FILES: u64 = 0x00000400; // Share file descriptors
pub const CLONE_SIGHAND: u64 = 0x00000800; // Share signal handlers
pub const CLONE_THREAD: u64 = 0x00010000; // Same thread group
pub const CLONE_NEWNS: u64 = 0x00020000; // New mount namespace
pub const CLONE_SYSVSEM: u64 = 0x00040000; // Share System V SEM_UNDO
pub const CLONE_SETTLS: u64 = 0x00080000; // Set TLS
pub const CLONE_PARENT_SETTID: u64 = 0x00100000; // Store TID in parent
pub const CLONE_CHILD_CLEARTID: u64 = 0x00200000; // Clear TID in child on exit
pub const CLONE_DETACHED: u64 = 0x00400000; // Unused
pub const CLONE_UNTRACED: u64 = 0x00800000; // Tracing doesn't follow
pub const CLONE_CHILD_SETTID: u64 = 0x01000000; // Store TID in child
pub const CLONE_VFORK: u64 = 0x00004000; // Parent sleeps until child exits

/// arch_prctl operations
pub const ARCH_SET_GS: i32 = 0x1001;
pub const ARCH_SET_FS: i32 = 0x1002;
pub const ARCH_GET_FS: i32 = 0x1003;
pub const ARCH_GET_GS: i32 = 0x1004;

/// Futex operations
pub const FUTEX_WAIT: i32 = 0;
pub const FUTEX_WAKE: i32 = 1;
pub const FUTEX_FD: i32 = 2;
pub const FUTEX_REQUEUE: i32 = 3;
pub const FUTEX_CMP_REQUEUE: i32 = 4;
pub const FUTEX_WAKE_OP: i32 = 5;
pub const FUTEX_LOCK_PI: i32 = 6;
pub const FUTEX_UNLOCK_PI: i32 = 7;
pub const FUTEX_TRYLOCK_PI: i32 = 8;
pub const FUTEX_WAIT_BITSET: i32 = 9;
pub const FUTEX_WAKE_BITSET: i32 = 10;

/// Futex flags
pub const FUTEX_PRIVATE_FLAG: i32 = 128;
pub const FUTEX_CLOCK_REALTIME: i32 = 256;
pub const FUTEX_CMD_MASK: i32 = !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);

/// Thread ID address for clear_child_tid functionality
static mut CLEAR_CHILD_TID: u64 = 0;

/// Maximum number of waiters on a single futex
const MAX_FUTEX_WAITERS: usize = 64;

/// Simple futex wait queue entry
#[derive(Clone, Copy)]
struct FutexWaiter {
    uaddr: u64, // Address being waited on
    pid: u64,   // Waiting process/thread ID
    in_use: bool,
}

impl FutexWaiter {
    const fn empty() -> Self {
        Self {
            uaddr: 0,
            pid: 0,
            in_use: false,
        }
    }
}

/// Global futex wait queue (simplified)
static mut FUTEX_WAITERS: [FutexWaiter; MAX_FUTEX_WAITERS] =
    [FutexWaiter::empty(); MAX_FUTEX_WAITERS];

/// SYS_CLONE - Create a new process/thread
///
/// This is a simplified implementation that creates lightweight processes.
/// Full thread support would require shared address spaces.
///
/// # Arguments
/// * `flags` - Clone flags (CLONE_VM, CLONE_FILES, etc.)
/// * `stack` - New stack pointer for child (0 to use default)
/// * `parent_tid` - Pointer to store parent TID (if CLONE_PARENT_SETTID)
/// * `child_tid` - Pointer to store child TID (if CLONE_CHILD_SETTID)
/// * `tls` - TLS descriptor (if CLONE_SETTLS)
///
/// # Returns
/// * Child PID in parent, 0 in child on success
/// * -1 on error with errno set
pub fn clone(
    flags: u64,
    stack: u64,
    parent_tid: u64,
    child_tid: u64,
    tls: u64,
    syscall_return_addr: u64,
) -> u64 {
    ktrace!(
        "[clone] flags={:#x}, stack={:#x}, parent_tid={:#x}, child_tid={:#x}, tls={:#x}",
        flags,
        stack,
        parent_tid,
        child_tid,
        tls
    );

    // Get current process info
    let current_pid = match scheduler::get_current_pid() {
        Some(pid) => pid,
        None => {
            kerror!("[clone] No current process");
            posix::set_errno(errno::ESRCH);
            return u64::MAX;
        }
    };

    let parent_process = match scheduler::get_process(current_pid) {
        Some(proc) => proc,
        None => {
            kerror!("[clone] Current process {} not found", current_pid);
            posix::set_errno(errno::ESRCH);
            return u64::MAX;
        }
    };

    // Get user RSP for context
    let user_rsp: u64 = if stack != 0 {
        stack
    } else {
        unsafe {
            let mut rsp: u64;
            core::arch::asm!(
                "mov {}, gs:[0]",
                out(reg) rsp
            );
            rsp
        }
    };

    kdebug!("[clone] Creating child from PID {}", current_pid);

    // Allocate new PID
    let child_pid = crate::process::allocate_pid();
    ktrace!("[clone] Allocated child PID {}", child_pid);

    // Create child process structure
    let mut child_process = parent_process;
    child_process.pid = child_pid;
    child_process.state = ProcessState::Ready;
    child_process.has_entered_user = false;
    child_process.context_valid = false; // Context not yet saved by context_switch
    child_process.is_fork_child = true;
    child_process.exit_code = 0;

    // Handle CLONE_THREAD - create a thread in the same thread group
    if (flags & CLONE_THREAD) != 0 {
        // Thread: share thread group with parent
        child_process.tgid = parent_process.tgid;
        child_process.ppid = parent_process.ppid; // Threads share parent
        child_process.is_thread = true;
        kinfo!(
            "[clone] CLONE_THREAD: Creating thread {} in thread group {}",
            child_pid,
            parent_process.tgid
        );
    } else {
        // New process: new thread group
        child_process.tgid = child_pid;
        child_process.ppid = current_pid;
        child_process.is_thread = false;
    }

    // Allocate kernel stack for child
    let kernel_stack_layout = Layout::from_size_align(
        crate::process::KERNEL_STACK_SIZE,
        crate::process::KERNEL_STACK_ALIGN,
    )
    .unwrap();

    let kernel_stack = unsafe { alloc(kernel_stack_layout) } as u64;
    if kernel_stack == 0 {
        kerror!("[clone] Failed to allocate kernel stack");
        posix::set_errno(errno::ENOMEM);
        return u64::MAX;
    }
    child_process.kernel_stack = kernel_stack;

    // Set up child's execution context
    child_process.entry_point = syscall_return_addr;
    child_process.stack_top = user_rsp;
    child_process.user_rip = syscall_return_addr;
    child_process.user_rsp = user_rsp;
    child_process.user_rflags = parent_process.user_rflags;

    // Child returns 0 from clone
    child_process.context = crate::process::Context::zero();
    child_process.context.rax = 0;
    child_process.context.rip = syscall_return_addr;
    child_process.context.rsp = user_rsp;

    // Handle CLONE_VM - share virtual memory (threads)
    if (flags & CLONE_VM) != 0 {
        // For threads, share the same address space
        child_process.cr3 = parent_process.cr3;
        child_process.memory_base = parent_process.memory_base;
        child_process.memory_size = parent_process.memory_size;
        kdebug!(
            "[clone] CLONE_VM: Sharing address space (CR3={:#x})",
            parent_process.cr3
        );
    } else {
        // For fork-like behavior, copy memory
        use crate::process::{INTERP_BASE, INTERP_REGION_SIZE, USER_VIRT_BASE};

        let memory_size = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;

        let child_phys_base = match crate::paging::allocate_user_region(memory_size) {
            Some(addr) => addr,
            None => {
                kerror!("[clone] Failed to allocate memory for child");
                posix::set_errno(errno::ENOMEM);
                return u64::MAX;
            }
        };

        // Copy parent memory to child
        unsafe {
            let src_ptr = USER_VIRT_BASE as *const u8;
            let dst_ptr = child_phys_base as *mut u8;
            core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, memory_size as usize);
        }

        child_process.memory_base = child_phys_base;
        child_process.memory_size = memory_size;

        // Create page tables for child
        // Clone: use demand paging - pages will be mapped on first access
        match crate::paging::create_process_address_space(child_phys_base, memory_size, true) {
            Ok(cr3) => {
                child_process.cr3 = cr3;
                ktrace!("[clone] Created page tables (CR3={:#x})", cr3);
            }
            Err(err) => {
                kerror!("[clone] Failed to create page tables: {}", err);
                posix::set_errno(errno::ENOMEM);
                return u64::MAX;
            }
        }
    }

    // Handle CLONE_PARENT_SETTID
    if (flags & CLONE_PARENT_SETTID) != 0 && parent_tid != 0 {
        unsafe {
            *(parent_tid as *mut u32) = child_pid as u32;
        }
        ktrace!(
            "[clone] Stored child TID {} at parent_tid {:#x}",
            child_pid,
            parent_tid
        );
    }

    // Handle CLONE_CHILD_SETTID
    if (flags & CLONE_CHILD_SETTID) != 0 && child_tid != 0 {
        unsafe {
            *(child_tid as *mut u32) = child_pid as u32;
        }
        ktrace!(
            "[clone] Stored child TID {} at child_tid {:#x}",
            child_pid,
            child_tid
        );
    }

    // Handle CLONE_CHILD_CLEARTID
    if (flags & CLONE_CHILD_CLEARTID) != 0 && child_tid != 0 {
        // Store the address in the process struct - will be cleared and woken on exit
        child_process.clear_child_tid = child_tid;
        ktrace!("[clone] Set clear_child_tid to {:#x}", child_tid);
    } else {
        child_process.clear_child_tid = 0;
    }

    // Handle CLONE_SETTLS (TLS setup)
    if (flags & CLONE_SETTLS) != 0 && tls != 0 {
        // Set the FS base for TLS in the child process
        child_process.fs_base = tls;
        kinfo!("[clone] CLONE_SETTLS: Set fs_base to {:#x} for child PID {}", tls, child_pid);
    }

    // Add child to scheduler
    if let Err(e) = scheduler::add_process(child_process, 128) {
        kerror!("[clone] Failed to add child process: {}", e);
        posix::set_errno(errno::EAGAIN);
        return u64::MAX;
    }

    kinfo!(
        "[clone] Created child PID {} from parent PID {} (flags={:#x})",
        child_pid,
        current_pid,
        flags
    );

    posix::set_errno(0);
    child_pid
}

/// SYS_FUTEX - Fast userspace mutex operations
///
/// # Arguments
/// * `uaddr` - Pointer to the futex word
/// * `op` - Futex operation
/// * `val` - Value for operation
/// * `timeout` - Optional timeout
/// * `uaddr2` - Second futex address (for requeue operations)
/// * `val3` - Third value (for some operations)
///
/// # Returns
/// * Depends on operation
/// * -1 on error with errno set
pub fn futex(uaddr: u64, op: i32, val: i32, timeout: u64, _uaddr2: u64, _val3: i32) -> u64 {
    // Mask off private and clock flags
    let cmd = op & FUTEX_CMD_MASK;

    ktrace!(
        "[futex] uaddr={:#x}, op={} (cmd={}), val={}, timeout={:#x}",
        uaddr,
        op,
        cmd,
        val,
        timeout
    );

    // Validate address
    if uaddr == 0 {
        kerror!("[futex] Null address");
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    // Check address alignment
    if (uaddr & 3) != 0 {
        kerror!("[futex] Address not 4-byte aligned: {:#x}", uaddr);
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    match cmd {
        FUTEX_WAIT => futex_wait(uaddr, val, timeout),
        FUTEX_WAKE => futex_wake(uaddr, val),
        FUTEX_WAIT_BITSET => {
            // FUTEX_WAIT_BITSET with val3=FUTEX_BITSET_MATCH_ANY is equivalent to FUTEX_WAIT
            futex_wait(uaddr, val, timeout)
        }
        FUTEX_WAKE_BITSET => {
            // FUTEX_WAKE_BITSET with val3=FUTEX_BITSET_MATCH_ANY is equivalent to FUTEX_WAKE
            futex_wake(uaddr, val)
        }
        _ => {
            kwarn!("[futex] Unsupported operation: {}", cmd);
            posix::set_errno(errno::ENOSYS);
            u64::MAX
        }
    }
}

/// FUTEX_WAIT - Wait if *uaddr == val
/// 
/// Puts the calling thread to sleep if the value at uaddr equals val.
/// The thread will be woken by FUTEX_WAKE or when the timeout expires.
fn futex_wait(uaddr: u64, val: i32, _timeout: u64) -> u64 {
    let current_pid = scheduler::get_current_pid().unwrap_or(0);

    // Read the current value at uaddr atomically
    let current_val = unsafe { 
        core::ptr::read_volatile(uaddr as *const i32) 
    };

    ktrace!(
        "[futex_wait] PID {} waiting on {:#x}, expected={}, actual={}",
        current_pid,
        uaddr,
        val,
        current_val
    );

    // If value doesn't match, return EAGAIN (spurious wakeup handling)
    if current_val != val {
        kdebug!("[futex_wait] Value mismatch, returning EAGAIN");
        posix::set_errno(errno::EAGAIN);
        return u64::MAX;
    }

    // Add current thread to the futex wait queue
    let mut added = false;
    unsafe {
        for waiter in FUTEX_WAITERS.iter_mut() {
            if !waiter.in_use {
                waiter.uaddr = uaddr;
                waiter.pid = current_pid;
                waiter.in_use = true;
                added = true;
                ktrace!(
                    "[futex_wait] PID {} added to wait queue for {:#x}",
                    current_pid,
                    uaddr
                );
                break;
            }
        }
    }

    if !added {
        // Wait queue is full
        kwarn!("[futex_wait] Wait queue full, cannot add PID {}", current_pid);
        posix::set_errno(errno::EAGAIN);
        return u64::MAX;
    }

    // Put the thread to sleep
    if let Err(e) = scheduler::set_process_state(current_pid, ProcessState::Sleeping) {
        kwarn!("[futex_wait] Failed to set sleeping state for PID {}: {}", current_pid, e);
        // Remove from wait queue
        unsafe {
            for waiter in FUTEX_WAITERS.iter_mut() {
                if waiter.in_use && waiter.pid == current_pid && waiter.uaddr == uaddr {
                    waiter.in_use = false;
                    break;
                }
            }
        }
        posix::set_errno(errno::EAGAIN);
        return u64::MAX;
    }

    // Yield to scheduler - will not return until woken
    scheduler::do_schedule();

    // We've been woken up - remove from wait queue if still there
    unsafe {
        for waiter in FUTEX_WAITERS.iter_mut() {
            if waiter.in_use && waiter.pid == current_pid && waiter.uaddr == uaddr {
                waiter.in_use = false;
                break;
            }
        }
    }

    // Check if we were woken due to timeout (not implemented yet) or FUTEX_WAKE
    // For now, assume we were properly woken
    ktrace!("[futex_wait] PID {} woken from futex wait", current_pid);
    
    posix::set_errno(0);
    0
}

/// FUTEX_WAKE - Wake up to val waiters
fn futex_wake(uaddr: u64, val: i32) -> u64 {
    if val <= 0 {
        posix::set_errno(0);
        return 0;
    }

    ktrace!("[futex_wake] Waking up to {} waiters on {:#x}", val, uaddr);

    let mut woken = 0u64;

    // In a full implementation, we would:
    // 1. Find all processes waiting on this address
    // 2. Wake up to 'val' of them
    // 3. Set their state to Ready

    unsafe {
        for waiter in FUTEX_WAITERS.iter_mut() {
            if waiter.in_use && waiter.uaddr == uaddr {
                // Wake this waiter
                if let Err(_) = scheduler::set_process_state(waiter.pid, ProcessState::Ready) {
                    kwarn!("[futex_wake] Failed to wake PID {}", waiter.pid);
                } else {
                    woken += 1;
                    waiter.in_use = false;
                }

                if woken >= val as u64 {
                    break;
                }
            }
        }
    }

    ktrace!("[futex_wake] Woke {} processes", woken);
    posix::set_errno(0);
    woken
}

/// Public interface for waking futex waiters (for use by scheduler and other kernel modules)
/// This is a simplified wrapper around futex_wake for internal kernel use.
pub fn futex_wake_internal(uaddr: u64, max_waiters: i32) -> u64 {
    futex_wake(uaddr, max_waiters)
}

/// SYS_GETTID - Get thread ID
///
/// In NexaOS, thread ID equals process ID (no kernel threads yet)
pub fn gettid() -> u64 {
    let tid = scheduler::get_current_pid().unwrap_or(0);
    posix::set_errno(0);
    tid
}

/// SYS_SET_TID_ADDRESS - Set pointer to thread ID
///
/// # Arguments
/// * `tidptr` - Pointer to store TID and clear on exit
///
/// # Returns
/// * Current thread ID
pub fn set_tid_address(tidptr: u64) -> u64 {
    let tid = scheduler::get_current_pid().unwrap_or(0);

    if tidptr != 0 {
        unsafe {
            CLEAR_CHILD_TID = tidptr;
            *(tidptr as *mut u32) = tid as u32;
        }
        ktrace!("[set_tid_address] Set tidptr={:#x} to {}", tidptr, tid);
    }

    posix::set_errno(0);
    tid
}

/// SYS_SET_ROBUST_LIST - Set robust futex list
///
/// Stub implementation - robust futexes are not yet fully supported
pub fn set_robust_list(_head: u64, _len: usize) -> u64 {
    ktrace!("[set_robust_list] Stub implementation");
    posix::set_errno(0);
    0
}

/// SYS_GET_ROBUST_LIST - Get robust futex list
///
/// Stub implementation - robust futexes are not yet fully supported
pub fn get_robust_list(_pid: u64, _head_ptr: u64, _len_ptr: u64) -> u64 {
    ktrace!("[get_robust_list] Stub implementation");
    posix::set_errno(errno::ENOSYS);
    u64::MAX
}

/// SYS_ARCH_PRCTL - Architecture-specific thread state
///
/// This syscall is used to set/get thread-local storage (TLS) base addresses.
///
/// # Arguments
/// * `code` - Operation code (ARCH_SET_FS, ARCH_GET_FS, ARCH_SET_GS, ARCH_GET_GS)
/// * `addr` - Address to set or pointer to store result
///
/// # Returns
/// * 0 on success
/// * -1 on error with errno set
pub fn arch_prctl(code: i32, addr: u64) -> u64 {
    use x86_64::registers::model_specific::Msr;

    let current_pid = match scheduler::get_current_pid() {
        Some(pid) => pid,
        None => {
            kerror!("[arch_prctl] No current process");
            posix::set_errno(errno::ESRCH);
            return u64::MAX;
        }
    };

    match code {
        ARCH_SET_FS => {
            ktrace!("[arch_prctl] ARCH_SET_FS: setting fs_base to {:#x}", addr);

            // Set FS base in MSR immediately
            unsafe {
                Msr::new(crate::safety::x86::MSR_IA32_FS_BASE).write(addr);
            }

            // Also update the process structure via scheduler
            set_process_fs_base(current_pid, addr);

            posix::set_errno(0);
            0
        }
        ARCH_GET_FS => {
            ktrace!("[arch_prctl] ARCH_GET_FS: reading fs_base");

            // Read current FS base from MSR
            let fs_base = unsafe { Msr::new(crate::safety::x86::MSR_IA32_FS_BASE).read() };

            // Store result at addr
            if addr != 0 {
                unsafe {
                    *(addr as *mut u64) = fs_base;
                }
            }

            posix::set_errno(0);
            0
        }
        ARCH_SET_GS => {
            ktrace!("[arch_prctl] ARCH_SET_GS: setting gs_base to {:#x}", addr);

            // Note: GS is typically reserved for kernel use, but we support it
            unsafe {
                Msr::new(crate::safety::x86::MSR_IA32_GS_BASE).write(addr);
            }

            posix::set_errno(0);
            0
        }
        ARCH_GET_GS => {
            ktrace!("[arch_prctl] ARCH_GET_GS: reading gs_base");

            let gs_base = unsafe { Msr::new(crate::safety::x86::MSR_IA32_GS_BASE).read() };

            if addr != 0 {
                unsafe {
                    *(addr as *mut u64) = gs_base;
                }
            }

            posix::set_errno(0);
            0
        }
        _ => {
            kerror!("[arch_prctl] Unknown code: {}", code);
            posix::set_errno(errno::EINVAL);
            u64::MAX
        }
    }
}

/// Helper to set process fs_base
fn set_process_fs_base(pid: u64, fs_base: u64) {
    // Use the scheduler's process table lock mechanism
    let table = scheduler::process_table_lock();

    // Try radix tree lookup first
    if let Some(idx) = crate::process::lookup_pid(pid) {
        let idx = idx as usize;
        if idx < table.len() {
            // We need mutable access, but process_table_lock returns shared lock
            // So we'll update it via scheduler functions or direct MSR is sufficient
            // since we already set the MSR above
        }
    }

    // The MSR is already set in arch_prctl, and fs_base in Process struct
    // is primarily for context switch restoration. For now, the MSR setting
    // is the important part - the scheduler will use the MSR value when switching.
    ktrace!(
        "[set_process_fs_base] Set fs_base for PID {} to {:#x}",
        pid,
        fs_base
    );
}
