//! Process type definitions
//!
//! This module contains all type definitions used by the process subsystem,
//! including the Process structure, ProcessState enum, CPU Context, and
//! memory layout constants.

use core::sync::atomic::{AtomicU64, Ordering};

/// Process ID type
pub type Pid = u64;

/// Process state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Sleeping,
    Zombie,
}

/// Virtual base address where userspace expects to be mapped.
/// IMPORTANT: Must be placed AFTER the kernel's BSS section end (around 0x840000).
/// Using 16MB (0x1000000) to safely avoid overlap with kernel memory regions
/// including IDT, GDT, and other kernel data structures.
pub const USER_VIRT_BASE: u64 = 0x1000000;
/// Physical base address used when copying the userspace image.
/// Must be placed after the kernel image (kernel .bss ends around 0x83f000)
/// Using 16MB (0x1000000) to provide safe margin from kernel memory.
pub const USER_PHYS_BASE: u64 = 0x1000000;
/// Virtual address where the heap begins in userspace.
/// Starts at USER_VIRT_BASE + 2MB (after code/data segments)
pub const HEAP_BASE: u64 = USER_VIRT_BASE + 0x200000;
/// Maximum size of heap reserved for userspace (8MB for dynamic allocation).
/// Heap region: 0x1200000 - 0x1A00000
pub const HEAP_SIZE: u64 = 0x800000;
/// Virtual address chosen for the base of the userspace stack region.
/// Placed after heap: HEAP_BASE + HEAP_SIZE = 0x1A00000
pub const STACK_BASE: u64 = HEAP_BASE + HEAP_SIZE;
/// Size of the userspace stack in bytes (must stay 2 MiB aligned for huge pages).
pub const STACK_SIZE: u64 = 0x200000;
/// Virtual base where the dynamic loader and shared objects are staged.
pub const INTERP_BASE: u64 = STACK_BASE + STACK_SIZE;
/// Reserved size for the dynamic loader and dependent shared objects.
/// 16MB should be sufficient for several shared libraries.
pub const INTERP_REGION_SIZE: u64 = 0x1000000; // 16MB (was 6MB)
/// Total virtual span that must be mapped for the userspace image, heap, stack, and interpreter region.
pub const USER_REGION_SIZE: u64 = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;

/// Kernel stack size (32 KB)
pub const KERNEL_STACK_SIZE: usize = 32 * 1024;
/// Kernel stack alignment
pub const KERNEL_STACK_ALIGN: usize = 16;

/// Maximum number of processes supported
pub const MAX_PROCESSES: usize = 64;
/// Maximum number of arguments for a process
pub const MAX_PROCESS_ARGS: usize = 32;
/// Maximum size of command line storage per process (null-separated arguments)
pub const MAX_CMDLINE_SIZE: usize = 1024;

/// CPU context saved during context switch
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Context {
    // General purpose registers
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,

    // Instruction pointer and stack pointer
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
}

impl Context {
    pub const fn zero() -> Self {
        Self {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            r11: 0,
            r10: 0,
            r9: 0,
            r8: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            rdx: 0,
            rcx: 0,
            rbx: 0,
            rax: 0,
            rip: 0,
            rsp: 0,
            rflags: 0x202, // IF flag set (interrupts enabled)
        }
    }
}

/// Clone flags for thread creation (matching Linux)
pub mod clone_flags {
    /// Share virtual memory with parent
    pub const CLONE_VM: u64 = 0x00000100;
    /// Share filesystem info with parent
    pub const CLONE_FS: u64 = 0x00000200;
    /// Share file descriptors with parent
    pub const CLONE_FILES: u64 = 0x00000400;
    /// Share signal handlers with parent
    pub const CLONE_SIGHAND: u64 = 0x00000800;
    /// Create in same thread group (true thread)
    pub const CLONE_THREAD: u64 = 0x00010000;
    /// Share System V SEM_UNDO semantics
    pub const CLONE_SYSVSEM: u64 = 0x00040000;
    /// Set TLS for the new thread
    pub const CLONE_SETTLS: u64 = 0x00080000;
    /// Store child TID at parent_tid location
    pub const CLONE_PARENT_SETTID: u64 = 0x00100000;
    /// Clear child TID at child_tid location on exit
    pub const CLONE_CHILD_CLEARTID: u64 = 0x00200000;
    /// Store child TID at child_tid location
    pub const CLONE_CHILD_SETTID: u64 = 0x01000000;
}

