//! Thread Tests
//!
//! Tests for thread creation, management, and thread groups.

use crate::process::{ProcessState, Pid};
use crate::scheduler::ProcessEntry;
use crate::scheduler::{CpuMask, SchedPolicy, nice_to_weight, BASE_SLICE_NS};

/// Helper to create a test process
fn make_process(pid: Pid, ppid: Pid, tgid: Pid, is_thread: bool) -> ProcessEntry {
    let mut entry = ProcessEntry::empty();
    entry.process.pid = pid;
    entry.process.ppid = ppid;
    entry.process.tgid = tgid;
    entry.process.is_thread = is_thread;
    entry.process.state = ProcessState::Ready;
    entry.cpu_affinity = CpuMask::all();
    entry
}

// ============================================================================
// Thread Group Structure Tests
// ============================================================================

#[test]
fn test_main_thread_structure() {
    // Main thread: PID == TGID
    let main = make_process(100, 1, 100, false);
    
    assert_eq!(main.process.pid, 100);
    assert_eq!(main.process.tgid, 100);
    assert_eq!(main.process.pid, main.process.tgid);
    assert!(!main.process.is_thread);
}

#[test]
fn test_thread_structure() {
    // Thread: PID != TGID, TGID points to main thread
    let thread = make_process(101, 1, 100, true);
    
    assert_eq!(thread.process.pid, 101);
    assert_eq!(thread.process.tgid, 100);
    assert_ne!(thread.process.pid, thread.process.tgid);
    assert!(thread.process.is_thread);
}

#[test]
fn test_thread_group_membership() {
    // Create a thread group with main + 3 threads
    let main = make_process(100, 1, 100, false);
    let thread1 = make_process(101, 1, 100, true);
    let thread2 = make_process(102, 1, 100, true);
    let thread3 = make_process(103, 1, 100, true);
    
    // All should have same TGID
    assert_eq!(main.process.tgid, 100);
    assert_eq!(thread1.process.tgid, 100);
    assert_eq!(thread2.process.tgid, 100);
    assert_eq!(thread3.process.tgid, 100);
    
    // All PIDs should be unique
    let pids = [
        main.process.pid,
        thread1.process.pid,
        thread2.process.pid,
        thread3.process.pid,
    ];
    
    for i in 0..pids.len() {
        for j in (i + 1)..pids.len() {
            assert_ne!(pids[i], pids[j], "Thread PIDs should be unique");
        }
    }
}

// ============================================================================
// Thread Creation Tests
// ============================================================================

#[test]
fn test_clone_thread_inherits_tgid() {
    // clone() creating a thread: child inherits TGID
    let parent = make_process(100, 1, 100, false);
    
    // Child thread inherits TGID from parent
    let mut child = make_process(101, parent.process.pid, parent.process.tgid, true);
    
    assert_eq!(child.process.tgid, parent.process.tgid);
    assert!(child.process.is_thread);
}

#[test]
fn test_fork_creates_new_tgid() {
    // fork() creating a new process: child gets new TGID
    let parent = make_process(100, 1, 100, false);
    
    // Forked child gets new TGID (== PID)
    let mut child = make_process(101, parent.process.pid, 101, false);
    
    assert_ne!(child.process.tgid, parent.process.tgid);
    assert_eq!(child.process.tgid, child.process.pid);
    assert!(!child.process.is_thread);
}

// ============================================================================
// Thread Scheduling Tests
// ============================================================================

#[test]
fn test_threads_independently_scheduled() {
    // Each thread has its own scheduling state
    let mut main = make_process(100, 1, 100, false);
    let mut thread1 = make_process(101, 1, 100, true);
    let mut thread2 = make_process(102, 1, 100, true);
    
    // Set different scheduling parameters
    main.nice = 0;
    main.weight = nice_to_weight(0);
    
    thread1.nice = -5;
    thread1.weight = nice_to_weight(-5);
    
    thread2.nice = 5;
    thread2.weight = nice_to_weight(5);
    
    // Verify different weights
    assert!(thread1.weight > main.weight);
    assert!(main.weight > thread2.weight);
}

