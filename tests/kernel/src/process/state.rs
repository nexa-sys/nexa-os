//! Process State Tests
//!
//! Tests for process state transitions and state management.

use crate::process::{ProcessState, Pid};

// ============================================================================
// ProcessState Tests
// ============================================================================

#[test]
fn test_process_state_values() {
    // Verify all states are distinct
    assert_ne!(ProcessState::Ready, ProcessState::Running);
    assert_ne!(ProcessState::Running, ProcessState::Sleeping);
    assert_ne!(ProcessState::Sleeping, ProcessState::Zombie);
    assert_ne!(ProcessState::Zombie, ProcessState::Ready);
}

#[test]
fn test_process_state_copy() {
    let state = ProcessState::Running;
    let copy = state;
    assert_eq!(state, copy);
}

#[test]
fn test_process_state_transitions() {
    // Valid transitions:
    // Ready -> Running (scheduled)
    // Running -> Ready (preempted/yield)
    // Running -> Sleeping (blocking I/O)
    // Sleeping -> Ready (wakeup)
    // Running -> Zombie (exit)
    // Ready/Running/Sleeping -> Stopped (SIGSTOP)
    // Stopped -> Ready (SIGCONT)
    
    // These are conceptual - actual transitions are managed by scheduler
}

// ============================================================================
// Process Entry State Management Tests
// ============================================================================

#[test]
fn test_process_entry_initial_state() {
    use crate::scheduler::ProcessEntry;
    
    let entry = ProcessEntry::empty();
    assert_eq!(entry.process.state, ProcessState::Ready);
    assert_eq!(entry.process.pid, 0);
    assert_eq!(entry.process.ppid, 0);
}

#[test]
fn test_process_entry_exit_code() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    entry.process.pid = 1;
    
    // Set exit code
    entry.process.exit_code = 42;
    assert_eq!(entry.process.exit_code, 42);
    
    // Transition to Zombie
    entry.process.state = ProcessState::Zombie;
    assert_eq!(entry.process.state, ProcessState::Zombie);
    assert_eq!(entry.process.exit_code, 42);
}

#[test]
fn test_process_entry_term_signal() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    entry.process.pid = 1;
    
    // No termination signal initially
    assert!(entry.process.term_signal.is_none());
    
    // Set termination signal (e.g., SIGTERM = 15)
    entry.process.term_signal = Some(15);
    assert_eq!(entry.process.term_signal, Some(15));
}

// ============================================================================
// Thread Group Tests
// ============================================================================

#[test]
fn test_thread_group_id() {
    use crate::scheduler::ProcessEntry;
    
    // Main process
    let mut main = ProcessEntry::empty();
    main.process.pid = 100;
    main.process.tgid = 100; // Thread group ID = PID for main thread
    main.process.is_thread = false;
    
    // Thread in same group
    let mut thread = ProcessEntry::empty();
    thread.process.pid = 101;
    thread.process.tgid = 100; // Same thread group
    thread.process.is_thread = true;
    
    assert_eq!(main.process.tgid, main.process.pid);
    assert_eq!(thread.process.tgid, main.process.tgid);
    assert_ne!(thread.process.pid, main.process.pid);
}

#[test]
fn test_thread_detection() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // Not a thread by default
    assert!(!entry.process.is_thread);
    
    // Mark as thread
    entry.process.is_thread = true;
    assert!(entry.process.is_thread);
}

// ============================================================================
// Fork/Clone State Tests
// ============================================================================

#[test]
fn test_fork_child_flag() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // Not a fork child initially
    assert!(!entry.process.is_fork_child);
    
    // Mark as fork child (set during fork syscall)
    entry.process.is_fork_child = true;
    assert!(entry.process.is_fork_child);
}

#[test]
fn test_parent_child_relationship() {
    use crate::scheduler::ProcessEntry;
    
    // Parent process
    let mut parent = ProcessEntry::empty();
    parent.process.pid = 1;
    parent.process.ppid = 0; // Init has no parent
    
    // Child process
    let mut child = ProcessEntry::empty();
    child.process.pid = 2;
    child.process.ppid = 1; // Parent is PID 1
    
    assert_eq!(child.process.ppid, parent.process.pid);
}

// ============================================================================
// User Context Tests
// ============================================================================

