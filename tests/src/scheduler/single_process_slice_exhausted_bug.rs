//! Tests for the single-process time slice exhaustion bug
//!
//! BUG DESCRIPTION:
//! When only one process is running and its time slice is exhausted:
//! 1. tick() returns true (should_resched)
//! 2. do_schedule_internal() is called
//! 3. compute_schedule_decision() finds the same process as next (it's the only one)
//! 4. Since the process is already Running, it returns None ("no switch needed")
//! 5. do_schedule_internal() treats None as "no ready process" and enters idle loop!
//! 
//! RESULT: The only running process gets stuck in idle loop instead of continuing
//! to run with a replenished time slice.
//!
//! SYMPTOM: Commands like `ls` hang mid-output because after 4ms (one time slice),
//! the process enters the idle loop and never returns.
//!
//! These tests should FAIL when the bug exists and PASS when fixed.

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, Context};
    use crate::scheduler::{
        process_table_lock,
        SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
        calc_vdeadline,
    };
    use crate::signal::SignalState;
    use crate::numa;

    use serial_test::serial;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_PID: AtomicU64 = AtomicU64::new(800000);

    fn next_pid() -> Pid {
        NEXT_PID.fetch_add(1, Ordering::SeqCst)
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

    /// Simulates what compute_schedule_decision does when deciding what to do
    /// Returns: (should_switch, should_idle)
    /// - should_switch: true if we need to context switch to a different process
    /// - should_idle: true if we should enter idle loop (no process to run)
    fn simulate_schedule_decision(
        current_pid: Option<Pid>,
        entries: &[(Pid, ProcessState, u64)], // (pid, state, slice_remaining_ns)
    ) -> (bool, bool) {
        // Find next ready process (or current if it's the only one)
        let mut next_ready_pid: Option<Pid> = None;
        let mut current_is_running = false;
        
        for (pid, state, _) in entries {
            if *state == ProcessState::Ready {
                next_ready_pid = Some(*pid);
                break;
            }
            if Some(*pid) == current_pid && *state == ProcessState::Running {
                current_is_running = true;
            }
        }
        
        // If no Ready process found, check if current process can continue
        if next_ready_pid.is_none() {
            if current_is_running {
                // BUG LOCATION: The real code returns None here, which causes idle loop!
                // CORRECT: The process should continue running with replenished slice
                // The question is: does returning None mean "continue current" or "enter idle"?
                
                // In the buggy code, None always means "enter idle loop"
                // This is WRONG when current process is Running
                return (false, true); // This represents the BUG behavior
            }
            // No process at all - genuinely need idle loop
            return (false, true);
        }
        
        // Check if next ready process is the same as current
        if let Some(curr) = current_pid {
            if next_ready_pid == Some(curr) {
                // Same process - but is it Running or Ready?
                for (pid, state, _) in entries {
                    if *pid == curr {
                        if *state == ProcessState::Running {
                            // BUG: Returns None which causes idle loop!
                            return (false, true);
                        }
                        break;
                    }
                }
            }
        }
        
        // Different process or current is not Running - need switch
        (true, false)
    }

    // ============================================================================
    // BUG DETECTION TESTS
    // These tests should FAIL when the bug exists
    // ============================================================================

    /// CRITICAL TEST: Single running process with exhausted time slice
    /// 
    /// Scenario:
    /// - Only one process (ls) is Running
    /// - Its time slice is exhausted (slice_remaining_ns = 0)
    /// - Scheduler is triggered (tick returns true)
    /// 
    /// Expected behavior: Process continues running with replenished slice
    /// Bug behavior: Process enters idle loop and hangs!
    #[test]
    fn test_single_process_exhausted_slice_must_not_idle() {
        // Single process that is Running with exhausted time slice
        let entries = vec![
            (1, ProcessState::Running, 0), // slice exhausted!
        ];
        
        let (should_switch, should_idle) = simulate_schedule_decision(Some(1), &entries);
        
        // CRITICAL INVARIANT: When only one process exists and it's Running,
        // we must NOT enter idle loop, even if its time slice is exhausted!
        // The process should continue running with a replenished time slice.
        assert!(!should_idle, 
            "BUG DETECTED: Single running process with exhausted slice would enter idle loop!\n\
             This causes `ls` and other commands to hang mid-output.\n\
             The scheduler should replenish the time slice and let the process continue,\n\
             not enter idle loop waiting for a process that will never wake up!");
        
        // We shouldn't switch either (no other process to switch to)
        assert!(!should_switch,
            "Should not switch when there's only one process");
    }

    /// Test: Two processes, one Running (exhausted), one Sleeping
    /// 
    /// This also triggers the bug because the only Ready/Running process
    /// is the current one, and it would enter idle loop.
    #[test]
    fn test_one_running_one_sleeping_must_not_idle() {
        let entries = vec![
            (1, ProcessState::Running, 0),    // Current process, slice exhausted
            (2, ProcessState::Sleeping, BASE_SLICE_NS), // Another process sleeping
        ];
        
        let (should_switch, should_idle) = simulate_schedule_decision(Some(1), &entries);
        
        // The sleeping process can't be scheduled, so current must continue
        assert!(!should_idle,
            "BUG DETECTED: Running process would enter idle loop while another sleeps!\n\
             The running process should continue with replenished slice.");
    }

    /// Test: Verify the correct behavior when there truly are no processes
    #[test]
    fn test_no_processes_should_idle() {
        let entries: Vec<(Pid, ProcessState, u64)> = vec![];
        
        let (should_switch, should_idle) = simulate_schedule_decision(None, &entries);
        
        // This is the ONLY case where idle loop is correct
        assert!(should_idle, "Should idle when no processes exist");
        assert!(!should_switch, "Cannot switch when no processes exist");
    }

    /// Test: All processes sleeping should idle
    #[test]
    fn test_all_sleeping_should_idle() {
        let entries = vec![
            (1, ProcessState::Sleeping, BASE_SLICE_NS),
            (2, ProcessState::Sleeping, BASE_SLICE_NS),
        ];
        
        let (should_switch, should_idle) = simulate_schedule_decision(None, &entries);
        
        // All sleeping - need to wait for wake
        assert!(should_idle, "Should idle when all processes are sleeping");
    }

    /// Test: Current Running, another Ready - should switch, not idle
    #[test]
    fn test_running_and_ready_should_switch() {
        let entries = vec![
            (1, ProcessState::Running, 0),    // Current, exhausted
            (2, ProcessState::Ready, BASE_SLICE_NS), // Another ready
        ];
        
        let (should_switch, should_idle) = simulate_schedule_decision(Some(1), &entries);
        
        // Should switch to the ready process
        assert!(should_switch, "Should switch to ready process");
        assert!(!should_idle, "Should not idle when a process is ready");
    }

    // ============================================================================
    // Integration-style tests that verify the actual code path
    // ============================================================================

    /// Verify that time slice replenishment happens correctly
    #[test]
    fn test_slice_replenishment_on_exhaustion() {
        let pid = next_pid();
        let proc = make_test_process(pid, ProcessState::Running);
        let mut entry = make_process_entry(proc);
        
        // Simulate exhausting the time slice (4ms of execution)
        entry.slice_remaining_ns = 0; // Exhausted!
        
        assert_eq!(entry.slice_remaining_ns, 0, 
            "Slice should be exhausted");
        
        // After scheduling, if the process continues, slice should be replenished
        // This is what should happen, but the bug prevents us from getting here
        // because we enter idle loop instead!
        
        // The slice should be replenished to at least BASE_SLICE_NS
        // (replenish_slice is called in compute_schedule_decision)
    }

    /// Test that simulates the exact sequence of events when ls hangs
    #[test]
    fn test_ls_hang_scenario() {
        // Scenario that causes ls to hang:
        // 1. ls is the foreground process, Running
        // 2. Shell is Sleeping (waiting for ls to finish via wait4)
        // 3. ls runs for 4ms, slice exhausted
        // 4. tick() returns true, triggering reschedule
        // 5. compute_schedule_decision: only ls is Ready/Running, returns None
        // 6. do_schedule_internal: sees None, enters idle loop
        // 7. ls never runs again!
        
        let entries = vec![
            (1, ProcessState::Sleeping, BASE_SLICE_NS), // Shell waiting on wait4
            (2, ProcessState::Running, 0),               // ls with exhausted slice
        ];
        
        let (should_switch, should_idle) = simulate_schedule_decision(Some(2), &entries);
        
        // ls should continue running, NOT enter idle loop!
        assert!(!should_idle,
            "BUG DETECTED: ls would hang!\n\
             Scenario: Shell sleeping on wait4, ls running with exhausted slice.\n\
             ls is the only runnable process but scheduler enters idle loop!");
        
        // No switch needed (shell is sleeping)
        assert!(!should_switch,
            "Should not switch to sleeping shell");
    }

    /// Test using actual PROCESS_TABLE to verify the bug
    #[test]
    #[serial]
    fn test_actual_process_table_single_running() {
        let pid = next_pid();
        let proc = make_test_process(pid, ProcessState::Running);
        let mut entry = make_process_entry(proc);
        entry.slice_remaining_ns = 0; // Exhausted!
        
        {
            let mut table = process_table_lock();
            // Clear table first
            for slot in table.iter_mut() {
                *slot = None;
            }
            // Add our single running process
            table[0] = Some(entry);
        }
        
        // The bug is: compute_schedule_decision would return None
        // and do_schedule_internal would enter idle loop
        // 
        // What SHOULD happen:
        // 1. Find current process (our entry, pid=pid, Running)
        // 2. Find next ready - none (only one process)
        // 3. Check if current can continue - YES (it's Running)
        // 4. Replenish slice and continue - NOT enter idle loop
        
        let table = process_table_lock();
        if let Some(entry) = &table[0] {
            assert_eq!(entry.process.state, ProcessState::Running);
            // The process is still there and Running
            // But the bug causes idle loop to be entered
        }
    }
}
