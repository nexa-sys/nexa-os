//! Foreground Process Starvation Tests
//!
//! These tests detect the bug where foreground/interactive processes (shell, login)
//! become unresponsive because they get starved by background processes.
//!
//! ## Bug Scenario:
//!
//! 1. Shell (PID 8) waits for keyboard input (Sleeping)
//! 2. DHCP client (PID 2) runs periodically, accumulates vruntime reasonably
//! 3. User types "roo" - shell wakes up
//! 4. BUG: Shell's vruntime is somehow very high (228M vs expected ~4M)
//! 5. EEVDF picks PID 2 (lower vruntime) over PID 8
//! 6. Shell becomes unresponsive
//!
//! ## Root Cause Hypothesis:
//!
//! Either:
//! A) vruntime is updated for Sleeping processes (shouldn't happen)
//! B) wake_process doesn't properly adjust vruntime
//! C) Some code path marks process as Running without proper vruntime handling
//!
//! These tests isolate each hypothesis.

use crate::scheduler::{
    wake_process, set_process_state, process_table_lock,
    SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
    get_min_vruntime, calc_vdeadline, is_eligible,
};
use crate::scheduler::percpu::{init_percpu_sched, check_need_resched, set_need_resched};
use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, MAX_PROCESSES};
use crate::signal::SignalState;

use std::sync::Once;
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

fn get_vruntime(pid: Pid) -> Option<u64> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .find(|e| e.process.pid == pid)
        .map(|e| e.vruntime)
}

fn get_lag(pid: Pid) -> Option<i64> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .find(|e| e.process.pid == pid)
        .map(|e| e.lag)
}

fn get_vdeadline(pid: Pid) -> Option<u64> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .find(|e| e.process.pid == pid)
        .map(|e| e.vdeadline)
}

fn get_state(pid: Pid) -> Option<ProcessState> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .find(|e| e.process.pid == pid)
        .map(|e| e.process.state)
}

