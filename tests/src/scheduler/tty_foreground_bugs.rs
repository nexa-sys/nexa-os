//! TTY Foreground Process Bug Detection Tests
//!
//! CRITICAL: These tests are designed to FAIL when bugs exist and PASS when fixed.
//!
//! This module specifically tests bugs that can cause foreground processes
//! (shell, login, interactive programs) to become unresponsive even when
//! there are no bugs in the process itself.
//!
//! ## Tested Bug Categories:
//!
//! 1. **Wake-Sleep Race Conditions**
//!    - Wake arrives before process sleeps (wake_pending mechanism)
//!    - Multiple rapid wakes lost due to flag not being sticky
//!
//! 2. **TTY Session/Foreground Group Issues**
//!    - Process not in foreground group but should be
//!    - Session leader exit orphans foreground processes
//!
//! 3. **Signal-State Interaction Bugs**
//!    - SIGCONT delivered to Ready process (lost)
//!    - SIGSTOP/SIGTSTP state machine errors
//!
//! 4. **Scheduler Priority Inversion**
//!    - Interactive process starved by batch jobs
//!    - EEVDF vruntime not reset on wake
//!
//! ## Expected Behavior:
//!
//! - Tests FAIL when kernel has the bug
//! - Tests PASS when kernel is correctly implemented

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, Context};
    use crate::scheduler::{
        wake_process, set_process_state, process_table_lock,
        SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
        calc_vdeadline, nice_to_weight, get_min_vruntime,
        get_process_state, get_process_vruntime,
    };
    use crate::scheduler::percpu::{init_percpu_sched, check_need_resched, set_need_resched};
    use crate::signal::SignalState;
    use crate::numa;

    use serial_test::serial;
    use std::sync::Once;
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
            context: Context::zero(),
            has_entered_user: true,
            context_valid: true,
            is_fork_child: false,
            is_thread: false,
            cr3: 0x1000,
            tty: 1, // Assigned to tty1
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
            numa_preferred_node: numa::NUMA_NO_NODE,
            numa_policy: numa::NumaPolicy::Local,
        }
    }

    fn add_process_full(pid: Pid, state: ProcessState, vruntime: u64) {
        ensure_percpu_init();
        let mut table = process_table_lock();
        for (idx, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                crate::process::register_pid_mapping(pid, idx as u16);
                let entry = make_process_entry(make_test_process(pid, state), vruntime);
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
    // BUG CATEGORY 1: Wake-Sleep Race Conditions
    // =========================================================================

    /// BUG TEST: wake_pending must be set when wake arrives on Ready process
    ///
    /// Scenario:
    /// 1. Shell is Ready (just added waiter, about to sleep)
    /// 2. Keyboard interrupt fires
    /// 3. wake_process(shell) called - shell is Ready
    /// 4. Shell then calls set_process_state(Sleeping)
    ///
    /// EXPECTED: wake_pending blocks the sleep, shell stays Ready
    /// BUG: If wake_pending not implemented, shell sleeps forever
    #[test]
    #[serial]
    fn bug_wake_on_ready_must_set_wake_pending() {
        let pid = next_pid();
        add_process_full(pid, ProcessState::Ready, 0);

        // Verify initial state
        assert_eq!(get_wake_pending(pid), Some(false), "wake_pending should start false");

        // Wake arrives while Ready
        let woke = wake_process(pid);

        // Check that wake_pending was set
        let pending_after_wake = get_wake_pending(pid);
        
        cleanup_process(pid);

        // wake_process returns false for Ready, but must set wake_pending
        assert!(!woke, "wake_process should return false for Ready process");
        assert_eq!(pending_after_wake, Some(true),
            "BUG: wake_process did NOT set wake_pending for Ready process! \
             This causes wake-before-sleep race condition.");
    }

    /// BUG TEST: set_process_state must check wake_pending before sleeping
    ///
    /// If wake_pending is set, transitioning to Sleeping must be blocked.
    #[test]
    #[serial]
    fn bug_sleep_must_check_wake_pending() {
        let pid = next_pid();
        add_process_full(pid, ProcessState::Ready, 0);

        // Pre-condition: wake_pending already set (e.g., by prior wake on Ready)
        set_wake_pending(pid, true);

        // Try to sleep
        let _ = set_process_state(pid, ProcessState::Sleeping);

        let state = get_process_state(pid);
        let pending = get_wake_pending(pid);

        cleanup_process(pid);

        // Process should NOT be Sleeping
        assert_ne!(state, Some(ProcessState::Sleeping),
            "BUG: set_process_state allowed sleep despite wake_pending! \
             Process will be stuck forever.");
        
        // wake_pending should be consumed (cleared)
        assert_eq!(pending, Some(false),
            "BUG: wake_pending was not consumed after blocking sleep!");
    }

    /// BUG TEST: Double wake before sleep - only need one wake_pending
    ///
    /// Scenario: Two interrupts fire rapidly before process sleeps
    #[test]
    #[serial]
    fn bug_double_wake_before_sleep() {
        let pid = next_pid();
        add_process_full(pid, ProcessState::Ready, 0);

        // First wake on Ready
        wake_process(pid);
        // Second wake on Ready
        wake_process(pid);

        // Try to sleep
        let _ = set_process_state(pid, ProcessState::Sleeping);

        let state = get_process_state(pid);

        cleanup_process(pid);

        assert_ne!(state, Some(ProcessState::Sleeping),
            "BUG: Double wake before sleep should prevent sleeping!");
    }

    /// BUG TEST: Wake-sleep-wake-sleep rapid sequence
    ///
    /// Interactive processes often have rapid wake/sleep cycles
    #[test]
    #[serial]
    fn bug_rapid_wake_sleep_cycles() {
        let pid = next_pid();
        add_process_full(pid, ProcessState::Ready, 0);

        let mut stuck_count = 0;

        for _i in 0..100 {
            // Wake arrives while process is Ready
            wake_process(pid);
            // Process tries to sleep
            let _ = set_process_state(pid, ProcessState::Sleeping);

            if get_process_state(pid) == Some(ProcessState::Sleeping) {
                stuck_count += 1;
                // Recover for next iteration
                wake_process(pid);
            }
        }

        cleanup_process(pid);

        assert_eq!(stuck_count, 0,
            "BUG: Process got stuck {} times in rapid wake/sleep cycles! \
             Wake-before-sleep race is not properly handled.", stuck_count);
    }

    // =========================================================================
    // BUG CATEGORY 2: EEVDF Scheduling Fairness
    // =========================================================================

    /// BUG TEST: Woken interactive process must have competitive vruntime
    ///
    /// After sleeping for keyboard input, shell should not be starved
    /// by background processes that accumulated lower vruntime.
    #[test]
    #[serial]
    fn bug_woken_process_vruntime_reset() {
        // Background process with accumulated vruntime
        let bg_pid = next_pid();
        add_process_full(bg_pid, ProcessState::Ready, 100_000_000); // 100ms

        // Shell sleeping (vruntime unchanged while sleeping)
        let shell_pid = next_pid();
        add_process_full(shell_pid, ProcessState::Sleeping, 200_000_000); // 200ms from before

        // User types, shell wakes
        wake_process(shell_pid);

        let shell_vrt = get_process_vruntime(shell_pid).unwrap_or(u64::MAX);
        let bg_vrt = get_process_vruntime(bg_pid).unwrap_or(0);

        cleanup_process(bg_pid);
        cleanup_process(shell_pid);

        // Shell's vruntime should be adjusted to near min_vruntime
        // NOT remain at old high value
        assert!(shell_vrt <= bg_vrt + BASE_SLICE_NS,
            "BUG: Woken shell vruntime ({}) >> background ({}). \
             EEVDF will starve the shell! \
             FIX: Reset vruntime to min_vruntime on wake.", 
            shell_vrt, bg_vrt);
    }

    /// BUG TEST: Wake must give slight priority boost for interactivity
    ///
    /// EEVDF should give woken processes a small credit to ensure
    /// they get scheduled promptly for interactive response.
    #[test]
    #[serial]
    fn bug_wake_priority_boost() {
        let min_vrt = get_min_vruntime();
        
        let pid = next_pid();
        add_process_full(pid, ProcessState::Sleeping, min_vrt + 1_000_000);

        wake_process(pid);

        let vrt_after = get_process_vruntime(pid).unwrap_or(u64::MAX);

        cleanup_process(pid);

        // Woken process should have vruntime AT or BELOW min_vruntime
        // This ensures it gets scheduled soon
        assert!(vrt_after <= min_vrt,
            "BUG: Woken process vruntime ({}) > min_vruntime ({}). \
             Process may not get scheduled promptly!", 
            vrt_after, min_vrt);
    }

    // =========================================================================
    // BUG CATEGORY 3: need_resched Handling
    // =========================================================================

    /// BUG TEST: wake_process must set need_resched
    ///
    /// Without this, woken process won't run until next timer tick,
    /// causing noticeable input lag.
    #[test]
    #[serial]
    fn bug_wake_must_set_need_resched() {
        let pid = next_pid();
        add_process_full(pid, ProcessState::Sleeping, 0);

        // Clear need_resched
        let _ = check_need_resched();

        // Wake
        wake_process(pid);

        // Check need_resched was set
        let need_resched = check_need_resched();

        cleanup_process(pid);

        assert!(need_resched,
            "BUG: wake_process did not set need_resched! \
             Woken process won't run until next timer tick.");
    }

    // =========================================================================
    // BUG CATEGORY 4: State Transition Consistency
    // =========================================================================

    /// BUG TEST: Sleeping -> Ready transition must clear wake_pending
    ///
    /// If wake_pending is not cleared after actual wake, subsequent
    /// sleeps may incorrectly see stale wake_pending.
    #[test]
    #[serial]
    fn bug_wake_clears_wake_pending() {
        let pid = next_pid();
        add_process_full(pid, ProcessState::Sleeping, 0);

        // Set wake_pending (shouldn't be set for Sleeping, but test defensively)
        set_wake_pending(pid, true);

        // Normal wake from Sleeping
        wake_process(pid);

        let pending = get_wake_pending(pid);

        cleanup_process(pid);

        assert_eq!(pending, Some(false),
            "BUG: wake_pending not cleared after waking from Sleeping! \
             Could cause issues in subsequent wake/sleep cycles.");
    }

    /// BUG TEST: Zombie process cannot be woken
    ///
    /// Attempting to wake a Zombie should be a no-op.
    #[test]
    #[serial]
    fn bug_cannot_wake_zombie() {
        let pid = next_pid();
        add_process_full(pid, ProcessState::Zombie, 0);

        let woke = wake_process(pid);
        let state = get_process_state(pid);

        cleanup_process(pid);

        assert!(!woke, "wake_process should return false for Zombie");
        assert_eq!(state, Some(ProcessState::Zombie),
            "Zombie process state should not change");
    }

    // =========================================================================
    // BUG CATEGORY 5: TTY-specific Issues
    // =========================================================================

    /// BUG TEST: Foreground shell with high vruntime vs background
    ///
    /// Reproduces the exact production bug:
    /// - Shell (PID 8) waiting for input, vruntime = 228M
    /// - DHCP (PID 2) vruntime = 4M
    /// - User types
    /// - Shell wakes but EEVDF picks DHCP
    #[test]
    #[serial]
    fn bug_foreground_shell_starvation_scenario() {
        // Background daemon
        let dhcp_pid = next_pid();
        add_process_full(dhcp_pid, ProcessState::Ready, 4_000_000); // 4ms

        // Shell sleeping (was starved before)
        let shell_pid = next_pid();
        add_process_full(shell_pid, ProcessState::Sleeping, 228_000_000); // 228ms

        // User types - shell wakes
        wake_process(shell_pid);

        let shell_vrt = get_process_vruntime(shell_pid).unwrap_or(u64::MAX);
        let dhcp_vrt = get_process_vruntime(dhcp_pid).unwrap_or(0);
        let shell_state = get_process_state(shell_pid);

        cleanup_process(dhcp_pid);
        cleanup_process(shell_pid);

        // Shell must be Ready
        assert_eq!(shell_state, Some(ProcessState::Ready),
            "Shell should be Ready after wake");

        // Shell's vruntime should be competitive with daemon
        assert!(shell_vrt < dhcp_vrt + BASE_SLICE_NS * 2,
            "BUG: Shell vruntime ({}) >> daemon ({}). \
             Shell will be starved! This is the exact production bug.", 
            shell_vrt, dhcp_vrt);
    }

    /// BUG TEST: Multiple foreground processes (shell + editor)
    ///
    /// When user runs vim from shell, both should remain responsive.
    #[test]
    #[serial]
    fn bug_multiple_foreground_processes() {
        // Background
        let bg_pid = next_pid();
        add_process_full(bg_pid, ProcessState::Ready, 50_000_000);

        // Shell (parent)
        let shell_pid = next_pid();
        add_process_full(shell_pid, ProcessState::Sleeping, 100_000_000);

        // Vim (child of shell)
        let vim_pid = next_pid();
        add_process_full(vim_pid, ProcessState::Sleeping, 100_000_000);

        // User types in vim
        wake_process(vim_pid);
        
        let vim_vrt = get_process_vruntime(vim_pid).unwrap_or(u64::MAX);
        let bg_vrt = get_process_vruntime(bg_pid).unwrap_or(0);

        cleanup_process(bg_pid);
        cleanup_process(shell_pid);
        cleanup_process(vim_pid);

        assert!(vim_vrt <= bg_vrt + BASE_SLICE_NS,
            "BUG: Foreground vim vruntime ({}) >> background ({}). \
             Editor will be unresponsive!", 
            vim_vrt, bg_vrt);
    }

    // =========================================================================
    // BUG CATEGORY 6: Concurrent Access
    // =========================================================================

    /// BUG TEST: Concurrent wake and sleep attempts
    ///
    /// Tests that the lock ordering doesn't cause deadlock or lost wakes.
    /// 
    /// NOTE: Uses threads to create real SMP-like race conditions.
    /// wake_process() on one thread races with set_process_state(Sleeping) on another.
    /// The wake_pending mechanism must prevent lost wakes.
    #[test]
    #[serial]
    fn bug_concurrent_wake_sleep_stress() {
        use std::thread;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

        let pid = next_pid();
        add_process_full(pid, ProcessState::Ready, 0);

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = stop_flag.clone();

        // Waker thread: represents interrupt handler or another CPU calling wake_process
        let waker = thread::spawn(move || {
            let mut wake_count = 0u64;
            while !stop_flag_clone.load(AtomicOrdering::Relaxed) {
                wake_process(pid);
                wake_count += 1;
                // Small yield to allow interleaving
                if wake_count % 100 == 0 {
                    thread::yield_now();
                }
            }
            wake_count
        });

        // Main thread: represents process trying to sleep (races with waker)
        let mut stuck_count = 0;
        let mut recovered_count = 0;
        let mut wake_failed_but_ready = 0;
        let iterations = 1000;
        
        for i in 0..iterations {
            let _ = set_process_state(pid, ProcessState::Sleeping);
            if get_process_state(pid) == Some(ProcessState::Sleeping) {
                stuck_count += 1;
                // Recover - try to wake the sleeping process
                let wake_result = wake_process(pid);
                let state_after_wake = get_process_state(pid);
                if wake_result {
                    recovered_count += 1;
                } else {
                    // wake_process returned false - check why
                    if state_after_wake == Some(ProcessState::Ready) {
                        // Another thread already woke it - that's fine
                        wake_failed_but_ready += 1;
                        recovered_count += 1; // Count as recovered
                    } else {
                        eprintln!(
                            "CRITICAL: wake_process returned false, state={:?} (iter {})",
                            state_after_wake, i
                        );
                    }
                }
            }
            thread::yield_now();
        }

        // Stop waker thread
        stop_flag.store(true, AtomicOrdering::Relaxed);
        let total_wakes = waker.join().unwrap();

        cleanup_process(pid);

        // With proper wake_pending mechanism, most sleeps should be blocked
        // when there's a concurrent waker. Some sleeps are expected when
        // there's no wake in flight.
        //
        // The key invariant: if process got stuck (Sleeping), it MUST be
        // recoverable by wake_process(). If recovery fails, that's a bug.
        let unrecoverable = stuck_count - recovered_count;
        
        eprintln!(
            "Concurrent stress: {} iterations, {} stuck, {} recovered, {} wakes sent",
            iterations, stuck_count, recovered_count, total_wakes
        );

        assert_eq!(unrecoverable, 0,
            "BUG: {} processes stuck and could not be recovered! \
             This indicates a fundamental wake/sleep race condition.", unrecoverable);
        
        // Warn if too many got stuck (suggests wake_pending not working well)
        // but don't fail - some races are expected in testing without real atomics
        if stuck_count > iterations / 2 {
            eprintln!(
                "WARNING: High stuck rate ({}/{}). wake_pending may not be fully effective \
                 in test environment. Check real kernel behavior.", 
                stuck_count, iterations
            );
        }
    }

    // =========================================================================
    // BUG CATEGORY 7: Signal Interactions
    // =========================================================================

    /// BUG TEST: SIGCONT on Ready process must not be lost
    ///
    /// Similar to wake-before-sleep, SIGCONT delivery must handle
    /// the case where process is Ready.
    #[test]
    #[serial]
    fn bug_sigcont_on_ready_process() {
        let pid = next_pid();
        add_process_full(pid, ProcessState::Ready, 0);

        // SIGCONT delivery path calls wake_process internally
        wake_process(pid);

        // Process tries to sleep
        let _ = set_process_state(pid, ProcessState::Sleeping);

        let state = get_process_state(pid);

        cleanup_process(pid);

        assert_ne!(state, Some(ProcessState::Sleeping),
            "BUG: Process slept after SIGCONT-equivalent wake on Ready state!");
    }

    // =========================================================================
    // Integration Test: Full Keyboard Input Scenario
    // =========================================================================

    // Hardware mock: Keyboard waiter queue (mirrors src/drivers/keyboard.rs)
    const MAX_KEYBOARD_WAITERS: usize = 16;

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
            false
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

        fn contains(&self, pid: Pid) -> bool {
            self.waiters.iter().any(|s| *s == Some(pid))
        }
    }

    /// Integration test: Full keyboard read flow with waiter mock
    ///
    /// Tests exact read_raw_for_tty() sequence:
    /// 1. Shell is Ready
    /// 2. Shell calls add_waiter (still Ready)
    /// 3. Keyboard interrupt fires
    /// 4. wake_all_waiters -> wake_process(shell) [shell is Ready]
    /// 5. Shell calls set_current_process_state(Sleeping)
    ///
    /// Expected: Shell stays Ready (wake_pending prevents sleep)
    #[test]
    #[serial]
    fn integration_keyboard_read_flow() {
        let shell_pid = next_pid();
        add_process_full(shell_pid, ProcessState::Ready, 0);

        let mut waiters = KeyboardWaiterList::new();

        // Step 1: try_read_char() returns None (buffer empty)
        let has_char = false;

        if !has_char {
            // Step 2: add_waiter(shell_pid)
            let added = waiters.add_waiter(shell_pid);
            assert!(added, "Waiter queue should accept shell");
            assert!(waiters.contains(shell_pid), "Shell should be in waiter list");

            // Step 3-4: INTERRUPT - keyboard fires, wake_all_waiters called
            // Shell is still Ready (hasn't slept yet)
            waiters.wake_all_waiters();

            // Shell removed from waiter list
            assert!(!waiters.contains(shell_pid), "Shell removed from waiter list");

            // Step 5: Shell tries to sleep
            let _ = set_process_state(shell_pid, ProcessState::Sleeping);
        }

        // Check result
        let final_state = get_process_state(shell_pid);
        let wake_pending_final = get_wake_pending(shell_pid);

        cleanup_process(shell_pid);

        // Shell should NOT be sleeping - wake_pending blocked the transition
        assert_ne!(final_state, Some(ProcessState::Sleeping),
            "INTEGRATION BUG: Shell stuck in Sleeping after keyboard read flow! \
             Shell is no longer in waiter list, so nothing will wake it. \
             This is the exact bug causing shell unresponsiveness.");

        // wake_pending should be consumed
        assert_eq!(wake_pending_final, Some(false),
            "wake_pending should be consumed after blocking sleep");
    }
}
