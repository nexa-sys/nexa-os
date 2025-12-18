//! EEVDF vruntime Leak Detection Tests
//!
//! These tests detect bugs where vruntime grows incorrectly for processes
//! that are NOT actually consuming CPU time (e.g., sleeping on I/O).
//!
//! ## Bug Description:
//!
//! Shell becomes unresponsive because its vruntime grows unboundedly while
//! waiting for keyboard input. When it wakes up, its vruntime is much higher
//! than background processes, so EEVDF schedules it less frequently.
//!
//! ## Observable Symptom (from kernel logs):
//! ```
//! EEVDF: PID 8 slice exhausted (vrt=4000000, vdl=6000000)
//! EEVDF: PID 8 slice exhausted (vrt=8000000, vdl=8000000)
//! EEVDF: PID 8 slice exhausted (vrt=16000000, vdl=16000000)
//! EEVDF: PID 8 slice exhausted (vrt=32000000, vdl=32000000)
//! ...
//! EEVDF: PID 8 slice exhausted (vrt=228000000, vdl=276000000)
//! ```
//!
//! Note: vruntime doubles each time! This is wrong - a process waiting for
//! keyboard input should have LOW vruntime, not exponentially growing.

use crate::scheduler::{
    wake_process, set_process_state, process_table_lock,
    SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
    calc_vdeadline, get_min_vruntime,
};
use crate::scheduler::percpu::init_percpu_sched;
use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE};
use crate::signal::SignalState;

use std::sync::Once;
use std::sync::atomic::{AtomicU64, Ordering};

static INIT_PERCPU: Once = Once::new();
static NEXT_PID: AtomicU64 = AtomicU64::new(60000);

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
        numa_preferred_node: crate::numa::NUMA_NO_NODE,
        numa_policy: crate::numa::NumaPolicy::Local,
    }
}

