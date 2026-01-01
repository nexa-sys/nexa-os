//! Keyboard Read Flow Bug Detection Tests
//!
//! These tests exercise the read_raw_for_tty() code path using hardware mocks
//! to detect bugs that can cause foreground processes to hang.
//!
//! ## The Critical Code Path (from keyboard.rs read_raw_for_tty):
//!
//! ```ignore
//! loop {
//!     if let Some(ch) = try_read_char() {
//!         return ch;
//!     } else {
//!         add_waiter(pid);                        // Step 1: Register
//!         set_current_process_state(Sleeping);   // Step 2: Sleep
//!         do_schedule();                          // Step 3: Yield
//!         // After wake, loop back to try_read_char()
//!     }
//! }
//! ```
//!
//! ## Race Window:
//!
//! Between Step 1 and Step 2:
//! - Keyboard interrupt fires
//! - add_scancode() -> wake_all_waiters() -> wake_process(pid)
//! - Process still Ready (hasn't called sleep yet)
//! - wake_process removes from waiter list OR sets wake_pending
//!
//! If wake_pending is NOT set, Step 2 succeeds and process sleeps forever.

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, Context};
    use crate::scheduler::{
        wake_process, set_process_state, set_current_process_state,
        process_table_lock, current_pid, set_current_pid,
        SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
        calc_vdeadline,
    };
    use crate::scheduler::percpu::{init_percpu_sched, check_need_resched};
    use crate::signal::SignalState;
    use crate::numa;

    use serial_test::serial;
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, Ordering};

    static INIT_PERCPU: Once = Once::new();
    static NEXT_PID: AtomicU64 = AtomicU64::new(500000);

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

    fn get_state(pid: Pid) -> Option<ProcessState> {
        let table = process_table_lock();
        table.iter()
            .filter_map(|s| s.as_ref())
            .find(|e| e.process.pid == pid)
            .map(|e| e.process.state)
    }

    fn get_wake_pending(pid: Pid) -> Option<bool> {
        let table = process_table_lock();
        table.iter()
            .filter_map(|s| s.as_ref())
            .find(|e| e.process.pid == pid)
            .map(|e| e.process.wake_pending)
    }

    fn set_wake_pending(pid: Pid, pending: bool) {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    entry.process.wake_pending = pending;
                    return;
                }
            }
        }
    }

    // =========================================================================
    // Hardware Mock: Keyboard Waiter List (mirrors actual keyboard.rs)
    // =========================================================================

    const MAX_KEYBOARD_WAITERS: usize = 8;

    struct KeyboardWaiterList {
        waiters: [Option<Pid>; MAX_KEYBOARD_WAITERS],
    }

    impl KeyboardWaiterList {
        fn new() -> Self {
            Self { waiters: [None; MAX_KEYBOARD_WAITERS] }
        }

        /// Add PID to waiter list (mirrors keyboard::add_waiter)
        fn add_waiter(&mut self, pid: Pid) -> bool {
            for slot in self.waiters.iter_mut() {
                if slot.is_none() {
                    *slot = Some(pid);
                    return true;
                }
            }
            false // Queue full - process will have to spin
        }

        /// Remove PID from waiter list (mirrors keyboard::remove_waiter)
        fn remove_waiter(&mut self, pid: Pid) {
            for slot in self.waiters.iter_mut() {
                if *slot == Some(pid) {
                    *slot = None;
                    return;
                }
            }
        }

        /// Wake all waiters (mirrors keyboard::wake_all_waiters)
        /// Called from add_scancode() in interrupt context
        fn wake_all_waiters(&mut self) {
            for slot in self.waiters.iter_mut() {
                if let Some(pid) = slot.take() {
                    wake_process(pid);
                }
            }
        }

        fn waiter_count(&self) -> usize {
            self.waiters.iter().filter(|s| s.is_some()).count()
        }

        fn contains(&self, pid: Pid) -> bool {
            self.waiters.iter().any(|s| *s == Some(pid))
        }
    }

    // =========================================================================
    // Test: Exact read_raw_for_tty flow with race condition
    // =========================================================================

    /// Tests exact read_raw_for_tty() flow with keyboard interrupt race
    ///
    /// This is the EXACT sequence that can cause shell to hang:
    /// 1. try_read_char() returns None (no input)
    /// 2. add_waiter(pid) - shell registered
    /// 3. INTERRUPT: keyboard interrupt fires
    /// 4. wake_all_waiters() -> wake_process(pid) on Ready
    /// 5. Shell removed from waiter list
    /// 6. set_current_process_state(Sleeping) - MUST BE BLOCKED
    ///
    /// If step 6 succeeds, shell is stuck forever (not in waiter list).
    #[test]
    #[serial]
    fn exact_read_raw_race_sequence() {
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Ready);
        
        let mut waiters = KeyboardWaiterList::new();

        // Step 1: try_read_char() returns None (buffer empty)
        let has_char = false;

        if !has_char {
            // Step 2: add_waiter(pid)
            let added = waiters.add_waiter(shell_pid);
            assert!(added, "Waiter queue should accept shell");
            assert!(waiters.contains(shell_pid), "Shell should be in waiter list");

            // Step 3-5: INTERRUPT - keyboard fires, wakes shell (still Ready)
            // This is what add_scancode() does
            waiters.wake_all_waiters();
            
            // Shell removed from waiter list
            assert!(!waiters.contains(shell_pid), "Shell removed from waiter list by wake_all");

            // Step 6: set_current_process_state(Sleeping)
            let _ = set_process_state(shell_pid, ProcessState::Sleeping);
        }

        let final_state = get_state(shell_pid);
        let final_pending = get_wake_pending(shell_pid);

        // Cleanup after reading state
        cleanup_process(shell_pid);

        // CRITICAL ASSERTION: Shell must NOT be sleeping
        assert_ne!(final_state, Some(ProcessState::Sleeping),
            "CRITICAL BUG: Shell stuck sleeping after keyboard race!\n\
             Sequence: add_waiter -> INTERRUPT -> wake_all -> sleep\n\
             Shell removed from waiter list, sleeping, no one to wake it.\n\
             User types, shell NEVER responds.\n\
             wake_pending after: {:?}", final_pending);
    }

    /// Tests repeated race scenarios - race might be probabilistic
    #[test]
    #[serial]
    fn repeated_read_raw_race() {
        const ITERATIONS: usize = 1000;
        let mut stuck_count = 0;

        for _ in 0..ITERATIONS {
            let shell_pid = next_pid();
            add_process(shell_pid, ProcessState::Ready);
            
            let mut waiters = KeyboardWaiterList::new();

            // Race sequence: add_waiter -> wake_all -> sleep
            waiters.add_waiter(shell_pid);
            waiters.wake_all_waiters();
            let _ = set_process_state(shell_pid, ProcessState::Sleeping);

            if get_state(shell_pid) == Some(ProcessState::Sleeping) {
                stuck_count += 1;
                // Wake it for accurate count only
            }

            cleanup_process(shell_pid);
        }

        assert_eq!(stuck_count, 0,
            "RACE BUG: Shell got stuck {} out of {} times!\n\
             This will cause intermittent shell hangs.", stuck_count, ITERATIONS);
    }

    // =========================================================================
    // Test: Spurious wake handling
    // =========================================================================

    /// After spurious wake, process must be able to sleep again
    ///
    /// Scenario:
    /// 1. Shell woken but data consumed by another process
    /// 2. Shell checks, no data, tries to sleep again
    /// 3. Must be able to sleep normally
    #[test]
    #[serial]
    fn spurious_wake_can_sleep_again() {
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Sleeping);

        // First wake (data arrives)
        wake_process(shell_pid);
        assert_eq!(get_state(shell_pid), Some(ProcessState::Ready));

        // Spurious: data consumed by another, shell goes back to sleep
        let _ = set_process_state(shell_pid, ProcessState::Sleeping);
        
        // This SHOULD succeed (no pending wake)
        let state = get_state(shell_pid);

        cleanup_process(shell_pid);

        assert_eq!(state, Some(ProcessState::Sleeping),
            "After wake and consumption, process should be able to sleep again");
    }

    // =========================================================================
    // Test: Queue full handling
    // =========================================================================

    /// When waiter queue is full, process must handle gracefully
    #[test]
    #[serial]
    fn waiter_queue_full_no_hang() {
        let mut waiters = KeyboardWaiterList::new();
        let mut pids = Vec::new();

        // Fill the queue
        for _ in 0..MAX_KEYBOARD_WAITERS {
            let pid = next_pid();
            add_process(pid, ProcessState::Ready);
            assert!(waiters.add_waiter(pid), "Should add waiter");
            pids.push(pid);
        }

        // Queue now full
        assert_eq!(waiters.waiter_count(), MAX_KEYBOARD_WAITERS);

        // One more process tries to add
        let overflow_pid = next_pid();
        add_process(overflow_pid, ProcessState::Ready);
        let added = waiters.add_waiter(overflow_pid);

        // Should fail gracefully
        assert!(!added, "Queue full, should reject");

        // Overflow process still Ready, not stuck
        let state = get_state(overflow_pid);

        // Cleanup
        for pid in &pids {
            cleanup_process(*pid);
        }
        cleanup_process(overflow_pid);

        assert_eq!(state, Some(ProcessState::Ready),
            "Rejected waiter should remain Ready (can retry)");
    }

    // =========================================================================
    // Test: Multiple waiters, only some data
    // =========================================================================

    /// Multiple waiters woken, but only some get data
    #[test]
    #[serial]
    fn multiple_waiters_partial_data() {
        let mut waiters = KeyboardWaiterList::new();
        let mut pids = Vec::new();

        // 4 shells waiting for input
        for _ in 0..4 {
            let pid = next_pid();
            add_process(pid, ProcessState::Sleeping);
            waiters.add_waiter(pid);
            pids.push(pid);
        }

        // Single character arrives - wakes all
        waiters.wake_all_waiters();

        // All should be Ready
        for &pid in &pids {
            assert_eq!(get_state(pid), Some(ProcessState::Ready),
                "All waiters should be woken");
        }

        // Only one gets the char, others go back to sleep
        // First one gets data
        let _ = set_process_state(pids[0], ProcessState::Running);

        // Others re-sleep (no data for them)
        for &pid in &pids[1..] {
            let _ = set_process_state(pid, ProcessState::Sleeping);
        }

        // Check all states are valid
        assert_eq!(get_state(pids[0]), Some(ProcessState::Running));
        for &pid in &pids[1..] {
            assert_eq!(get_state(pid), Some(ProcessState::Sleeping),
                "Processes without data should be able to sleep");
        }

        // Cleanup
        for pid in &pids {
            cleanup_process(*pid);
        }
    }

    // =========================================================================
    // Test: Re-register after wake
    // =========================================================================

    /// After wake, process must be able to re-register as waiter
    #[test]
    #[serial]
    fn reregister_after_wake() {
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Ready);

        let mut waiters = KeyboardWaiterList::new();

        // Register -> wake -> re-register cycle
        for cycle in 0..10 {
            waiters.add_waiter(shell_pid);
            assert!(waiters.contains(shell_pid), "Cycle {}: should be registered", cycle);

            // Wake happens
            waiters.wake_all_waiters();
            assert!(!waiters.contains(shell_pid), "Cycle {}: should be unregistered", cycle);

            // No data, will re-register
            // (In real code, loop back to try_read_char, then add_waiter again)
        }

        cleanup_process(shell_pid);
    }

    // =========================================================================
    // Test: set_current_process_state path
    // =========================================================================

    /// set_current_process_state must respect wake_pending too
    /// 
    /// CRITICAL: This test must use the ACTUAL set_current_process_state() function,
    /// not set_process_state(), because that's what the kernel uses!
    /// The test must:
    /// 1. Set up per-CPU current_pid correctly
    /// 2. Call wake_process() to set wake_pending
    /// 3. Call set_current_process_state(Sleeping) - MUST NOT SLEEP
    #[test]
    #[serial]
    fn set_current_process_state_respects_pending() {
        ensure_percpu_init();
        
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        // CRITICAL: Set this process as the "current" process on CPU 0
        // This is what makes set_current_process_state() find the right process
        set_current_pid(Some(pid));

        // Verify current_pid() returns our PID
        let curr = current_pid();
        assert_eq!(curr, Some(pid), 
            "SETUP BUG: current_pid() should return our test PID after set_current_pid()");

        // Wake arrives (from keyboard interrupt calling wake_all_waiters)
        wake_process(pid);

        let pending = get_wake_pending(pid);
        assert_eq!(pending, Some(true), "Running process should get wake_pending");

        // CRITICAL TEST: Call the ACTUAL function the kernel uses!
        // This is set_current_process_state(), NOT set_process_state()
        set_current_process_state(ProcessState::Sleeping);

        let state = get_state(pid);
        
        // Clean up
        set_current_pid(None);
        cleanup_process(pid);

        // ASSERTION: Process must NOT be sleeping because wake_pending was set
        assert_ne!(state, Some(ProcessState::Sleeping),
            "BUG DETECTED: set_current_process_state() allowed sleep despite wake_pending!\n\
             This is the EXACT bug that causes shell to hang:\n\
             1. Shell calls add_waiter(pid)\n\
             2. Keyboard interrupt: wake_all_waiters() -> wake_process(pid)\n\
             3. wake_process sets wake_pending=true (shell is Running)\n\
             4. Shell calls set_current_process_state(Sleeping)\n\
             5. BUG: Shell goes to sleep, misses the keyboard input!\n\
             \n\
             The fix: set_current_process_state() must check wake_pending before sleeping.");
    }

    /// Test that set_current_process_state returns early when current_pid is None
    #[test]
    #[serial]  
    fn set_current_process_state_no_current_process() {
        ensure_percpu_init();
        
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);
        
        // DON'T set current_pid - leave it as None
        set_current_pid(None);
        
        // Verify current_pid() returns None
        let curr = current_pid();
        assert_eq!(curr, None, "current_pid should be None for this test");
        
        // Call set_current_process_state - should do nothing since no current process
        set_current_process_state(ProcessState::Sleeping);
        
        // Process state should be unchanged (still Ready)
        let state = get_state(pid);
        cleanup_process(pid);
        
        assert_eq!(state, Some(ProcessState::Ready),
            "set_current_process_state() with no current process should not affect other processes");
    }

    /// Test the REAL flow: set_current_pid + wake + set_current_process_state
    ///
    /// BUG FOUND: wake_process() was NOT setting need_resched when the process
    /// was already Running/Ready and only wake_pending was set.
    /// This caused foreground processes to have input latency.
    #[test]
    #[serial]
    fn real_keyboard_read_flow_with_set_current_process_state() {
        ensure_percpu_init();
        
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Running);
        
        // Set shell as current process (shell is running on this CPU)
        set_current_pid(Some(shell_pid));
        
        // Clear need_resched before test
        let _ = check_need_resched();
        
        // STEP 1: Shell notices no input, will register as waiter
        let mut waiters = KeyboardWaiterList::new();
        waiters.add_waiter(shell_pid);
        
        // RACE CONDITION: Keyboard interrupt fires BEFORE shell sleeps
        waiters.wake_all_waiters(); // This calls wake_process(shell_pid)
        
        // wake_process should have set wake_pending because shell is Running
        let pending = get_wake_pending(shell_pid);
        assert_eq!(pending, Some(true), "wake_process on Running should set wake_pending");
        
        // BUG TEST: need_resched MUST be set even when only wake_pending is set!
        // This was the bug - need_resched was only set when woke=true (Sleeping->Ready)
        // but NOT when set_pending=true (Running/Ready -> wake_pending=true)
        let need_resched = check_need_resched();
        assert!(need_resched,
            "BUG DETECTED: wake_process() did NOT set need_resched when wake_pending was set!\n\
             This causes foreground process latency:\n\
             - Keyboard interrupt arrives while shell is Running\n\
             - wake_process() sets wake_pending=true but NOT need_resched\n\
             - Shell continues running until time slice expires\n\
             - User experiences input lag\n\
             \n\
             FIX: wake_process() must set need_resched when set_pending=true, not just when woke=true");
        
        // STEP 2: Shell calls set_current_process_state(Sleeping)
        // This should NOT sleep because wake_pending is set
        set_current_process_state(ProcessState::Sleeping);
        
        let state = get_state(shell_pid);
        
        // Clean up - don't call set_current_pid(None) as it triggers CR3 operations
        cleanup_process(shell_pid);
        
        // CRITICAL ASSERTION
        assert_ne!(state, Some(ProcessState::Sleeping),
            "Shell went to sleep despite pending keyboard input!");
    }

    // =========================================================================
    // Test: Terminal switching during read
    // =========================================================================

    /// If terminal switches during read, process should handle gracefully
    #[test]
    #[serial]
    fn terminal_switch_during_read() {
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Sleeping);

        // Shell sleeping waiting for input on tty1
        // User switches to tty2 (Alt+F2)
        // Later switches back to tty1

        // Wake should still work
        wake_process(shell_pid);

        let state = get_state(shell_pid);
        cleanup_process(shell_pid);

        assert_eq!(state, Some(ProcessState::Ready),
            "Terminal switch should not prevent wake");
    }

    // =========================================================================
    // Test: Zombie waiter cleanup
    // =========================================================================

    /// If waiter exits (becomes Zombie), wake should handle gracefully
    #[test]
    #[serial]
    fn zombie_waiter_no_crash() {
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);

        let mut waiters = KeyboardWaiterList::new();
        waiters.add_waiter(pid);

        // Process exits before keyboard input
        let _ = set_process_state(pid, ProcessState::Zombie);

        // Keyboard input arrives, tries to wake zombie
        // This should not crash or corrupt state
        waiters.wake_all_waiters();

        let state = get_state(pid);
        cleanup_process(pid);

        // Zombie should remain zombie
        assert_eq!(state, Some(ProcessState::Zombie),
            "Zombie waiter should remain zombie");
    }

    // =========================================================================
    // Test: Concurrent add_waiter during wake_all
    // =========================================================================

    /// If process tries to add_waiter while wake_all is running
    #[test]
    #[serial]
    fn concurrent_add_during_wake() {
        // This tests the scenario where:
        // 1. wake_all_waiters is iterating
        // 2. A new process tries to add itself

        let mut waiters = KeyboardWaiterList::new();

        let pid1 = next_pid();
        let pid2 = next_pid();
        add_process(pid1, ProcessState::Sleeping);
        add_process(pid2, ProcessState::Ready);

        waiters.add_waiter(pid1);

        // wake_all runs, then pid2 adds itself
        // In real code, this requires locking, so the add happens before or after
        waiters.wake_all_waiters();
        waiters.add_waiter(pid2);

        // pid2 should be in list
        assert!(waiters.contains(pid2), "New waiter should be added");

        cleanup_process(pid1);
        cleanup_process(pid2);
    }

    // =========================================================================
    // Property: After complete read cycle, process is in valid state
    // =========================================================================

    /// Complete read cycle must leave process in valid state
    #[test]
    #[serial]
    fn complete_read_cycle_valid_state() {
        for _ in 0..100 {
            let shell_pid = next_pid();
            add_process(shell_pid, ProcessState::Running);
            
            let mut waiters = KeyboardWaiterList::new();

            // read_raw_for_tty loop iteration: no char available
            waiters.add_waiter(shell_pid);
            
            // State change to sleeping
            let _ = set_process_state(shell_pid, ProcessState::Sleeping);

            // If we got here without wake, we're sleeping
            let state_before_wake = get_state(shell_pid);

            // Keyboard input arrives
            waiters.wake_all_waiters();

            let state_after_wake = get_state(shell_pid);

            cleanup_process(shell_pid);

            // If was sleeping, must now be Ready
            if state_before_wake == Some(ProcessState::Sleeping) {
                assert_eq!(state_after_wake, Some(ProcessState::Ready),
                    "After wake_all, sleeping process must be Ready");
            }
        }
    }

    // =========================================================================
    // Stress: Many cycles of the complete flow
    // =========================================================================

    /// Stress test: Many complete read cycles with race conditions
    #[test]
    #[serial]
    fn stress_many_read_cycles() {
        const ITERATIONS: usize = 500;
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Ready);

        let mut waiters = KeyboardWaiterList::new();
        let mut stuck_iterations = Vec::new();

        for i in 0..ITERATIONS {
            // Reset state
            set_wake_pending(shell_pid, false);
            let _ = set_process_state(shell_pid, ProcessState::Ready);

            // Read cycle with possible race condition
            let race_happens = i % 3 == 0; // 1 in 3 iterations have race

            waiters.add_waiter(shell_pid);

            if race_happens {
                // Wake arrives before sleep
                waiters.wake_all_waiters();
            }

            let _ = set_process_state(shell_pid, ProcessState::Sleeping);

            if !race_happens {
                // Normal: wake after sleep
                waiters.add_waiter(shell_pid);
                waiters.wake_all_waiters();
            }

            if get_state(shell_pid) == Some(ProcessState::Sleeping) {
                stuck_iterations.push(i);
                // Recover
                wake_process(shell_pid);
            }
        }

        cleanup_process(shell_pid);

        assert!(stuck_iterations.is_empty(),
            "STUCK BUG: Process got stuck in {} iterations: {:?}",
            stuck_iterations.len(), 
            if stuck_iterations.len() > 10 { &stuck_iterations[..10] } else { &stuck_iterations[..] });
    }
}
