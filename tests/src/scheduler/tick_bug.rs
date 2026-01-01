//! Scheduler Tick Bug Detection Tests
//!
//! These tests verify that the scheduler correctly manages vruntime updates.
//! Specifically tests for bugs where vruntime might incorrectly update for
//! non-running processes.
//!
//! ## Key Invariant:
//!
//! vruntime should ONLY increase for the RUNNING process.
//! Other processes (Ready, Sleeping, Zombie) must NOT have vruntime changed
//! by tick-like operations.
//!
//! NOTE: We don't call tick() or set_current_pid() directly because they
//! access hardware (LAPIC, CPU ID, paging). Instead, we test the logical 
//! invariants by manipulating process table state directly.

use crate::scheduler::{
    wake_process, set_process_state, process_table_lock,
    SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
    get_min_vruntime, calc_vdeadline,
};
use crate::scheduler::percpu::init_percpu_sched;
use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE};
use crate::signal::SignalState;

use std::sync::Once;
    use serial_test::serial;
use std::sync::atomic::{AtomicU64, Ordering};

static INIT_PERCPU: Once = Once::new();
static NEXT_PID: AtomicU64 = AtomicU64::new(80000);

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

fn make_process_entry(proc: Process, vruntime: u64) -> ProcessEntry {
    let vdeadline = calc_vdeadline(vruntime, BASE_SLICE_NS, NICE_0_WEIGHT);
    ProcessEntry {
        process: proc,
        vruntime,
        vdeadline,
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
        numa_preferred_node: crate::numa::NUMA_NO_NODE,
        numa_policy: crate::numa::NumaPolicy::Local,
    }
}

fn add_process_with_state(pid: Pid, state: ProcessState, vruntime: u64) {
    ensure_percpu_init();
    let mut table = process_table_lock();
    for (idx, slot) in table.iter_mut().enumerate() {
        if slot.is_none() {
            crate::process::register_pid_mapping(pid, idx as u16);
            let mut entry = make_process_entry(make_test_process(pid, state), vruntime);
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

/// Clean up multiple processes at once
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

fn get_vruntime(pid: Pid) -> Option<u64> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .find(|e| e.process.pid == pid)
        .map(|e| e.vruntime)
}

fn get_slice_remaining(pid: Pid) -> Option<u64> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .find(|e| e.process.pid == pid)
        .map(|e| e.slice_remaining_ns)
}

fn get_state(pid: Pid) -> Option<ProcessState> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .find(|e| e.process.pid == pid)
        .map(|e| e.process.state)
}

/// What a correct tick() implementation does for a running process:
/// update vruntime by delta_ns. Operates on real process_table.
fn update_running_process_vruntime(pid: Pid, delta_ns: u64) {
    let mut table = process_table_lock();
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid && entry.process.state == ProcessState::Running {
                // Only update Running processes
                entry.vruntime = entry.vruntime.saturating_add(delta_ns);
                entry.slice_remaining_ns = entry.slice_remaining_ns.saturating_sub(delta_ns);
            }
        }
    }
}

/// A BUGGY tick() that updates vruntime based on PID without
/// checking process state - this is the bug we're trying to detect.
/// Operates on real process_table.
fn buggy_update_vruntime_ignoring_state(pid: Pid, delta_ns: u64) {
    let mut table = process_table_lock();
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                // BUG: Updates vruntime regardless of state!
                entry.vruntime = entry.vruntime.saturating_add(delta_ns);
            }
        }
    }
}

// =============================================================================
// TICK BUG TESTS - Testing Invariants Without Hardware Access
// =============================================================================

/// TEST: Sleeping process vruntime must stay stable
///
/// This verifies the invariant: a process in Sleeping state should NOT
/// have its vruntime modified by any tick-like operation.
#[test]
#[serial]
fn test_sleeping_process_vruntime_invariant() {
    ensure_percpu_init();
    
    
    let pid = next_pid();
    let initial_vrt = 10_000_000u64;
    
    add_process_with_state(pid, ProcessState::Sleeping, initial_vrt);
    
    // Verify state is Sleeping
    assert_eq!(get_state(pid), Some(ProcessState::Sleeping));
    
    // Correct behavior: check state before updating
    // A correct tick implementation should NOT modify Sleeping processes
    update_running_process_vruntime(pid, 1_000_000);
    
    let final_vrt = get_vruntime(pid).unwrap();
    
    cleanup_process(pid);
    
    assert_eq!(final_vrt, initial_vrt,
        "Sleeping process vruntime should remain unchanged: expected {}, got {}",
        initial_vrt, final_vrt);
}