/// Find which process EEVDF would select (lowest eligible vdeadline)
fn find_eevdf_winner() -> Option<Pid> {
    let table = process_table_lock();
    
    let mut best: Option<(Pid, u64, bool)> = None; // (pid, vdeadline, eligible)
    
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
                // Eligible beats non-eligible
                if eligible && !best_elig {
                    true
                } else if !eligible && best_elig {
                    false
                } else {
                    // Same eligibility: earlier deadline wins
                    vdl < best_vdl
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
// FOREGROUND STARVATION TESTS
// =============================================================================

/// TEST: EEVDF must select woken interactive process over background
///
/// After keyboard input wakes shell, EEVDF should pick shell, not DHCP client.
#[test]
fn test_eevdf_picks_woken_interactive() {
    // DHCP client running, has accumulated some vruntime
    let dhcp_pid = next_pid();
    add_process_full(dhcp_pid, ProcessState::Ready, 50_000_000, 0); // 50ms vruntime
    
    // Login shell was sleeping, just woke up
    let login_pid = next_pid();
    add_process_full(login_pid, ProcessState::Sleeping, 0, 0);
    wake_process(login_pid);
    
    let winner = find_eevdf_winner();
    
    let dhcp_vrt = get_vruntime(dhcp_pid).unwrap();
    let login_vrt = get_vruntime(login_pid).unwrap();
    let dhcp_vdl = get_vdeadline(dhcp_pid).unwrap();
    let login_vdl = get_vdeadline(login_pid).unwrap();
    
    cleanup_process(dhcp_pid);
    cleanup_process(login_pid);
    
    eprintln!("DHCP: vrt={}, vdl={}", dhcp_vrt, dhcp_vdl);
    eprintln!("Login: vrt={}, vdl={}", login_vrt, login_vdl);
    eprintln!("Winner: {:?}", winner);
    
    // FAILS if DHCP is selected over just-woken login
    assert_eq!(winner, Some(login_pid),
        "BUG: EEVDF picked DHCP ({}) over woken login ({})! \
         Login vruntime={} should be <= DHCP vruntime={}, \
         so login's vdeadline should be earlier.",
        dhcp_pid, login_pid, login_vrt, dhcp_vrt);
}

/// TEST: Simulates the exact observed bug scenario
///
/// Reproduces: shell vruntime grows to 228M while background is only ~50M
#[test]
fn test_reproduce_observed_starvation_bug() {
    // This is the exact scenario from the kernel logs:
    // - PID 2 (DHCP) runs periodically, sleeps 10 seconds at a time
    // - PID 8 (login) waits for keyboard input
    // - After user types, PID 8 has vruntime=228M (!), PID 2 has normal vruntime
    
    let dhcp_pid = next_pid();
    let login_pid = next_pid();
    
    // Initial state: both start at similar vruntime
    add_process_full(dhcp_pid, ProcessState::Sleeping, 4_000_000, 0);
    add_process_full(login_pid, ProcessState::Ready, 4_000_000, 0);
    
    // Simulate 60 seconds of operation
    for cycle in 0..6 {
        // DHCP wakes up
        wake_process(dhcp_pid);
        
        // DHCP runs for ~500ms doing network stuff
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if let Some(entry) = slot {
                    if entry.process.pid == dhcp_pid {
                        entry.process.state = ProcessState::Running;
                        // Simulate vruntime increase from running 500ms
                        entry.vruntime = entry.vruntime.saturating_add(500_000_000);
                        break;
                    }
                }
            }
        }
        
        // DHCP sleeps for 10 seconds
        let _ = set_process_state(dhcp_pid, ProcessState::Sleeping);
        
        // Login should NOT have its vruntime increase during this time
        // (it's Sleeping, waiting for keyboard)
        let _ = set_process_state(login_pid, ProcessState::Sleeping);
        
        // Time passes... (login stays sleeping)
    }
    
    // User types - login wakes up
    wake_process(login_pid);
    
    let dhcp_vrt = get_vruntime(dhcp_pid).unwrap();
    let login_vrt = get_vruntime(login_pid).unwrap();
    
    cleanup_process(dhcp_pid);
    cleanup_process(login_pid);
    
    eprintln!("After 60s simulation:");
    eprintln!("  DHCP vruntime: {}", dhcp_vrt);
    eprintln!("  Login vruntime: {}", login_vrt);
    
    // BUG CHECK: Login vruntime should NOT be much higher than DHCP
    // DHCP actually ran for 3 seconds (6 cycles * 500ms), login ran for 0 seconds
    // So login vruntime should be <= DHCP vruntime
    
    // Allow some slack for vruntime adjustments, but definitely not 4x+
    let max_acceptable_login_vrt = dhcp_vrt * 2;
    
    assert!(login_vrt <= max_acceptable_login_vrt,
        "BUG REPRODUCED: Login vruntime ({}) is much higher than DHCP ({})! \
         This matches the observed bug where PID 8 had vrt=228M. \
         Login was SLEEPING the whole time, its vruntime should not grow!",
        login_vrt, dhcp_vrt);
    
    // Stronger check: login should have LOWER vruntime (didn't run)
    assert!(login_vrt <= dhcp_vrt,
        "Login vruntime ({}) > DHCP vruntime ({}) but login never ran! \
         EEVDF will starve the interactive process.",
        login_vrt, dhcp_vrt);
}

/// TEST: Rapidly typing should not accumulate vruntime
///
/// User types multiple characters quickly. Shell should process each without
/// building up vruntime.
#[test]
fn test_rapid_keystrokes_vruntime_stable() {
    let shell_pid = next_pid();
    add_process_full(shell_pid, ProcessState::Ready, 0, 0);
    
    let initial_vrt = get_vruntime(shell_pid).unwrap();
    
    // Simulate typing 10 characters rapidly
    for _ in 0..10 {
        // Shell processes character (very fast, ~1ms)
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if let Some(entry) = slot {
                    if entry.process.pid == shell_pid {
                        entry.process.state = ProcessState::Running;
                        // Simulate vruntime increase from running 1ms
                        entry.vruntime = entry.vruntime.saturating_add(1_000_000);
                        break;
                    }
                }
            }
        }
        
        // Shell sleeps waiting for next char
        let _ = set_process_state(shell_pid, ProcessState::Sleeping);
        
        // Next key arrives (immediately for rapid typing)
        wake_process(shell_pid);
    }
    
    let final_vrt = get_vruntime(shell_pid).unwrap();
    
    cleanup_process(shell_pid);
    
    // Shell ran for 10ms total (10 chars * 1ms each)
    // vruntime should be ~10ms, not exponentially higher
    let expected_vrt_increase = 10_000_000; // 10ms
    let max_acceptable = expected_vrt_increase * 3; // Allow 3x tolerance
    
    let actual_increase = final_vrt - initial_vrt;
    
    assert!(actual_increase <= max_acceptable,
        "BUG: Typing 10 chars increased vruntime by {} (expected ~{}). \
         Sleep/wake cycles should not accumulate vruntime!",
        actual_increase, expected_vrt_increase);
}

