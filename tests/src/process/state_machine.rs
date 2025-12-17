//! Process State Machine Tests
//!
//! Tests for process state transitions and invariants.
//! These tests verify that state transitions follow POSIX semantics.

#[cfg(test)]
mod tests {
    use crate::process::ProcessState;

    // =========================================================================
    // State Machine Definition
    // =========================================================================

    /// Simulates the process state machine
    struct ProcessStateMachine {
        state: ProcessState,
        exit_code: Option<i32>,
        term_signal: Option<u32>,
    }

    impl ProcessStateMachine {
        fn new() -> Self {
            Self {
                state: ProcessState::Ready,
                exit_code: None,
                term_signal: None,
            }
        }

        /// Transition to Running (scheduler picks this process)
        fn dispatch(&mut self) -> Result<(), &'static str> {
            match self.state {
                ProcessState::Ready => {
                    self.state = ProcessState::Running;
                    Ok(())
                }
                _ => Err("Can only dispatch Ready processes"),
            }
        }

        /// Transition to Ready (process yields or time slice expires)
        fn preempt(&mut self) -> Result<(), &'static str> {
            match self.state {
                ProcessState::Running => {
                    self.state = ProcessState::Ready;
                    Ok(())
                }
                _ => Err("Can only preempt Running processes"),
            }
        }

        /// Transition to Sleeping (process waits for event)
        fn sleep(&mut self) -> Result<(), &'static str> {
            match self.state {
                ProcessState::Running => {
                    self.state = ProcessState::Sleeping;
                    Ok(())
                }
                _ => Err("Can only sleep Running processes"),
            }
        }

        /// Transition from Sleeping to Ready (event occurred)
        fn wake(&mut self) -> Result<(), &'static str> {
            match self.state {
                ProcessState::Sleeping => {
                    self.state = ProcessState::Ready;
                    Ok(())
                }
                _ => Err("Can only wake Sleeping processes"),
            }
        }

        /// Transition to Zombie (process exits)
        fn exit(&mut self, code: i32) -> Result<(), &'static str> {
            match self.state {
                ProcessState::Running => {
                    self.state = ProcessState::Zombie;
                    self.exit_code = Some(code);
                    Ok(())
                }
                _ => Err("Can only exit Running processes"),
            }
        }

        /// Transition to Zombie via signal
        fn kill(&mut self, signal: u32) -> Result<(), &'static str> {
            match self.state {
                ProcessState::Running | ProcessState::Sleeping | ProcessState::Ready => {
                    self.state = ProcessState::Zombie;
                    self.term_signal = Some(signal);
                    Ok(())
                }
                ProcessState::Zombie => Err("Process is already dead"),
            }
        }

        /// Reap zombie (parent calls wait)
        fn reap(&mut self) -> Result<(Option<i32>, Option<u32>), &'static str> {
            match self.state {
                ProcessState::Zombie => {
                    let result = (self.exit_code, self.term_signal);
                    // Process is now truly gone
                    Ok(result)
                }
                _ => Err("Can only reap Zombie processes"),
            }
        }
    }

    // =========================================================================
    // Basic State Transition Tests
    // =========================================================================

    #[test]
    fn test_initial_state() {
        let proc = ProcessStateMachine::new();
        assert_eq!(proc.state, ProcessState::Ready);
    }

    #[test]
    fn test_ready_to_running() {
        let mut proc = ProcessStateMachine::new();
        assert!(proc.dispatch().is_ok());
        assert_eq!(proc.state, ProcessState::Running);
    }

    #[test]
    fn test_running_to_ready() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        assert!(proc.preempt().is_ok());
        assert_eq!(proc.state, ProcessState::Ready);
    }

    #[test]
    fn test_running_to_sleeping() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        assert!(proc.sleep().is_ok());
        assert_eq!(proc.state, ProcessState::Sleeping);
    }

    #[test]
    fn test_sleeping_to_ready() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        proc.sleep().unwrap();
        assert!(proc.wake().is_ok());
        assert_eq!(proc.state, ProcessState::Ready);
    }

    #[test]
    fn test_running_to_zombie() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        assert!(proc.exit(0).is_ok());
        assert_eq!(proc.state, ProcessState::Zombie);
        assert_eq!(proc.exit_code, Some(0));
    }

    #[test]
    fn test_zombie_reap() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        proc.exit(42).unwrap();
        
        let result = proc.reap();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), (Some(42), None));
    }

    // =========================================================================
    // Invalid State Transition Tests
    // =========================================================================

    #[test]
    fn test_cannot_dispatch_running() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        assert!(proc.dispatch().is_err());
    }

    #[test]
    fn test_cannot_dispatch_sleeping() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        proc.sleep().unwrap();
        assert!(proc.dispatch().is_err());
    }

    #[test]
    fn test_cannot_dispatch_zombie() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        proc.exit(0).unwrap();
        assert!(proc.dispatch().is_err());
    }

    #[test]
    fn test_cannot_preempt_ready() {
        let mut proc = ProcessStateMachine::new();
        assert!(proc.preempt().is_err());
    }

    #[test]
    fn test_cannot_preempt_sleeping() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        proc.sleep().unwrap();
        assert!(proc.preempt().is_err());
    }

    #[test]
    fn test_cannot_sleep_ready() {
        let mut proc = ProcessStateMachine::new();
        assert!(proc.sleep().is_err());
    }

    #[test]
    fn test_cannot_wake_ready() {
        let mut proc = ProcessStateMachine::new();
        assert!(proc.wake().is_err());
    }

    #[test]
    fn test_cannot_wake_running() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        assert!(proc.wake().is_err());
    }

    #[test]
    fn test_cannot_exit_sleeping() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        proc.sleep().unwrap();
        assert!(proc.exit(0).is_err());
    }

    #[test]
    fn test_cannot_reap_living() {
        let mut proc = ProcessStateMachine::new();
        assert!(proc.reap().is_err());
        
        proc.dispatch().unwrap();
        assert!(proc.reap().is_err());
    }

    // =========================================================================
    // Signal (Kill) Transition Tests
    // =========================================================================

    #[test]
    fn test_kill_ready() {
        let mut proc = ProcessStateMachine::new();
        assert!(proc.kill(9).is_ok()); // SIGKILL
        assert_eq!(proc.state, ProcessState::Zombie);
        assert_eq!(proc.term_signal, Some(9));
    }

    #[test]
    fn test_kill_running() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        assert!(proc.kill(15).is_ok()); // SIGTERM
        assert_eq!(proc.state, ProcessState::Zombie);
    }

    #[test]
    fn test_kill_sleeping() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        proc.sleep().unwrap();
        assert!(proc.kill(9).is_ok());
        assert_eq!(proc.state, ProcessState::Zombie);
    }

    #[test]
    fn test_cannot_kill_zombie() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        proc.exit(0).unwrap();
        assert!(proc.kill(9).is_err());
    }

    // =========================================================================
    // Complex State Sequence Tests
    // =========================================================================

    #[test]
    fn test_typical_lifecycle() {
        let mut proc = ProcessStateMachine::new();
        
        // Ready -> Running -> Ready (preempted)
        proc.dispatch().unwrap();
        proc.preempt().unwrap();
        assert_eq!(proc.state, ProcessState::Ready);
        
        // Ready -> Running -> Sleeping -> Ready (woken)
        proc.dispatch().unwrap();
        proc.sleep().unwrap();
        proc.wake().unwrap();
        assert_eq!(proc.state, ProcessState::Ready);
        
        // Ready -> Running -> Zombie
        proc.dispatch().unwrap();
        proc.exit(0).unwrap();
        assert_eq!(proc.state, ProcessState::Zombie);
    }

    #[test]
    fn test_io_bound_lifecycle() {
        let mut proc = ProcessStateMachine::new();
        
        // IO-bound process: frequent sleep/wake cycles
        for _ in 0..5 {
            proc.dispatch().unwrap();
            proc.sleep().unwrap();
            proc.wake().unwrap();
        }
        
        proc.dispatch().unwrap();
        proc.exit(0).unwrap();
        
        let (code, _) = proc.reap().unwrap();
        assert_eq!(code, Some(0));
    }

    #[test]
    fn test_cpu_bound_lifecycle() {
        let mut proc = ProcessStateMachine::new();
        
        // CPU-bound process: frequent preemptions
        for _ in 0..10 {
            proc.dispatch().unwrap();
            proc.preempt().unwrap();
        }
        
        proc.dispatch().unwrap();
        proc.exit(0).unwrap();
        assert_eq!(proc.state, ProcessState::Zombie);
    }

    // =========================================================================
    // Concurrent State Access Simulation
    // =========================================================================

    #[test]
    fn test_state_consistency() {
        // Simulate what might happen with concurrent access
        use std::sync::{Arc, Mutex};
        use std::thread;

        let state = Arc::new(Mutex::new(ProcessState::Ready));
        
        // Multiple "threads" trying to check and modify state
        let state1 = state.clone();
        let state2 = state.clone();
        
        let handle1 = thread::spawn(move || {
            let mut s = state1.lock().unwrap();
            if *s == ProcessState::Ready {
                *s = ProcessState::Running;
            }
        });
        
        let handle2 = thread::spawn(move || {
            let mut s = state2.lock().unwrap();
            if *s == ProcessState::Ready {
                *s = ProcessState::Running;
            }
        });
        
        handle1.join().unwrap();
        handle2.join().unwrap();
        
        // Final state should be Running (one of them succeeded)
        assert_eq!(*state.lock().unwrap(), ProcessState::Running);
    }

    // =========================================================================
    // Wait Status Encoding Tests
    // =========================================================================

    #[test]
    fn test_wait_status_encoding() {
        // POSIX wait status macros simulation
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
    fn test_exit_code_preservation() {
        let mut proc = ProcessStateMachine::new();
        proc.dispatch().unwrap();
        
        // Various exit codes
        for code in [0, 1, 42, 127, 128, 255, -1] {
            let mut p = ProcessStateMachine::new();
            p.dispatch().unwrap();
            p.exit(code).unwrap();
            
            let (exit_code, _) = p.reap().unwrap();
            assert_eq!(exit_code, Some(code));
        }
    }
}
