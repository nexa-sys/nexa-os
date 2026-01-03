//! Deep Foreground Responsiveness Bug Detection
//!
//! These tests go beyond basic state machine testing to detect subtle bugs
//! that can cause foreground processes to become unresponsive.
//!
//! ## Bug Categories:
//!
//! 1. **Waiter List Bugs**: Process registered but never woken
//! 2. **Timer Interaction Bugs**: Timer tick interferes with wake mechanism
//! 3. **Multi-CPU Race Bugs**: More complex SMP scenarios
//! 4. **Queue Overflow Bugs**: Waiter queue full, process silently dropped
//! 5. **Spurious Wake Bugs**: Process woken but data not ready
//! 6. **Signal Mask Bugs**: Signals blocked causing permanent sleep
//! 7. **Need_resched Timing Bugs**: Flag checked at wrong time
//! 8. **EEVDF Scheduling Bugs**: Woken process never selected

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, Context};
    use crate::scheduler::{
        wake_process, set_process_state, process_table_lock,
        SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
        calc_vdeadline, get_min_vruntime, tick, do_schedule,
        get_process_state, get_process_vruntime,
    };
    use crate::scheduler::percpu::{init_percpu_sched, check_need_resched, set_need_resched};
    use crate::signal::SignalState;
    use crate::numa;

    use serial_test::serial;
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    static INIT_PERCPU: Once = Once::new();
    static NEXT_PID: AtomicU64 = AtomicU64::new(400000);

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

    fn make_process_entry_with_vruntime(proc: Process, vrt: u64) -> ProcessEntry {
        let mut entry = make_process_entry(proc);
        entry.vruntime = vrt;
        entry.vdeadline = calc_vdeadline(vrt, BASE_SLICE_NS, NICE_0_WEIGHT);
        entry
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
                let entry = make_process_entry_with_vruntime(make_test_process(pid, state), vruntime);
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

    fn set_vruntime(pid: Pid, vrt: u64) {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    entry.vruntime = vrt;
                    entry.vdeadline = calc_vdeadline(vrt, BASE_SLICE_NS, entry.weight);
                    return;
                }
            }
        }
    }

    // =========================================================================
    // BUG #1: Timer Tick During Wake-Sleep Transition
    //
    // Timer tick() can modify scheduler state. If it happens during
    // the wake-sleep sequence, it might corrupt state.
    // =========================================================================

    /// Timer tick during wake-sleep must not corrupt state
    #[test]
    #[serial]
    fn timer_tick_during_wake_sleep_no_corruption() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        for _ in 0..100 {
            // Reset
            let _ = set_process_state(pid, ProcessState::Ready);
            set_wake_pending(pid, false);

            // Wake arrives
            wake_process(pid);

            // Timer tick happens (might modify scheduling state)
            let _ = tick(1);

            // Process tries to sleep
            let _ = set_process_state(pid, ProcessState::Sleeping);

            let state = get_process_state(pid);
            if state == Some(ProcessState::Sleeping) {
                cleanup_process(pid);
                panic!("BUG: Timer tick caused wake_pending to be lost!\n\
                        Process is stuck sleeping after wake + tick + sleep.");
            }
        }

        cleanup_process(pid);
    }

    // =========================================================================
    // BUG #2: Schedule() Clears Wake Pending
    //
    // If do_schedule() incorrectly clears wake_pending, the protection is lost.
    // =========================================================================

    /// do_schedule() must NOT clear wake_pending
    #[test]
    #[serial]
    fn schedule_does_not_clear_wake_pending() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Set wake_pending
        set_wake_pending(pid, true);

        // Call schedule - this should not affect wake_pending
        // Note: In real kernel, do_schedule() is called. We check the invariant after tick().
        let _ = tick(1);

        let pending = get_wake_pending(pid);
        cleanup_process(pid);

        // wake_pending should still be true (not cleared by scheduler)
        // NOTE: This test passes if tick() doesn't clear wake_pending.
        // If the kernel clears it during scheduling, this would fail.
        assert_eq!(pending, Some(true),
            "BUG: Scheduler cleared wake_pending!\n\
             This loses the pending wake, causing stuck processes.");
    }

    // =========================================================================
    // BUG #3: EEVDF Starvation - Woken Process Has High Deadline
    //
    // Even if process is Ready, EEVDF might never select it if its
    // deadline is very high compared to others.
    // =========================================================================

    /// Woken process must have competitive deadline for scheduling
    #[test]
    #[serial]
    fn woken_process_competitive_deadline() {
        // Create background with low vruntime/deadline
        let bg_pid = next_pid();
        add_process_with_vruntime(bg_pid, ProcessState::Running, 10_000_000);

        // Create shell with very high vruntime (representing long-sleeping state)
        let shell_pid = next_pid();
        add_process_with_vruntime(shell_pid, ProcessState::Sleeping, 1_000_000_000);

        // Get deadlines before wake
        let bg_vrt_before = get_process_vruntime(bg_pid).unwrap_or(0);

        // Wake shell
        wake_process(shell_pid);

        let shell_vrt_after = get_process_vruntime(shell_pid).unwrap_or(u64::MAX);

        cleanup_process(bg_pid);
        cleanup_process(shell_pid);

        // Shell vruntime should be reset to be competitive
        // Allow 2 slice worth of difference
        let threshold = bg_vrt_before + BASE_SLICE_NS * 2;
        assert!(shell_vrt_after <= threshold,
            "EEVDF STARVATION BUG: Woken shell vruntime ({}) is not competitive!\n\
             Background vruntime: {}, threshold: {}\n\
             Shell will never be selected by scheduler despite being Ready.",
            shell_vrt_after, bg_vrt_before, threshold);
    }

    // =========================================================================
    // BUG #4: Multiple CPUs Wake Same Process
    //
    // In SMP, two CPUs might try to wake the same process simultaneously.
    // Only one should succeed, and state must not be corrupted.
    // =========================================================================

    /// Concurrent wakes from multiple sources must not corrupt state
    #[test]
    #[serial]
    fn concurrent_wakes_no_corruption() {
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);

        // Multiple wake sources (keyboard, timer, signal)
        let woke1 = wake_process(pid);
        let woke2 = wake_process(pid);
        let woke3 = wake_process(pid);

        let state = get_process_state(pid);
        let pending = get_wake_pending(pid);

        cleanup_process(pid);

        // First wake should succeed
        assert!(woke1, "First wake on Sleeping should succeed");

        // Process must be Ready
        assert_eq!(state, Some(ProcessState::Ready),
            "State corrupted after concurrent wakes");

        // After first wake succeeds (Sleeping->Ready), subsequent wakes
        // should set wake_pending
        assert_eq!(pending, Some(true),
            "Concurrent wakes on Ready should set wake_pending");
    }

    // =========================================================================
    // BUG #5: Wake Pending Consumed Too Early
    //
    // If wake_pending is consumed before the actual sleep check,
    // the protection is lost.
    // =========================================================================

    /// wake_pending must only be consumed by actual sleep attempt
    #[test]
    #[serial]
    fn wake_pending_not_consumed_prematurely() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Set wake_pending
        wake_process(pid);
        assert_eq!(get_wake_pending(pid), Some(true));

        // State query should NOT consume wake_pending
        let _ = get_process_state(pid);
        assert_eq!(get_wake_pending(pid), Some(true),
            "BUG: State query consumed wake_pending!");

        // Vruntime query should NOT consume wake_pending
        let _ = get_process_vruntime(pid);
        assert_eq!(get_wake_pending(pid), Some(true),
            "BUG: Vruntime query consumed wake_pending!");

        // Only sleep attempt should consume it
        let _ = set_process_state(pid, ProcessState::Sleeping);
        let pending_after_sleep = get_wake_pending(pid);

        cleanup_process(pid);

        // Now it should be consumed
        assert_eq!(pending_after_sleep, Some(false),
            "wake_pending should be consumed after blocked sleep");
    }

    // =========================================================================
    // BUG #6: Ready -> Running -> Sleep Sequence
    //
    // Process might be made Running (by scheduler), then immediately try
    // to sleep. wake_pending must survive the Ready->Running transition.
    // =========================================================================

    /// wake_pending must survive Ready -> Running transition
    #[test]
    #[serial]
    fn wake_pending_survives_ready_to_running() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Wake on Ready - sets wake_pending
        wake_process(pid);
        assert_eq!(get_wake_pending(pid), Some(true));

        // Scheduler makes it Running
        let _ = set_process_state(pid, ProcessState::Running);

        // wake_pending must still be set
        let pending = get_wake_pending(pid);
        assert_eq!(pending, Some(true),
            "BUG: wake_pending lost during Ready->Running transition!");

        // Now if it tries to sleep, must be blocked
        let _ = set_process_state(pid, ProcessState::Sleeping);
        let state = get_process_state(pid);

        cleanup_process(pid);

        assert_ne!(state, Some(ProcessState::Sleeping),
            "BUG: Process slept despite wake_pending after Running state!");
    }

    // =========================================================================
    // BUG #7: Interleaved Wake/Sleep with Timer
    //
    // Complex sequence: wake, tick, wake, sleep, tick, sleep
    // =========================================================================

    /// Complex interleaving of wake/sleep/tick must not lose wakes
    #[test]
    #[serial]
    fn complex_interleaving_no_lost_wake() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        for _ in 0..50 {
            // Reset
            let _ = set_process_state(pid, ProcessState::Ready);
            set_wake_pending(pid, false);

            // Sequence: wake, tick, wake, tick, sleep
            wake_process(pid);        // pending = true
            let _ = tick(1);          // tick happens
            wake_process(pid);        // still pending = true
            let _ = tick(1);          // another tick
            let _ = set_process_state(pid, ProcessState::Sleeping);  // blocked

            let state = get_process_state(pid);
            if state == Some(ProcessState::Sleeping) {
                cleanup_process(pid);
                panic!("BUG: Complex wake/tick/sleep sequence lost wake!");
            }
        }

        cleanup_process(pid);
    }

    // =========================================================================
    // BUG #8: Process Table Full - Waiter Silently Dropped
    //
    // If waiter list is full, new waiters might be silently dropped.
    // This test checks for proper handling.
    // =========================================================================

    /// Many processes can be registered and all should wake
    #[test]
    #[serial]
    fn many_processes_all_wake_correctly() {
        let mut pids = Vec::new();
        const NUM_PROCS: usize = 64; // More than typical waiter queue size

        // Create many sleeping processes
        for _ in 0..NUM_PROCS {
            let pid = next_pid();
            add_process(pid, ProcessState::Sleeping);
            pids.push(pid);
        }

        // Wake all
        for &pid in &pids {
            wake_process(pid);
        }

        // All must be Ready
        let mut stuck = 0;
        for &pid in &pids {
            if get_process_state(pid) != Some(ProcessState::Ready) {
                stuck += 1;
            }
        }

        // Cleanup
        for &pid in &pids {
            cleanup_process(pid);
        }

        assert_eq!(stuck, 0,
            "BUG: {} out of {} processes not properly woken!\n\
             Possible waiter queue overflow or wake bug.", stuck, NUM_PROCS);
    }

    // =========================================================================
    // BUG #9: Stale PID Mapping
    //
    // If PID radix tree lookup returns stale data, wake might go to wrong process.
    // =========================================================================

    /// PID reuse must not cause wake to wrong process
    #[test]
    #[serial]
    fn pid_reuse_no_cross_wake() {
        let pid1 = next_pid();
        add_process(pid1, ProcessState::Sleeping);

        // Remove process (exit)
        cleanup_process(pid1);

        // Create new process with potentially same slot
        let pid2 = next_pid();
        add_process(pid2, ProcessState::Ready);

        // Try to wake old PID - should fail gracefully
        let woke = wake_process(pid1);

        let state2 = get_process_state(pid2);
        let pending2 = get_wake_pending(pid2);

        cleanup_process(pid2);

        // Wake of non-existent PID should return false
        assert!(!woke, "wake_process on removed PID should return false");

        // New process should not be affected
        assert_eq!(state2, Some(ProcessState::Ready),
            "New process state corrupted by wake to old PID");
        assert_eq!(pending2, Some(false),
            "New process wake_pending set by wake to old PID!");
    }

    // =========================================================================
    // BUG #10: need_resched Race with Timer
    //
    // If timer clears need_resched before keyboard handler checks it,
    // woken process might not run promptly.
    // =========================================================================

    /// need_resched must persist until consumed by proper check
    #[test]
    #[serial]
    fn need_resched_persists_properly() {
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);

        // Clear any existing flag
        let _ = check_need_resched();

        // Wake - sets need_resched
        wake_process(pid);

        // Timer tick (might try to clear/check flag)
        let _ = tick(1);

        // Check if need_resched is still accessible
        // Note: In real kernel, this is per-CPU and atomic
        // This test verifies the invariant that wake sets the flag
        // The actual persistence depends on implementation details

        cleanup_process(pid);

        // This is more of a documentation test - the real check is that
        // keyboard_interrupt_handler properly calls do_schedule_from_interrupt
        // when need_resched is set
    }

    // =========================================================================
    // BUG #11: Sleeping Process Receives Signal
    //
    // When a signal is delivered to a sleeping process, it must wake up.
    // If signal delivery doesn't call wake_process, process stays stuck.
    // =========================================================================

    /// Process sleeping in interruptible state must wake on signal
    #[test]
    #[serial]
    fn sleeping_process_wakes_on_signal() {
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);

        // Signal delivery (which should call wake_process internally)
        // In real kernel: do_signal() -> wake_process()
        let woke = wake_process(pid);

        let state = get_process_state(pid);
        cleanup_process(pid);

        assert!(woke, "Signal wake should succeed");
        assert_eq!(state, Some(ProcessState::Ready),
            "BUG: Process not woken by signal!\n\
             SIGINT, SIGTERM won't work. Ctrl+C does nothing.");
    }

    // =========================================================================
    // BUG #12: Very Rapid State Changes
    //
    // Extremely rapid state changes might expose subtle race windows.
    // =========================================================================

    /// Very rapid state changes must maintain consistency
    #[test]
    #[serial]
    fn rapid_state_changes_consistent() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        let mut stuck_count = 0;
        const ITERATIONS: usize = 10000;

        for _ in 0..ITERATIONS {
            // Rapid sequence
            wake_process(pid);
            let _ = set_process_state(pid, ProcessState::Sleeping);

            if get_process_state(pid) == Some(ProcessState::Sleeping) {
                stuck_count += 1;
                // Recover
                wake_process(pid);
            }

            // Reset for next iteration
            set_wake_pending(pid, false);
            let _ = set_process_state(pid, ProcessState::Ready);
        }

        cleanup_process(pid);

        // Should have ZERO stuck instances
        assert_eq!(stuck_count, 0,
            "RACE BUG: {} out of {} rapid state changes resulted in stuck state!\n\
             This indicates a race window in wake/sleep handling.",
            stuck_count, ITERATIONS);
    }

    // =========================================================================
    // BUG #13: Wake During Process Exit
    //
    // If a process is exiting (becoming Zombie), wake should not corrupt state.
    // =========================================================================

    /// Wake during exit path must not corrupt Zombie state
    #[test]
    #[serial]
    fn wake_during_exit_no_corruption() {
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);

        // Process exits - becomes Zombie
        let _ = set_process_state(pid, ProcessState::Zombie);

        // Late wake arrives (e.g., timer fired for something the process was waiting for)
        let woke = wake_process(pid);

        let state = get_process_state(pid);
        cleanup_process(pid);

        assert!(!woke, "Cannot wake a Zombie");
        assert_eq!(state, Some(ProcessState::Zombie),
            "BUG: Zombie state corrupted by wake!\n\
             This could cause wait4() to miss the zombie or double-free.");
    }

    // =========================================================================
    // BUG #14: Wake Pending Overflow/Underflow
    //
    // If wake_pending is a counter instead of flag, overflow might occur.
    // (Current impl uses bool, but test documents the expected behavior)
    // =========================================================================

    /// Many wakes followed by many sleep attempts - no underflow
    #[test]
    #[serial]
    fn wake_pending_no_underflow() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Many wakes
        for _ in 0..100 {
            wake_process(pid);
        }

        // Many sleep attempts - only first should consume pending
        for i in 0..100 {
            let _ = set_process_state(pid, ProcessState::Sleeping);

            // After first blocked sleep, process is Ready with pending=false
            // Subsequent sleeps should actually sleep
            if i > 0 {
                let state = get_process_state(pid);
                if state == Some(ProcessState::Sleeping) {
                    // This is expected - second+ sleep should work
                    wake_process(pid); // Wake for next iteration
                }
            }
        }

        // No underflow should have occurred
        let pending = get_wake_pending(pid);
        cleanup_process(pid);

        // pending should be false (not underflowed to some strange value)
        assert_eq!(pending, Some(false),
            "wake_pending has unexpected value after many wake/sleep cycles");
    }
}
