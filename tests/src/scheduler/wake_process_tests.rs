//! wake_process Bug Detection Tests
//!
//! These tests call the REAL kernel functions and verify they work correctly.
//! Tests should FAIL when bugs exist and PASS when fixed.
//!
//! NOTE: These tests share global state (PROCESS_TABLE) so they use a mutex
//! to serialize execution.

use crate::scheduler::{
    wake_process, 
    SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
    process_table_lock,
};
use crate::scheduler::table::PROCESS_TABLE;
use crate::scheduler::percpu::{check_need_resched, init_percpu_sched};
use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE};
use crate::signal::SignalState;

use std::sync::{atomic::Ordering, Once, Mutex};
    use serial_test::serial;

static INIT_PERCPU: Once = Once::new();
static TEST_MUTEX: Mutex<()> = Mutex::new(());

/// Initialize per-CPU scheduler for testing (only once)
fn ensure_percpu_init() {
    INIT_PERCPU.call_once(|| {
        init_percpu_sched(0);
    });
}

/// Helper: Create a minimal test process
fn make_test_process(pid: Pid) -> Process {
    Process {
        pid,
        ppid: 1,
        tgid: pid,
        state: ProcessState::Ready,
        entry_point: 0,
        stack_top: 0,
        heap_start: 0,
        heap_end: 0,
        signal_state: SignalState::new(),
        context: crate::process::Context::zero(),
        has_entered_user: false,
        context_valid: false,
        is_fork_child: false,
        is_thread: false,
        cr3: 0,
        tty: 0,
        memory_base: 0,
        memory_size: 0,
        user_rip: 0,
        user_rsp: 0,
        user_rflags: 0,
        user_r10: 0,
        user_r8: 0,
        user_r9: 0,
        exit_code: 0,
        term_signal: None,
        kernel_stack: 0,
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

/// Clear the process table for a clean test environment
fn clear_process_table() {
    ensure_percpu_init();
    let mut table = process_table_lock();
    for slot in table.iter_mut() {
        *slot = None;
    }
}

/// Add a test process directly to the table
fn add_test_process(pid: Pid, state: ProcessState) -> Result<(), &'static str> {
    let mut table = process_table_lock();
    
    for (idx, slot) in table.iter_mut().enumerate() {
        if slot.is_none() {
            let mut entry = ProcessEntry::empty();
            entry.process = make_test_process(pid);
            entry.process.state = state;
            entry.vruntime = 0;
            entry.vdeadline = BASE_SLICE_NS;
            entry.weight = NICE_0_WEIGHT;
            entry.slice_ns = BASE_SLICE_NS;
            entry.slice_remaining_ns = BASE_SLICE_NS;
            entry.cpu_affinity = CpuMask::all();
            entry.policy = SchedPolicy::Normal;
            
            // Register PID mapping
            crate::process::register_pid_mapping(pid, idx as u16);
            
            *slot = Some(entry);
            return Ok(());
        }
    }
    Err("Process table full")
}

// =============================================================================
// REAL BUG DETECTION TESTS
// =============================================================================

#[test]
#[serial]
fn test_wake_process_sets_state_to_ready() {
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_process_table();
    
    // Add a sleeping process
    add_test_process(100, ProcessState::Sleeping).unwrap();
    
    // Verify it's sleeping
    {
        let table = process_table_lock();
        let entry = table.iter()
            .filter_map(|s| s.as_ref())
            .find(|e| e.process.pid == 100)
            .expect("Process not found");
        assert_eq!(entry.process.state, ProcessState::Sleeping);
    }
    
    // Wake it using the REAL kernel function
    let woke = wake_process(100);
    
    assert!(woke, "wake_process should return true for sleeping process");
    
    // Verify state changed to Ready
    {
        let table = process_table_lock();
        let entry = table.iter()
            .filter_map(|s| s.as_ref())
            .find(|e| e.process.pid == 100)
            .expect("Process not found");
        assert_eq!(entry.process.state, ProcessState::Ready, 
            "BUG: wake_process did not change state to Ready!");
    }
    
    clear_process_table();
}

#[test]
#[serial]
fn test_wake_process_sets_need_resched_flag() {
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_process_table();
    
    // Add a sleeping process
    add_test_process(101, ProcessState::Sleeping).unwrap();
    
    // Clear any existing need_resched flag
    let _ = check_need_resched(); // This clears the flag
    
    // Wake the process using REAL kernel function
    let woke = wake_process(101);
    assert!(woke);
    
    // THE CRITICAL TEST: need_resched flag MUST be set
    // If this fails, interactive processes will be unresponsive!
    let need_resched = check_need_resched();
    
    assert!(need_resched, 
        "BUG DETECTED: wake_process() did NOT set need_resched flag! \
         This causes the shell to be unresponsive when waiting for keyboard input. \
         The woken process will have to wait for the current process's time slice \
         to expire before being scheduled.");
    
    clear_process_table();
}

#[test]
#[serial]
fn test_wake_process_gives_vruntime_credit() {
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_process_table();
    
    // Use unique PID to avoid conflicts with parallel tests
    let pid = 2002;
    
    // Add a sleeping process with high vruntime
    add_test_process(pid, ProcessState::Sleeping).unwrap();
    
    // Set a high vruntime (simulating process that ran a lot before sleeping)
    {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    entry.vruntime = 100_000_000; // 100ms worth
                    break;
                }
            }
        }
    }
    
    // Wake it
    let woke = wake_process(pid);
    assert!(woke, "wake_process should succeed for sleeping process");
    
    // Check vruntime - it should get credit (lower vruntime = higher priority)
    {
        let table = process_table_lock();
        let mut found_vruntime = None;
        for slot in table.iter() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    found_vruntime = Some(entry.vruntime);
                    break;
                }
            }
        }
        let vruntime = found_vruntime.expect("Process not found");
        
        // Woken process should have vruntime <= min_vruntime (gets credit)
        // At minimum, vruntime should not have INCREASED
        assert!(vruntime <= 100_000_000,
            "BUG: wake_process should not increase vruntime! Got {}",
            vruntime);
    }
    
    clear_process_table();
}

