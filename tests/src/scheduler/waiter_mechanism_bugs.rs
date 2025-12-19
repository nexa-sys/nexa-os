//! Process Waiter Mechanism Bug Detection Tests
//!
//! These tests detect bugs in the keyboard/tty waiter mechanism that can
//! cause foreground processes to become unresponsive.
//!
//! ## Bug Categories:
//!
//! 1. **Waiter List Race Conditions**
//!    - Process removed from waiter list before it sleeps
//!    - Waiter list overflow drops waiters silently
//!
//! 2. **Wake All vs Wake One Issues**
//!    - wake_all_waiters wakes all, but only one should read
//!    - Multiple waiters race for single input byte
//!
//! 3. **Spurious Wakeup Handling**
//!    - Process woken but no data available
//!    - Process must re-register as waiter
//!
//! ## Test Philosophy:
//!
//! These tests FAIL when bugs exist and PASS when fixed.

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, Context};
    use crate::scheduler::{
        wake_process, set_process_state, process_table_lock,
        SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
        calc_vdeadline,
    };
    use crate::scheduler::percpu::{init_percpu_sched, check_need_resched};
    use crate::signal::SignalState;
    use crate::numa;

    use serial_test::serial;
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::collections::HashSet;

    static INIT_PERCPU: Once = Once::new();
    static NEXT_PID: AtomicU64 = AtomicU64::new(90000);

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

    // =========================================================================
    // Simulated Waiter List (mirrors keyboard.rs structure)
    // =========================================================================

    const MAX_WAITERS: usize = 8;

    struct MockWaiterList {
        waiters: [Option<Pid>; MAX_WAITERS],
    }

    impl MockWaiterList {
        fn new() -> Self {
            Self { waiters: [None; MAX_WAITERS] }
        }

        fn add_waiter(&mut self, pid: Pid) -> bool {
            for slot in self.waiters.iter_mut() {
                if slot.is_none() {
                    *slot = Some(pid);
                    return true;
                }
            }
            false // Queue full
        }

        fn remove_waiter(&mut self, pid: Pid) {
            for slot in self.waiters.iter_mut() {
                if *slot == Some(pid) {
                    *slot = None;
                    return;
                }
            }
        }

        fn wake_all(&mut self) -> Vec<Pid> {
            let mut woken = Vec::new();
            for slot in self.waiters.iter_mut() {
                if let Some(pid) = slot.take() {
                    wake_process(pid);
                    woken.push(pid);
                }
            }
            woken
        }

        fn count(&self) -> usize {
            self.waiters.iter().filter(|s| s.is_some()).count()
        }
    }

    // =========================================================================
    // BUG TEST: Waiter removed but process not yet sleeping
    // =========================================================================

    /// BUG TEST: wake_all removes waiter, but process hasn't slept yet
    ///
    /// Sequence:
    /// 1. Shell calls add_waiter() - registered
    /// 2. Interrupt fires
    /// 3. wake_all_waiters() runs:
    ///    a. Takes shell PID from waiter list (REMOVED)
    ///    b. Calls wake_process(shell) - shell is Ready, returns false
    /// 4. Shell calls set_process_state(Sleeping)
    /// 5. Shell is now sleeping, NOT in waiter list, will NEVER wake
    #[test]
    #[serial]
    fn bug_waiter_removed_before_sleep() {
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Ready);

        let mut waiters = MockWaiterList::new();

        // Step 1: Shell registers as waiter
        assert!(waiters.add_waiter(shell_pid));
        assert_eq!(waiters.count(), 1);

        // Steps 2-3: Interrupt fires, wake_all removes shell and calls wake_process
        let woken = waiters.wake_all();
        assert_eq!(woken.len(), 1);
        assert_eq!(woken[0], shell_pid);
        assert_eq!(waiters.count(), 0); // Shell REMOVED from list

        // Step 4: Shell tries to sleep
        let _ = set_process_state(shell_pid, ProcessState::Sleeping);

        let final_state = get_state(shell_pid);

        cleanup_process(shell_pid);

        // BUG CHECK: If shell is Sleeping and not in waiter list, it's stuck!
        assert_ne!(final_state, Some(ProcessState::Sleeping),
            "BUG: Shell is Sleeping but removed from waiter list! \
             It will NEVER wake up. This is the exact foreground unresponsive bug. \
             FIX: wake_process must set wake_pending for Ready processes.");
    }

    /// BUG TEST: Multiple wake_all before any process sleeps
    ///
    /// Rapid keyboard input can trigger multiple interrupts.
    #[test]
    #[serial]
    fn bug_multiple_wake_all_rapid() {
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Ready);

        let mut waiters = MockWaiterList::new();
        waiters.add_waiter(shell_pid);

        // First interrupt - removes shell
        waiters.wake_all();
        
        // Shell re-registers (still Ready)
        waiters.add_waiter(shell_pid);
        
        // Second interrupt - removes shell again
        waiters.wake_all();

        // Shell finally tries to sleep
        let _ = set_process_state(shell_pid, ProcessState::Sleeping);

        let final_state = get_state(shell_pid);

        cleanup_process(shell_pid);

        // With proper wake_pending, shell should stay Ready
        assert_ne!(final_state, Some(ProcessState::Sleeping),
            "BUG: Multiple wake_all's lost - shell stuck sleeping!");
    }

    // =========================================================================
    // BUG TEST: Waiter list overflow
    // =========================================================================

    /// BUG TEST: Waiter list overflow drops processes silently
    ///
    /// If more than MAX_WAITERS processes wait for keyboard, some are dropped.
    #[test]
    #[serial]
    fn bug_waiter_list_overflow() {
        let mut pids = Vec::new();
        let mut waiters = MockWaiterList::new();

        // Create MAX_WAITERS + 2 processes
        for _ in 0..(MAX_WAITERS + 2) {
            let pid = next_pid();
            add_process(pid, ProcessState::Ready);
            pids.push(pid);
        }

        // All try to register as waiters
        let mut registered = 0;
        let mut dropped = Vec::new();
        for &pid in &pids {
            if waiters.add_waiter(pid) {
                registered += 1;
            } else {
                dropped.push(pid);
            }
        }

        // Cleanup
        for pid in &pids {
            cleanup_process(*pid);
        }

        // Verify overflow detection
        assert_eq!(registered, MAX_WAITERS,
            "Should only be able to register {} waiters", MAX_WAITERS);
        assert_eq!(dropped.len(), 2,
            "Two processes should have been dropped");
        
        // This test documents the limitation - not necessarily a "bug" to fix,
        // but important to understand. The dropped processes will spin-wait,
        // which may cause high CPU usage but at least they won't hang.
    }

    // =========================================================================
    // BUG TEST: Spurious wakeup handling
    // =========================================================================

    /// BUG TEST: Spurious wakeup - no data available after wake
    ///
    /// Process is woken but another process already consumed the input.
    /// Process must re-register as waiter and sleep again.
    #[test]
    #[serial]
    fn bug_spurious_wakeup_reregistration() {
        let pid1 = next_pid();
        let pid2 = next_pid();
        
        add_process(pid1, ProcessState::Sleeping);
        add_process(pid2, ProcessState::Sleeping);

        let mut waiters = MockWaiterList::new();
        waiters.add_waiter(pid1);
        waiters.add_waiter(pid2);

        // Keyboard interrupt - both wake
        waiters.wake_all();

        let state1 = get_state(pid1);
        let state2 = get_state(pid2);

        cleanup_process(pid1);
        cleanup_process(pid2);

        // Both should be Ready (woken from Sleeping)
        assert_eq!(state1, Some(ProcessState::Ready),
            "Process 1 should be Ready after wake");
        assert_eq!(state2, Some(ProcessState::Ready),
            "Process 2 should be Ready after wake");

        // Note: The spurious wakeup itself is not a bug - it's expected.
        // The bug would be if the spurious-woken process can't re-register
        // or gets stuck. This is handled by userspace retry logic.
    }

    // =========================================================================
    // BUG TEST: Process table index vs PID confusion
    // =========================================================================

    /// BUG TEST: Radix tree stale entry after process exit/reuse
    ///
    /// If PID is reused and radix tree has stale entry, wake_process
    /// might wake wrong process.
    #[test]
    #[serial]
    fn bug_pid_reuse_stale_mapping() {
        let pid = next_pid();
        
        // Create and remove process
        add_process(pid, ProcessState::Ready);
        cleanup_process(pid);

        // Verify process is gone
        let state_after_cleanup = get_state(pid);
        assert_eq!(state_after_cleanup, None, "Process should be removed");

        // Try to wake the non-existent PID
        let woke = wake_process(pid);

        // Should not crash or wake a wrong process
        assert!(!woke, "wake_process should return false for non-existent PID");
    }

    // =========================================================================
    // BUG TEST: State consistency across multiple wake attempts
    // =========================================================================

    /// BUG TEST: Process wake_pending survives until sleep attempt
    ///
    /// wake_pending must remain set until the process actually tries to sleep.
    #[test]
    #[serial]
    fn bug_wake_pending_persistence() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Wake on Ready - sets wake_pending
        wake_process(pid);
        
        let pending1 = get_wake_pending(pid);
        assert_eq!(pending1, Some(true), "wake_pending should be set");

        // Multiple state queries shouldn't clear it
        let _ = get_state(pid);
        let _ = get_state(pid);
        let _ = get_wake_pending(pid);

        let pending2 = get_wake_pending(pid);
        assert_eq!(pending2, Some(true), 
            "BUG: wake_pending cleared by read operations!");

        // Only set_process_state(Sleeping) should clear it
        let _ = set_process_state(pid, ProcessState::Sleeping);
        
        let pending3 = get_wake_pending(pid);
        assert_eq!(pending3, Some(false),
            "wake_pending should be consumed by blocked sleep");

        cleanup_process(pid);
    }

    // =========================================================================
    // Integration: Full read() syscall simulation
    // =========================================================================

    /// Integration: Simulates complete read() syscall on /dev/tty
    ///
    /// This mimics the actual read_raw_for_tty() flow in the kernel.
    #[test]
    #[serial]
    fn integration_tty_read_syscall_flow() {
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Running); // Currently running

        let mut waiters = MockWaiterList::new();
        let mut simulated_buffer: Vec<u8> = Vec::new();

        // Simulate: read() syscall entry
        // 1. Check if data available (none)
        assert!(simulated_buffer.is_empty());

        // 2. Register as waiter
        let _ = waiters.add_waiter(shell_pid);

        // 3. State changes to Ready before sleep
        //    (kernel may do some prep work here)
        let _ = set_process_state(shell_pid, ProcessState::Ready);

        // --- RACE WINDOW STARTS ---
        // Between add_waiter() and sleep, interrupt can fire

        // 4. Keyboard interrupt fires!
        simulated_buffer.push(b'a');
        let woken = waiters.wake_all(); // Removes shell, calls wake_process
        assert_eq!(woken.len(), 1);

        // --- RACE WINDOW ENDS ---

        // 5. Shell tries to sleep (hasn't seen the data yet in this flow)
        let _ = set_process_state(shell_pid, ProcessState::Sleeping);

        // Check final state
        let final_state = get_state(shell_pid);
        let data_available = !simulated_buffer.is_empty();

        cleanup_process(shell_pid);

        // Data is available, shell should NOT be sleeping
        assert!(data_available, "Data should be in buffer");
        assert_ne!(final_state, Some(ProcessState::Sleeping),
            "CRITICAL BUG: Shell stuck sleeping despite data available! \
             User typed but shell won't respond. \
             FIX: wake_pending mechanism must prevent this sleep.");
    }

    /// Integration: Process re-registers after successful read
    ///
    /// After reading one byte, process goes back to wait for more input.
    #[test]
    #[serial]
    fn integration_read_loop() {
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Ready);

        let mut waiters = MockWaiterList::new();

        for i in 0..10 {
            // Register waiter
            waiters.add_waiter(shell_pid);

            // Keyboard interrupt
            wake_process(shell_pid); // Sets wake_pending if Ready
            waiters.wake_all(); // Removes from list

            // Try to sleep
            let _ = set_process_state(shell_pid, ProcessState::Sleeping);

            let state = get_state(shell_pid);
            assert_ne!(state, Some(ProcessState::Sleeping),
                "BUG: Shell stuck on iteration {}", i);

            // "Read" the data, back to Ready
            let _ = set_process_state(shell_pid, ProcessState::Ready);
        }

        cleanup_process(shell_pid);
    }
}