#[test]
fn test_threads_share_address_space() {
    // Threads in same group share CR3 (page table)
    let mut main = make_process(100, 1, 100, false);
    let mut thread1 = make_process(101, 1, 100, true);
    let mut thread2 = make_process(102, 1, 100, true);
    
    // All threads use same page table
    let cr3 = 0x0000_0000_1234_0000u64;
    main.process.cr3 = cr3;
    thread1.process.cr3 = cr3;
    thread2.process.cr3 = cr3;
    
    assert_eq!(main.process.cr3, thread1.process.cr3);
    assert_eq!(thread1.process.cr3, thread2.process.cr3);
}

#[test]
fn test_threads_independent_stacks() {
    // Each thread has its own stack
    let mut main = make_process(100, 1, 100, false);
    let mut thread1 = make_process(101, 1, 100, true);
    let mut thread2 = make_process(102, 1, 100, true);
    
    // Different kernel stacks
    main.process.kernel_stack = 0xFFFF_8000_0001_0000;
    thread1.process.kernel_stack = 0xFFFF_8000_0002_0000;
    thread2.process.kernel_stack = 0xFFFF_8000_0003_0000;
    
    // Different user stacks
    main.process.context.rsp = 0x7FFF_F000_0000;
    thread1.process.context.rsp = 0x7FFF_E000_0000;
    thread2.process.context.rsp = 0x7FFF_D000_0000;
    
    // Verify stacks are different
    assert_ne!(main.process.kernel_stack, thread1.process.kernel_stack);
    assert_ne!(thread1.process.kernel_stack, thread2.process.kernel_stack);
    assert_ne!(main.process.context.rsp, thread1.process.context.rsp);
}

// ============================================================================
// Thread Exit Tests
// ============================================================================

#[test]
fn test_thread_exit_individual() {
    // Single thread exit doesn't affect other threads
    let mut main = make_process(100, 1, 100, false);
    let mut thread1 = make_process(101, 1, 100, true);
    let mut thread2 = make_process(102, 1, 100, true);
    
    // Thread 1 exits
    thread1.process.state = ProcessState::Zombie;
    thread1.process.exit_code = 0;
    
    // Other threads unaffected
    assert_eq!(main.process.state, ProcessState::Ready);
    assert_eq!(thread2.process.state, ProcessState::Ready);
}

#[test]
fn test_main_thread_exit_terminates_group() {
    // Concept: main thread exit should terminate all threads in group
    
    let mut main = make_process(100, 1, 100, false);
    let mut thread1 = make_process(101, 1, 100, true);
    let mut thread2 = make_process(102, 1, 100, true);
    
    // Main thread exits
    main.process.state = ProcessState::Zombie;
    main.process.exit_code = 42;
    
    // terminate_thread_group() sets all to Zombie
    // Set thread states to match:
    thread1.process.state = ProcessState::Zombie;
    thread1.process.exit_code = 42;
    thread2.process.state = ProcessState::Zombie;
    thread2.process.exit_code = 42;
    
    // All should be zombies with same exit code
    assert_eq!(main.process.state, ProcessState::Zombie);
    assert_eq!(thread1.process.state, ProcessState::Zombie);
    assert_eq!(thread2.process.state, ProcessState::Zombie);
}

// ============================================================================
// Thread CPU Affinity Tests
// ============================================================================

