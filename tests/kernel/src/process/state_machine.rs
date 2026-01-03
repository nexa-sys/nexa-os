//! Process State Machine Tests
//!
//! Tests for process state transitions using the REAL kernel scheduler.
//! These tests verify that state transitions follow POSIX semantics.

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Context};
    use crate::scheduler::{
        add_process, remove_process, set_process_state, wake_process,
        get_process, process_table_lock,
    };
    use serial_test::serial;

    /// Helper to get process state by PID
    fn get_process_state(pid: u64) -> Option<ProcessState> {
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

    /// Helper to clear test processes from the table
    fn cleanup_test_pids(pids: &[u64]) {
        for &pid in pids {
            let _ = remove_process(pid);
        }
    }

    /// Helper to create a minimal test process
    fn create_test_process(pid: u64) -> Process {
        Process {
            pid,
            ppid: 1,
            tgid: pid,
            state: ProcessState::Ready,
            entry_point: 0x1000000,
            stack_top: 0x1A00000,
            heap_start: 0x1200000,
            heap_end: 0x1200000,
            signal_state: crate::ipc::signal::SignalState::new(),
            context: Context::zero(),
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
        }
    }

    // =========================================================================
    // Basic State Transition Tests (using REAL kernel code)
    // =========================================================================

    #[test]
    #[serial]
    fn test_initial_state() {
        let test_pids = [1100u64];
        cleanup_test_pids(&test_pids);
        
        let proc = create_test_process(1100);
        assert_eq!(proc.state, ProcessState::Ready);
        
        add_process(proc, 20).unwrap();
        
        // Verify process is in the scheduler with Ready state
        let state = get_process_state(1100);
        assert_eq!(state, Some(ProcessState::Ready));
        
        cleanup_test_pids(&test_pids);
    }

    #[test]
    #[serial]
    fn test_ready_to_sleeping() {
        let test_pids = [1101u64];
        cleanup_test_pids(&test_pids);
        
        let proc = create_test_process(1101);
        add_process(proc, 20).unwrap();
        
        // Transition Ready -> Sleeping
        set_process_state(1101, ProcessState::Sleeping).unwrap();
        
        let state = get_process_state(1101);
        assert_eq!(state, Some(ProcessState::Sleeping));
        
        cleanup_test_pids(&test_pids);
    }

    #[test]
    #[serial]
    fn test_sleeping_to_ready_via_wake() {
        let test_pids = [1102u64];
        cleanup_test_pids(&test_pids);
        
        let proc = create_test_process(1102);
        add_process(proc, 20).unwrap();
        
        // Put to sleep
        set_process_state(1102, ProcessState::Sleeping).unwrap();
        assert_eq!(get_process_state(1102), Some(ProcessState::Sleeping));
        
        // Wake up - should transition to Ready
        let woke = wake_process(1102);
        assert!(woke, "wake_process should return true for sleeping process");
        
        let state = get_process_state(1102);
        assert_eq!(state, Some(ProcessState::Ready));
        
        cleanup_test_pids(&test_pids);
    }

    #[test]
    #[serial]
    fn test_wake_pending_prevents_sleep() {
        let test_pids = [1103u64];
        cleanup_test_pids(&test_pids);
        
        let proc = create_test_process(1103);
        add_process(proc, 20).unwrap();
        
        // Process is Ready, wake_process sets wake_pending
        let woke = wake_process(1103);
        assert!(!woke, "wake_process on Ready process returns false but sets pending");
        
        // Now try to sleep - should be blocked by wake_pending
        set_process_state(1103, ProcessState::Sleeping).unwrap();
        
        // Process should still be Ready due to wake_pending
        let state = get_process_state(1103);
        assert_eq!(state, Some(ProcessState::Ready), 
            "Process should stay Ready when wake_pending is set");
        
        cleanup_test_pids(&test_pids);
    }

    #[test]
    #[serial]
    fn test_ready_to_zombie() {
        let test_pids = [1104u64];
        cleanup_test_pids(&test_pids);
        
        let proc = create_test_process(1104);
        add_process(proc, 20).unwrap();
        
        // Transition to Zombie (process exits)
        set_process_state(1104, ProcessState::Zombie).unwrap();
        
        let state = get_process_state(1104);
        assert_eq!(state, Some(ProcessState::Zombie));
        
        cleanup_test_pids(&test_pids);
    }

    #[test]
    #[serial]
    fn test_zombie_is_terminal() {
        let test_pids = [1105u64];
        cleanup_test_pids(&test_pids);
        
        let proc = create_test_process(1105);
        add_process(proc, 20).unwrap();
        
        // Become zombie
        set_process_state(1105, ProcessState::Zombie).unwrap();
        
        // Try to transition out of Zombie - should be ignored
        set_process_state(1105, ProcessState::Ready).unwrap();
        assert_eq!(get_process_state(1105), Some(ProcessState::Zombie));
        
        set_process_state(1105, ProcessState::Running).unwrap();
        assert_eq!(get_process_state(1105), Some(ProcessState::Zombie));
        
        set_process_state(1105, ProcessState::Sleeping).unwrap();
        assert_eq!(get_process_state(1105), Some(ProcessState::Zombie));
        
        cleanup_test_pids(&test_pids);
    }

    #[test]
    #[serial]
    fn test_cannot_wake_zombie() {
        let test_pids = [1106u64];
        cleanup_test_pids(&test_pids);
        
        let proc = create_test_process(1106);
        add_process(proc, 20).unwrap();
        
        set_process_state(1106, ProcessState::Zombie).unwrap();
        
        // Try to wake a zombie
        let woke = wake_process(1106);
        assert!(!woke, "Should not be able to wake a zombie");
        
        assert_eq!(get_process_state(1106), Some(ProcessState::Zombie));
        
        cleanup_test_pids(&test_pids);
    }

    // =========================================================================
    // Complex State Sequence Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_io_bound_lifecycle() {
        let test_pids = [1110u64];
        cleanup_test_pids(&test_pids);
        
        let proc = create_test_process(1110);
        add_process(proc, 20).unwrap();
        
        // IO-bound process: frequent sleep/wake cycles
        for i in 0..5 {
            // Sleep (waiting for I/O)
            set_process_state(1110, ProcessState::Sleeping).unwrap();
            assert_eq!(get_process_state(1110), Some(ProcessState::Sleeping),
                "Iteration {}: should be sleeping", i);
            
            // Wake (I/O completed)
            wake_process(1110);
            assert_eq!(get_process_state(1110), Some(ProcessState::Ready),
                "Iteration {}: should be ready after wake", i);
        }
        
        // Finally exit
        set_process_state(1110, ProcessState::Zombie).unwrap();
        assert_eq!(get_process_state(1110), Some(ProcessState::Zombie));
        
        cleanup_test_pids(&test_pids);
    }

    #[test]
    #[serial]
    fn test_multiple_processes_independent_states() {
        let test_pids: Vec<u64> = (1120..1125).collect();
        cleanup_test_pids(&test_pids);
        
        // Add multiple processes
        for &pid in &test_pids {
            let proc = create_test_process(pid);
            add_process(proc, 20).unwrap();
        }
        
        // Set different states for each
        set_process_state(1120, ProcessState::Ready).unwrap();
        set_process_state(1121, ProcessState::Sleeping).unwrap();
        set_process_state(1122, ProcessState::Running).unwrap();
        set_process_state(1123, ProcessState::Zombie).unwrap();
        set_process_state(1124, ProcessState::Sleeping).unwrap();
        
        // Verify each has its own state
        assert_eq!(get_process_state(1120), Some(ProcessState::Ready));
        assert_eq!(get_process_state(1121), Some(ProcessState::Sleeping));
        assert_eq!(get_process_state(1122), Some(ProcessState::Running));
        assert_eq!(get_process_state(1123), Some(ProcessState::Zombie));
        assert_eq!(get_process_state(1124), Some(ProcessState::Sleeping));
        
        // Wake sleeping processes
        wake_process(1121);
        wake_process(1124);
        
        assert_eq!(get_process_state(1121), Some(ProcessState::Ready));
        assert_eq!(get_process_state(1124), Some(ProcessState::Ready));
        
        // Zombie should still be zombie
        assert_eq!(get_process_state(1123), Some(ProcessState::Zombie));
        
        cleanup_test_pids(&test_pids);
    }

    // =========================================================================
    // Wait Status Encoding Tests (pure algorithm, no kernel state needed)
    // =========================================================================

    #[test]
    fn test_wait_status_encoding() {
        // POSIX wait status macros
        fn wifexited(status: i32) -> bool {
            (status & 0x7F) == 0
        }
        
        fn wexitstatus(status: i32) -> i32 {
            (status >> 8) & 0xFF
        }
        
        fn wifsignaled(status: i32) -> bool {
            ((status & 0x7F) + 1) >> 1 > 0
        }
        
        fn wtermsig(status: i32) -> i32 {
            status & 0x7F
        }
        
        // Normal exit with code 42
        let status = 42 << 8;
        assert!(wifexited(status));
        assert_eq!(wexitstatus(status), 42);
        
        // Killed by signal 9
        let status = 9;
        assert!(wifsignaled(status));
        assert_eq!(wtermsig(status), 9);
    }

    #[test]
    #[serial]
    fn test_exit_code_preservation() {
        let test_pids: Vec<u64> = (1200..1206).collect();
        cleanup_test_pids(&test_pids);
        
        // Test various exit codes are preserved in the Process struct
        for (i, code) in [0i32, 1, 42, 127, 128, 255].iter().enumerate() {
            let pid = 1200 + i as u64;
            let mut proc = create_test_process(pid);
            proc.exit_code = *code;
            
            add_process(proc, 20).unwrap();
            
            // Verify exit code is stored
            let table = process_table_lock();
            let entry = table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid);
            
            assert!(entry.is_some(), "Process {} should exist", pid);
            assert_eq!(entry.unwrap().process.exit_code, *code,
                "Exit code {} should be preserved", code);
        }
        
        cleanup_test_pids(&test_pids);
    }

    #[test]
    #[serial]
    fn test_process_removal() {
        let test_pids = [1300u64];
        cleanup_test_pids(&test_pids);
        
        let proc = create_test_process(1300);
        add_process(proc, 20).unwrap();
        
        assert!(get_process_state(1300).is_some());
        
        // Remove the process
        remove_process(1300).unwrap();
        
        // Should no longer exist
        assert!(get_process_state(1300).is_none());
    }
}