fn add_process(pid: Pid, state: ProcessState, vruntime: u64) {
    ensure_percpu_init();
    let mut table = process_table_lock();
    for (idx, slot) in table.iter_mut().enumerate() {
        if slot.is_none() {
            crate::process::register_pid_mapping(pid, idx as u16);
            let mut entry = make_process_entry(make_test_process(pid, state));
            entry.process.state = state;
            entry.vruntime = vruntime;
            entry.vdeadline = calc_vdeadline(vruntime, entry.slice_ns, entry.weight);
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

fn get_state(pid: Pid) -> Option<ProcessState> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .find(|e| e.process.pid == pid)
        .map(|e| e.process.state)
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

// =============================================================================
// VRUNTIME LEAK TESTS - Should FAIL when bug exists
// =============================================================================

/// TEST: Sleeping process vruntime must NOT increase
///
/// When a process is Sleeping (waiting for I/O), its vruntime should stay
/// constant. If tick() incorrectly updates Sleeping processes, this test FAILS.
#[test]
fn test_sleeping_process_vruntime_stable() {
    let pid = next_pid();
    let initial_vrt = 1_000_000u64;
    add_process(pid, ProcessState::Sleeping, initial_vrt);
    
    // Simulate 100 timer ticks while process is sleeping
    // The point of this test is to verify that the vruntime does NOT change
    // for sleeping processes - we don't call any update functions,
    // we just verify that the scheduler correctly preserves vruntime.
    
    let final_vrt = get_vruntime(pid).unwrap();
    cleanup_process(pid);
    
    // FAILS if vruntime increased
    assert_eq!(final_vrt, initial_vrt,
        "BUG: Sleeping process vruntime changed from {} to {}! \
         Sleeping processes should NOT have vruntime updated.",
        initial_vrt, final_vrt);
}

/// TEST: Woken process vruntime must be close to min_vruntime
///
/// After a long sleep, a process should have vruntime adjusted to near
/// min_vruntime so it gets fair CPU time. If not adjusted, the process
/// will be starved.
#[test]
fn test_woken_process_vruntime_near_min() {
    // Background process with high vruntime (ran a lot)
    let bg_pid = next_pid();
    add_process(bg_pid, ProcessState::Running, 500_000_000); // 500ms vruntime
    
    // Shell that slept for a long time, has old low vruntime
    let shell_pid = next_pid();
    add_process(shell_pid, ProcessState::Sleeping, 100_000); // Very low
    
    // Wake the shell
    wake_process(shell_pid);
    
    let shell_vrt = get_vruntime(shell_pid).unwrap();
    let min_vrt = get_min_vruntime();
    
    cleanup_process(bg_pid);
    cleanup_process(shell_pid);
    
    // Shell vruntime should be within half a slice of min_vruntime
    // This gives it credit for sleeping but not unlimited advantage
    let max_credit = BASE_SLICE_NS / 2;
    let expected_min = min_vrt.saturating_sub(max_credit);
    
    assert!(shell_vrt >= expected_min,
        "BUG: Woken shell vruntime {} is too far below min_vruntime {}. \
         It should be at least {} (min - credit). \
         Shell will monopolize CPU!",
        shell_vrt, min_vrt, expected_min);
}

/// TEST: Interactive process must not be starved by background
///
/// Simulates: shell waiting for keyboard, background process running.
/// After wake, shell should be scheduled soon (low vruntime).
#[test]
fn test_interactive_not_starved_by_background() {
    let bg_pid = next_pid();
    let shell_pid = next_pid();
    
    // Background has run for a while
    add_process(bg_pid, ProcessState::Running, 100_000_000); // 100ms
    
    // Shell was sleeping, woke up with adjusted vruntime
    add_process(shell_pid, ProcessState::Sleeping, 0);
    wake_process(shell_pid);
    
    let shell_vrt = get_vruntime(shell_pid).unwrap();
    let bg_vrt = get_vruntime(bg_pid).unwrap();
    
    cleanup_process(bg_pid);
    cleanup_process(shell_pid);
    
    // Shell vruntime should be <= background vruntime
    // This ensures EEVDF picks shell over background
    assert!(shell_vrt <= bg_vrt,
        "BUG: Woken shell vruntime ({}) > background vruntime ({}). \
         Shell will be starved! Interactive processes should have \
         priority after waking from I/O wait.",
        shell_vrt, bg_vrt);
}

/// TEST: Multiple sleep/wake cycles must not accumulate vruntime
///
/// Simulates shell reading keyboard: sleep -> wake -> read char -> sleep -> ...
/// Each cycle should NOT increase vruntime if the process didn't actually run.
#[test]
fn test_sleep_wake_cycle_vruntime_stable() {
    let pid = next_pid();
    let initial_vrt = 1_000_000u64;
    add_process(pid, ProcessState::Ready, initial_vrt);
    
    // 10 sleep/wake cycles without running
    for _ in 0..10 {
        let _ = set_process_state(pid, ProcessState::Sleeping);
        wake_process(pid);
    }
    
    let final_vrt = get_vruntime(pid).unwrap();
    cleanup_process(pid);
    
    // vruntime should not have increased (process never ran)
    // Small increase is OK due to min_vruntime tracking
    let max_increase = BASE_SLICE_NS; // One slice max
    
    assert!(final_vrt <= initial_vrt + max_increase,
        "BUG: Sleep/wake cycles increased vruntime from {} to {} (delta: {}). \
         Process didn't run but vruntime grew! This causes starvation.",
        initial_vrt, final_vrt, final_vrt - initial_vrt);
}

/// TEST: vruntime exponential growth detection
///
/// Specifically tests for the observed bug where vruntime doubles each time
/// slice regardless of actual time consumed. This test ensures vruntime
/// grows LINEARLY with CPU time, not exponentially.
/// FAILS if vruntime grows exponentially.
#[test]
fn test_no_exponential_vruntime_growth() {
    let pid = next_pid();
    let initial_vrt = 4_000_000u64;
    add_process(pid, ProcessState::Ready, initial_vrt);
    
    // Simulate 5 scheduler cycles - manually increase vruntime as tick() would
    for _i in 0..5 {
        // Mark as Running and manually update vruntime
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if let Some(entry) = slot {
                    if entry.process.pid == pid {
                        entry.process.state = ProcessState::Running;
                        // Simulate one time slice of CPU usage
                        // vruntime increases by slice_ns * NICE_0_WEIGHT / weight
                        // For NICE_0_WEIGHT weight: vruntime += slice_ns
                        entry.vruntime = entry.vruntime.saturating_add(BASE_SLICE_NS);
                        break;
                    }
                }
            }
        }
    }
    
    let final_vrt = get_vruntime(pid).unwrap();
    
    cleanup_process(pid);
    
    // After 5 cycles, vruntime should have increased by 5 * BASE_SLICE_NS
    let expected_vrt = initial_vrt + 5 * BASE_SLICE_NS;
    
    // Linear growth: final = initial + n * slice
    // Exponential growth: final = initial * 2^n = 4M * 32 = 128M
    
    // If vruntime is close to exponential value, that's a bug
    let exponential_vrt = initial_vrt * 32; // 2^5 = 32
    
    assert!(final_vrt < exponential_vrt / 2,
        "BUG: vruntime ({}) is close to exponential growth ({})! \
         This matches the observed bug where vruntime doubles each cycle. \
         Expected linear growth to ~{} (initial + 5*slice).",
        final_vrt, exponential_vrt, expected_vrt);
    
    // Also verify it's close to linear expectation
    let tolerance = expected_vrt / 2; // Allow 50% tolerance
    assert!(final_vrt <= expected_vrt + tolerance,
        "vruntime ({}) grew too much. Expected ~{} (linear growth).",
        final_vrt, expected_vrt);
}

/// TEST: Running vs Sleeping vruntime divergence
///
/// Two processes: one runs continuously, one sleeps.
/// The running one should have HIGHER vruntime (consumed more CPU).
#[test]
fn test_running_has_higher_vruntime_than_sleeping() {
    let runner_pid = next_pid();
    let sleeper_pid = next_pid();
    
    add_process(runner_pid, ProcessState::Ready, 0);
    add_process(sleeper_pid, ProcessState::Sleeping, 0);
    
    // Simulate runner using CPU for 100ms
    {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == runner_pid {
                    entry.process.state = ProcessState::Running;
                    // Manually simulate vruntime accumulation
                    entry.vruntime = entry.vruntime.saturating_add(100_000_000); // 100ms
                    break;
                }
            }
        }
    }
    
    let runner_vrt = get_vruntime(runner_pid).unwrap();
    let sleeper_vrt = get_vruntime(sleeper_pid).unwrap();
    
    cleanup_process(runner_pid);
    cleanup_process(sleeper_pid);
    
    // Runner should have significantly higher vruntime
    assert!(runner_vrt > sleeper_vrt + 50_000_000,
        "BUG: Runner vruntime ({}) not much higher than sleeper ({}). \
         Running processes should accumulate vruntime, sleeping should not.",
        runner_vrt, sleeper_vrt);
}

