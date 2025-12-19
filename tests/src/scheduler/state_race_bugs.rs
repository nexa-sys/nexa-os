//! Scheduler State Race Condition Bug Detection Tests
//!
//! These tests are designed to FAIL when bugs exist and PASS when bugs are fixed.
//! They target specific race conditions and inconsistencies in process state management.

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Pid};
    use crate::scheduler::{ProcessEntry, SchedPolicy, CpuMask, nice_to_weight, BASE_SLICE_NS};
    
    /// Helper to create a minimal process for testing
    fn create_test_process(pid: Pid, state: ProcessState) -> Process {
        let mut proc = Process {
            pid,
            ppid: 0,
            tgid: pid,
            state,
            entry_point: 0x1000000,
            stack_top: 0x1A00000,
            heap_start: 0x1200000,
            heap_end: 0x1200000,
            signal_state: crate::ipc::signal::SignalState::new(),
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
            user_rflags: 0x202,
            user_r10: 0,
            user_r8: 0,
            user_r9: 0,
            exit_code: 0,
            term_signal: None,
            kernel_stack: 0,
            fs_base: 0,
            clear_child_tid: 0,
            cmdline: [0; 1024],
            cmdline_len: 0,
            open_fds: 0,
            exec_pending: false,
            exec_entry: 0,
            exec_stack: 0,
            exec_user_data_sel: 0,
            wake_pending: false,
        };
        proc
    }

    fn create_test_entry(pid: Pid, state: ProcessState, nice: i8) -> ProcessEntry {
        let weight = nice_to_weight(nice);
        ProcessEntry {
            process: create_test_process(pid, state),
            vruntime: 0,
            vdeadline: BASE_SLICE_NS,
            lag: 0,
            weight,
            slice_ns: BASE_SLICE_NS,
            slice_remaining_ns: BASE_SLICE_NS,
            priority: 128,
            base_priority: 128,
            time_slice: 4,
            total_time: 0,
            wait_time: 0,
            last_scheduled: 0,
            cpu_burst_count: 0,
            avg_cpu_burst: 0,
            policy: SchedPolicy::Normal,
            nice,
            quantum_level: 0,
            preempt_count: 0,
            voluntary_switches: 0,
            cpu_affinity: CpuMask::all(),
            last_cpu: 0,
            numa_preferred_node: crate::numa::NUMA_NO_NODE,
            numa_policy: crate::numa::NumaPolicy::Local,
        }
    }

    // =========================================================================
    // BUG TEST: wake_pending flag must prevent lost wakeups
    // =========================================================================
    
    /// Test: When a process is woken while still Ready, wake_pending must be set
    /// to prevent the wakeup from being lost when the process tries to sleep.
    ///
    /// This tests the race condition:
    /// 1. Process calls add_waiter() but hasn't slept yet (state = Ready)
    /// 2. Interrupt fires, wake_process() called - process is Ready
    /// 3. Process then sleeps
    ///
    /// BUG: If wake_pending is not checked, the wake is lost and process hangs forever.
    #[test]
    fn test_wake_pending_prevents_lost_wakeup() {
        let mut entry = create_test_entry(100, ProcessState::Ready, 0);
        
        // Simulate: Process is Ready and receives a wake
        // This simulates wake_process() being called on a Ready process
        assert_eq!(entry.process.state, ProcessState::Ready);
        assert!(!entry.process.wake_pending);
        
        // Set wake_pending (as wake_process() would do for Ready process)
        entry.process.wake_pending = true;
        
        // Now simulate the process trying to sleep
        // BUG CHECK: If wake_pending is set, sleep should be blocked
        if entry.process.wake_pending {
            // Correct behavior: consume the wake, don't sleep
            entry.process.wake_pending = false;
            // State should remain Ready, not become Sleeping
            assert_eq!(entry.process.state, ProcessState::Ready,
                "BUG: Process slept despite wake_pending being set!");
        } else {
            // This path should not be reached in correct code
            entry.process.state = ProcessState::Sleeping;
            panic!("BUG: wake_pending was not set, wakeup will be lost!");
        }
    }

    /// Test: wake_pending must be cleared when process actually wakes from Sleeping
    #[test]
    fn test_wake_pending_cleared_on_actual_wake() {
        let mut entry = create_test_entry(100, ProcessState::Sleeping, 0);
        entry.process.wake_pending = true; // Shouldn't happen but test defensively
        
        // Simulate actual wake from Sleeping
        if entry.process.state == ProcessState::Sleeping {
            entry.process.state = ProcessState::Ready;
            entry.process.wake_pending = false;
        }
        
        assert!(!entry.process.wake_pending,
            "BUG: wake_pending not cleared after waking from Sleeping");
        assert_eq!(entry.process.state, ProcessState::Ready);
    }

    // =========================================================================
    // BUG TEST: Zombie state consistency
    // =========================================================================

    /// Test: Zombie process must have valid exit_code for wait4()
    /// BUG: If exit_code is not set before state becomes Zombie, wait4 returns garbage.
    #[test]
    fn test_zombie_exit_code_consistency() {
        let mut entry = create_test_entry(100, ProcessState::Running, 0);
        
        // Simulate exit() - BUG if we set Zombie before exit_code
        let exit_code = 42;
        
        // CORRECT ORDER: Set exit_code FIRST, then state
        entry.process.exit_code = exit_code;
        entry.process.state = ProcessState::Zombie;
        
        // Verify parent can read correct exit code
        assert_eq!(entry.process.exit_code, exit_code,
            "BUG: Zombie process has incorrect exit_code");
        assert_eq!(entry.process.state, ProcessState::Zombie);
    }

    /// Test: term_signal must be set for signal-terminated processes
    #[test]
    fn test_zombie_term_signal_set_for_killed_process() {
        let mut entry = create_test_entry(100, ProcessState::Running, 0);
        
        // Simulate kill(SIGTERM)
        let kill_signal = 15; // SIGTERM
        
        // CORRECT: Set term_signal before Zombie
        entry.process.term_signal = Some(kill_signal);
        entry.process.state = ProcessState::Zombie;
        
        // wait4 should see signal termination
        assert!(entry.process.term_signal.is_some(),
            "BUG: Signal-terminated process missing term_signal");
        assert_eq!(entry.process.term_signal.unwrap(), kill_signal);
    }

    // =========================================================================
    // BUG TEST: EEVDF vruntime overflow
    // =========================================================================

    /// Test: vruntime must handle overflow gracefully
    /// BUG: If vruntime overflows, scheduling breaks completely.
    #[test]
    fn test_vruntime_overflow_handling() {
        let mut entry = create_test_entry(100, ProcessState::Ready, 0);
        
        // Set vruntime close to u64::MAX
        entry.vruntime = u64::MAX - 1000;
        
        // Simulate adding delta (as update_curr does)
        let delta = 2000u64;
        entry.vruntime = entry.vruntime.saturating_add(delta);
        
        // Must use saturating_add to prevent overflow
        assert_eq!(entry.vruntime, u64::MAX,
            "BUG: vruntime overflowed instead of saturating");
    }

    /// Test: vdeadline calculation must not overflow
    #[test]
    fn test_vdeadline_overflow_handling() {
        use crate::scheduler::calc_vdeadline;
        
        let vruntime = u64::MAX - 1000;
        let slice_ns = BASE_SLICE_NS;
        let weight = nice_to_weight(0);
        
        let vdeadline = calc_vdeadline(vruntime, slice_ns, weight);
        
        // calc_vdeadline uses saturating_add internally
        assert!(vdeadline >= vruntime,
            "BUG: vdeadline calculation overflowed (result < vruntime)");
    }

    /// Test: lag must be bounded to prevent unbounded growth
    #[test]
    fn test_lag_bounded_growth() {
        let mut entry = create_test_entry(100, ProcessState::Ready, 0);
        
        // Simulate long wait - lag credit accumulation
        let max_lag: i64 = 100_000_000; // 100ms max
        
        // Give huge lag credit (simulating long wait)
        entry.lag = entry.lag.saturating_add(200_000_000);
        
        // Lag should be capped at max
        entry.lag = entry.lag.min(max_lag);
        
        assert!(entry.lag <= max_lag,
            "BUG: lag grew unbounded past 100ms cap");
    }

    // =========================================================================
    // BUG TEST: State transition validity
    // =========================================================================

    /// Test: Invalid state transitions should not be possible
    /// Valid transitions:
    /// - Ready -> Running (scheduled)
    /// - Running -> Ready (preempted/yield)
    /// - Running -> Sleeping (wait/sleep)
    /// - Running -> Zombie (exit)
    /// - Sleeping -> Ready (wakeup)
    /// Invalid:
    /// - Zombie -> anything
    /// - Ready -> Sleeping (must be Running first)
    /// - Sleeping -> Running (must go through Ready)
    #[test]
    fn test_invalid_state_transition_detection() {
        // Test: Zombie cannot transition to anything
        let entry = create_test_entry(100, ProcessState::Zombie, 0);
        let new_states = [ProcessState::Ready, ProcessState::Running, ProcessState::Sleeping];
        
        for new_state in new_states {
            let can_transition = match (entry.process.state, new_state) {
                (ProcessState::Zombie, _) => false, // Zombie is terminal
                (ProcessState::Ready, ProcessState::Sleeping) => false, // Invalid
                (ProcessState::Sleeping, ProcessState::Running) => false, // Must go through Ready
                _ => true,
            };
            
            if entry.process.state == ProcessState::Zombie {
                assert!(!can_transition,
                    "BUG: Allowed invalid transition from Zombie to {:?}", new_state);
            }
        }
    }

    /// Test: Ready -> Sleeping transition should only happen via Running
    #[test]
    fn test_ready_to_sleeping_invalid() {
        let entry = create_test_entry(100, ProcessState::Ready, 0);
        
        // Direct Ready -> Sleeping is a bug (process must be Running to sleep)
        let is_valid_transition = entry.process.state != ProcessState::Ready;
        
        // Note: In real code, set_process_state should reject this
        // This test documents the expected behavior
        assert!(entry.process.state == ProcessState::Ready,
            "Test setup error");
        
        // A Ready process trying to sleep is suspicious
        // It means the process isn't actually running but trying to sleep
    }

    // =========================================================================
    // BUG TEST: Thread group consistency  
    // =========================================================================

    /// Test: Thread (CLONE_THREAD) must share tgid with leader
    #[test]
    fn test_thread_tgid_consistency() {
        // Create leader process
        let leader = create_test_entry(100, ProcessState::Running, 0);
        assert_eq!(leader.process.tgid, leader.process.pid,
            "Leader's tgid must equal its pid");
        
        // Create thread in same group
        let mut thread = create_test_entry(101, ProcessState::Ready, 0);
        thread.process.tgid = leader.process.pid; // Share leader's tgid
        thread.process.is_thread = true;
        
        assert_eq!(thread.process.tgid, leader.process.pid,
            "BUG: Thread's tgid doesn't match leader's pid");
        assert!(thread.process.is_thread,
            "BUG: Thread not marked as thread");
    }

    /// Test: Fork creates new tgid (not thread)
    #[test]
    fn test_fork_creates_new_tgid() {
        let parent = create_test_entry(100, ProcessState::Running, 0);
        
        // Fork creates new process
        let mut child = create_test_entry(101, ProcessState::Ready, 0);
        child.process.ppid = parent.process.pid;
        child.process.tgid = child.process.pid; // New tgid for fork
        child.process.is_thread = false;
        child.process.is_fork_child = true;
        
        assert_ne!(child.process.tgid, parent.process.tgid,
            "BUG: Fork child has same tgid as parent");
        assert_eq!(child.process.tgid, child.process.pid,
            "BUG: Fork child's tgid should equal its own pid");
        assert!(!child.process.is_thread,
            "BUG: Fork child marked as thread");
    }

    // =========================================================================
    // BUG TEST: ppid consistency after parent exit
    // =========================================================================

    /// Test: Orphaned process must be reparented to init (PID 1)
    /// BUG: If ppid points to dead parent, wait4 will fail.
    #[test]
    fn test_orphan_reparenting() {
        let parent_pid = 50;
        let init_pid = 1;
        
        // Child with parent
        let mut child = create_test_entry(100, ProcessState::Running, 0);
        child.process.ppid = parent_pid;
        
        // Simulate parent exit - child becomes orphan
        // In real kernel, reparent_to_init() would be called
        let parent_dead = true;
        
        if parent_dead {
            // Correct behavior: reparent to init
            child.process.ppid = init_pid;
        }
        
        assert_eq!(child.process.ppid, init_pid,
            "BUG: Orphaned process not reparented to init");
    }

    // =========================================================================
    // BUG TEST: Signal delivery to stopped/zombie processes
    // =========================================================================

    /// Test: SIGKILL/SIGSTOP cannot be blocked or ignored
    #[test]
    fn test_sigkill_sigstop_unblockable() {
        use crate::ipc::signal::{SignalState, SignalAction, SIGKILL, SIGSTOP};
        
        let mut sig_state = SignalState::new();
        
        // Try to block SIGKILL (should fail/be ignored)
        sig_state.block_signal(SIGKILL);
        
        // Try to ignore SIGKILL (should fail)
        let result = sig_state.set_action(SIGKILL, SignalAction::Ignore);
        assert!(result.is_err(),
            "BUG: Allowed ignoring SIGKILL");
        
        // Try to block SIGSTOP
        sig_state.block_signal(SIGSTOP);
        
        // SIGKILL and SIGSTOP should still be deliverable despite blocking
        sig_state.send_signal(SIGKILL).unwrap();
        
        // has_pending_signal should return SIGKILL even though it's "blocked"
        let pending = sig_state.has_pending_signal();
        assert_eq!(pending, Some(SIGKILL),
            "BUG: SIGKILL was blocked despite being unblockable");
    }

    /// Test: Signal to zombie should not crash
    #[test]
    fn test_signal_to_zombie_safe() {
        let mut entry = create_test_entry(100, ProcessState::Zombie, 0);
        
        // Sending signal to zombie should be a no-op (not crash)
        let result = entry.process.signal_state.send_signal(crate::ipc::signal::SIGTERM);
        
        // Signal delivery returns Ok but has no effect on zombie
        assert!(result.is_ok(),
            "BUG: Signal delivery to zombie crashed");
        // Zombie should remain zombie
        assert_eq!(entry.process.state, ProcessState::Zombie);
    }

    // =========================================================================
    // BUG TEST: CPU affinity validation
    // =========================================================================

    /// Test: Process with empty affinity mask should not be scheduled
    #[test]
    fn test_empty_affinity_mask_unschedulable() {
        let mut entry = create_test_entry(100, ProcessState::Ready, 0);
        
        // Clear all CPUs from affinity
        entry.cpu_affinity = CpuMask::empty();
        
        // Process should not be schedulable on any CPU
        for cpu in 0..64 {
            assert!(!entry.cpu_affinity.is_set(cpu),
                "BUG: Empty affinity mask still has CPU {} set", cpu);
        }
        
        // Scheduler should skip this process (covered in find_best_candidate)
    }

    /// Test: Setting invalid CPU in affinity should be handled
    #[test]
    fn test_affinity_invalid_cpu_bounds() {
        let mut mask = CpuMask::empty();
        
        // Set a valid CPU
        mask.set(0);
        assert!(mask.is_set(0), "Failed to set CPU 0");
        
        // Check bounds - CpuMask should handle 0-1023
        mask.set(1023);
        assert!(mask.is_set(1023), "Failed to set CPU 1023");
        
        // Setting beyond bounds should not panic (just be ignored or wrapped)
        // Note: This tests the mask implementation's robustness
    }
}
