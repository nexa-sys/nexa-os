//! EEVDF vruntime Leak Detection Tests
//!
//! Uses REAL kernel functions - NO local re-implementations.
//!
//! These tests detect bugs where vruntime grows incorrectly for processes
//! that are NOT actually consuming CPU time (e.g., sleeping on I/O).

use crate::scheduler::{
    wake_process, set_process_state, process_table_lock,
    SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
    calc_vdeadline, get_min_vruntime,
    // Use REAL kernel query/setter functions
    get_process_state, get_process_vruntime, set_process_vruntime,
};
use crate::scheduler::percpu::init_percpu_sched;
use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE};
use crate::signal::SignalState;

use std::sync::Once;
use serial_test::serial;
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

// =============================================================================
// VRUNTIME LEAK TESTS - Using REAL kernel functions
// =============================================================================

#[test]
#[serial]
fn test_sleeping_process_vruntime_stable() {
    let pid = next_pid();
    let initial_vrt = 1_000_000u64;
    add_process(pid, ProcessState::Sleeping, initial_vrt);
    
    // Use REAL kernel function
    let final_vrt = get_process_vruntime(pid).unwrap();
    assert_eq!(final_vrt, initial_vrt, "Sleeping process vruntime changed!");
    
    cleanup_process(pid);
}

#[test]
#[serial]
fn test_vruntime_not_doubled_on_sleep_wake_cycle() {
    let pid = next_pid();
    let initial_vrt = 1_000_000u64;
    add_process(pid, ProcessState::Running, initial_vrt);
    
    // Simulate sleep/wake cycle
    let _ = set_process_state(pid, ProcessState::Sleeping);
    wake_process(pid);
    
    // Use REAL kernel function
    let final_vrt = get_process_vruntime(pid).unwrap();
    
    // vruntime should NOT double during sleep/wake
    assert!(final_vrt < initial_vrt * 2, 
        "vruntime doubled from {} to {} during sleep/wake!", initial_vrt, final_vrt);
    
    cleanup_process(pid);
}

#[test]
#[serial]
fn test_wake_preserves_reasonable_vruntime() {
    let pid = next_pid();
    let initial_vrt = 1_000_000u64;
    add_process(pid, ProcessState::Sleeping, initial_vrt);
    
    wake_process(pid);
    
    // Use REAL kernel function
    let woken_vrt = get_process_vruntime(pid).unwrap();
    let min_vrt = get_min_vruntime();
    
    // After wake, vruntime should be close to initial or min_vruntime
    // (depending on how wake_process adjusts it)
    assert!(woken_vrt <= initial_vrt.max(min_vrt) + BASE_SLICE_NS,
        "vruntime {} is too high after wake (was {}, min={})", 
        woken_vrt, initial_vrt, min_vrt);
    
    cleanup_process(pid);
}

#[test]
#[serial]
fn test_ready_process_vruntime_stable() {
    let pid = next_pid();
    let initial_vrt = 2_000_000u64;
    add_process(pid, ProcessState::Ready, initial_vrt);
    
    // Use REAL kernel function
    let vrt = get_process_vruntime(pid).unwrap();
    assert_eq!(vrt, initial_vrt, "Ready process vruntime changed!");
    
    cleanup_process(pid);
}

#[test]
#[serial]
fn test_set_and_get_vruntime_kernel_functions() {
    let pid = next_pid();
    add_process(pid, ProcessState::Ready, 0);
    
    // Use REAL kernel setter
    set_process_vruntime(pid, 12345678).unwrap();
    
    // Use REAL kernel getter
    let vrt = get_process_vruntime(pid).unwrap();
    assert_eq!(vrt, 12345678);
    
    cleanup_process(pid);
}

#[test]
#[serial]
fn test_state_transitions_via_kernel() {
    let pid = next_pid();
    add_process(pid, ProcessState::Ready, 0);
    
    // Use REAL kernel function
    assert_eq!(get_process_state(pid), Some(ProcessState::Ready));
    
    let _ = set_process_state(pid, ProcessState::Running);
    assert_eq!(get_process_state(pid), Some(ProcessState::Running));
    
    let _ = set_process_state(pid, ProcessState::Sleeping);
    assert_eq!(get_process_state(pid), Some(ProcessState::Sleeping));
    
    wake_process(pid);
    assert_eq!(get_process_state(pid), Some(ProcessState::Ready));
    
    cleanup_process(pid);
}

#[test]
#[serial]
fn test_multiple_sleep_wake_cycles() {
    let pid = next_pid();
    let initial_vrt = 1_000_000u64;
    add_process(pid, ProcessState::Ready, initial_vrt);
    
    // Simulate multiple sleep/wake cycles
    for _ in 0..10 {
        let _ = set_process_state(pid, ProcessState::Sleeping);
        wake_process(pid);
    }
    
    // Use REAL kernel function
    let final_vrt = get_process_vruntime(pid).unwrap();
    
    // vruntime should NOT have grown exponentially
    assert!(final_vrt < initial_vrt * 10,
        "vruntime grew too much after sleep/wake cycles: {} (was {})", 
        final_vrt, initial_vrt);
    
    cleanup_process(pid);
}
