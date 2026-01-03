//! Process State Machine Bug Detection Tests
//!
//! These tests verify the correctness of ProcessState transitions and
//! detect bugs that can cause processes to get stuck or behave incorrectly.
//!
//! ## State Machine:
//!
//! ```text
//!                  +-------+
//!                  | Ready |<----+
//!                  +---+---+     |
//!                      |        |
//!          schedule()  |        | wake_process()
//!                      v        |
//!                  +-------+    |
//!           +----->|Running|----+
//!           |      +---+---+
//!           |          |
//! preempt() |          | sleep()/exit()
//!           |          v
//!           |      +-------+     +-------+
//!           +------+Sleeping|    | Zombie|
//!                  +-------+     +-------+
//! ```
//!
//! ## Critical Invariants:
//!
//! 1. Running -> Ready (preemption) must preserve context
//! 2. Running -> Sleeping must set wake_pending check
//! 3. Sleeping -> Ready must reset vruntime appropriately
//! 4. Any -> Zombie must set exit_code first
//! 5. Zombie cannot transition to any other state

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, Context};
    use crate::scheduler::{
        wake_process, set_process_state, process_table_lock,
        SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
        calc_vdeadline, get_process_state,
    };
    use crate::scheduler::percpu::{init_percpu_sched, check_need_resched};
    use crate::signal::SignalState;
    use crate::numa;

    use serial_test::serial;
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, Ordering};

    static INIT_PERCPU: Once = Once::new();
    static NEXT_PID: AtomicU64 = AtomicU64::new(100000);

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
            context: Context::zero(),
            has_entered_user: true,
            context_valid: true,
            is_fork_child: false,
            is_thread: false,
            cr3: 0x1000,
            tty: 1,
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
        let vdeadline = calc_vdeadline(0, BASE_SLICE_NS, NICE_0_WEIGHT);
        ProcessEntry {
            process: proc,
            vruntime: 0,
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
            numa_preferred_node: numa::NUMA_NO_NODE,
            numa_policy: numa::NumaPolicy::Local,
        }
    }

    fn add_process(pid: Pid, state: ProcessState) {
        ensure_percpu_init();
        let mut table = process_table_lock();
        for (idx, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                crate::process::register_pid_mapping(pid, idx as u16);
                let entry = make_process_entry(make_test_process(pid, state));
                *slot = Some(entry);
                return;
            }
        }
        panic!("Process table full");
    }

    fn add_process_with_entry<F>(pid: Pid, state: ProcessState, modify: F)
    where
        F: FnOnce(&mut ProcessEntry),
    {
        ensure_percpu_init();
        let mut table = process_table_lock();
        for (idx, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                crate::process::register_pid_mapping(pid, idx as u16);
                let mut entry = make_process_entry(make_test_process(pid, state));
                modify(&mut entry);
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
                    return;
                }
            }
        }
    }

    fn get_exit_code(pid: Pid) -> Option<i32> {
        let table = process_table_lock();
        table.iter()
            .filter_map(|s| s.as_ref())
            .find(|e| e.process.pid == pid)
            .map(|e| e.process.exit_code)
    }

    fn get_term_signal(pid: Pid) -> Option<Option<i32>> {
        let table = process_table_lock();
        table.iter()
            .filter_map(|s| s.as_ref())
            .find(|e| e.process.pid == pid)
            .map(|e| e.process.term_signal)
    }

    fn get_wake_pending(pid: Pid) -> Option<bool> {
        let table = process_table_lock();
        table.iter()
            .filter_map(|s| s.as_ref())
            .find(|e| e.process.pid == pid)
            .map(|e| e.process.wake_pending)
    }

    fn set_exit_code(pid: Pid, code: i32) {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    entry.process.exit_code = code;
                    return;
                }
            }
        }
    }

    fn set_term_signal(pid: Pid, sig: Option<i32>) {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    entry.process.term_signal = sig;
                    return;
                }
            }
        }
    }

    // =========================================================================
    // Valid State Transitions
    // =========================================================================

    /// TEST: Ready -> Sleeping is valid
    #[test]
    #[serial]
    fn valid_ready_to_sleeping() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        let result = set_process_state(pid, ProcessState::Sleeping);
        let state = get_process_state(pid);

        cleanup_process(pid);

        assert!(result.is_ok());
        assert_eq!(state, Some(ProcessState::Sleeping));
    }

    /// TEST: Sleeping -> Ready via wake_process
    #[test]
    #[serial]
    fn valid_sleeping_to_ready() {
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);

        let woke = wake_process(pid);
        let state = get_process_state(pid);

        cleanup_process(pid);

        assert!(woke);
        assert_eq!(state, Some(ProcessState::Ready));
    }

    /// TEST: Running -> Ready (preemption)
    #[test]
    #[serial]
    fn valid_running_to_ready() {
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        let result = set_process_state(pid, ProcessState::Ready);
        let state = get_process_state(pid);

        cleanup_process(pid);

        assert!(result.is_ok());
        assert_eq!(state, Some(ProcessState::Ready));
    }

    /// TEST: Running -> Zombie (exit)
    #[test]
    #[serial]
    fn valid_running_to_zombie() {
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        // Set exit code first (correct order)
        set_exit_code(pid, 42);
        let result = set_process_state(pid, ProcessState::Zombie);
        
        let state = get_process_state(pid);
        let exit_code = get_exit_code(pid);

        cleanup_process(pid);

        assert!(result.is_ok());
        assert_eq!(state, Some(ProcessState::Zombie));
        assert_eq!(exit_code, Some(42));
    }

    // =========================================================================
    // Invalid State Transitions (Bug Detection)
    // =========================================================================

    /// BUG TEST: Zombie cannot transition to Ready
    #[test]
    #[serial]
    fn bug_zombie_to_ready_blocked() {
        let pid = next_pid();
        add_process(pid, ProcessState::Zombie);

        // Attempt to wake zombie (should fail)
        let woke = wake_process(pid);
        let state = get_process_state(pid);

        cleanup_process(pid);

        assert!(!woke, "wake_process should return false for Zombie");
        assert_eq!(state, Some(ProcessState::Zombie),
            "BUG: Zombie transitioned to Ready!");
    }

    /// BUG TEST: Zombie cannot transition to Sleeping
    #[test]
    #[serial]
    fn bug_zombie_to_sleeping_blocked() {
        let pid = next_pid();
        add_process(pid, ProcessState::Zombie);

        // Attempt to sleep zombie
        let _ = set_process_state(pid, ProcessState::Sleeping);
        let state = get_process_state(pid);

        cleanup_process(pid);

        // Note: set_process_state doesn't validate state machine,
        // but this documents expected behavior
        // If it's Sleeping, that's a bug
        if state == Some(ProcessState::Sleeping) {
            panic!("BUG: Zombie transitioned to Sleeping!");
        }
    }

    // =========================================================================
    // Exit Code Ordering (Critical for wait4)
    // =========================================================================

    /// BUG TEST: exit_code must be set BEFORE state becomes Zombie
    ///
    /// If state is set to Zombie before exit_code, wait4() might
    /// see the Zombie but read garbage exit_code.
    #[test]
    #[serial]
    fn bug_exit_code_set_before_zombie() {
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        // CORRECT ORDER: exit_code first, then Zombie
        set_exit_code(pid, 123);
        let _ = set_process_state(pid, ProcessState::Zombie);

        let state = get_process_state(pid);
        let exit_code = get_exit_code(pid);

        cleanup_process(pid);

        assert_eq!(state, Some(ProcessState::Zombie));
        assert_eq!(exit_code, Some(123),
            "BUG: exit_code not preserved after Zombie transition!");
    }

    /// BUG TEST: term_signal must be set for signal-killed processes
    #[test]
    #[serial]
    fn bug_term_signal_set_for_killed() {
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        // Process killed by SIGTERM (15)
        set_term_signal(pid, Some(15));
        let _ = set_process_state(pid, ProcessState::Zombie);

        let state = get_process_state(pid);
        let term_sig = get_term_signal(pid);

        cleanup_process(pid);

        assert_eq!(state, Some(ProcessState::Zombie));
        assert_eq!(term_sig, Some(Some(15)),
            "BUG: term_signal lost after Zombie transition!");
    }

    // =========================================================================
    // Wake Pending Mechanism
    // =========================================================================

    /// BUG TEST: wake_pending set on Ready, consumed on sleep attempt
    #[test]
    #[serial]
    fn bug_wake_pending_lifecycle() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Initial state
        assert_eq!(get_wake_pending(pid), Some(false),
            "wake_pending should start false");

        // Wake on Ready - sets pending
        wake_process(pid);
        assert_eq!(get_wake_pending(pid), Some(true),
            "BUG: wake_pending not set after wake on Ready!");

        // Sleep attempt - should be blocked and consume pending
        let _ = set_process_state(pid, ProcessState::Sleeping);
        
        let state = get_process_state(pid);
        let pending = get_wake_pending(pid);

        cleanup_process(pid);

        assert_ne!(state, Some(ProcessState::Sleeping),
            "BUG: Process slept despite wake_pending!");
        assert_eq!(pending, Some(false),
            "BUG: wake_pending not consumed after blocked sleep!");
    }

    /// BUG TEST: wake_pending cleared after actual wake from Sleeping
    #[test]
    #[serial]
    fn bug_wake_pending_cleared_on_real_wake() {
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);

        // Shouldn't have pending when Sleeping, but test defensively
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if let Some(entry) = slot {
                    if entry.process.pid == pid {
                        entry.process.wake_pending = true; // Force set
                    }
                }
            }
        }

        // Wake from Sleeping
        wake_process(pid);

        let state = get_process_state(pid);
        let pending = get_wake_pending(pid);

        cleanup_process(pid);

        assert_eq!(state, Some(ProcessState::Ready));
        assert_eq!(pending, Some(false),
            "BUG: wake_pending not cleared after real wake!");
    }

    // =========================================================================
    // EEVDF Invariants on State Change
    // =========================================================================

    /// BUG TEST: vruntime must be adjusted on wake
    ///
    /// A process that slept for a long time should not be penalized
    /// with its old high vruntime.
    #[test]
    #[serial]
    fn bug_vruntime_reset_on_wake() {
        let pid = next_pid();
        
        // Create process with high vruntime
        add_process_with_entry(pid, ProcessState::Sleeping, |entry| {
            entry.vruntime = 1_000_000_000; // 1 second
        });

        // Wake it
        wake_process(pid);

        let vrt = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| e.vruntime)
        };

        cleanup_process(pid);

        // vruntime should be reset to near min_vruntime (with credit)
        // Not remain at 1 billion
        assert!(vrt.unwrap_or(u64::MAX) < 500_000_000,
            "BUG: vruntime ({:?}) not reset on wake! Process will be starved.", vrt);
    }

    /// BUG TEST: lag must be reset on wake
    ///
    /// Negative lag makes process ineligible for EEVDF scheduling.
    #[test]
    #[serial]
    fn bug_lag_reset_on_wake() {
        let pid = next_pid();
        
        // Create process with negative lag
        add_process_with_entry(pid, ProcessState::Sleeping, |entry| {
            entry.lag = -100_000_000; // -100ms
        });

        // Wake it
        wake_process(pid);

        let lag = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| e.lag)
        };

        cleanup_process(pid);

        assert!(lag.unwrap_or(-1) >= 0,
            "BUG: lag ({:?}) not reset on wake! Process is EEVDF-ineligible.", lag);
    }

    // =========================================================================
    // Stress: Rapid State Changes
    // =========================================================================

    /// Stress test: Rapid state transitions
    #[test]
    #[serial]
    fn stress_rapid_state_changes() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        for i in 0..100 {
            // Ready -> Sleeping
            let _ = set_process_state(pid, ProcessState::Sleeping);
            
            // If actually sleeping, wake it
            if get_process_state(pid) == Some(ProcessState::Sleeping) {
                wake_process(pid);
            }

            // Should be Ready
            let state = get_process_state(pid);
            assert!(state == Some(ProcessState::Ready) || state == Some(ProcessState::Sleeping),
                "Unexpected state on iteration {}: {:?}", i, state);

            // Force back to Ready for next iteration
            if state == Some(ProcessState::Sleeping) {
                wake_process(pid);
            }
        }

        cleanup_process(pid);
    }

    /// Stress test: Multiple processes state changes
    #[test]
    #[serial]
    fn stress_multiple_process_state_changes() {
        let mut pids = Vec::new();

        // Create 20 processes
        for _ in 0..20 {
            let pid = next_pid();
            add_process(pid, ProcessState::Ready);
            pids.push(pid);
        }

        // Randomly change states
        for iteration in 0..50 {
            for (i, &pid) in pids.iter().enumerate() {
                let action = (iteration + i) % 4;
                match action {
                    0 => { let _ = set_process_state(pid, ProcessState::Sleeping); }
                    1 => { wake_process(pid); }
                    2 => { let _ = set_process_state(pid, ProcessState::Ready); }
                    _ => { /* no-op */ }
                }
            }
        }

        // All should be in valid states
        for &pid in &pids {
            let state = get_process_state(pid);
            assert!(matches!(state, Some(ProcessState::Ready) | Some(ProcessState::Sleeping) | Some(ProcessState::Running)),
                "Invalid state for PID {}: {:?}", pid, state);
            cleanup_process(pid);
        }
    }
}