#[test]
#[serial]
fn test_wake_process_does_nothing_for_ready_process() {
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_process_table();
    
    // Add a Ready process (not sleeping)
    add_test_process(103, ProcessState::Ready).unwrap();
    
    // Try to wake it
    let woke = wake_process(103);
    
    // Should return false - can't wake a non-sleeping process
    assert!(!woke, "wake_process should return false for Ready process");
    
    clear_process_table();
}

#[test]
#[serial]
fn test_wake_process_does_nothing_for_running_process() {
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_process_table();
    
    // Add a Running process
    add_test_process(104, ProcessState::Running).unwrap();
    
    // Try to wake it
    let woke = wake_process(104);
    
    assert!(!woke, "wake_process should return false for Running process");
    
    clear_process_table();
}

#[test]
#[serial]
fn test_wake_nonexistent_process() {
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_process_table();
    
    // Try to wake a process that doesn't exist
    let woke = wake_process(9999);
    
    assert!(!woke, "wake_process should return false for nonexistent PID");
}

#[test]
#[serial]
fn test_wake_process_recalculates_vdeadline() {
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_process_table();
    
    add_test_process(105, ProcessState::Sleeping).unwrap();
    
    // Set stale vdeadline
    {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == 105 {
                    entry.vdeadline = 0; // Clearly wrong
                    entry.vruntime = 1000;
                    break;
                }
            }
        }
    }
    
    // Wake it
    wake_process(105);
    
    // vdeadline should be recalculated
    {
        let table = process_table_lock();
        let mut found = None;
        for slot in table.iter() {
            if let Some(entry) = slot {
                if entry.process.pid == 105 {
                    found = Some((entry.vruntime, entry.vdeadline));
                    break;
                }
            }
        }
        let (vruntime, vdeadline) = found.expect("Process not found");
        
        // For nice 0: vdeadline = vruntime + slice_ns
        assert!(vdeadline > vruntime,
            "BUG: wake_process should recalculate vdeadline! \
             vruntime={}, vdeadline={}", vruntime, vdeadline);
    }
    
    clear_process_table();
}

// =============================================================================
// Scenario Tests - These test the actual user-reported bug
// =============================================================================

#[test]
#[serial]
fn test_keyboard_wake_shell_scenario() {
    // This test reproduces the exact bug scenario:
    // - DHCP client (PID 2) is Running with full time slice
    // - Shell (PID 8) is Sleeping, waiting for keyboard
    // - User types -> keyboard ISR calls wake_process(8)
    // - Shell MUST be scheduled immediately (next tick)
    
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_process_table();
    
    // DHCP running
    add_test_process(2, ProcessState::Running).unwrap();
    
    // Shell sleeping (waiting for keyboard)
    add_test_process(8, ProcessState::Sleeping).unwrap();
    
    // Clear need_resched
    let _ = check_need_resched();
    
    // Simulate keyboard interrupt waking shell
    let woke = wake_process(8);
    assert!(woke, "Shell should be woken");
    
    // THE BUG: If need_resched is not set, shell waits for DHCP's slice to exhaust
    let need_resched = check_need_resched();
    
    assert!(need_resched,
        "CRITICAL BUG: Keyboard wake did not set need_resched! \
         The shell will be unresponsive for up to 4ms (one time slice). \
         Users will experience laggy typing.");
    
    // Shell should now be Ready
    {
        let table = process_table_lock();
        let mut shell_state = None;
        for slot in table.iter() {
            if let Some(entry) = slot {
                if entry.process.pid == 8 {
                    shell_state = Some(entry.process.state);
                    break;
                }
            }
        }
        assert_eq!(shell_state, Some(ProcessState::Ready));
    }
    
    clear_process_table();
}

#[test]
#[serial]
fn test_multiple_wakes_all_set_resched() {
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_process_table();
    
    // Use unique PIDs to avoid conflicts with parallel tests
    let pids: Vec<u64> = (3010..3015).collect();
    
    // Add multiple sleeping processes
    for &pid in &pids {
        add_test_process(pid, ProcessState::Sleeping).unwrap();
    }
    
    // Wake them one by one, each should set need_resched
    for &pid in &pids {
        let _ = check_need_resched(); // Clear flag
        
        let woke = wake_process(pid);
        assert!(woke, "wake_process should succeed for PID {}", pid);
        
        let need_resched = check_need_resched();
        assert!(need_resched, 
            "Every wake_process call must set need_resched! Failed for PID {}", pid);
    }
    
    clear_process_table();
}
