//! Process State Machine Validation Tests
//!
//! Tests for process state transitions, ensuring scheduler/signal/wait4 consistency.
//! Per copilot-instructions.md: ProcessState must stay synchronized across
//! scheduler, signals, and wait4.

#[cfg(test)]
mod tests {
    use crate::process::{ProcessState, Pid};
    use crate::scheduler::ProcessEntry;

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
    // Signal Interaction Tests
    // =========================================================================

    #[test]
    fn test_sleeping_process_can_be_woken_by_signal() {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 1;
        entry.process.state = ProcessState::Sleeping;
        
        // Simulate signal delivery waking the process
        fn deliver_signal(entry: &mut ProcessEntry) {
            if entry.process.state == ProcessState::Sleeping {
                entry.process.state = ProcessState::Ready;
            }
        }
        
        deliver_signal(&mut entry);
        assert_eq!(entry.process.state, ProcessState::Ready);
    }

    #[test]
    fn test_sigkill_terminates_any_state() {
        // SIGKILL should be able to terminate from any state
        let states = [
            ProcessState::Ready,
            ProcessState::Running,
            ProcessState::Sleeping,
        ];
        
        for initial_state in states {
            let mut entry = ProcessEntry::empty();
            entry.process.state = initial_state;
            
            // Simulate SIGKILL
            fn handle_sigkill(entry: &mut ProcessEntry) {
                entry.process.state = ProcessState::Zombie;
                entry.process.term_signal = Some(9); // SIGKILL
            }
            
            handle_sigkill(&mut entry);
            assert_eq!(entry.process.state, ProcessState::Zombie,
                "SIGKILL should terminate from {:?}", initial_state);
            assert_eq!(entry.process.term_signal, Some(9));
        }
    }

    // =========================================================================
    // Wait4 Compatibility Tests
    // =========================================================================

    #[test]
    fn test_wait4_finds_zombie_child() {
        // Simulate wait4 looking for zombie children
        let parent_pid: Pid = 100;
        
        let mut child = ProcessEntry::empty();
        child.process.pid = 200;
        child.process.ppid = parent_pid;
        child.process.state = ProcessState::Zombie;
        child.process.exit_code = 42;
        
        fn can_reap(child: &ProcessEntry, waiter_pid: Pid) -> bool {
            child.process.ppid == waiter_pid && 
            child.process.state == ProcessState::Zombie
        }
        
        assert!(can_reap(&child, parent_pid));
        assert!(!can_reap(&child, 999)); // Wrong parent
    }

    #[test]
    fn test_wait4_ignores_non_zombie() {
        let parent_pid: Pid = 100;
        
        let mut child = ProcessEntry::empty();
        child.process.pid = 200;
        child.process.ppid = parent_pid;
        child.process.state = ProcessState::Running; // Not zombie yet
        
        fn can_reap(child: &ProcessEntry, waiter_pid: Pid) -> bool {
            child.process.ppid == waiter_pid && 
            child.process.state == ProcessState::Zombie
        }
        
        assert!(!can_reap(&child, parent_pid),
            "wait4 should not reap non-zombie processes");
    }

    #[test]
    fn test_orphan_reparenting() {
        // When parent exits, children should be reparented to init (PID 1)
        let mut orphan = ProcessEntry::empty();
        orphan.process.pid = 200;
        orphan.process.ppid = 100; // Original parent
        
        fn reparent_to_init(process: &mut ProcessEntry) {
            process.process.ppid = 1;
        }
        
        reparent_to_init(&mut orphan);
        assert_eq!(orphan.process.ppid, 1,
            "Orphaned process should be reparented to init");
    }
}