/// Process structure
///
/// In NexaOS, threads are implemented as lightweight processes (LWP) that share
/// resources with their parent. When CLONE_THREAD is used, the new process:
/// - Shares the same thread group ID (tgid) as the parent
/// - Shares virtual memory (cr3), file descriptors, and signal handlers
/// - Has its own PID (thread ID) but belongs to the parent's thread group
#[derive(Clone, Copy)]
pub struct Process {
    pub pid: Pid,
    pub ppid: Pid, // Parent process ID (POSIX)
    pub tgid: Pid, // Thread group ID (equals pid for main thread, leader's pid for other threads)
    pub state: ProcessState,
    pub entry_point: u64,
    pub stack_top: u64,
    pub heap_start: u64,
    pub heap_end: u64,
    pub signal_state: crate::signal::SignalState, // POSIX signal handling
    pub context: Context,                         // CPU context for context switching
    pub has_entered_user: bool,
    pub context_valid: bool, // True if context was saved by context_switch (valid for Switch restore)
    pub is_fork_child: bool, // True if this process was created by fork (not exec/init)
    pub is_thread: bool,     // True if this is a thread (created with CLONE_THREAD)
    pub cr3: u64, // Page table root (for process-specific page tables) - 0 means use kernel page table
    pub tty: usize, // Controlling virtual terminal index
    pub memory_base: u64, // Physical base address of process memory (for fork)
    pub memory_size: u64, // Size of process memory region (for fork)
    pub user_rip: u64, // Saved user-mode RIP for syscall return
    pub user_rsp: u64, // Saved user-mode RSP for syscall return
    pub user_rflags: u64, // Saved user-mode RFLAGS for syscall return
    pub user_r10: u64, // Saved user-mode R10 (syscall arg4) for context switch
    pub user_r8: u64, // Saved user-mode R8 (syscall arg5) for context switch
    pub user_r9: u64, // Saved user-mode R9 (syscall arg6) for context switch
    pub exit_code: i32, // Last exit code reported by this process (if zombie)
    pub term_signal: Option<i32>, // Signal that terminated this process (None = normal exit)
    pub kernel_stack: u64, // Pointer to kernel stack allocation (bottom)
    pub fs_base: u64, // FS segment base for TLS (Thread Local Storage)
    pub clear_child_tid: u64, // Address to clear and futex wake on thread exit (CLONE_CHILD_CLEARTID)
    pub cmdline: [u8; MAX_CMDLINE_SIZE], // Command line arguments (null-separated, double-null terminated)
    pub cmdline_len: usize,              // Actual length of command line data
    pub open_fds: u16, // Bitmask of open file descriptors (bits 0-15 correspond to fd 3-18)
    // Per-process exec context (fixes race condition with global EXEC_CONTEXT)
    pub exec_pending: bool,   // True if execve has set new entry/stack
    pub exec_entry: u64,      // New entry point after execve
    pub exec_stack: u64,      // New stack pointer after execve  
    pub exec_user_data_sel: u64, // User data segment selector
}

/// Legacy global PID counter (kept for reference, use pid_tree::allocate_pid instead)
#[allow(dead_code)]
static NEXT_PID: AtomicU64 = AtomicU64::new(1);

/// Legacy PID allocation function
/// NOTE: This is deprecated. Use crate::process::allocate_pid() from pid_tree module instead,
/// which provides radix tree based PID management with O(log N) operations and PID recycling.
#[deprecated(
    since = "0.1.0",
    note = "Use crate::process::allocate_pid() from pid_tree module instead"
)]
#[allow(dead_code)]
pub fn allocate_pid_legacy() -> Pid {
    NEXT_PID.fetch_add(1, Ordering::SeqCst)
}

/// Default argv[0] value when none provided
pub const DEFAULT_ARGV0: &[u8] = b"nexa";

/// Build a cmdline buffer from argv array.
/// Returns (buffer, actual_length).
/// Format: null-separated arguments, double-null terminated.
pub fn build_cmdline(argv: &[&[u8]]) -> ([u8; MAX_CMDLINE_SIZE], usize) {
    let mut buffer = [0u8; MAX_CMDLINE_SIZE];
    let mut pos = 0usize;

    for arg in argv {
        if pos + arg.len() + 1 > MAX_CMDLINE_SIZE {
            break; // Truncate if too long
        }
        buffer[pos..pos + arg.len()].copy_from_slice(arg);
        pos += arg.len();
        buffer[pos] = 0; // Null separator
        pos += 1;
    }

    // If we have at least one argument, the buffer ends with one null.
    // Add an extra null for double-null termination if there's room.
    if pos < MAX_CMDLINE_SIZE {
        buffer[pos] = 0;
    }

    (buffer, pos)
}
