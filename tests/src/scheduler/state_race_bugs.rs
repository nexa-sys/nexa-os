//! Scheduler State Race Condition Bug Detection Tests
//!
//! These tests call REAL scheduler functions (wake_process, set_process_state, etc.)
//! to test actual kernel behavior, not simulated local operations.
//!
//! Tests are designed to FAIL when bugs exist and PASS when bugs are fixed.

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, Context, register_pid_mapping, unregister_pid_mapping};
    use crate::scheduler::{
        wake_process, set_process_state, process_table_lock,
        ProcessEntry, SchedPolicy, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
        calc_vdeadline, nice_to_weight, get_process_state,
    };
    use crate::signal::SignalState;
    use crate::numa;
    use crate::ipc::signal::{SignalAction, SIGKILL, SIGSTOP};

    use serial_test::serial;
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, Ordering};

    static INIT: Once = Once::new();
    static NEXT_PID: AtomicU64 = AtomicU64::new(90000);

    fn next_pid() -> Pid {
        NEXT_PID.fetch_add(1, Ordering::SeqCst)
    }

    fn ensure_init() {
        INIT.call_once(|| {
            crate::scheduler::percpu::init_percpu_sched(0);
        });
    }

    fn make_process(pid: Pid, state: ProcessState) -> Process {
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

    fn make_entry(proc: Process) -> ProcessEntry {
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

    /// Add process to REAL scheduler process table
    fn add_process(pid: Pid, state: ProcessState) {
        ensure_init();
        let mut table = process_table_lock();
        for (idx, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                register_pid_mapping(pid, idx as u16);
                *slot = Some(make_entry(make_process(pid, state)));
                return;
            }
        }
        panic!("Process table full");
    }

    /// Remove process from REAL scheduler process table
    fn cleanup_process(pid: Pid) {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    unregister_pid_mapping(pid);
                    *slot = None;
                    return;
                }
            }
        }
    }

    fn get_wake_pending(pid: Pid) -> Option<bool> {
        let table = process_table_lock();
        table.iter()
            .filter_map(|s| s.as_ref())
            .find(|e| e.process.pid == pid)
            .map(|e| e.process.wake_pending)
    }

    // =========================================================================
    // BUG TEST: wake_pending flag - REAL scheduler tests
    // =========================================================================
    
    /// Test: wake_process on Ready MUST set wake_pending
    ///
    /// Race condition:
    /// 1. Process calls add_waiter() but hasn't slept yet (state = Ready)
    /// 2. Interrupt fires, wake_process() called - process is Ready
    /// 3. Process then calls set_process_state(Sleeping)
    ///
    /// BUG: If wake_pending is not set, the wake is lost and process hangs forever.
    #[test]
    #[serial]
    fn test_wake_pending_on_ready_process() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Call REAL wake_process on Ready process
        let woke = wake_process(pid);

        let pending = get_wake_pending(pid);
        cleanup_process(pid);

        // wake_process returns false for Ready (already awake)
        assert!(!woke, "wake_process should return false for Ready");
        // BUT must set wake_pending
        assert_eq!(pending, Some(true),
            "BUG: wake_process on Ready did NOT set wake_pending! \
             This causes lost wakeups and stuck processes.");
    }

    /// Test: set_process_state(Sleeping) MUST check wake_pending
    #[test]
    #[serial]
    fn test_sleep_blocked_by_wake_pending() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Call REAL wake_process (sets wake_pending)
        wake_process(pid);

        // Call REAL set_process_state
        let _ = set_process_state(pid, ProcessState::Sleeping);

        let state = get_process_state(pid);
        let pending = get_wake_pending(pid);
        cleanup_process(pid);

        // Process MUST NOT be sleeping (wake_pending blocks it)
        assert_ne!(state, Some(ProcessState::Sleeping),
            "BUG: set_process_state allowed Sleeping despite wake_pending!");
        // wake_pending should be consumed
        assert_eq!(pending, Some(false),
            "BUG: wake_pending not consumed after blocking sleep!");
    }

    /// Test: wake_pending cleared after waking from Sleeping
    #[test]
    #[serial]
    fn test_wake_pending_cleared_on_sleep_to_ready() {
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);

        // Call REAL wake_process on Sleeping
        let woke = wake_process(pid);

        let state = get_process_state(pid);
        let pending = get_wake_pending(pid);
        cleanup_process(pid);

        assert!(woke, "wake_process should return true for Sleeping");
        assert_eq!(state, Some(ProcessState::Ready), "Should be Ready after wake");
        assert_eq!(pending, Some(false), "wake_pending should be false after actual wake");
    }

    // =========================================================================
    // BUG TEST: State transitions - REAL scheduler tests
    // =========================================================================

    /// Test: Zombie process cannot be woken
    #[test]
    #[serial]
    fn test_zombie_cannot_wake() {
        let pid = next_pid();
        add_process(pid, ProcessState::Zombie);

        // Call REAL wake_process on Zombie
        let woke = wake_process(pid);
        let state = get_process_state(pid);

        cleanup_process(pid);

        assert!(!woke, "wake_process should return false for Zombie");
        assert_eq!(state, Some(ProcessState::Zombie), "Zombie should stay Zombie");
    }

    /// Test: Running process wake sets wake_pending
    #[test]
    #[serial]
    fn test_running_wake_sets_pending() {
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        // Call REAL wake_process on Running
        let woke = wake_process(pid);
        let pending = get_wake_pending(pid);

        cleanup_process(pid);

        assert!(!woke, "wake_process should return false for Running");
        assert_eq!(pending, Some(true),
            "BUG: wake_process on Running did NOT set wake_pending!");
    }

    // =========================================================================
    // BUG TEST: vruntime overflow - uses real calc functions
    // =========================================================================

    /// Test: vruntime calculations must not overflow
    #[test]
    fn test_vruntime_overflow_protection() {
        // Use REAL calc_vdeadline function
        let vruntime = u64::MAX - 1000;
        let vdeadline = calc_vdeadline(vruntime, BASE_SLICE_NS, NICE_0_WEIGHT);

        // Must use saturating_add, not wrap
        assert!(vdeadline >= vruntime,
            "BUG: vdeadline ({}) < vruntime ({}), overflow occurred!",
            vdeadline, vruntime);
    }

    /// Test: nice_to_weight returns valid values
    #[test]
    fn test_nice_to_weight_valid() {
        // Use REAL nice_to_weight function
        for nice in -20i8..=19 {
            let weight = nice_to_weight(nice);
            assert!(weight > 0, "BUG: weight for nice {} is 0!", nice);
        }

        // Higher nice = lower weight
        let high_prio_weight = nice_to_weight(-20);
        let low_prio_weight = nice_to_weight(19);
        assert!(high_prio_weight > low_prio_weight,
            "BUG: nice -20 should have higher weight than nice 19");
    }

    // =========================================================================
    // BUG TEST: Signal delivery - uses real SignalState
    // =========================================================================

    /// Test: SIGKILL/SIGSTOP cannot be ignored
    #[test]
    fn test_sigkill_sigstop_unblockable() {
        let mut sig_state = SignalState::new();

        // Try to ignore SIGKILL - MUST fail
        let result = sig_state.set_action(SIGKILL, SignalAction::Ignore);
        assert!(result.is_err(),
            "BUG: Allowed ignoring SIGKILL!");

        // Try to ignore SIGSTOP - MUST fail
        let result = sig_state.set_action(SIGSTOP, SignalAction::Ignore);
        assert!(result.is_err(),
            "BUG: Allowed ignoring SIGSTOP!");
    }

    /// Test: SIGKILL cannot be blocked
    #[test]
    fn test_sigkill_unblockable_delivery() {
        let mut sig_state = SignalState::new();

        // Block SIGKILL (kernel should ignore this)
        sig_state.block_signal(SIGKILL);

        // Send SIGKILL
        sig_state.send_signal(SIGKILL).unwrap();

        // SIGKILL MUST still be pending (not blocked)
        let pending = sig_state.has_pending_signal();
        assert_eq!(pending, Some(SIGKILL),
            "BUG: SIGKILL was blocked despite being unblockable!");
    }

    // =========================================================================
    // BUG TEST: Rapid state transitions - stress test
    // =========================================================================

    /// Test: Rapid wake/sleep cycles must not lose wakes
    #[test]
    #[serial]
    fn test_rapid_wake_sleep_no_lost_wakes() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        let mut stuck_count = 0;

        for _ in 0..100 {
            // Wake on Ready
            wake_process(pid);
            // Try to sleep
            let _ = set_process_state(pid, ProcessState::Sleeping);

            if get_process_state(pid) == Some(ProcessState::Sleeping) {
                stuck_count += 1;
                // Recover
                wake_process(pid);
            }
        }

        cleanup_process(pid);

        assert_eq!(stuck_count, 0,
            "BUG: Process got stuck {} times in rapid wake/sleep cycles!", stuck_count);
    }

    // =========================================================================
    // BUG TEST: CPU affinity - uses real CpuMask
    // =========================================================================

    /// Test: Empty affinity mask
    #[test]
    fn test_cpu_affinity_empty_mask() {
        let mask = CpuMask::empty();
        
        for cpu in 0..64 {
            assert!(!mask.is_set(cpu),
                "Empty mask should have no CPUs set");
        }
    }

    /// Test: All CPUs mask
    #[test]
    fn test_cpu_affinity_all_mask() {
        let mask = CpuMask::all();
        
        // At least CPU 0 should be set
        assert!(mask.is_set(0), "all() mask should have CPU 0");
    }

    /// Test: Set/clear individual CPUs
    #[test]
    fn test_cpu_affinity_set_clear() {
        let mut mask = CpuMask::empty();
        
        mask.set(0);
        assert!(mask.is_set(0), "Failed to set CPU 0");
        
        mask.set(7);
        assert!(mask.is_set(7), "Failed to set CPU 7");
        
        mask.clear(0);
        assert!(!mask.is_set(0), "Failed to clear CPU 0");
        assert!(mask.is_set(7), "CPU 7 should still be set");
    }
}
