//! Foreground Process Unresponsive Bug Detection Tests
//!
//! CRITICAL: These tests MUST FAIL when the bug exists and PASS only when fixed.
//!
//! ## Bug Description:
//!
//! Shell becomes unresponsive after keyboard input due to race condition in
//! read_raw_for_tty() between add_waiter() and set_current_process_state(Sleeping).
//!
//! ## Race Condition Sequence:
//!
//! 1. Shell calls add_waiter(pid) - registered for keyboard wake
//! 2. Keyboard interrupt fires
//! 3. wake_all_waiters() calls wake_process(pid) 
//! 4. wake_process sees state=Ready (not Sleeping), returns false, removes from waiter list
//! 5. Shell then calls set_current_process_state(Sleeping)
//! 6. Shell is now stuck: state=Sleeping, not in waiter list, no one will wake it
//!
//! ## Expected Test Behavior:
//!
//! - Tests FAIL now (bug exists)
//! - Tests PASS after fix is applied

use crate::scheduler::{
    wake_process, set_process_state,
    SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
    process_table_lock,
};
use crate::scheduler::percpu::{check_need_resched, init_percpu_sched};
use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE};
use crate::signal::SignalState;

use std::sync::Once;
    use serial_test::serial;
use std::sync::atomic::{AtomicU64, Ordering};

static INIT_PERCPU: Once = Once::new();
// Unique PID generator to avoid conflicts (no Mutex needed - atomic is enough)
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
            // Register PID mapping BEFORE adding to table
            crate::process::register_pid_mapping(pid, idx as u16);
            let mut entry = make_process_entry(make_test_process(pid, state));
            entry.process.state = state;  // Ensure state is set correctly
            *slot = Some(entry);
            return;
        }
    }
    panic!("Process table full");
}

fn clear_table() {
    ensure_percpu_init();
    let mut table = process_table_lock();
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            // Unregister PID mapping
            crate::process::unregister_pid_mapping(entry.process.pid);
        }
        *slot = None;
    }
}

fn get_state(pid: Pid) -> Option<ProcessState> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .find(|e| e.process.pid == pid)
        .map(|e| e.process.state)
}

fn get_lag(pid: Pid) -> Option<i64> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .find(|e| e.process.pid == pid)
        .map(|e| e.lag)
}

fn get_vruntime(pid: Pid) -> Option<u64> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .find(|e| e.process.pid == pid)
        .map(|e| e.vruntime)
}

fn set_vruntime(pid: Pid, vrt: u64) {
    let mut table = process_table_lock();
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                entry.vruntime = vrt;
                return;
            }
        }
    }
}

fn set_lag(pid: Pid, lag: i64) {
    let mut table = process_table_lock();
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                entry.lag = lag;
                return;
            }
        }
    }
}

// =============================================================================
// BUG DETECTION TESTS - These should FAIL when bug exists
// =============================================================================

