//! Foreground Process Unresponsive Bug Detection Tests
//!
//! Uses REAL kernel functions - NO local re-implementations.
//!
//! CRITICAL: These tests MUST FAIL when the bug exists and PASS only when fixed.

use crate::scheduler::{
    wake_process, set_process_state,
    SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
    process_table_lock,
    // Use REAL kernel query/setter functions
    get_process_state, get_process_vruntime, get_process_lag,
    set_process_vruntime, set_process_lag,
};
use crate::scheduler::percpu::{check_need_resched, init_percpu_sched};
use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE};
use crate::signal::SignalState;

use std::sync::Once;
use serial_test::serial;
use std::sync::atomic::{AtomicU64, Ordering};

static INIT_PERCPU: Once = Once::new();
static NEXT_PID: AtomicU64 = AtomicU64::new(50000);

fn next_pid() -> Pid {
    NEXT_PID.fetch_add(1, Ordering::SeqCst)
}

fn ensure_percpu_init() {
    INIT_PERCPU.call_once(|| {
        init_percpu_sched(0);
    });
}

fn make_test_process(pid: Pid, state: ProcessState) -> Process {
    Process {
        pid,
        ppid: 1,
        tgid: pid,
        state,
        entry_point: 0x1000000,
        stack_top: 0x1A00000,
        heap_start: 0x1200000,
        heap_end: 0x1200000,
        signal_state: SignalState::new(),
        context: crate::process::Context::zero(),
        has_entered_user: true,
        context_valid: true,
        is_fork_child: false,
        is_thread: false,
        cr3: 0x1000,
        tty: 0,
        memory_base: 0x1000000,
        memory_size: 0x1000000,
        user_rip: 0x1000100,
        user_rsp: 0x19FFF00,
        user_rflags: 0x202,
        user_r10: 0,
        user_r8: 0,
        user_r9: 0,
        exit_code: 0,
        term_signal: None,
        kernel_stack: 0x2000000,
        fs_base: 0,
        clear_child_tid: 0,
        cmdline: [0; MAX_CMDLINE_SIZE],
        cmdline_len: 0,
        open_fds: 0,
        exec_pending: false,
        exec_entry: 0,
        exec_stack: 0,
        exec_user_data_sel: 0,
        wake_pending: false,
    }
}

fn make_process_entry(proc: Process) -> ProcessEntry {
    ProcessEntry {
        process: proc,
        vruntime: 0,
        vdeadline: BASE_SLICE_NS,
        lag: 0,
        weight: NICE_0_WEIGHT,
        slice_ns: BASE_SLICE_NS,
        slice_remaining_ns: BASE_SLICE_NS,
        priority: 100,
        base_priority: 100,
        time_slice: 100,
        total_time: 0,
        wait_time: 0,
        last_scheduled: 0,
        cpu_burst_count: 0,
        avg_cpu_burst: 0,
        policy: SchedPolicy::Normal,
        nice: 0,
        quantum_level: 0,
        preempt_count: 0,
        voluntary_switches: 0,
        cpu_affinity: CpuMask::all(),
        last_cpu: 0,
        numa_preferred_node: 0,
        numa_policy: crate::numa::NumaPolicy::Local,
    }
}

fn add_process(pid: Pid, state: ProcessState) {
    ensure_percpu_init();
    let mut table = process_table_lock();
    for (idx, slot) in table.iter_mut().enumerate() {
        if slot.is_none() {
            crate::process::register_pid_mapping(pid, idx as u16);
            let mut entry = make_process_entry(make_test_process(pid, state));
            entry.process.state = state;
            *slot = Some(entry);
            return;
        }
    }
    panic!("Process table full");
}

fn cleanup_process(pid: Pid) {
    let mut table = process_table_lock();
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                crate::process::unregister_pid_mapping(pid);
                *slot = None;
                break;
            }
        }
    }
}

fn cleanup_processes(pids: &[Pid]) {
    let mut table = process_table_lock();
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if pids.contains(&entry.process.pid) {
                crate::process::unregister_pid_mapping(entry.process.pid);
                *slot = None;
            }
        }
    }
}

// =============================================================================
// BUG DETECTION TESTS - Using REAL kernel functions
// =============================================================================

#[test]
#[serial]
fn test_race_wake_before_sleep_must_not_strand() {
    let pid = next_pid();
    add_process(pid, ProcessState::Ready);
    
    let _woke = wake_process(pid);
    let _ = set_process_state(pid, ProcessState::Sleeping);
    
    // Use REAL kernel function
    let state = get_process_state(pid);
    
    cleanup_process(pid);
    
    assert_ne!(state, Some(ProcessState::Sleeping),
        "BUG: Process stuck in Sleeping after wake-before-sleep race!");
}