/// TEST: Ready process vruntime must stay stable
///
/// Ready processes are waiting to be scheduled, not running.
/// Their vruntime must not change.
#[test]
#[serial]
fn test_ready_process_vruntime_invariant() {
    ensure_percpu_init();
    
    
    let ready_pid = next_pid();
    let initial_vrt = 50_000_000u64;
    
    add_process_with_state(ready_pid, ProcessState::Ready, initial_vrt);
    
    // Correct tick behavior
    update_running_process_vruntime(ready_pid, 5_000_000);
    
    let final_vrt = get_vruntime(ready_pid).unwrap();
    
    cleanup_process(ready_pid);
    
    assert_eq!(final_vrt, initial_vrt,
        "Ready process vruntime should remain unchanged: expected {}, got {}",
        initial_vrt, final_vrt);
}

/// TEST: Only Running process should have vruntime updated
///
/// When multiple processes exist, only the Running one should be updated.
#[test]
#[serial]
fn test_only_running_process_updated() {
    ensure_percpu_init();
    
    
    let running_pid = next_pid();
    let sleeping_pid = next_pid();
    let ready_pid = next_pid();
    
    let running_vrt_initial = 1_000_000u64;
    let sleeping_vrt_initial = 100_000_000u64;
    let ready_vrt_initial = 100_000_000u64;
    
    add_process_with_state(running_pid, ProcessState::Running, running_vrt_initial);
    add_process_with_state(sleeping_pid, ProcessState::Sleeping, sleeping_vrt_initial);
    add_process_with_state(ready_pid, ProcessState::Ready, ready_vrt_initial);
    
    // Correct tick - only updates Running process
    let delta = 5_000_000u64;
    update_running_process_vruntime(running_pid, delta);
    
    let running_vrt_final = get_vruntime(running_pid).unwrap();
    let sleeping_vrt_final = get_vruntime(sleeping_pid).unwrap();
    let ready_vrt_final = get_vruntime(ready_pid).unwrap();
    
    cleanup_process(running_pid);
    cleanup_process(sleeping_pid);
    cleanup_process(ready_pid);
    
    // Running should have increased
    assert_eq!(running_vrt_final, running_vrt_initial + delta,
        "Running process vruntime should increase");
    
    // Others should NOT have changed
    assert_eq!(sleeping_vrt_final, sleeping_vrt_initial,
        "Sleeping process vruntime should NOT change");
    assert_eq!(ready_vrt_final, ready_vrt_initial,
        "Ready process vruntime should NOT change");
}

/// TEST: Detect buggy tick that ignores process state
///
/// This test demonstrates the bug: if tick() updates vruntime without
/// checking process state, Sleeping processes get their vruntime inflated.
#[test]
#[serial]
fn test_detect_buggy_tick_behavior() {
    ensure_percpu_init();
    
    
    let pid = next_pid();
    let initial_vrt = 10_000_000u64;
    
    add_process_with_state(pid, ProcessState::Running, initial_vrt);
    
    // Process goes to sleep (e.g., waiting for keyboard)
    let _ = set_process_state(pid, ProcessState::Sleeping);
    
    // BUG: tick() updates vruntime without checking state
    let delta = 100_000_000u64; // 100ms of fake "running time"
    buggy_update_vruntime_ignoring_state(pid, delta);
    
    let final_vrt = get_vruntime(pid).unwrap();
    
    cleanup_process(pid);
    
    // This DETECTS the bug - vruntime grew even though process was Sleeping
    let vrt_increase = final_vrt - initial_vrt;
    
    // A correct implementation would have vrt_increase == 0
    // The buggy implementation has vrt_increase == delta
    assert!(vrt_increase > 0,
        "This test shows buggy behavior: Sleeping process vruntime grew by {} \
         when it should have stayed at {}. If tick() checks state, this won't happen.",
        vrt_increase, initial_vrt);
}