/// TEST: need_resched must be honored for interactive response
///
/// When keyboard input arrives, need_resched should force immediate scheduling.
#[test]
fn test_need_resched_forces_interactive_scheduling() {
    ensure_percpu_init();
    
    let bg_pid = next_pid();
    let shell_pid = next_pid();
    
    // Background running
    add_process_full(bg_pid, ProcessState::Running, 10_000_000, 0);
    
    // Shell sleeping
    add_process_full(shell_pid, ProcessState::Sleeping, 10_000_000, 0);
    
    // Clear need_resched
    let _ = check_need_resched();
    
    // Keyboard interrupt wakes shell
    wake_process(shell_pid);
    
    // Check that need_resched was set
    let need_resched = check_need_resched();
    
    cleanup_process(bg_pid);
    cleanup_process(shell_pid);
    
    assert!(need_resched,
        "BUG: wake_process did not set need_resched! \
         Interactive process won't get scheduled until next timer tick, \
         causing noticeable input lag.");
}

/// TEST: Eligibility check after wake
///
/// Woken process must be eligible to run (lag >= 0).
#[test]
fn test_woken_process_is_eligible() {
    let pid = next_pid();
    
    // Process sleeping with negative lag (consumed more than fair share before sleeping)
    add_process_full(pid, ProcessState::Sleeping, 50_000_000, -10_000_000);
    
    // Wake it up
    wake_process(pid);
    
    let lag = get_lag(pid).unwrap();
    
    cleanup_process(pid);
    
    // After wake, lag should be reset to 0 (eligible)
    assert!(lag >= 0,
        "BUG: Woken process has negative lag ({}). \
         EEVDF won't schedule it (ineligible)! \
         wake_process must reset lag to >= 0.",
        lag);
}

/// TEST: Long sleep should not penalize process
///
/// Process that slept for a long time should be FAVORED when it wakes,
/// not penalized with high vruntime.
#[test]
fn test_long_sleep_not_penalized() {
    let worker_pid = next_pid();  // Runs constantly
    let timer_pid = next_pid();   // Wakes up every 10 seconds
    
    // Both start equal
    add_process_full(worker_pid, ProcessState::Ready, 0, 0);
    add_process_full(timer_pid, ProcessState::Sleeping, 0, 0);
    
    // Worker runs for 10 seconds
    {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == worker_pid {
                    entry.process.state = ProcessState::Running;
                    // Simulate vruntime increase from running 10 seconds
                    entry.vruntime = entry.vruntime.saturating_add(10_000_000_000);
                    break;
                }
            }
        }
    }
    
    // Timer wakes up
    wake_process(timer_pid);
    
    let worker_vrt = get_vruntime(worker_pid).unwrap();
    let timer_vrt = get_vruntime(timer_pid).unwrap();
    
    cleanup_process(worker_pid);
    cleanup_process(timer_pid);
    
    // Timer should have MUCH lower vruntime (it didn't run for 10 seconds)
    // It should be selected next by EEVDF
    assert!(timer_vrt < worker_vrt,
        "BUG: Timer vruntime ({}) >= worker vruntime ({}) after long sleep! \
         Sleeping processes should not accumulate vruntime.",
        timer_vrt, worker_vrt);
    
    // Timer vruntime should be close to min_vruntime (with some credit for sleeping)
    let min_vrt = get_min_vruntime();
    let max_timer_vrt = min_vrt + BASE_SLICE_NS;  // At most one slice above min
    
    assert!(timer_vrt <= max_timer_vrt,
        "BUG: Timer vruntime ({}) is too high after waking. \
         Expected <= {} (min_vrt + one slice credit).",
        timer_vrt, max_timer_vrt);
}
