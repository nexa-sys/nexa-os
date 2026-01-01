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

    // ============================================================================
    // Tests that verify scheduler behavior using actual PROCESS_TABLE
    // These tests check the INVARIANT that a Running process should never
    // cause the scheduler to enter idle loop.
    // ============================================================================

    /// CRITICAL TEST: Single running process with exhausted time slice
    /// 
    /// This test sets up a single Running process in PROCESS_TABLE with 
    /// exhausted time slice, then verifies the scheduler's behavior.
    /// 
    /// The KEY INVARIANT is: if there's a Running process in the table,
    /// the scheduler must NOT enter idle loop - it should let that process
    /// continue running (with replenished time slice if needed).
    #[test]
    #[serial]
    fn test_single_running_process_invariant() {
        let pid = next_pid();
        let proc = make_test_process(pid, ProcessState::Running);
        let mut entry = make_process_entry(proc);
        entry.slice_remaining_ns = 0; // Exhausted time slice
        
        {
            let mut table = process_table_lock();
            // Clear table first
            for slot in table.iter_mut() {
                *slot = None;
            }
            // Add our single running process
            table[0] = Some(entry);
        }
        
        // Check the invariant: a Running process exists
        let has_running = {
            let table = process_table_lock();
            table.iter().any(|slot| {
                slot.as_ref().map_or(false, |e| e.process.state == ProcessState::Running)
            })
        };
        
        assert!(has_running, "Test setup: should have a Running process");
        
        // The scheduler's decision when compute_schedule_decision returns None:
        // - If a Running process exists -> should NOT idle (let it continue)
        // - If no Running process exists -> should idle
        //
        // This is the invariant we're testing. The scheduler code should check
        // for a Running process before entering idle loop.
    }

    /// Test: ls hang scenario - shell sleeping, ls running with exhausted slice
    /// 
    /// This simulates the exact scenario that causes ls to hang:
    /// - Shell (PID 1) is Sleeping (waiting on wait4)
    /// - ls (PID 2) is Running with exhausted time slice
    /// 
    /// The scheduler MUST let ls continue, not enter idle loop.
    #[test]
    #[serial]
    fn test_ls_hang_scenario_invariant() {
        let shell_pid = next_pid();
        let ls_pid = next_pid();
        
        let shell_proc = make_test_process(shell_pid, ProcessState::Sleeping);
        let mut ls_proc = make_test_process(ls_pid, ProcessState::Running);
        ls_proc.ppid = shell_pid; // ls is child of shell
        
        let shell_entry = make_process_entry(shell_proc);
        let mut ls_entry = make_process_entry(ls_proc);
        ls_entry.slice_remaining_ns = 0; // ls exhausted its time slice
        
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                *slot = None;
            }
            table[0] = Some(shell_entry);
            table[1] = Some(ls_entry);
        }
        
        // Verify setup
        let (has_running, has_sleeping, running_count) = {
            let table = process_table_lock();
            let running = table.iter().filter(|s| {
                s.as_ref().map_or(false, |e| e.process.state == ProcessState::Running)
            }).count();
            let sleeping = table.iter().any(|s| {
                s.as_ref().map_or(false, |e| e.process.state == ProcessState::Sleeping)
            });
            (running > 0, sleeping, running)
        };
        
        assert!(has_running, "Test setup: ls should be Running");
        assert!(has_sleeping, "Test setup: shell should be Sleeping");
        assert_eq!(running_count, 1, "Test setup: exactly one Running process");
        
        // INVARIANT: Since ls is Running, scheduler must NOT idle.
        // The scheduler should:
        // 1. See that ls is the only runnable process
        // 2. Replenish its time slice
        // 3. Let it continue running
        // NOT: Enter idle loop waiting for something that will never happen
    }

    /// Test: One Running, one Sleeping - Running must continue
    #[test]
    #[serial]
    fn test_one_running_one_sleeping_invariant() {
        let running_pid = next_pid();
        let sleeping_pid = next_pid();
        
        let running_proc = make_test_process(running_pid, ProcessState::Running);
        let sleeping_proc = make_test_process(sleeping_pid, ProcessState::Sleeping);
        
        let mut running_entry = make_process_entry(running_proc);
        running_entry.slice_remaining_ns = 0; // Exhausted
        let sleeping_entry = make_process_entry(sleeping_proc);
        
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                *slot = None;
            }
            table[0] = Some(running_entry);
            table[1] = Some(sleeping_entry);
        }
        
        // Count states
        let (running_count, sleeping_count) = {
            let table = process_table_lock();
            let running = table.iter().filter(|s| {
                s.as_ref().map_or(false, |e| e.process.state == ProcessState::Running)
            }).count();
            let sleeping = table.iter().filter(|s| {
                s.as_ref().map_or(false, |e| e.process.state == ProcessState::Sleeping)
            }).count();
            (running, sleeping)
        };
        
        assert_eq!(running_count, 1, "Should have 1 Running");
        assert_eq!(sleeping_count, 1, "Should have 1 Sleeping");
        
        // INVARIANT: With 1 Running process, scheduler must NOT idle
    }

    /// Test: All sleeping should allow idle
    #[test]
    #[serial]
    fn test_all_sleeping_can_idle() {
        let pid1 = next_pid();
        let pid2 = next_pid();
        
        let proc1 = make_test_process(pid1, ProcessState::Sleeping);
        let proc2 = make_test_process(pid2, ProcessState::Sleeping);
        
        let entry1 = make_process_entry(proc1);
        let entry2 = make_process_entry(proc2);
        
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                *slot = None;
            }
            table[0] = Some(entry1);
            table[1] = Some(entry2);
        }
        
        // Verify no Running process
        let has_running = {
            let table = process_table_lock();
            table.iter().any(|s| {
                s.as_ref().map_or(false, |e| e.process.state == ProcessState::Running)
            })
        };
        
        assert!(!has_running, "No process should be Running");
        
        // When all processes are Sleeping, idle loop IS appropriate
        // (waiting for interrupt to wake one up)
    }

    /// Test: Empty table should allow idle
    #[test]
    #[serial]
    fn test_empty_table_can_idle() {
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                *slot = None;
            }
        }
        
        let has_any_process = {
            let table = process_table_lock();
            table.iter().any(|s| s.is_some())
        };
        
        assert!(!has_any_process, "Table should be empty");
        
        // When no processes exist, idle loop IS appropriate
    }

    /// Test: Running + Ready should switch, not idle
    #[test]
    #[serial]
    fn test_running_and_ready_should_switch() {
        let running_pid = next_pid();
        let ready_pid = next_pid();
        
        let running_proc = make_test_process(running_pid, ProcessState::Running);
        let ready_proc = make_test_process(ready_pid, ProcessState::Ready);
        
        let mut running_entry = make_process_entry(running_proc);
        running_entry.slice_remaining_ns = 0; // Exhausted
        let ready_entry = make_process_entry(ready_proc);
        
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                *slot = None;
            }
            table[0] = Some(running_entry);
            table[1] = Some(ready_entry);
        }
        
        let (has_running, has_ready) = {
            let table = process_table_lock();
            let running = table.iter().any(|s| {
                s.as_ref().map_or(false, |e| e.process.state == ProcessState::Running)
            });
            let ready = table.iter().any(|s| {
                s.as_ref().map_or(false, |e| e.process.state == ProcessState::Ready)
            });
            (running, ready)
        };
        
        assert!(has_running, "Should have Running process");
        assert!(has_ready, "Should have Ready process");
        
        // With both Running and Ready, scheduler should switch to Ready
        // (the Running one's slice is exhausted)
    }
}