#[test]
#[serial]
fn test_wake_ready_prevents_sleep() {
    let pid = next_pid();
    add_process(pid, ProcessState::Ready);
    
    wake_process(pid);
    let _ = set_process_state(pid, ProcessState::Sleeping);
    
    // Use REAL kernel function
    let state = get_process_state(pid);
    
    cleanup_process(pid);
    
    assert_ne!(state, Some(ProcessState::Sleeping),
        "BUG: Process slept after wake was called on Ready state!");
}

#[test]
#[serial]
fn test_rapid_race_no_stuck_process() {
    let pid = next_pid();
    add_process(pid, ProcessState::Ready);
    
    let mut stuck_iterations = Vec::new();
    
    for i in 0..50 {
        let _ = set_process_state(pid, ProcessState::Ready);
        wake_process(pid);
        let _ = set_process_state(pid, ProcessState::Sleeping);
        
        // Use REAL kernel function
        if get_process_state(pid) == Some(ProcessState::Sleeping) {
            stuck_iterations.push(i);
            wake_process(pid);
        }
    }
    
    cleanup_process(pid);
    
    assert!(stuck_iterations.is_empty(),
        "BUG: Process got stuck in {} iterations: {:?}", 
        stuck_iterations.len(), stuck_iterations);
}

#[test]
#[serial]
fn test_wake_must_set_need_resched() {
    let pid = next_pid();
    add_process(pid, ProcessState::Sleeping);
    
    let _ = check_need_resched();
    let woke = wake_process(pid);
    let need_resched = check_need_resched();
    
    cleanup_process(pid);
    
    assert!(woke, "wake_process should succeed for Sleeping process");
    assert!(need_resched, "BUG: wake_process did not set need_resched!");
}

#[test]
#[serial]
fn test_wake_must_reset_lag_nonnegative() {
    let pid = next_pid();
    add_process(pid, ProcessState::Sleeping);
    
    // Use REAL kernel setter
    let _ = set_process_lag(pid, -50_000_000);
    
    wake_process(pid);
    
    // Use REAL kernel getter
    let lag = get_process_lag(pid);
    
    cleanup_process(pid);
    
    assert!(lag.unwrap_or(-1) >= 0,
        "BUG: lag ({:?}) is negative after wake!", lag);
}

#[test]
#[serial]
fn test_woken_vruntime_allows_scheduling() {
    let bg_pid = next_pid();
    let shell_pid = next_pid();
    
    add_process(bg_pid, ProcessState::Running);
    // Use REAL kernel setter
    let _ = set_process_vruntime(bg_pid, 500_000_000);
    
    add_process(shell_pid, ProcessState::Sleeping);
    let _ = set_process_vruntime(shell_pid, 0);
    
    wake_process(shell_pid);
    
    // Use REAL kernel getter
    let shell_vrt = get_process_vruntime(shell_pid).unwrap_or(u64::MAX);
    let bg_vrt = get_process_vruntime(bg_pid).unwrap_or(0);
    
    cleanup_processes(&[bg_pid, shell_pid]);
    
    assert!(shell_vrt <= bg_vrt,
        "BUG: Woken shell vruntime ({}) > background ({})", shell_vrt, bg_vrt);
}

#[test]
#[serial]
fn test_keyboard_read_race_sequence() {
    let shell_pid = next_pid();
    add_process(shell_pid, ProcessState::Ready);
    
    let _woke = wake_process(shell_pid);
    let _ = set_process_state(shell_pid, ProcessState::Sleeping);
    
    // Use REAL kernel function
    let state = get_process_state(shell_pid);
    
    cleanup_process(shell_pid);
    
    assert_ne!(state, Some(ProcessState::Sleeping),
        "BUG: Shell stuck after keyboard read race!");
}

#[test]
#[serial]
fn test_normal_wake_sleeping_works() {
    let pid = next_pid();
    add_process(pid, ProcessState::Sleeping);
    
    // Use REAL kernel function
    let state_before = get_process_state(pid);
    assert_eq!(state_before, Some(ProcessState::Sleeping));
    
    let woke = wake_process(pid);
    
    // Use REAL kernel function
    let state = get_process_state(pid);
    
    cleanup_process(pid);
    
    assert!(woke, "wake_process should return true for Sleeping process");
    assert_eq!(state, Some(ProcessState::Ready));
}