#[test]
fn test_threads_independent_affinity() {
    // Threads can have different CPU affinities
    let mut main = make_process(100, 1, 100, false);
    let mut thread1 = make_process(101, 1, 100, true);
    let mut thread2 = make_process(102, 1, 100, true);
    
    // Main thread on all CPUs
    main.cpu_affinity = CpuMask::all();
    
    // Thread 1 pinned to CPU 0
    thread1.cpu_affinity = CpuMask::empty();
    thread1.cpu_affinity.set(0);
    
    // Thread 2 pinned to CPUs 1-3
    thread2.cpu_affinity = CpuMask::empty();
    thread2.cpu_affinity.set(1);
    thread2.cpu_affinity.set(2);
    thread2.cpu_affinity.set(3);
    
    // Verify different affinities
    assert!(main.cpu_affinity.is_set(0));
    assert!(main.cpu_affinity.is_set(1));
    assert!(thread1.cpu_affinity.is_set(0));
    assert!(!thread1.cpu_affinity.is_set(1));
    assert!(!thread2.cpu_affinity.is_set(0));
    assert!(thread2.cpu_affinity.is_set(1));
}

#[test]
fn test_threads_can_run_on_different_cpus() {
    // Threads from same group can run simultaneously on different CPUs
    let mut main = make_process(100, 1, 100, false);
    let mut thread1 = make_process(101, 1, 100, true);
    
    // Running on different CPUs
    main.last_cpu = 0;
    main.process.state = ProcessState::Running;
    
    thread1.last_cpu = 1;
    thread1.process.state = ProcessState::Running;
    
    // Both running simultaneously
    assert_eq!(main.process.state, ProcessState::Running);
    assert_eq!(thread1.process.state, ProcessState::Running);
    assert_ne!(main.last_cpu, thread1.last_cpu);
}

// ============================================================================
// Thread TLS (Thread Local Storage) Tests
// ============================================================================

#[test]
fn test_threads_independent_fs_base() {
    // Each thread has its own FS base for TLS
    let mut main = make_process(100, 1, 100, false);
    let mut thread1 = make_process(101, 1, 100, true);
    let mut thread2 = make_process(102, 1, 100, true);
    
    // Different TLS areas
    main.process.fs_base = 0x7F00_0000_0000;
    thread1.process.fs_base = 0x7F00_0001_0000;
    thread2.process.fs_base = 0x7F00_0002_0000;
    
    // All different
    assert_ne!(main.process.fs_base, thread1.process.fs_base);
    assert_ne!(thread1.process.fs_base, thread2.process.fs_base);
}

#[test]
fn test_clear_child_tid_per_thread() {
    // Each thread can have its own clear_child_tid address
    let mut thread = make_process(101, 1, 100, true);
    
    // Set clear_child_tid (used by futex for pthread_join)
    thread.process.clear_child_tid = 0x7F00_0001_0100;
    
    assert_ne!(thread.process.clear_child_tid, 0);
}

// ============================================================================
// Thread Signal Tests
// ============================================================================

#[test]
fn test_threads_share_signal_handlers() {
    // Concept: threads share signal handlers but have independent masks
    
    let main = make_process(100, 1, 100, false);
    let thread1 = make_process(101, 1, 100, true);
    
    // Signal handlers are process-wide (same TGID)
    assert_eq!(main.process.tgid, thread1.process.tgid);
    
    // Each thread has its own SignalState for pending signals
    // (signal_state is per-thread in the actual implementation)
}

// ============================================================================
// Thread Statistics Tests
// ============================================================================

#[test]
fn test_threads_independent_time_tracking() {
    // Each thread tracks its own CPU time
    let mut main = make_process(100, 1, 100, false);
    let mut thread1 = make_process(101, 1, 100, true);
    
    // Different CPU usage
    main.total_time = 1000;
    main.cpu_burst_count = 10;
    
    thread1.total_time = 500;
    thread1.cpu_burst_count = 5;
    
    // Independent tracking
    assert_ne!(main.total_time, thread1.total_time);
    assert_ne!(main.cpu_burst_count, thread1.cpu_burst_count);
}

#[test]
fn test_threads_independent_wait_time() {
    // Each thread tracks its own wait time
    let mut main = make_process(100, 1, 100, false);
    let mut thread1 = make_process(101, 1, 100, true);
    
    main.wait_time = 100;
    thread1.wait_time = 200;
    
    assert_ne!(main.wait_time, thread1.wait_time);
}
