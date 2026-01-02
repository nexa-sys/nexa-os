//! Comprehensive IPC and signal handling tests
//!
//! Tests signal delivery, pipes, socketpairs, and inter-process communication
//! using the REAL kernel implementations from src/ipc/.

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use crate::ipc::signal::{
        SignalState, SignalAction, NSIG,
        SIGHUP, SIGINT, SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGBUS, SIGFPE,
        SIGKILL, SIGUSR1, SIGSEGV, SIGUSR2, SIGPIPE, SIGALRM, SIGTERM,
        SIGCHLD, SIGCONT, SIGSTOP, SIGTSTP,
        default_signal_action,
    };
    use crate::ipc::pipe::{
        create_pipe, pipe_read, pipe_write, close_pipe_read, close_pipe_write,
        create_socketpair, socketpair_read, socketpair_write, close_socketpair_end,
        socketpair_has_data,
    };

    // =========================================================================
    // Signal Number Tests (using kernel constants)
    // =========================================================================

    #[test]
    fn test_standard_signal_numbers() {
        // Verify POSIX standard signal numbers from kernel
        assert_eq!(SIGHUP, 1);
        assert_eq!(SIGINT, 2);
        assert_eq!(SIGQUIT, 3);
        assert_eq!(SIGILL, 4);
        assert_eq!(SIGTRAP, 5);
        assert_eq!(SIGABRT, 6);
        assert_eq!(SIGBUS, 7);
        assert_eq!(SIGFPE, 8);
        assert_eq!(SIGKILL, 9);
        assert_eq!(SIGUSR1, 10);
        assert_eq!(SIGSEGV, 11);
        assert_eq!(SIGUSR2, 12);
        assert_eq!(SIGPIPE, 13);
        assert_eq!(SIGALRM, 14);
        assert_eq!(SIGTERM, 15);
    }

    #[test]
    fn test_signal_range() {
        // Kernel signal range
        assert_eq!(NSIG, 32);
        assert!(SIGTERM < NSIG as u32);
        assert!(SIGKILL < NSIG as u32);
    }

    #[test]
    fn test_fatal_signals() {
        // SIGKILL cannot be caught - verify it's signal 9
        assert_eq!(SIGKILL, 9);
        assert_eq!(SIGTERM, 15);
        assert_ne!(SIGKILL, SIGTERM);
    }

    #[test]
    fn test_job_control_signals() {
        // Job control signals
        assert_eq!(SIGSTOP, 19);
        assert_eq!(SIGCONT, 18);
        assert_eq!(SIGTSTP, 20);
        assert_ne!(SIGSTOP, SIGCONT);
    }

    // =========================================================================
    // Signal State Tests (using kernel SignalState)
    // =========================================================================

    #[test]
    fn test_signal_state_creation() {
        let state = SignalState::new();
        // New state should have no pending signals
        assert!(state.has_pending_signal().is_none());
    }

    #[test]
    fn test_signal_send_and_pending() {
        let mut state = SignalState::new();
        
        // Send SIGUSR1
        let result = state.send_signal(SIGUSR1);
        assert!(result.is_ok());
        
        // Should be pending
        assert_eq!(state.has_pending_signal(), Some(SIGUSR1));
    }

    #[test]
    fn test_signal_clear() {
        let mut state = SignalState::new();
        
        state.send_signal(SIGUSR1).unwrap();
        assert!(state.has_pending_signal().is_some());
        
        state.clear_signal(SIGUSR1);
        assert!(state.has_pending_signal().is_none());
    }

    #[test]
    fn test_signal_blocking() {
        let mut state = SignalState::new();
        
        // Block SIGUSR1 then send it
        state.block_signal(SIGUSR1);
        state.send_signal(SIGUSR1).unwrap();
        
        // Signal is pending but blocked - should not be deliverable
        assert!(state.has_pending_signal().is_none());
        
        // Unblock it
        state.unblock_signal(SIGUSR1);
        
        // Now should be deliverable
        assert_eq!(state.has_pending_signal(), Some(SIGUSR1));
    }

    #[test]
    fn test_sigkill_cannot_be_blocked() {
        let mut state = SignalState::new();
        
        // Try to block SIGKILL (should be ignored per POSIX)
        state.block_signal(SIGKILL);
        state.send_signal(SIGKILL).unwrap();
        
        // SIGKILL should still be deliverable
        assert_eq!(state.has_pending_signal(), Some(SIGKILL));
    }

    #[test]
    fn test_sigstop_cannot_be_blocked() {
        let mut state = SignalState::new();
        
        // Try to block SIGSTOP (should be ignored per POSIX)
        state.block_signal(SIGSTOP);
        state.send_signal(SIGSTOP).unwrap();
        
        // SIGSTOP should still be deliverable
        assert_eq!(state.has_pending_signal(), Some(SIGSTOP));
    }

    #[test]
    fn test_signal_action_cannot_change_sigkill() {
        let mut state = SignalState::new();
        
        // Cannot ignore SIGKILL
        let result = state.set_action(SIGKILL, SignalAction::Ignore);
        assert!(result.is_err());
        
        // Cannot set handler for SIGKILL
        let result = state.set_action(SIGKILL, SignalAction::Handler(0x1000));
        assert!(result.is_err());
    }

    #[test]
    fn test_signal_action_cannot_change_sigstop() {
        let mut state = SignalState::new();
        
        // Cannot ignore SIGSTOP
        let result = state.set_action(SIGSTOP, SignalAction::Ignore);
        assert!(result.is_err());
        
        // Cannot set handler for SIGSTOP
        let result = state.set_action(SIGSTOP, SignalAction::Handler(0x1000));
        assert!(result.is_err());
    }

    #[test]
    fn test_signal_action_change() {
        let mut state = SignalState::new();
        
        // Set SIGTERM to ignore
        let old = state.set_action(SIGTERM, SignalAction::Ignore).unwrap();
        assert_eq!(old, SignalAction::Default);
        
        // Verify change
        let current = state.get_action(SIGTERM).unwrap();
        assert_eq!(current, SignalAction::Ignore);
    }

    #[test]
    fn test_signal_handler_registration() {
        let mut state = SignalState::new();
        
        let handler_addr: u64 = 0x400000;
        let old = state.set_action(SIGUSR1, SignalAction::Handler(handler_addr)).unwrap();
        assert_eq!(old, SignalAction::Default);
        
        let action = state.get_action(SIGUSR1).unwrap();
        assert_eq!(action, SignalAction::Handler(handler_addr));
    }

    #[test]
    fn test_signal_zero_invalid() {
        let mut state = SignalState::new();
        
        // Signal 0 should be rejected for send_signal
        let result = state.send_signal(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_signal_out_of_range() {
        let mut state = SignalState::new();
        
        // Signal >= NSIG should be rejected
        let result = state.send_signal(NSIG as u32);
        assert!(result.is_err());
        
        let result = state.send_signal(100);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_pending_signals() {
        let mut state = SignalState::new();
        
        // Send multiple signals
        state.send_signal(SIGTERM).unwrap();
        state.send_signal(SIGINT).unwrap();
        state.send_signal(SIGUSR1).unwrap();
        
        // Should have pending signals (lowest number first)
        let first = state.has_pending_signal();
        assert!(first.is_some());
        
        // Clear it and check next
        state.clear_signal(first.unwrap());
        let second = state.has_pending_signal();
        assert!(second.is_some());
        assert_ne!(first, second);
    }

    #[test]
    fn test_signal_reset_to_default() {
        let mut state = SignalState::new();
        
        // Set up some state
        state.send_signal(SIGUSR1).unwrap();
        state.set_action(SIGTERM, SignalAction::Ignore).unwrap();
        
        // Reset (simulates exec)
        state.reset_to_default();
        
        // Pending should be cleared
        assert!(state.has_pending_signal().is_none());
        
        // Action should be reset
        assert_eq!(state.get_action(SIGTERM).unwrap(), SignalAction::Default);
    }

    #[test]
    fn test_default_signal_actions() {
        // SIGCHLD default is ignore
        assert_eq!(default_signal_action(SIGCHLD), SignalAction::Ignore);
        
        // SIGCONT default is ignore
        assert_eq!(default_signal_action(SIGCONT), SignalAction::Ignore);
        
        // Most other signals default to terminate
        assert_eq!(default_signal_action(SIGTERM), SignalAction::Default);
    }

    // =========================================================================
    // Pipe Tests (using kernel pipe implementation)
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_creation() {
        let result = create_pipe();
        assert!(result.is_ok(), "Failed to create pipe");
        
        let (read_end, write_end) = result.unwrap();
        // Both ends should be valid indices
        assert!(read_end < 16); // MAX_PIPES
        assert!(write_end < 16);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_write_and_read() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Write some data
        let data = b"Hello, pipe!";
        let written = pipe_write(write_end, data).unwrap();
        assert_eq!(written, data.len());
        
        // Read it back
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, data.len());
        assert_eq!(&buffer[..read], data);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_empty_read() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Reading from empty pipe returns 0 bytes
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 0);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_close_write_end() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Write some data
        pipe_write(write_end, b"data").unwrap();
        
        // Close write end
        close_pipe_write(write_end).unwrap();
        
        // Read remaining data
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 4);
        
        // Further read should return EOF (0)
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 0);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
    }

    #[test]
    #[serial]
    fn test_pipe_close_read_end() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Close read end
        close_pipe_read(read_end).unwrap();
        
        // Writing should fail (broken pipe / SIGPIPE)
        let result = pipe_write(write_end, b"data");
        assert!(result.is_err());
        
        // Cleanup
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_invalid_id() {
        // Invalid pipe ID should fail
        let mut buffer = [0u8; 64];
        let result = pipe_read(999, &mut buffer);
        assert!(result.is_err());
        
        let result = pipe_write(999, b"data");
        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn test_pipe_multiple_writes() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Multiple writes
        pipe_write(write_end, b"Hello").unwrap();
        pipe_write(write_end, b" ").unwrap();
        pipe_write(write_end, b"World").unwrap();
        
        // Single read should get all data
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 11); // "Hello World"
        assert_eq!(&buffer[..read], b"Hello World");
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_partial_read() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        pipe_write(write_end, b"Hello World").unwrap();
        
        // Read only 5 bytes
        let mut buffer = [0u8; 5];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buffer[..read], b"Hello");
        
        // Read remaining
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 6);
        assert_eq!(&buffer[..read], b" World");
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Socketpair Tests (using kernel socketpair implementation)
    // =========================================================================

    #[test]
    fn test_socketpair_creation() {
        let result = create_socketpair();
        assert!(result.is_ok(), "Failed to create socketpair");
        
        let pair_id = result.unwrap();
        assert!(pair_id < 8); // MAX_SOCKETPAIRS
    }

    #[test]
    fn test_socketpair_bidirectional() {
        let pair_id = create_socketpair().unwrap();
        
        // Write from end 0, read from end 1
        socketpair_write(pair_id, 0, b"Hello from 0").unwrap();
        
        let mut buffer = [0u8; 64];
        let read = socketpair_read(pair_id, 1, &mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"Hello from 0");
        
        // Write from end 1, read from end 0
        socketpair_write(pair_id, 1, b"Hello from 1").unwrap();
        
        let read = socketpair_read(pair_id, 0, &mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"Hello from 1");
    }

    #[test]
    fn test_socketpair_empty_read() {
        let pair_id = create_socketpair().unwrap();
        
        let mut buffer = [0u8; 64];
        let read = socketpair_read(pair_id, 0, &mut buffer).unwrap();
        assert_eq!(read, 0);
    }

    #[test]
    fn test_socketpair_has_data() {
        let pair_id = create_socketpair().unwrap();
        
        // Initially no data
        assert!(!socketpair_has_data(pair_id, 0).unwrap());
        assert!(!socketpair_has_data(pair_id, 1).unwrap());
        
        // Write from end 0
        socketpair_write(pair_id, 0, b"data").unwrap();
        
        // End 1 should have data (from end 0)
        assert!(socketpair_has_data(pair_id, 1).unwrap());
        // End 0 should not have data (no one wrote to it)
        assert!(!socketpair_has_data(pair_id, 0).unwrap());
    }

    #[test]
    fn test_socketpair_close_one_end() {
        let pair_id = create_socketpair().unwrap();
        
        // Close end 0
        close_socketpair_end(pair_id, 0).unwrap();
        
        // Writing from end 0 should fail
        let result = socketpair_write(pair_id, 0, b"data");
        assert!(result.is_err());
        
        // Writing to end 0 (from end 1) should report SIGPIPE
        let result = socketpair_write(pair_id, 1, b"data");
        assert!(result.is_err());
    }

    #[test]
    fn test_socketpair_close_both_ends() {
        let pair_id = create_socketpair().unwrap();
        
        close_socketpair_end(pair_id, 0).unwrap();
        close_socketpair_end(pair_id, 1).unwrap();
        
        // Both operations should fail now
        let result = socketpair_read(pair_id, 0, &mut [0u8; 64]);
        assert!(result.is_err());
        
        let result = socketpair_write(pair_id, 1, b"data");
        assert!(result.is_err());
    }

    #[test]
    fn test_socketpair_invalid_id() {
        let result = socketpair_read(999, 0, &mut [0u8; 64]);
        assert!(result.is_err());
        
        let result = socketpair_write(999, 0, b"data");
        assert!(result.is_err());
        
        let result = socketpair_has_data(999, 0);
        assert!(result.is_err());
    }

    // =========================================================================
    // Integration Tests (signals + pipes)
    // =========================================================================

    #[test]
    fn test_sigpipe_signal_number() {
        // SIGPIPE should be 13 per POSIX
        assert_eq!(SIGPIPE, 13);
        
        // Writing to closed pipe would generate SIGPIPE
        let mut state = SignalState::new();
        state.send_signal(SIGPIPE).unwrap();
        assert_eq!(state.has_pending_signal(), Some(SIGPIPE));
    }

    #[test]
    fn test_signal_mask_full_range() {
        let mut state = SignalState::new();
        
        // Send all valid signals (1 to NSIG-1)
        for sig in 1..NSIG as u32 {
            let _ = state.send_signal(sig);
        }
        
        // Should have at least one pending
        assert!(state.has_pending_signal().is_some());
    }

    #[test]
    fn test_signal_delivery_order() {
        let mut state = SignalState::new();
        
        // Send signals out of order
        state.send_signal(SIGTERM).unwrap(); // 15
        state.send_signal(SIGINT).unwrap();  // 2
        state.send_signal(SIGUSR1).unwrap(); // 10
        
        // Lowest numbered signal should be delivered first
        let first = state.has_pending_signal().unwrap();
        assert_eq!(first, SIGINT); // 2 is lowest
    }
}