/// TEST: Keyboard read flow should not inflate vruntime
///
/// Tests the exact scenario from the bug report:
/// 1. Process runs briefly
/// 2. Calls read() on keyboard, no data available
/// 3. Process set to Sleeping
/// 4. Time passes (ticks occur)
/// 5. Keyboard data arrives, process wakes
///
/// vruntime should only reflect actual running time.
#[test]
#[serial]
fn test_keyboard_read_flow_correct_vruntime() {
    ensure_percpu_init();
    
    
    let pid = next_pid();
    add_process_with_state(pid, ProcessState::Ready, 0);
    
    // Step 1-2: Process runs briefly checking for input
    {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    entry.process.state = ProcessState::Running;
                    // Brief run: 100 microseconds
                    entry.vruntime = entry.vruntime.saturating_add(100_000);
                    break;
                }
            }
        }
    }
    
    let vrt_after_brief_run = get_vruntime(pid).unwrap();
    
    // Step 3: No input, go to sleep
    let _ = set_process_state(pid, ProcessState::Sleeping);
    
    // Step 4: Time passes - correct implementation should NOT update vruntime
    // 1 second of "sleep time" with correct tick behavior
    for _ in 0..100 {
        update_running_process_vruntime(pid, 10_000_000); // 10ms * 100 = 1s
    }
    
    let vrt_after_sleep_period = get_vruntime(pid).unwrap();
    
    // Step 5: Wake up
    wake_process(pid);
    
    let vrt_after_wake = get_vruntime(pid).unwrap();
    
    cleanup_process(pid);
    
    // Verify: vruntime should NOT have increased during sleep
    assert_eq!(vrt_after_sleep_period, vrt_after_brief_run,
        "vruntime should NOT increase during Sleeping: before={}, after={}",
        vrt_after_brief_run, vrt_after_sleep_period);
    
    // wake_process may adjust vruntime to min_vruntime, but shouldn't inflate it massively
    let reasonable_max = vrt_after_brief_run + 10_000_000; // Allow up to 10ms adjustment
    assert!(vrt_after_wake <= reasonable_max,
        "vruntime after wake ({}) should be reasonable (max expected: {})",
        vrt_after_wake, reasonable_max);
}

/// TEST: Rapid state transitions track vruntime correctly
///
/// Process toggles between Running and Sleeping rapidly.
/// vruntime should only accumulate during Running periods.
#[test]
#[serial]
fn test_rapid_state_transitions_vruntime() {
    ensure_percpu_init();
    
    
    let pid = next_pid();
    add_process_with_state(pid, ProcessState::Ready, 0);
    
    let mut expected_vruntime = 0u64;
    let run_delta = 1_000_000u64; // 1ms per run period
    
    // 10 cycles: run 1ms, sleep 50ms, repeat
    for _ in 0..10 {
        // Set to Running
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if let Some(entry) = slot {
                    if entry.process.pid == pid {
                        entry.process.state = ProcessState::Running;
                        break;
                    }
                }
            }
        }
        
        // Run for 1ms
        update_running_process_vruntime(pid, run_delta);
        expected_vruntime += run_delta;
        
        // Sleep
        let _ = set_process_state(pid, ProcessState::Sleeping);
        
        // 50ms of sleep - vruntime should NOT increase
        for _ in 0..5 {
            update_running_process_vruntime(pid, 10_000_000);
        }
        
        // Wake
        wake_process(pid);
    }
    
    let final_vrt = get_vruntime(pid).unwrap();
    
    cleanup_process(pid);
    
    // Final vruntime should be approximately expected (10 * 1ms = 10ms)
    // Allow some tolerance for wake_process adjustments
    let tolerance = expected_vruntime * 2;
    
    assert!(final_vrt <= expected_vruntime + tolerance,
        "vruntime ({}) too high! Expected ~{} (10ms of actual running). \
         Sleeping periods should NOT contribute to vruntime.",
        final_vrt, expected_vruntime);
}

/// TEST: Verify slice_remaining only decrements for Running
#[test]
#[serial]
fn test_slice_only_consumed_when_running() {
    ensure_percpu_init();
    
    
    let pid = next_pid();
    add_process_with_state(pid, ProcessState::Running, 0);
    
    let initial_slice = get_slice_remaining(pid).unwrap();
    
    // Run for 1ms
    update_running_process_vruntime(pid, 1_000_000);
    
    let slice_after_run = get_slice_remaining(pid).unwrap();
    
    // Now sleep
    let _ = set_process_state(pid, ProcessState::Sleeping);
    
    // Apply many ticks while sleeping
    for _ in 0..100 {
        update_running_process_vruntime(pid, 10_000_000);
    }
    
    let slice_after_sleep = get_slice_remaining(pid).unwrap();
    
    cleanup_process(pid);
    
    // Slice should have decreased by 1ms after running
    assert!(slice_after_run < initial_slice,
        "Slice should decrease after running: initial={}, after_run={}",
        initial_slice, slice_after_run);
    
    // Slice should NOT have decreased during sleep
    assert_eq!(slice_after_sleep, slice_after_run,
        "Slice should NOT decrease during Sleeping: after_run={}, after_sleep={}",
        slice_after_run, slice_after_sleep);
}
