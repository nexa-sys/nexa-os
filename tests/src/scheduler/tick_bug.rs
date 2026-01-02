//! Scheduler Tick Bug Detection Tests
//!
//! Uses REAL kernel functions - NO local re-implementations.
//!
//! Key Invariant: vruntime should ONLY increase for the RUNNING process.

use crate::scheduler::{
    wake_process, set_process_state, process_table_lock,
    SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
    get_min_vruntime, calc_vdeadline,
    // Use REAL kernel query/setter functions
    get_process_state, get_process_vruntime, get_process_slice_remaining,
    set_process_vruntime, set_current_pid,
    // Use REAL tick and update functions
    tick, update_curr,
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
// TICK BUG TESTS - Using REAL kernel tick() and update_curr() functions
// =============================================================================
// NOTE: These tests use the actual kernel tick() function from scheduler::core.
// The kernel tick() function correctly updates only the RUNNING process's vruntime.

#[test]
#[serial]
fn test_sleeping_process_vruntime_invariant() {
    let sleeping_pid = next_pid();
    let running_pid = next_pid();
    
    add_process_with_state(sleeping_pid, ProcessState::Sleeping, 1_000_000);
    add_process_with_state(running_pid, ProcessState::Running, 500_000);
    // Set current_pid so tick() can find the running process
    set_current_pid(Some(running_pid));
    
    // Get vruntime before tick using REAL kernel function
    let sleep_vrt_before = get_process_vruntime(sleeping_pid).unwrap();
    
    // Use REAL kernel tick() - this should only update Running process
    tick(1); // 1 ms elapsed
    
    // Sleeping process vruntime must NOT change
    let sleep_vrt_after = get_process_vruntime(sleeping_pid).unwrap();
    assert_eq!(sleep_vrt_before, sleep_vrt_after,
        "Sleeping process vruntime changed during tick!");
    
    cleanup_processes(&[sleeping_pid, running_pid]);
}

#[test]
#[serial]
fn test_ready_process_vruntime_invariant() {
    let ready_pid = next_pid();
    let running_pid = next_pid();
    
    add_process_with_state(ready_pid, ProcessState::Ready, 1_000_000);
    add_process_with_state(running_pid, ProcessState::Running, 500_000);
    // Set current_pid so tick() can find the running process
    set_current_pid(Some(running_pid));
    
    // Get vruntime before tick using REAL kernel function
    let ready_vrt_before = get_process_vruntime(ready_pid).unwrap();
    
    // Use REAL kernel tick()
    tick(1); // 1 ms elapsed
    
    // Ready process vruntime must NOT change
    let ready_vrt_after = get_process_vruntime(ready_pid).unwrap();
    assert_eq!(ready_vrt_before, ready_vrt_after,
        "Ready process vruntime changed during tick!");
    
    cleanup_processes(&[ready_pid, running_pid]);
}

#[test]
#[serial]
fn test_running_process_vruntime_increases() {
    let running_pid = next_pid();
    
    add_process_with_state(running_pid, ProcessState::Running, 1_000_000);
    // Set current_pid so tick() can find the running process
    set_current_pid(Some(running_pid));
    
    // Get vruntime before tick using REAL kernel function
    let vrt_before = get_process_vruntime(running_pid).unwrap();
    
    // Use REAL kernel tick() - running process vruntime should increase
    tick(1); // 1 ms elapsed
    
    // Running process vruntime should increase
    let vrt_after = get_process_vruntime(running_pid).unwrap();
    assert!(vrt_after > vrt_before,
        "Running process vruntime did not increase after tick");
    
    cleanup_process(running_pid);
}

#[test]
#[serial]
fn test_tick_only_updates_running_process() {
    // This test verifies the core EEVDF invariant: tick() only updates
    // the vruntime of the currently RUNNING process, not Sleeping/Ready ones
    let sleeping_pid = next_pid();
    let ready_pid = next_pid();
    let running_pid = next_pid();
    
    add_process_with_state(sleeping_pid, ProcessState::Sleeping, 1_000_000);
    add_process_with_state(ready_pid, ProcessState::Ready, 1_000_000);
    add_process_with_state(running_pid, ProcessState::Running, 1_000_000);
    // Set current_pid so tick() can find the running process
    set_current_pid(Some(running_pid));
    
    // Capture all vruntimes before tick
    let sleep_vrt_before = get_process_vruntime(sleeping_pid).unwrap();
    let ready_vrt_before = get_process_vruntime(ready_pid).unwrap();
    let running_vrt_before = get_process_vruntime(running_pid).unwrap();
    
    // Use REAL kernel tick()
    tick(1);
    
    // Verify invariants
    let sleep_vrt_after = get_process_vruntime(sleeping_pid).unwrap();
    let ready_vrt_after = get_process_vruntime(ready_pid).unwrap();
    let running_vrt_after = get_process_vruntime(running_pid).unwrap();
    
    assert_eq!(sleep_vrt_before, sleep_vrt_after, "Sleeping vruntime changed!");
    assert_eq!(ready_vrt_before, ready_vrt_after, "Ready vruntime changed!");
    assert!(running_vrt_after > running_vrt_before, "Running vruntime should increase!");
    
    cleanup_processes(&[sleeping_pid, ready_pid, running_pid]);
}

#[test]
#[serial]
fn test_slice_remaining_decreases_for_running() {
    let running_pid = next_pid();
    
    add_process_with_state(running_pid, ProcessState::Running, 0);
    // Set current_pid so tick() can find the running process
    set_current_pid(Some(running_pid));
    
    // Get slice_remaining before tick using REAL kernel function
    let slice_before = get_process_slice_remaining(running_pid).unwrap();
    
    // Use REAL kernel tick()
    tick(1); // 1 ms = 1_000_000 ns
    
    let slice_after = get_process_slice_remaining(running_pid).unwrap();
    assert!(slice_after < slice_before,
        "slice_remaining should decrease for running process");
    
    cleanup_process(running_pid);
}

#[test]
#[serial]
fn test_state_query_via_kernel_function() {
    let pid = next_pid();
    
    add_process_with_state(pid, ProcessState::Sleeping, 0);
    
    // Use REAL kernel function
    assert_eq!(get_process_state(pid), Some(ProcessState::Sleeping));
    
    let _ = set_process_state(pid, ProcessState::Ready);
    assert_eq!(get_process_state(pid), Some(ProcessState::Ready));
    
    let _ = set_process_state(pid, ProcessState::Running);
    assert_eq!(get_process_state(pid), Some(ProcessState::Running));
    
    cleanup_process(pid);
}

#[test]
#[serial]
fn test_set_vruntime_via_kernel_function() {
    let pid = next_pid();
    
    add_process_with_state(pid, ProcessState::Ready, 1_000_000);
    
    // Use REAL kernel setter
    set_process_vruntime(pid, 5_000_000).unwrap();
    
    // Use REAL kernel getter
    let vrt = get_process_vruntime(pid).unwrap();
    assert_eq!(vrt, 5_000_000);
    
    cleanup_process(pid);
}
