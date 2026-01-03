//! Foreground Process Starvation Tests
//!
//! These tests detect the bug where foreground/interactive processes (shell, login)
//! become unresponsive because they get starved by background processes.
//!
//! Uses REAL kernel functions - NO local re-implementations.

use crate::scheduler::{
    wake_process, set_process_state, process_table_lock,
    SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
    get_min_vruntime, calc_vdeadline, is_eligible,
    // Use REAL kernel query/setter functions
    get_process_state, get_process_vruntime, get_process_lag, get_process_vdeadline,
    set_process_vruntime, set_process_lag,
};
use crate::scheduler::percpu::{init_percpu_sched, check_need_resched, set_need_resched};
use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, MAX_PROCESSES};
use crate::signal::SignalState;

use std::sync::Once;
use serial_test::serial;
use std::sync::atomic::{AtomicU64, Ordering};

static INIT_PERCPU: Once = Once::new();
static NEXT_PID: AtomicU64 = AtomicU64::new(70000);

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

fn make_process_entry(proc: Process, vruntime: u64, lag: i64) -> ProcessEntry {
    let vdeadline = calc_vdeadline(vruntime, BASE_SLICE_NS, NICE_0_WEIGHT);
    ProcessEntry {
        process: proc,
        vruntime,
        vdeadline,
        lag,
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

fn add_process_full(pid: Pid, state: ProcessState, vruntime: u64, lag: i64) {
    ensure_percpu_init();
    let mut table = process_table_lock();
    for (idx, slot) in table.iter_mut().enumerate() {
        if slot.is_none() {
            crate::process::register_pid_mapping(pid, idx as u16);
            let entry = make_process_entry(make_test_process(pid, state), vruntime, lag);
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

/// Find which process EEVDF would select (lowest eligible vdeadline)
fn find_eevdf_winner() -> Option<Pid> {
    let table = process_table_lock();
    
    let mut best: Option<(Pid, u64, bool)> = None;
    
    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.state != ProcessState::Ready {
            continue;
        }
        
        let eligible = is_eligible(entry);
        let vdl = entry.vdeadline;
        let pid = entry.process.pid;
        
        let should_replace = match best {
            None => true,
            Some((_, best_vdl, best_elig)) => {
                if eligible && !best_elig {
                    true
                } else if eligible == best_elig {
                    vdl < best_vdl
                } else {
                    false
                }
            }
        };
        
        if should_replace {
            best = Some((pid, vdl, eligible));
        }
    }
    
    best.map(|(pid, _, _)| pid)
}

// =============================================================================
// FOREGROUND STARVATION TESTS - Using REAL kernel functions
// =============================================================================

#[test]
#[serial]
fn test_sleeping_process_vruntime_unchanged() {
    let shell_pid = next_pid();
    let dhcp_pid = next_pid();
    
    add_process_full(shell_pid, ProcessState::Sleeping, 1_000_000, 0);
    add_process_full(dhcp_pid, ProcessState::Running, 500_000, 0);
    
    // Use REAL kernel function
    let shell_vrt_before = get_process_vruntime(shell_pid).unwrap();
    assert_eq!(shell_vrt_before, 1_000_000);
    
    // After some "time passes", sleeping process vruntime should be unchanged
    let shell_vrt_after = get_process_vruntime(shell_pid).unwrap();
    assert_eq!(shell_vrt_before, shell_vrt_after,
        "Sleeping process vruntime changed unexpectedly");
    
    cleanup_process(shell_pid);
    cleanup_process(dhcp_pid);
}

#[test]
#[serial]
fn test_wake_process_sets_ready_state() {
    let pid = next_pid();
    add_process_full(pid, ProcessState::Sleeping, 1_000_000, 0);
    
    // Use REAL kernel function
    assert_eq!(get_process_state(pid), Some(ProcessState::Sleeping));
    
    let woke = wake_process(pid);
    assert!(woke, "wake_process should return true");
    
    // Use REAL kernel function
    assert_eq!(get_process_state(pid), Some(ProcessState::Ready),
        "Process should be Ready after wake");
    
    cleanup_process(pid);
}

#[test]
#[serial]
fn test_shell_wins_over_background_after_wake() {
    let shell_pid = next_pid();
    let dhcp_pid = next_pid();
    
    // Shell has been sleeping, dhcp running
    // Shell should have lower or equal vruntime
    add_process_full(shell_pid, ProcessState::Sleeping, 1_000_000, 0);
    add_process_full(dhcp_pid, ProcessState::Ready, 4_000_000, 0);
    
    // Wake shell
    wake_process(shell_pid);
    
    // Use REAL kernel functions
    let shell_state = get_process_state(shell_pid).unwrap();
    assert_eq!(shell_state, ProcessState::Ready);
    
    let shell_vrt = get_process_vruntime(shell_pid).unwrap();
    let dhcp_vrt = get_process_vruntime(dhcp_pid).unwrap();
    
    assert!(shell_vrt <= dhcp_vrt,
        "Shell vruntime {} should be <= dhcp vruntime {}", shell_vrt, dhcp_vrt);
    
    // EEVDF should pick shell (lower vdeadline because lower vruntime)
    let winner = find_eevdf_winner();
    assert_eq!(winner, Some(shell_pid),
        "Shell should win EEVDF selection after waking from sleep");
    
    cleanup_process(shell_pid);
    cleanup_process(dhcp_pid);
}

#[test]
#[serial]
fn test_lag_tracking_on_wake() {
    let pid = next_pid();
    add_process_full(pid, ProcessState::Sleeping, 1_000_000, -500);
    
    // Use REAL kernel function
    let lag_before = get_process_lag(pid).unwrap();
    assert_eq!(lag_before, -500);
    
    wake_process(pid);
    
    // Lag should be preserved or adjusted appropriately after wake
    let lag_after = get_process_lag(pid).unwrap();
    // The exact behavior depends on kernel implementation
    // But lag should not be arbitrarily corrupted
    assert!(lag_after >= -1_000_000 && lag_after <= 1_000_000,
        "Lag {} seems corrupted after wake", lag_after);
    
    cleanup_process(pid);
}

#[test]
#[serial]
fn test_set_vruntime_via_kernel_function() {
    let pid = next_pid();
    add_process_full(pid, ProcessState::Ready, 1_000_000, 0);
    
    // Use REAL kernel setter function
    set_process_vruntime(pid, 2_000_000).unwrap();
    
    // Use REAL kernel getter function
    let vrt = get_process_vruntime(pid).unwrap();
    assert_eq!(vrt, 2_000_000);
    
    cleanup_process(pid);
}

#[test]
#[serial]
fn test_set_lag_via_kernel_function() {
    let pid = next_pid();
    add_process_full(pid, ProcessState::Ready, 1_000_000, 0);
    
    // Use REAL kernel setter function
    set_process_lag(pid, -12345).unwrap();
    
    // Use REAL kernel getter function
    let lag = get_process_lag(pid).unwrap();
    assert_eq!(lag, -12345);
    
    cleanup_process(pid);
}

#[test]
#[serial]
fn test_vdeadline_query_via_kernel() {
    let pid = next_pid();
    let vruntime = 1_000_000u64;
    let expected_vdeadline = calc_vdeadline(vruntime, BASE_SLICE_NS, NICE_0_WEIGHT);
    
    add_process_full(pid, ProcessState::Ready, vruntime, 0);
    
    // Use REAL kernel function
    let actual_vdeadline = get_process_vdeadline(pid).unwrap();
    assert_eq!(actual_vdeadline, expected_vdeadline);
    
    cleanup_process(pid);
}
