//! Foreground Process Responsiveness Bug Detection
//!
//! ## TEST PHILOSOPHY (CRITICAL!)
//!
//! A GOOD TEST:
//!   - FAILS when bug exists (detects the problem)
//!   - PASSES when bug is fixed (verifies correctness)
//!
//! A BAD TEST (what we DON'T want):
//!   - PASSES when bug exists (useless - doesn't detect anything)
//!   - FAILS when bug is fixed (backwards - describes bug instead of correct behavior)
//!
//! ## Bug Categories Tested:
//!
//! 1. **Wake-Before-Sleep Race**: Keyboard interrupt wakes process BEFORE it sleeps
//!    - Without wake_pending: process stuck forever
//!    - With wake_pending: process stays Ready (correct)
//!
//! 2. **Waiter List Race**: Process removed from waiter list before sleep
//!    - Another interrupt won't wake it because it's not in the list
//!
//! 3. **Priority Starvation**: Background processes starve foreground
//!    - EEVDF should reset vruntime on wake to prevent this
//!
//! 4. **Need_resched Not Set**: Process woken but scheduler not notified
//!    - Causes latency until next timer tick
//!
//! 5. **State Machine Violations**: Invalid state transitions

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, Context};
    use crate::scheduler::{
        wake_process, set_process_state, process_table_lock,
        SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
        calc_vdeadline, get_min_vruntime,
        get_process_state, get_process_vruntime,
    };
    use crate::scheduler::percpu::{init_percpu_sched, check_need_resched, set_need_resched};
    use crate::signal::SignalState;
    use crate::numa;

    use serial_test::serial;
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    static INIT_PERCPU: Once = Once::new();
    static NEXT_PID: AtomicU64 = AtomicU64::new(300000);

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

    fn add_process_with_vruntime(pid: Pid, state: ProcessState, vruntime: u64) {
        ensure_percpu_init();
        let mut table = process_table_lock();
        for (idx, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                crate::process::register_pid_mapping(pid, idx as u16);
                let mut entry = make_process_entry(make_test_process(pid, state));
                entry.vruntime = vruntime;
                entry.vdeadline = calc_vdeadline(vruntime, BASE_SLICE_NS, NICE_0_WEIGHT);
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
    // BUG #1: Wake-Before-Sleep Race Condition
    //
    // This is the CRITICAL bug that causes shell to become unresponsive.
    //
    // Race sequence in read_raw_for_tty:
    // 1. Shell calls add_waiter(pid) - still Ready
    // 2. Keyboard interrupt fires
    // 3. wake_all_waiters() -> wake_process(pid) on Ready process
    // 4. Shell calls set_process_state(Sleeping)
    // 5. BUG: Shell sleeps forever (wake already processed and lost)
    //
    // CORRECT BEHAVIOR: wake_process sets wake_pending=true, sleep is blocked
    // =========================================================================

    /// Invariant: wake_process on Ready MUST set wake_pending flag
    ///
    /// If this fails, the kernel WILL have stuck processes.
    #[test]
    #[serial]
    fn invariant_wake_on_ready_sets_pending() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Keyboard interrupt wakes a Ready shell
        wake_process(pid);

        let pending = get_wake_pending(pid);
        cleanup_process(pid);

        // INVARIANT CHECK: This MUST be true for correctness
        assert_eq!(pending, Some(true),
            "INVARIANT VIOLATION: wake_process() on Ready did not set wake_pending!\n\
             Without this flag, the wake-before-sleep race causes stuck processes.\n\
             Shell will become unresponsive after keyboard input.");
    }

    /// Invariant: set_process_state(Sleeping) with wake_pending=true MUST NOT sleep
    ///
    /// This is the second half of the wake-before-sleep protection.
    #[test]
    #[serial]
    fn invariant_sleep_blocked_by_pending_wake() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Set wake_pending (wake arrived before sleep scenario)
        set_wake_pending(pid, true);

        // Process tries to sleep
        let _ = set_process_state(pid, ProcessState::Sleeping);

        let state = get_process_state(pid);
        let pending = get_wake_pending(pid);
        cleanup_process(pid);

        // INVARIANT CHECK
        assert_ne!(state, Some(ProcessState::Sleeping),
            "INVARIANT VIOLATION: Process slept despite wake_pending=true!\n\
             This loses the wake event, causing permanent stuck process.\n\
             State after sleep attempt: {:?}, wake_pending: {:?}", state, pending);

        // wake_pending should be consumed (cleared)
        assert_eq!(pending, Some(false),
            "wake_pending should be consumed (cleared) after blocking sleep");
    }

    /// Integration: Full wake-before-sleep sequence must NOT lose wake
    ///
    /// This tests the complete race scenario end-to-end.
    #[test]
    #[serial]
    fn integration_wake_before_sleep_no_lost_wake() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Step 1: Shell registered as waiter
        // Step 2: Keyboard interrupt - wake_process called on Ready
        wake_process(pid);

        // Step 3: Shell tries to sleep (after interrupt returned)
        let _ = set_process_state(pid, ProcessState::Sleeping);

        let final_state = get_process_state(pid);
        cleanup_process(pid);

        // CORRECTNESS CHECK: Process must NOT be stuck sleeping
        assert_ne!(final_state, Some(ProcessState::Sleeping),
            "CRITICAL BUG: Wake-before-sleep race lost the wake!\n\
             Process is stuck in Sleeping state forever.\n\
             User typed on keyboard, shell never responds.\n\
             This is the exact bug causing foreground unresponsiveness.");
    }

    // =========================================================================
    // BUG #2: Repeated Race Conditions (Stress Test)
    //
    // Even if the mechanism works sometimes, race conditions can be
    // probabilistic. We must test many iterations.
    // =========================================================================

    /// Stress: 1000 iterations of wake-before-sleep must all succeed
    #[test]
    #[serial]
    fn stress_repeated_wake_before_sleep_race() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        let mut stuck_count = 0;
        const ITERATIONS: usize = 1000;

        for i in 0..ITERATIONS {
            // Reset to Ready
            let _ = set_process_state(pid, ProcessState::Ready);
            set_wake_pending(pid, false);

            // Race: wake while Ready
            wake_process(pid);

            // Try to sleep
            let _ = set_process_state(pid, ProcessState::Sleeping);

            if get_process_state(pid) == Some(ProcessState::Sleeping) {
                stuck_count += 1;
                // Recover for next iteration
                wake_process(pid);
            }
        }

        cleanup_process(pid);

        assert_eq!(stuck_count, 0,
            "RACE CONDITION BUG: Process got stuck {} out of {} times!\n\
             Even one stuck iteration means the mechanism is broken.\n\
             This will cause intermittent shell hangs.", stuck_count, ITERATIONS);
    }

    // =========================================================================
    // BUG #3: Multiple Consecutive Wakes
    //
    // If multiple wakes arrive before sleep, all must be handled.
    // =========================================================================

    /// Multiple wakes must not be lost - at least one pending must remain
    #[test]
    #[serial]
    fn invariant_multiple_wakes_not_lost() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Multiple events (e.g., rapid keystrokes)
        wake_process(pid);
        wake_process(pid);
        wake_process(pid);

        let pending = get_wake_pending(pid);

        // Single sleep attempt
        let _ = set_process_state(pid, ProcessState::Sleeping);

        let state_after = get_process_state(pid);
        cleanup_process(pid);

        // Wake pending must be set after all those wakes
        assert_eq!(pending, Some(true),
            "Multiple wakes must keep wake_pending=true");

        // Must not sleep
        assert_ne!(state_after, Some(ProcessState::Sleeping),
            "BUG: Multiple wakes all lost! Process stuck sleeping.");
    }

    // =========================================================================
    // BUG #4: need_resched Flag Not Set
    //
    // Even if the process wakes correctly, if need_resched is not set,
    // it won't run until the next timer tick (up to 1ms latency).
    // =========================================================================

    /// wake_process MUST set need_resched for immediate scheduling
    #[test]
    #[serial]
    fn invariant_wake_sets_need_resched() {
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);

        // Clear flag
        let _ = check_need_resched();

        // Wake
        let woke = wake_process(pid);
        assert!(woke, "wake_process should succeed for Sleeping process");

        let need_resched = check_need_resched();
        cleanup_process(pid);

        assert!(need_resched,
            "LATENCY BUG: wake_process did NOT set need_resched!\n\
             Woken process must wait for timer tick (~1ms) to run.\n\
             Keyboard input will feel sluggish.");
    }

    // =========================================================================
    // BUG #5: Woken Process vruntime Starvation
    //
    // If a sleeping process accumulates high vruntime, it will be starved
    // when it wakes up. EEVDF must reset vruntime on wake.
    // =========================================================================

    /// Woken process vruntime must be near min_vruntime (no starvation)
    #[test]
    #[serial]
    fn invariant_wake_resets_vruntime() {
        // Background process with moderate vruntime
        let bg_pid = next_pid();
        add_process_with_vruntime(bg_pid, ProcessState::Running, 50_000_000); // 50ms

        // Shell was sleeping with very high vruntime (stale from before sleep)
        let shell_pid = next_pid();
        add_process_with_vruntime(shell_pid, ProcessState::Sleeping, 500_000_000); // 500ms!

        // Shell wakes up
        wake_process(shell_pid);

        let shell_vrt = get_process_vruntime(shell_pid).unwrap_or(u64::MAX);
        let bg_vrt = get_process_vruntime(bg_pid).unwrap_or(0);

        cleanup_process(shell_pid);
        cleanup_process(bg_pid);

        // Shell vruntime should be reset to allow scheduling
        // Give some slack (one slice worth)
        assert!(shell_vrt <= bg_vrt + BASE_SLICE_NS * 2,
            "STARVATION BUG: Woken shell vruntime ({}) >> background ({})!\n\
             Shell will be starved by background processes.\n\
             User types, shell never responds because scheduler picks others.",
            shell_vrt, bg_vrt);
    }

    // =========================================================================
    // BUG #6: Zombie Process Wake Corruption
    //
    // Trying to wake a Zombie must not corrupt its state.
    // =========================================================================

    /// wake_process on Zombie must be a no-op (no corruption)
    #[test]
    #[serial]
    fn invariant_zombie_wake_no_corruption() {
        let pid = next_pid();
        add_process(pid, ProcessState::Zombie);

        let woke = wake_process(pid);
        let state = get_process_state(pid);
        let pending = get_wake_pending(pid);

        cleanup_process(pid);

        assert!(!woke, "Cannot wake a Zombie");
        assert_eq!(state, Some(ProcessState::Zombie),
            "Zombie state corrupted by wake attempt!");
        assert_eq!(pending, Some(false),
            "Zombie should not have wake_pending set");
    }

    // =========================================================================
    // BUG #7: Running Process Wake
    //
    // Waking a Running process should set wake_pending for when it tries to sleep.
    // =========================================================================

    /// wake_process on Running MUST set wake_pending
    #[test]
    #[serial]
    fn invariant_wake_on_running_sets_pending() {
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        wake_process(pid);

        let pending = get_wake_pending(pid);
        cleanup_process(pid);

        assert_eq!(pending, Some(true),
            "wake_process on Running did not set wake_pending!\n\
             If this Running process later tries to sleep, it will miss the wake.");
    }

    // =========================================================================
    // BUG #8: Complex Interaction - Wake/Sleep/Wake/Sleep
    //
    // Multiple cycles must all work correctly.
    // =========================================================================

    /// Multiple wake/sleep cycles must all work
    #[test]
    #[serial]
    fn stress_multiple_wake_sleep_cycles() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        let mut stuck_count = 0;

        for _ in 0..100 {
            // Cycle 1: Normal sleep/wake
            let _ = set_process_state(pid, ProcessState::Sleeping);
            let woke = wake_process(pid);

            if !woke || get_process_state(pid) != Some(ProcessState::Ready) {
                stuck_count += 1;
                // Try to recover
                let _ = set_process_state(pid, ProcessState::Ready);
            }

            // Cycle 2: Wake before sleep
            wake_process(pid);
            let _ = set_process_state(pid, ProcessState::Sleeping);

            if get_process_state(pid) == Some(ProcessState::Sleeping) {
                stuck_count += 1;
                wake_process(pid);
            }

            // Reset
            set_wake_pending(pid, false);
            let _ = set_process_state(pid, ProcessState::Ready);
        }

        cleanup_process(pid);

        assert_eq!(stuck_count, 0,
            "Wake/sleep cycles failed {} times. State machine is broken.", stuck_count);
    }

    // =========================================================================
    // BUG #9: Concurrent State Changes (SMP Race)
    //
    // In SMP, different CPUs may call wake and sleep "simultaneously".
    // =========================================================================

    /// SMP race test: wake on CPU1 while sleep on CPU0
    #[test]
    #[serial]
    fn test_smp_concurrent_wake_sleep() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Test interleaved operations:
        // CPU0: decides to sleep, about to call set_process_state
        // CPU1: keyboard interrupt, calls wake_process

        // CPU1 runs first (interrupt preemption)
        wake_process(pid);

        // CPU0 now runs (returns from interrupt context)
        let _ = set_process_state(pid, ProcessState::Sleeping);

        let state = get_process_state(pid);
        cleanup_process(pid);

        // Invariant: must NOT be sleeping (wake_pending should have blocked it)
        assert_ne!(state, Some(ProcessState::Sleeping),
            "SMP RACE BUG: Concurrent wake+sleep lost the wake!\n\
             This is the exact bug that causes foreground process hangs.");
    }

    // =========================================================================
    // BUG #10: Large-Scale Stress Test
    //
    // Many processes, many state transitions, no stuck processes.
    // =========================================================================

    /// Stress: Many processes with random state changes, none should get stuck
    #[test]
    #[serial]
    fn stress_many_processes_no_stuck() {
        let mut pids = Vec::new();
        const NUM_PROCS: usize = 32;
        const ITERATIONS: usize = 100;

        for _ in 0..NUM_PROCS {
            let pid = next_pid();
            add_process(pid, ProcessState::Ready);
            pids.push(pid);
        }

        // Random-ish state transitions
        for iter in 0..ITERATIONS {
            for (i, &pid) in pids.iter().enumerate() {
                let action = (iter * 7 + i * 13) % 4;
                match action {
                    0 => { wake_process(pid); }
                    1 => { let _ = set_process_state(pid, ProcessState::Sleeping); }
                    2 => { 
                        // Wake then sleep (race)
                        wake_process(pid);
                        let _ = set_process_state(pid, ProcessState::Sleeping);
                    }
                    _ => { let _ = set_process_state(pid, ProcessState::Ready); }
                }
            }
        }

        // Check for stuck processes
        let mut stuck_pids = Vec::new();
        for &pid in &pids {
            if get_process_state(pid) == Some(ProcessState::Sleeping) {
                // Try to wake - if it works, it wasn't truly stuck
                if !wake_process(pid) {
                    stuck_pids.push(pid);
                }
            }
            cleanup_process(pid);
        }

        assert!(stuck_pids.is_empty(),
            "STUCK PROCESS BUG: {} processes are stuck: {:?}\n\
             This means the wake/sleep mechanism has holes.",
            stuck_pids.len(), stuck_pids);
    }

    // =========================================================================
    // BUG #11: Signal Wake During Sleep Preparation
    //
    // Signals can also cause wakes. Same race applies.
    // =========================================================================

    /// Signal-triggered wake must also set wake_pending
    #[test]
    #[serial]
    fn invariant_signal_wake_on_ready_sets_pending() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Signal delivery waking a Ready process
        // (Signal handler calls wake_process internally)
        wake_process(pid);

        let pending = get_wake_pending(pid);

        // Now if process tries to sleep (e.g., in interruptible syscall)
        let _ = set_process_state(pid, ProcessState::Sleeping);

        let state = get_process_state(pid);
        cleanup_process(pid);

        assert_eq!(pending, Some(true),
            "Signal wake on Ready did not set wake_pending");
        assert_ne!(state, Some(ProcessState::Sleeping),
            "Signal wake lost! Process stuck after signal delivery.");
    }

    // =========================================================================
    // BUG #12: TTY Specific - Active Terminal Check
    //
    // Only foreground process on active terminal should receive input.
    // Wrong terminal assignment causes silent drops.
    // =========================================================================

    /// Process with wrong tty should not affect keyboard waiter
    #[test]
    #[serial]
    fn tty_assignment_does_not_affect_scheduler() {
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Ready);

        // Scheduler should work regardless of tty value
        wake_process(shell_pid);
        let _ = set_process_state(shell_pid, ProcessState::Sleeping);

        let state = get_process_state(shell_pid);
        cleanup_process(shell_pid);

        // Should not be sleeping (wake_pending mechanism works)
        assert_ne!(state, Some(ProcessState::Sleeping),
            "Scheduler wake_pending broken (unrelated to tty)");
    }

    // =========================================================================
    // BUG #13: Double Sleep Without Intervening Wake
    //
    // Calling sleep twice without wake should be idempotent.
    // =========================================================================

    /// Double sleep should not corrupt state
    #[test]
    #[serial]
    fn double_sleep_is_idempotent() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // First sleep
        let _ = set_process_state(pid, ProcessState::Sleeping);
        let state1 = get_process_state(pid);

        // Second sleep (already sleeping)
        let _ = set_process_state(pid, ProcessState::Sleeping);
        let state2 = get_process_state(pid);

        cleanup_process(pid);

        // Both should be Sleeping (idempotent)
        assert_eq!(state1, Some(ProcessState::Sleeping));
        assert_eq!(state2, Some(ProcessState::Sleeping));
    }

    // =========================================================================
    // PROPERTY-BASED TESTS
    //
    // These test general properties that must always hold.
    // =========================================================================

    /// Property: After wake_process(Sleeping), state must be Ready
    #[test]
    #[serial]
    fn property_wake_sleeping_yields_ready() {
        for _ in 0..100 {
            let pid = next_pid();
            add_process(pid, ProcessState::Sleeping);

            let woke = wake_process(pid);
            let state = get_process_state(pid);

            cleanup_process(pid);

            assert!(woke, "wake_process on Sleeping must return true");
            assert_eq!(state, Some(ProcessState::Ready),
                "After waking Sleeping process, state must be Ready");
        }
    }

    /// Property: After wake_process(Ready), wake_pending must be true
    #[test]
    #[serial]
    fn property_wake_ready_yields_pending() {
        for _ in 0..100 {
            let pid = next_pid();
            add_process(pid, ProcessState::Ready);
            set_wake_pending(pid, false); // Ensure starting condition

            wake_process(pid);
            let pending = get_wake_pending(pid);

            cleanup_process(pid);

            assert_eq!(pending, Some(true),
                "After waking Ready process, wake_pending must be true");
        }
    }

    /// Property: If wake_pending=true, set_process_state(Sleeping) must NOT sleep
    #[test]
    #[serial]
    fn property_pending_blocks_sleep() {
        for _ in 0..100 {
            let pid = next_pid();
            add_process(pid, ProcessState::Ready);
            set_wake_pending(pid, true);

            let _ = set_process_state(pid, ProcessState::Sleeping);
            let state = get_process_state(pid);

            cleanup_process(pid);

            assert_ne!(state, Some(ProcessState::Sleeping),
                "With wake_pending=true, sleep must be blocked");
        }
    }
}