/// TEST: Race condition must not cause process to get stuck
///
/// Tests wake arriving BEFORE sleep: process sleeps and is stuck forever.
/// FAILS if bug exists (process stuck in Sleeping).
#[test]
#[serial]
fn test_race_wake_before_sleep_must_not_strand() {
    let pid = next_pid();
    add_process(pid, ProcessState::Ready);
    
    // Race: wake arrives while process is still Ready
    let _woke = wake_process(pid);  // Returns false - process not sleeping
    
    // Process then sets itself to Sleeping
    let _ = set_process_state(pid, ProcessState::Sleeping);
    
    // BUG CHECK: Is process stuck?
    let state = get_state(pid);
    
    // Cleanup this specific process
    {
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
    
    // Test FAILS if process is stuck in Sleeping (bug exists)
    // Test PASSES if kernel prevented the sleep (fix implemented)
    assert_ne!(state, Some(ProcessState::Sleeping),
        "BUG: Process stuck in Sleeping after wake-before-sleep race! \
         The wake was lost. FIX: Add pending-wake flag.");
}

/// TEST: wake_process on Ready must prevent subsequent sleep
///
/// FAILS if calling wake on Ready process allows it to sleep afterward.
#[test]
#[serial]
fn test_wake_ready_prevents_sleep() {
    let pid = next_pid();
    add_process(pid, ProcessState::Ready);
    
    // Wake on Ready (race condition scenario)
    wake_process(pid);
    
    // Try to sleep
    let _ = set_process_state(pid, ProcessState::Sleeping);
    
    let state = get_state(pid);
    
    // Cleanup
    {
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
    
    // FAILS if process is Sleeping (bug: no pending-wake mechanism)
    assert_ne!(state, Some(ProcessState::Sleeping),
        "BUG: Process slept after wake was called on Ready state! \
         FIX: wake_process should set pending flag for Ready processes.");
}

/// TEST: Multiple rapid wake-before-sleep must not lose any
///
/// FAILS if any iteration leaves process stuck.
#[test]
#[serial]
fn test_rapid_race_no_stuck_process() {
    let pid = next_pid();
    add_process(pid, ProcessState::Ready);
    
    let mut stuck_iterations = Vec::new();
    
    for i in 0..50 {
        // Reset
        let _ = set_process_state(pid, ProcessState::Ready);
        
        // Race: wake before sleep
        wake_process(pid);
        let _ = set_process_state(pid, ProcessState::Sleeping);
        
        if get_state(pid) == Some(ProcessState::Sleeping) {
            stuck_iterations.push(i);
            // Unstick for next iteration
            wake_process(pid);
        }
    }
    
    // Cleanup
    {
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
    
    // FAILS if any iteration got stuck
    assert!(stuck_iterations.is_empty(),
        "BUG: Process got stuck in {} iterations: {:?}. \
         Lost wakes due to race.", stuck_iterations.len(), stuck_iterations);
}

/// TEST: need_resched must be set after wake
///
/// FAILS if wake doesn't set need_resched (woken process won't run promptly).
#[test]
#[serial]
fn test_wake_must_set_need_resched() {
    let pid = next_pid();
    add_process(pid, ProcessState::Sleeping);
    
    // Clear flag
    let _ = check_need_resched();
    
    // Wake
    let woke = wake_process(pid);
    
    let need_resched = check_need_resched();
    
    // Cleanup
    {
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
    
    assert!(woke, "wake_process should succeed for Sleeping process");
    
    // FAILS if need_resched not set
    assert!(need_resched,
        "BUG: wake_process did not set need_resched! \
         Woken process won't run until next timer tick.");
}

/// TEST: EEVDF lag must be >= 0 after wake
///
/// FAILS if lag is negative (process ineligible for EEVDF scheduling).
#[test]
#[serial]
fn test_wake_must_reset_lag_nonnegative() {
    let pid = next_pid();
    add_process(pid, ProcessState::Sleeping);
    set_lag(pid, -50_000_000);  // -50ms negative lag
    
    wake_process(pid);
    
    let lag = get_lag(pid);
    
    // Cleanup
    {
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
    
    // FAILS if lag still negative
    assert!(lag.unwrap_or(-1) >= 0,
        "BUG: lag ({:?}) is negative after wake! Process is EEVDF-ineligible.", lag);
}

/// TEST: Woken process vruntime must allow scheduling vs background
///
/// FAILS if woken shell has higher vruntime than long-running background.
#[test]
#[serial]
fn test_woken_vruntime_allows_scheduling() {
    // Background with high vruntime
    let bg_pid = next_pid();
    add_process(bg_pid, ProcessState::Running);
    set_vruntime(bg_pid, 500_000_000);  // 500ms
    
    // Shell sleeping with low vruntime
    let shell_pid = next_pid();
    add_process(shell_pid, ProcessState::Sleeping);
    set_vruntime(shell_pid, 0);
    
    wake_process(shell_pid);
    
    let shell_vrt = get_vruntime(shell_pid).unwrap_or(u64::MAX);
    let bg_vrt = get_vruntime(bg_pid).unwrap_or(0);
    
    // Cleanup
    {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == bg_pid || entry.process.pid == shell_pid {
                    crate::process::unregister_pid_mapping(entry.process.pid);
                    *slot = None;
                }
            }
        }
    }
    
    // FAILS if shell vruntime > background (shell would be starved)
    assert!(shell_vrt <= bg_vrt,
        "BUG: Woken shell vruntime ({}) > background ({}). Shell starved!", 
        shell_vrt, bg_vrt);
}

/// TEST: Exact keyboard read race sequence
///
/// Tests read_raw_for_tty flow with interrupt during sleep prep.
/// FAILS if shell gets stuck.
#[test]
#[serial]
fn test_keyboard_read_race_sequence() {
    let shell_pid = next_pid();
    add_process(shell_pid, ProcessState::Ready);
    
    // read_raw_for_tty sequence with race condition:
    // 1. add_waiter(shell_pid) - registered (waiter mock not used, process is Ready)
    // 2. INTERRUPT: wake_all_waiters runs
    //    - removes shell from waiter list
    //    - calls wake_process(shell_pid)
    //    - wake_process sees Ready, returns false
    let _woke = wake_process(shell_pid);
    // 3. set_current_process_state(Sleeping)
    let _ = set_process_state(shell_pid, ProcessState::Sleeping);
    // 4. Shell would call do_schedule and block
    
    let state = get_state(shell_pid);
    
    // Cleanup
    {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == shell_pid {
                    crate::process::unregister_pid_mapping(shell_pid);
                    *slot = None;
                    break;
                }
            }
        }
    }
    
    // FAILS if shell is stuck in Sleeping
    assert_ne!(state, Some(ProcessState::Sleeping),
        "BUG: Shell stuck after keyboard read race! \
         This is the exact bug causing unresponsive shell. \
         FIX: Check pending wake before sleeping.");
}

/// TEST: Sleeping process state after normal wake
///
/// Basic test that wake_process correctly transitions Sleeping -> Ready.
/// This should PASS (wake on Sleeping works).
#[test]
#[serial]
fn test_normal_wake_sleeping_works() {
    let pid = next_pid();
    add_process(pid, ProcessState::Sleeping);
    
    // Verify process was added correctly
    let state_before = get_state(pid);
    assert_eq!(state_before, Some(ProcessState::Sleeping),
        "Setup failed: process not added as Sleeping, got {:?}", state_before);
    
    let woke = wake_process(pid);
    let state = get_state(pid);
    
    // Debug output
    eprintln!("DEBUG: pid={}, woke={}, state_before={:?}, state_after={:?}", 
              pid, woke, state_before, state);
    
    // Cleanup
    {
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
    
    assert!(woke, "wake_process should return true for Sleeping process");
    assert_eq!(state, Some(ProcessState::Ready),
        "Process should be Ready after wake from Sleeping");
}