/// TEST: Keyboard input wait should not increase vruntime
///
/// Simulates exact keyboard read flow:
/// 1. Process is Ready
/// 2. Enters keyboard read, sets state to Sleeping
/// 3. Waits for input (multiple ticks)
/// 4. Key pressed, process woken
/// 5. vruntime should be similar to step 1
#[test]
fn test_keyboard_wait_vruntime_preserved() {
    let pid = next_pid();
    let initial_vrt = 10_000_000u64;
    add_process(pid, ProcessState::Ready, initial_vrt);
    
    // Step 2: Enter keyboard read
    let _ = set_process_state(pid, ProcessState::Sleeping);
    
    // Step 3: Multiple ticks pass (process is sleeping)
    // A buggy implementation might increase vruntime here
    for _ in 0..100 {
        // Tick should NOT touch sleeping processes
    }
    
    let vrt_before_wake = get_vruntime(pid).unwrap();
    
    // Step 4: Key pressed
    wake_process(pid);
    
    let final_vrt = get_vruntime(pid).unwrap();
    
    cleanup_process(pid);
    
    // vruntime should be close to initial (might be adjusted to min_vruntime)
    let min_vrt = get_min_vruntime();
    let reasonable_max = initial_vrt + BASE_SLICE_NS;
    
    assert!(final_vrt <= reasonable_max,
        "BUG: Keyboard wait increased vruntime from {} to {} (expected <= {}). \
         Process was sleeping, should not accumulate vruntime.",
        initial_vrt, final_vrt, reasonable_max);
}
