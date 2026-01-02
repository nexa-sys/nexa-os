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
        calc_vdeadline, get_process_state,
    };
    use crate::scheduler::percpu::{init_percpu_sched, check_need_resched};
    // Use REAL kernel keyboard waiter functions
    use crate::drivers::keyboard::{add_waiter, remove_waiter, wake_all_waiters};
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
    // Test: Exact read_raw_for_tty flow with race condition
    // Using REAL kernel keyboard waiter functions
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

        // Step 1: try_read_char() returns None (buffer empty)
        let has_char = false;

        if !has_char {
            // Step 2: add_waiter(pid) using REAL kernel function
            add_waiter(shell_pid);

            // Step 3-5: INTERRUPT - keyboard fires, wakes shell (still Ready)
            // Using REAL kernel function
            wake_all_waiters();

            // Step 6: set_current_process_state(Sleeping)
            let _ = set_process_state(shell_pid, ProcessState::Sleeping);
        }

        let final_state = get_process_state(shell_pid);
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

            // Race sequence using REAL kernel functions
            add_waiter(shell_pid);
            wake_all_waiters();
            let _ = set_process_state(shell_pid, ProcessState::Sleeping);

            if get_process_state(shell_pid) == Some(ProcessState::Sleeping) {
                stuck_count += 1;
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
        assert_eq!(get_process_state(shell_pid), Some(ProcessState::Ready));

        // Spurious: data consumed by another, shell goes back to sleep
        let _ = set_process_state(shell_pid, ProcessState::Sleeping);
        
        // This SHOULD succeed (no pending wake)
        let state = get_process_state(shell_pid);

        cleanup_process(shell_pid);

        assert_eq!(state, Some(ProcessState::Sleeping),
            "After wake and consumption, process should be able to sleep again");
    }

    // =========================================================================
    // Test: Queue full handling
    // =========================================================================

    /// When waiter queue is full, process must handle gracefully
    /// Note: Kernel keyboard waiter queue has MAX_KEYBOARD_WAITERS = 8
    #[test]
    #[serial]
    fn waiter_queue_full_no_hang() {
        const MAX_KEYBOARD_WAITERS: usize = 8;
        let mut pids = Vec::new();

        // Fill the queue using REAL kernel function
        for _ in 0..MAX_KEYBOARD_WAITERS {
            let pid = next_pid();
            add_process(pid, ProcessState::Ready);
            add_waiter(pid);
            pids.push(pid);
        }

        // One more process tries to add
        let overflow_pid = next_pid();
        add_process(overflow_pid, ProcessState::Ready);
        add_waiter(overflow_pid); // Will silently fail (queue full)

        // Overflow process still Ready, not stuck
        let state = get_process_state(overflow_pid);

        // CRITICAL: Clear the waiter queue before cleanup to not pollute other tests
        wake_all_waiters();

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
        let mut pids = Vec::new();

        // 4 shells waiting for input
        for _ in 0..4 {
            let pid = next_pid();
            add_process(pid, ProcessState::Sleeping);
            add_waiter(pid);
            pids.push(pid);
        }

        // Single character arrives - wakes all using REAL kernel function
        wake_all_waiters();

        // All should be Ready
        for &pid in &pids {
            assert_eq!(get_process_state(pid), Some(ProcessState::Ready),
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
        assert_eq!(get_process_state(pids[0]), Some(ProcessState::Running));
        for &pid in &pids[1..] {
            assert_eq!(get_process_state(pid), Some(ProcessState::Sleeping),
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

        // Register -> wake -> re-register cycle using REAL kernel functions
        for _ in 0..10 {
            add_waiter(shell_pid);
            // Wake happens
            wake_all_waiters();
            // No data, will re-register (in real code, loop back)
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

        let state = get_process_state(pid);
        
        // Clean up
        set_current_pid(None);
        cleanup_process(pid);

        // ASSERTION: Process must NOT be sleeping because wake_pending was set
        assert_ne!(state, Some(ProcessState::Sleeping),
            "BUG DETECTED: set_current_process_state() allowed sleep despite wake_pending!\n\
             This is the EXACT bug that causes shell to hang.");
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
        let state = get_process_state(pid);
        cleanup_process(pid);
        
        assert_eq!(state, Some(ProcessState::Ready),
            "set_current_process_state() with no current process should not affect other processes");
    }

    /// Test the REAL flow: set_current_pid + wake + set_current_process_state
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
        // Using REAL kernel function
        add_waiter(shell_pid);
        
        // RACE CONDITION: Keyboard interrupt fires BEFORE shell sleeps
        // Using REAL kernel function
        wake_all_waiters(); // This calls wake_process(shell_pid)
        
        // wake_process should have set wake_pending because shell is Running
        let pending = get_wake_pending(shell_pid);
        assert_eq!(pending, Some(true), "wake_process on Running should set wake_pending");
        
        // BUG TEST: need_resched MUST be set even when only wake_pending is set!
        let need_resched = check_need_resched();
        assert!(need_resched,
            "BUG DETECTED: wake_process() did NOT set need_resched when wake_pending was set!");
        
        // STEP 2: Shell calls set_current_process_state(Sleeping)
        // This should NOT sleep because wake_pending is set
        set_current_process_state(ProcessState::Sleeping);
        
        let state = get_process_state(shell_pid);
        
        // Clean up
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

        let state = get_process_state(shell_pid);
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

        // Using REAL kernel function
        add_waiter(pid);

        // Process exits before keyboard input
        let _ = set_process_state(pid, ProcessState::Zombie);

        // Keyboard input arrives, tries to wake zombie
        // Using REAL kernel function
        wake_all_waiters();

        let state = get_process_state(pid);
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

        let pid1 = next_pid();
        let pid2 = next_pid();
        add_process(pid1, ProcessState::Sleeping);
        add_process(pid2, ProcessState::Ready);

        // Using REAL kernel functions
        add_waiter(pid1);

        // wake_all runs, then pid2 adds itself
        // In real code, this requires locking, so the add happens before or after
        wake_all_waiters();
        add_waiter(pid2);

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

            // read_raw_for_tty loop iteration: no char available
            // Using REAL kernel function
            add_waiter(shell_pid);
            
            // State change to sleeping
            let _ = set_process_state(shell_pid, ProcessState::Sleeping);

            // If we got here without wake, we're sleeping
            let state_before_wake = get_process_state(shell_pid);

            // Keyboard input arrives using REAL kernel function
            wake_all_waiters();

            let state_after_wake = get_process_state(shell_pid);

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

        let mut stuck_iterations = Vec::new();

        for i in 0..ITERATIONS {
            // Reset state
            set_wake_pending(shell_pid, false);
            let _ = set_process_state(shell_pid, ProcessState::Ready);

            // Read cycle with possible race condition
            let race_happens = i % 3 == 0; // 1 in 3 iterations have race

            // Using REAL kernel function
            add_waiter(shell_pid);

            if race_happens {
                // Wake arrives before sleep
                wake_all_waiters();
            }

            let _ = set_process_state(shell_pid, ProcessState::Sleeping);

            if !race_happens {
                // Normal: wake after sleep
                add_waiter(shell_pid);
                wake_all_waiters();
            }

            if get_process_state(shell_pid) == Some(ProcessState::Sleeping) {
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

    // =========================================================================
    // BUG: Double wake_all_waiters call - waiters already removed
    // =========================================================================

    /// BUG TEST: Repeated wake_all_waiters when waiters already cleared
    ///
    /// If interrupt fires twice quickly:
    /// 1. First wake_all_waiters clears waiter list
    /// 2. Second wake_all_waiters has empty list - no wakes
    /// 3. Process between step 1 and 2 may register and miss wake
    #[test]
    #[serial]
    fn double_wake_all_waiters_race() {
        let pid1 = next_pid();
        let pid2 = next_pid();
        add_process(pid1, ProcessState::Sleeping);
        add_process(pid2, ProcessState::Ready);

        // pid1 registered as waiter
        add_waiter(pid1);

        // First interrupt: wakes pid1, clears list
        wake_all_waiters();

        // pid2 registers BETWEEN the two interrupts
        add_waiter(pid2);
        let _ = set_process_state(pid2, ProcessState::Sleeping);

        // Second interrupt: should wake pid2
        wake_all_waiters();

        let state1 = get_process_state(pid1);
        let state2 = get_process_state(pid2);

        cleanup_process(pid1);
        cleanup_process(pid2);

        assert_eq!(state1, Some(ProcessState::Ready), "pid1 should be Ready");
        assert_eq!(state2, Some(ProcessState::Ready),
            "BUG: pid2 registered after first wake but before sleep, \
             second wake_all should have woken it!");
    }

    // =========================================================================
    // BUG: wake_pending not cleared after successful sleep
    // =========================================================================

    /// BUG TEST: Stale wake_pending from previous cycle
    ///
    /// If wake_pending is not properly cleared:
    /// 1. Cycle 1: wake_pending set, blocks sleep -> OK
    /// 2. Cycle 2: tries to sleep, but old wake_pending blocks it -> BUG
    #[test]
    #[serial]
    fn stale_wake_pending_between_cycles() {
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Ready);

        // Cycle 1: wake-before-sleep race
        add_waiter(shell_pid);
        wake_all_waiters();  // Sets wake_pending
        let _ = set_process_state(shell_pid, ProcessState::Sleeping); // Should NOT sleep

        let state1 = get_process_state(shell_pid);
        let pending1 = get_wake_pending(shell_pid);

        // Cycle 2: Normal operation (no race this time)
        // Process consumed the data, ready to sleep for more
        add_waiter(shell_pid);
        // NO wake this time
        let _ = set_process_state(shell_pid, ProcessState::Sleeping);

        let state2 = get_process_state(shell_pid);
        let pending2 = get_wake_pending(shell_pid);

        cleanup_process(shell_pid);

        assert_ne!(state1, Some(ProcessState::Sleeping), "Cycle 1 should block sleep");
        assert_eq!(pending1, Some(false), "wake_pending should be consumed in cycle 1");
        assert_eq!(state2, Some(ProcessState::Sleeping), 
            "BUG: Cycle 2 should sleep (no pending wake), but stale wake_pending blocked it!");
        assert_eq!(pending2, Some(false), "No wake in cycle 2");
    }

    // =========================================================================
    // BUG: wake_process on Running doesn't set need_resched
    // =========================================================================

    /// BUG TEST: wake on Running must set need_resched
    ///
    /// If process is Running and wake arrives:
    /// 1. wake_pending is set (correct)
    /// 2. But need_resched might not be set!
    /// 3. Process continues running until time slice ends
    /// 4. Only then tries to sleep, wake_pending blocks it
    /// 
    /// Problem: If process never sleeps voluntarily, it doesn't benefit from wake_pending
    /// Solution: need_resched should be set so scheduler can check wake_pending at next tick
    #[test]
    #[serial]
    fn wake_running_sets_need_resched() {
        ensure_percpu_init();
        
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        // Clear need_resched before test
        let _ = check_need_resched();

        // Wake arrives while Running
        wake_process(pid);

        let pending = get_wake_pending(pid);
        let resched = check_need_resched();

        cleanup_process(pid);

        assert_eq!(pending, Some(true), "wake_pending should be set");
        assert!(resched,
            "BUG: wake_process on Running did not set need_resched! \
             Interactive latency will suffer.");
    }

    // =========================================================================
    // BUG: remove_waiter not called before sleep retry
    // =========================================================================

    /// BUG TEST: Duplicate waiter entries if not removed before retry
    ///
    /// If process is woken, checks no data, re-adds waiter without removing:
    /// 1. add_waiter(pid)  -> [pid]
    /// 2. wake_all_waiters -> wakes pid, list now []
    /// 3. No data, process loops
    /// 4. add_waiter(pid)  -> [pid]  // OK if wake cleared it
    /// 
    /// But if implementation doesn't clear on wake:
    /// 1. add_waiter(pid)  -> [pid]
    /// 2. Data arrives, but no wake_all called yet
    /// 3. Process checks, has data, returns
    /// 4. Process returns for more
    /// 5. add_waiter(pid)  -> [pid, pid]  // DUPLICATE!
    #[test]
    #[serial]
    fn no_duplicate_waiter_entries() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Scenario: add multiple times without wake in between
        add_waiter(pid);
        remove_waiter(pid);  // Should be no-op or actually remove
        add_waiter(pid);     // Second add
        
        // Now wake - should wake exactly once
        wake_all_waiters();

        // Process should be Ready (was already Ready, wake_pending set)
        let pending = get_wake_pending(pid);

        // Re-add after wake
        add_waiter(pid);
        add_waiter(pid);  // Duplicate add - kernel should handle

        wake_all_waiters();

        let state = get_process_state(pid);

        cleanup_process(pid);

        assert_eq!(pending, Some(true), "First wake should set wake_pending");
        // After all the manipulation, process should still be in valid state
        assert_eq!(state, Some(ProcessState::Ready), "Process should remain Ready");
    }

    // =========================================================================
    // BUG: Sleeping process not in waiter list
    // =========================================================================

    /// BUG TEST: Process sleeps but never added to waiter list
    ///
    /// If code does:
    ///   set_process_state(Sleeping)  // WRONG ORDER
    ///   add_waiter(pid)
    ///
    /// Race window where keyboard fires after sleep but before add.
    #[test]
    #[serial]
    fn wrong_order_sleep_before_add() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // WRONG order (demonstrates bug if kernel code is wrong)
        let _ = set_process_state(pid, ProcessState::Sleeping);
        // If keyboard interrupt fires HERE, wake_all_waiters does nothing
        // because pid is not in waiter list yet
        add_waiter(pid);

        // Keyboard fires, but pid is Sleeping and WAS added after sleep
        wake_all_waiters();

        let state = get_process_state(pid);
        cleanup_process(pid);

        // This should work if add_waiter works on Sleeping process
        assert_eq!(state, Some(ProcessState::Ready),
            "wake_all should wake even if waiter added while Sleeping");
    }

    // =========================================================================
    // BUG: Thread blocked in read while main thread exits
    // =========================================================================

    /// BUG TEST: Thread waiting for input when main thread exits
    ///
    /// Multi-threaded process:
    /// - Thread 1 (main): exits
    /// - Thread 2: blocked in read(), waiting for keyboard
    /// 
    /// Thread 2 should receive some signal (SIGHUP?) or be killed
    #[test]
    #[serial]
    fn thread_blocked_read_main_exits() {
        let main_pid = next_pid();
        let thread_pid = next_pid();

        add_process(main_pid, ProcessState::Running);
        // Thread shares tgid with main
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if slot.is_none() {
                    let mut proc = make_test_process(thread_pid, ProcessState::Sleeping);
                    proc.tgid = main_pid;  // Same thread group
                    proc.is_thread = true;
                    crate::process::register_pid_mapping(thread_pid, 0); // Dummy index
                    *slot = Some(make_process_entry(proc));
                    break;
                }
            }
        }

        // Thread is waiting for keyboard
        add_waiter(thread_pid);

        // Main thread exits
        let _ = set_process_state(main_pid, ProcessState::Zombie);

        // At this point, something should happen to thread
        // In POSIX: orphaned threads continue, but should be wakeable
        
        // Keyboard input arrives
        wake_all_waiters();

        let thread_state = get_process_state(thread_pid);
        
        cleanup_process(thread_pid);
        cleanup_process(main_pid);

        // Thread should be woken (Ready) not stuck Sleeping forever
        assert_eq!(thread_state, Some(ProcessState::Ready),
            "BUG: Thread blocked in read stuck after main exits!");
    }

    // =========================================================================
    // BUG: Rapid keystroke loss under load
    // =========================================================================

    /// BUG TEST: Multiple rapid keystrokes with process switching
    ///
    /// User types fast while system is busy:
    /// 1. Shell sleeping
    /// 2. Key 1: wakes shell
    /// 3. Shell processes, sleeps again
    /// 4. Key 2: wakes shell (but shell might not have slept yet!)
    /// 5. Key 3: wakes shell (shell still processing)
    #[test]
    #[serial]
    fn rapid_keystrokes_no_loss() {
        let shell = next_pid();
        add_process(shell, ProcessState::Sleeping);
        add_waiter(shell);

        // Key 1
        wake_all_waiters();
        assert_eq!(get_process_state(shell), Some(ProcessState::Ready), "Key 1 should wake");

        // Shell processes (Running)
        let _ = set_process_state(shell, ProcessState::Running);

        // Key 2 arrives while Running
        add_waiter(shell);  // Shell re-registers (may or may not succeed)
        wake_all_waiters(); // Should set wake_pending

        // Shell finishes processing key 1, tries to sleep for more
        let _ = set_process_state(shell, ProcessState::Sleeping);

        // Should NOT sleep because key 2 is pending
        let state_after_key2 = get_process_state(shell);
        let pending = get_wake_pending(shell);

        // Key 3 arrives
        add_waiter(shell);
        wake_all_waiters();

        cleanup_process(shell);

        // Key 2 should have prevented sleep OR shell was woken for key 3
        assert!(state_after_key2 != Some(ProcessState::Sleeping) || pending == Some(true),
            "BUG: Keystroke lost! Shell slept despite pending input.");
    }

    // =========================================================================
    // BUG: EINTR handling leaves process unwakeable
    // =========================================================================

    /// BUG TEST: Signal interrupts read, process restarts but stuck
    ///
    /// 1. Shell in read(), registered as waiter, Sleeping
    /// 2. Signal arrives (e.g. SIGCHLD) - wakes shell with EINTR
    /// 3. Shell handles signal, restarts read()
    /// 4. Shell adds waiter, but wake_pending might be stale?
    #[test]
    #[serial]
    fn eintr_restart_not_stuck() {
        let shell = next_pid();
        add_process(shell, ProcessState::Sleeping);
        add_waiter(shell);

        // Signal arrives (we just wake the process to simulate)
        wake_process(shell);
        assert_eq!(get_process_state(shell), Some(ProcessState::Ready));

        // Shell handles signal, returns EINTR, libc restarts read()
        // Shell re-registers as waiter
        add_waiter(shell);
        let _ = set_process_state(shell, ProcessState::Sleeping);

        let state = get_process_state(shell);
        
        // Keyboard input arrives
        wake_all_waiters();

        let final_state = get_process_state(shell);

        cleanup_process(shell);

        assert_eq!(state, Some(ProcessState::Sleeping), 
            "Shell should sleep after EINTR handling (no pending input)");
        assert_eq!(final_state, Some(ProcessState::Ready),
            "BUG: Shell stuck after EINTR restart! Keyboard input ignored.");
    }

    // =========================================================================
    // BUG: Ctrl+C during read leaves shell stuck
    // =========================================================================

    /// BUG TEST: Ctrl+C sends SIGINT but shell might not wake properly
    ///
    /// Ctrl+C path:
    /// 1. Keyboard scancode 0x1D + 0x2E
    /// 2. Driver sends SIGINT to foreground process group
    /// 3. Shell receives SIGINT, should wake from read
    #[test]
    #[serial]
    fn ctrl_c_wakes_shell() {
        let shell = next_pid();
        add_process(shell, ProcessState::Sleeping);
        add_waiter(shell);

        // Ctrl+C: keyboard driver calls wake on shell (for SIGINT delivery)
        wake_process(shell);

        let state = get_process_state(shell);
        cleanup_process(shell);

        assert_eq!(state, Some(ProcessState::Ready),
            "BUG: Ctrl+C (SIGINT) did not wake shell from read!");
    }

    // =========================================================================
    // BUG: Background process promoted to foreground still blocked
    // =========================================================================

    /// BUG TEST: "fg" command brings job to foreground, must be responsive
    ///
    /// 1. Job running in background (Sleeping on I/O)
    /// 2. User types "fg" to bring to foreground
    /// 3. Shell sends SIGCONT to job
    /// 4. Job should become responsive to keyboard
    #[test]
    #[serial]
    fn fg_command_makes_responsive() {
        let bg_job = next_pid();
        add_process(bg_job, ProcessState::Sleeping);

        // Job is sleeping (stopped by SIGTSTP or waiting on pipe)
        
        // User types "fg", shell sends SIGCONT
        wake_process(bg_job);

        let state = get_process_state(bg_job);

        // Job now in foreground, tries to read keyboard
        add_waiter(bg_job);
        let _ = set_process_state(bg_job, ProcessState::Sleeping);

        // User types
        wake_all_waiters();

        let final_state = get_process_state(bg_job);
        cleanup_process(bg_job);

        assert_eq!(state, Some(ProcessState::Ready), "SIGCONT should wake job");
        assert_eq!(final_state, Some(ProcessState::Ready), 
            "BUG: Job promoted to foreground still stuck!");
    }
}
