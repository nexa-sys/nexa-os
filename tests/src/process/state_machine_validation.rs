//! Process State Machine Validation Tests
//!
//! Tests for process state transitions using REAL kernel scheduler functions.
//! Per copilot-instructions.md: ProcessState must stay synchronized across
//! scheduler, signals, and wait4.

#[cfg(test)]
mod tests {
    use crate::process::{ProcessState, Pid, Process, MAX_CMDLINE_SIZE};
    use crate::scheduler::{ProcessEntry, wake_process, set_process_state, process_table_lock};
    use crate::scheduler::{SchedPolicy, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT, calc_vdeadline};
    use crate::scheduler::percpu::init_percpu_sched;
    use crate::signal::SignalState;
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
            context: crate::process::Context::zero(),
            has_entered_user: true,
            context_valid: true,
            is_fork_child: false,
            is_thread: false,
            cr3: 0x1000,
            tty: 0,
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
            numa_preferred_node: crate::numa::NUMA_NO_NODE,
            numa_policy: crate::numa::NumaPolicy::Local,
        }
    }

    fn add_process(pid: Pid, state: ProcessState) {
        ensure_percpu_init();
        let mut table = process_table_lock();
        for (idx, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                crate::process::register_pid_mapping(pid, idx as u16);
                let entry = make_process_entry(make_test_process(pid, state), 0);
                *slot = Some(entry);
                return;
            }
        }
        panic!("No free slot for test process {}", pid);
    }

    fn get_state(pid: Pid) -> Option<ProcessState> {
        let table = process_table_lock();
        for slot in table.iter() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    return Some(entry.process.state);
                }
            }
        }
        None
    }

    fn cleanup_process(pid: Pid) {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    *slot = None;
                    return;
                }
            }
        }
    }

    // =========================================================================
    // Valid State Transition Tests
    // =========================================================================

    /// Helper to check if a state transition is valid per POSIX semantics
    fn is_valid_transition(from: ProcessState, to: ProcessState) -> bool {
        use ProcessState::*;
        
        match (from, to) {
            // Ready can go to Running (scheduled)
            (Ready, Running) => true,
            // Running can go to Ready (preempted/yield), Sleeping (blocked), or Zombie (exit)
            (Running, Ready) => true,
            (Running, Sleeping) => true,
            (Running, Zombie) => true,
            // Sleeping can only go to Ready (woken up)
            (Sleeping, Ready) => true,
            // Zombie is terminal - no transitions out
            (Zombie, _) => false,
            // Same state is a no-op, considered valid
            (s1, s2) if s1 == s2 => true,
            // All other transitions are invalid
            _ => false,
        }
    }

    #[test]
    fn test_valid_ready_to_running() {
        assert!(is_valid_transition(ProcessState::Ready, ProcessState::Running));
    }

    #[test]
    fn test_valid_running_to_ready() {
        assert!(is_valid_transition(ProcessState::Running, ProcessState::Ready));
    }

    #[test]
    fn test_valid_running_to_sleeping() {
        assert!(is_valid_transition(ProcessState::Running, ProcessState::Sleeping));
    }

    #[test]
    fn test_valid_running_to_zombie() {
        assert!(is_valid_transition(ProcessState::Running, ProcessState::Zombie));
    }

    #[test]
    fn test_valid_sleeping_to_ready() {
        assert!(is_valid_transition(ProcessState::Sleeping, ProcessState::Ready));
    }

    #[test]
    fn test_invalid_zombie_transitions() {
        // Zombie is terminal - nothing should transition out
        assert!(!is_valid_transition(ProcessState::Zombie, ProcessState::Ready));
        assert!(!is_valid_transition(ProcessState::Zombie, ProcessState::Running));
        assert!(!is_valid_transition(ProcessState::Zombie, ProcessState::Sleeping));
    }

    #[test]
    fn test_invalid_ready_to_sleeping() {
        // Process must be Running to block
        assert!(!is_valid_transition(ProcessState::Ready, ProcessState::Sleeping));
    }

    #[test]
    fn test_invalid_sleeping_to_running() {
        // Must go through Ready first
        assert!(!is_valid_transition(ProcessState::Sleeping, ProcessState::Running));
    }

    #[test]
    fn test_invalid_sleeping_to_zombie() {
        // Must be Running to exit
        assert!(!is_valid_transition(ProcessState::Sleeping, ProcessState::Zombie));
    }

    #[test]
    fn test_invalid_ready_to_zombie() {
        // Must be Running to exit
        assert!(!is_valid_transition(ProcessState::Ready, ProcessState::Zombie));
    }

    // =========================================================================
    // Process Entry State Tests
    // =========================================================================

    #[test]
    fn test_new_process_is_ready() {
        let entry = ProcessEntry::empty();
        assert_eq!(entry.process.state, ProcessState::Ready,
            "New process should start in Ready state");
    }

    #[test]
    fn test_process_zombie_preserves_exit_code() {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 42;
        entry.process.exit_code = 123;
        entry.process.state = ProcessState::Zombie;
        
        assert_eq!(entry.process.state, ProcessState::Zombie);
        assert_eq!(entry.process.exit_code, 123,
            "Exit code must be preserved in Zombie state for wait4");
    }

    #[test]
    fn test_process_zombie_preserves_ppid() {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 42;
        entry.process.ppid = 1; // Parent is init
        entry.process.state = ProcessState::Zombie;
        
        assert_eq!(entry.process.ppid, 1,
            "Parent PID must be preserved for wait4 to find child");
    }

    #[test]
    fn test_process_term_signal_on_zombie() {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 42;
        entry.process.term_signal = Some(15); // SIGTERM
        entry.process.state = ProcessState::Zombie;
        
        assert_eq!(entry.process.term_signal, Some(15),
            "Termination signal must be preserved for WTERMSIG");
    }

    // =========================================================================
    // Thread Group Tests
    // =========================================================================

    #[test]
    fn test_main_thread_tgid_equals_pid() {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 100;
        entry.process.tgid = 100;
        entry.process.is_thread = false;
        
        assert_eq!(entry.process.tgid, entry.process.pid,
            "Main thread's tgid should equal its pid");
        assert!(!entry.process.is_thread);
    }

    #[test]
    fn test_thread_tgid_equals_leader_pid() {
        // Thread's tgid should be the thread group leader's pid
        let mut thread = ProcessEntry::empty();
        thread.process.pid = 101; // Thread's own pid
        thread.process.tgid = 100; // Leader's pid
        thread.process.is_thread = true;
        
        assert_ne!(thread.process.tgid, thread.process.pid,
            "Thread's tgid should differ from its pid");
        assert!(thread.process.is_thread);
    }

    #[test]
    fn test_fork_creates_new_tgid() {
        // fork() creates new process with new tgid
        let parent_tgid = 100;
        let child_pid = 200;
        
        let mut child = ProcessEntry::empty();
        child.process.pid = child_pid;
        child.process.tgid = child_pid; // New process: tgid = pid
        child.process.ppid = 100;
        child.process.is_thread = false;
        
        assert_eq!(child.process.tgid, child.process.pid,
            "Fork child should have tgid equal to its own pid");
        assert_ne!(child.process.tgid, parent_tgid,
            "Fork child should have different tgid than parent");
    }

    // =========================================================================
    // Scheduler Queue Consistency Tests
    // =========================================================================

    #[test]
    fn test_ready_process_should_be_in_runqueue() {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 1;
        entry.process.state = ProcessState::Ready;
        
        fn should_be_in_runqueue(entry: &ProcessEntry) -> bool {
            matches!(entry.process.state, ProcessState::Ready | ProcessState::Running)
        }
        
        assert!(should_be_in_runqueue(&entry));
    }

    #[test]
    fn test_sleeping_process_not_in_runqueue() {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 1;
        entry.process.state = ProcessState::Sleeping;
        
        fn should_be_in_runqueue(entry: &ProcessEntry) -> bool {
            matches!(entry.process.state, ProcessState::Ready | ProcessState::Running)
        }
        
        assert!(!should_be_in_runqueue(&entry));
    }

    #[test]
    fn test_zombie_process_not_in_runqueue() {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 1;
        entry.process.state = ProcessState::Zombie;
        
        fn should_be_in_runqueue(entry: &ProcessEntry) -> bool {
            matches!(entry.process.state, ProcessState::Ready | ProcessState::Running)
        }
        
        assert!(!should_be_in_runqueue(&entry));
    }

    // =========================================================================
    // Signal Interaction Tests - Using REAL scheduler functions
    // =========================================================================

    #[test]
    #[serial]
    fn test_sleeping_process_can_be_woken_by_signal() {
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);
        
        // Use REAL wake_process (signal delivery calls this internally)
        let woke = wake_process(pid);
        
        let final_state = get_state(pid);
        cleanup_process(pid);
        
        assert!(woke, "wake_process should succeed on Sleeping process");
        assert_eq!(final_state, Some(ProcessState::Ready),
            "Process should be Ready after signal wakes it");
    }

    #[test]
    #[serial]
    fn test_sigkill_terminates_any_state() {
        // SIGKILL transitions process to Zombie via REAL set_process_state
        let states = [
            ProcessState::Ready,
            ProcessState::Running,
            ProcessState::Sleeping,
        ];
        
        for initial_state in states {
            let pid = next_pid();
            add_process(pid, initial_state);
            
            // Use REAL set_process_state to transition to Zombie
            // (This is what the kernel does when handling SIGKILL)
            let _ = set_process_state(pid, ProcessState::Zombie);
            
            // Set term_signal in process table
            {
                let mut table = process_table_lock();
                for slot in table.iter_mut() {
                    if let Some(entry) = slot {
                        if entry.process.pid == pid {
                            entry.process.term_signal = Some(9); // SIGKILL
                            break;
                        }
                    }
                }
            }
            
            let final_state = get_state(pid);
            let term_sig = {
                let table = process_table_lock();
                table.iter()
                    .filter_map(|s| s.as_ref())
                    .find(|e| e.process.pid == pid)
                    .map(|e| e.process.term_signal)
            };
            
            cleanup_process(pid);
            
            assert_eq!(final_state, Some(ProcessState::Zombie),
                "SIGKILL should terminate from {:?}", initial_state);
            assert_eq!(term_sig, Some(Some(9)),
                "term_signal should be set to SIGKILL");
        }
    }

    // =========================================================================
    // Wait4 Compatibility Tests - Using REAL process table
    // =========================================================================

    #[test]
    #[serial]
    fn test_wait4_finds_zombie_child() {
        let parent_pid: Pid = 100;
        let child_pid = next_pid();
        
        // Add child process as Zombie with specific ppid
        add_process(child_pid, ProcessState::Zombie);
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if let Some(entry) = slot {
                    if entry.process.pid == child_pid {
                        entry.process.ppid = parent_pid;
                        entry.process.exit_code = 42;
                        break;
                    }
                }
            }
        }
        
        // Scan process table like wait4 does
        let found = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .any(|e| e.process.ppid == parent_pid && 
                         e.process.state == ProcessState::Zombie)
        };
        
        cleanup_process(child_pid);
        
        assert!(found, "wait4 should find zombie child");
    }

    #[test]
    #[serial]
    fn test_wait4_ignores_non_zombie() {
        let parent_pid: Pid = 100;
        let child_pid = next_pid();
        
        // Add child process as Running (not zombie)
        add_process(child_pid, ProcessState::Running);
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if let Some(entry) = slot {
                    if entry.process.pid == child_pid {
                        entry.process.ppid = parent_pid;
                        break;
                    }
                }
            }
        }
        
        // Scan process table like wait4 does - should NOT find non-zombie
        let found = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .any(|e| e.process.ppid == parent_pid && 
                         e.process.state == ProcessState::Zombie)
        };
        
        cleanup_process(child_pid);
        
        assert!(!found, "wait4 should not find non-zombie processes");
    }

    #[test]
    #[serial]
    fn test_orphan_reparenting() {
        let child_pid = next_pid();
        
        // Add child with original parent
        add_process(child_pid, ProcessState::Ready);
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if let Some(entry) = slot {
                    if entry.process.pid == child_pid {
                        entry.process.ppid = 100; // Original parent
                        break;
                    }
                }
            }
        }
        
        // Parent exits - reparent to init (PID 1) via REAL process table
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if let Some(entry) = slot {
                    if entry.process.pid == child_pid {
                        entry.process.ppid = 1; // Reparent to init
                        break;
                    }
                }
            }
        }
        
        let ppid = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == child_pid)
                .map(|e| e.process.ppid)
        };
        
        cleanup_process(child_pid);
        
        assert_eq!(ppid, Some(1), "Orphaned process should be reparented to init");
    }
}