#[test]
fn test_user_context_fields() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // Set user context
    entry.process.user_rip = 0x0040_0000;  // User code address
    entry.process.user_rsp = 0x7FFF_FF00;  // User stack
    entry.process.user_rflags = 0x202;     // IF flag set
    
    assert_eq!(entry.process.user_rip, 0x0040_0000);
    assert_eq!(entry.process.user_rsp, 0x7FFF_FF00);
    assert_eq!(entry.process.user_rflags, 0x202);
}

#[test]
fn test_kernel_stack() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // Set kernel stack
    entry.process.kernel_stack = 0xFFFF_8000_0001_0000;
    
    // Kernel stack should be in kernel address space
    assert!(entry.process.kernel_stack >= 0xFFFF_8000_0000_0000);
}

// ============================================================================
// Memory Layout Tests
// ============================================================================

#[test]
fn test_memory_regions() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // Set memory regions
    entry.process.memory_base = 0x0100_0000;
    entry.process.memory_size = 0x0010_0000; // 1MB
    entry.process.heap_start = 0x0110_0000;
    entry.process.heap_end = 0x0110_1000;
    
    // Heap should be above memory base
    assert!(entry.process.heap_start >= entry.process.memory_base);
    
    // Heap end should be >= heap start
    assert!(entry.process.heap_end >= entry.process.heap_start);
}

#[test]
fn test_cr3_page_table() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // CR3 holds page table base
    entry.process.cr3 = 0x0000_0000_1234_0000;
    
    // CR3 should be page-aligned (4KB)
    assert_eq!(entry.process.cr3 & 0xFFF, 0);
}

// ============================================================================
// TTY and File Descriptor Tests
// ============================================================================

#[test]
fn test_controlling_tty() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // No controlling TTY initially
    assert_eq!(entry.process.tty, 0);
    
    // Set controlling TTY (e.g., /dev/tty1)
    entry.process.tty = 1;
    assert_eq!(entry.process.tty, 1);
}

#[test]
fn test_open_fd_count() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // Track open FDs
    assert_eq!(entry.process.open_fds, 0);
    
    // Opening files
    entry.process.open_fds = 3; // stdin, stdout, stderr
    assert_eq!(entry.process.open_fds, 3);
}

// ============================================================================
// Command Line Tests
// ============================================================================

#[test]
fn test_cmdline_storage() {
    use crate::scheduler::ProcessEntry;
    use crate::process::MAX_CMDLINE_SIZE;
    
    let mut entry = ProcessEntry::empty();
    
    // Command line is empty initially
    assert_eq!(entry.process.cmdline_len, 0);
    
    // Store command line
    let cmdline = b"/bin/init";
    let len = cmdline.len().min(MAX_CMDLINE_SIZE);
    entry.process.cmdline[..len].copy_from_slice(cmdline);
    entry.process.cmdline_len = len;
    
    assert_eq!(entry.process.cmdline_len, 9);
    assert_eq!(&entry.process.cmdline[..len], cmdline);
}

// ============================================================================
// Context Valid Flag Tests
// ============================================================================

#[test]
fn test_context_valid_flag() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // Context not valid initially (before first run)
    assert!(!entry.process.context_valid);
    
    // Mark context as valid (after first context save)
    entry.process.context_valid = true;
    assert!(entry.process.context_valid);
}

#[test]
fn test_has_entered_user_flag() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // Not entered user mode yet
    assert!(!entry.process.has_entered_user);
    
    // Mark as having entered user mode
    entry.process.has_entered_user = true;
    assert!(entry.process.has_entered_user);
}

// ============================================================================
// FS Base and Thread Local Storage Tests
// ============================================================================

#[test]
fn test_fs_base() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // FS base for thread-local storage
    entry.process.fs_base = 0x7F00_0000_0000;
    
    // Should be in user address space
    assert!(entry.process.fs_base < 0x8000_0000_0000_0000);
}

#[test]
fn test_clear_child_tid() {
    use crate::scheduler::ProcessEntry;
    
    let mut entry = ProcessEntry::empty();
    
    // Used by futex for thread exit notification
    assert_eq!(entry.process.clear_child_tid, 0);
    
    // Set clear_child_tid address
    entry.process.clear_child_tid = 0x7F00_0001_0000;
    assert_eq!(entry.process.clear_child_tid, 0x7F00_0001_0000);
}
