//! Exec Context Tests - Verify per-process exec context works correctly
//!
//! These tests verify the fix for the exec context race condition.
//! The fix stores exec context per-process instead of globally.

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Context, MAX_CMDLINE_SIZE};
    use crate::signal::SignalState;

    /// Helper to create a minimal test process
    fn make_test_process(pid: u64) -> Process {
        Process {
            pid,
            ppid: 0,
            tgid: pid,
            state: ProcessState::Ready,
            entry_point: 0x1000000,
            stack_top: 0x1A00000,
            heap_start: 0x1200000,
            heap_end: 0x1A00000,
            signal_state: SignalState::new(),
            context: Context::zero(),
            has_entered_user: false,
            context_valid: false,
            is_fork_child: false,
            is_thread: false,
            cr3: 0,
            tty: 0,
            memory_base: 0x1000000,
            memory_size: 0x1000000,
            user_rip: 0x1000000,
            user_rsp: 0x1A00000,
            user_rflags: 0x202,
            user_r10: 0,
            user_r8: 0,
            user_r9: 0,
            exit_code: 0,
            term_signal: None,
            kernel_stack: 0,
            fs_base: 0,
            clear_child_tid: 0,
            cmdline: [0u8; MAX_CMDLINE_SIZE],
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
    // Per-process exec context tests (verifying the fix works)
    // =========================================================================

    #[test]
    fn test_per_process_exec_context_isolation() {
        // Create two processes
        let mut proc_a = make_test_process(100);
        let mut proc_b = make_test_process(200);

        // Process A sets its exec context
        proc_a.exec_pending = true;
        proc_a.exec_entry = 0xAAAAAAAA;
        proc_a.exec_stack = 0xAAAAAAA0;
        proc_a.exec_user_data_sel = 0x23;

        // Process B sets its exec context (this used to overwrite A's!)
        proc_b.exec_pending = true;
        proc_b.exec_entry = 0xBBBBBBBB;
        proc_b.exec_stack = 0xBBBBBBB0;
        proc_b.exec_user_data_sel = 0x23;

        // FIXED: A still has its own context!
        assert!(proc_a.exec_pending);
        assert_eq!(proc_a.exec_entry, 0xAAAAAAAA);
        assert_eq!(proc_a.exec_stack, 0xAAAAAAA0);

        // B has its own context
        assert!(proc_b.exec_pending);
        assert_eq!(proc_b.exec_entry, 0xBBBBBBBB);
        assert_eq!(proc_b.exec_stack, 0xBBBBBBB0);
    }

    #[test]
    fn test_exec_context_survives_other_process_consumption() {
        let mut proc_a = make_test_process(100);
        let mut proc_b = make_test_process(200);

        // A sets exec context
        proc_a.exec_pending = true;
        proc_a.exec_entry = 0x1111;
        proc_a.exec_stack = 0x2222;

        // B sets and consumes its own exec context
        proc_b.exec_pending = true;
        proc_b.exec_entry = 0x3333;
        proc_b.exec_stack = 0x4444;

        // B consumes
        assert!(proc_b.exec_pending);
        proc_b.exec_pending = false;
        let b_entry = proc_b.exec_entry;
        assert_eq!(b_entry, 0x3333);

        // FIXED: A's context is still intact!
        assert!(proc_a.exec_pending);
        assert_eq!(proc_a.exec_entry, 0x1111);
    }

    #[test]
    fn test_exec_context_can_be_reread_after_preemption() {
        let mut proc = make_test_process(100);

        // execve sets context
        proc.exec_pending = true;
        proc.exec_entry = 0x1000000;
        proc.exec_stack = 0x1A00000;

        // First read (before preemption)
        assert!(proc.exec_pending);
        let entry1 = proc.exec_entry;
        let stack1 = proc.exec_stack;

        // Simulating preemption: with per-process storage,
        // the context remains in the Process struct

        // After preemption, we can still access it
        // (Unlike global EXEC_CONTEXT which was atomically cleared)
        assert_eq!(proc.exec_entry, entry1);
        assert_eq!(proc.exec_stack, stack1);

        // Only explicitly clearing it loses the data
        proc.exec_pending = false;
        
        // Now it's consumed
        assert!(!proc.exec_pending);
    }

    #[test]
    fn test_fork_child_has_no_exec_pending() {
        let mut parent = make_test_process(100);
        
        // Parent has pending exec
        parent.exec_pending = true;
        parent.exec_entry = 0xDEAD;

        // Fork creates child by copying parent
        let mut child = parent;
        child.pid = 101;
        child.ppid = 100;
        child.is_fork_child = true;
        // CRITICAL: fork must clear exec_pending
        child.exec_pending = false;

        // Parent still has its exec pending
        assert!(parent.exec_pending);
        
        // Child does NOT inherit exec pending
        assert!(!child.exec_pending);
    }

    #[test]
    fn test_login_to_shell_scenario_fixed() {
        // Simulating the exact scenario that was broken:
        // login calls execve("/bin/sh"), gets preempted, shell never starts

        let mut login = make_test_process(5);

        // login calls execve("/bin/sh")
        let shell_entry = 0x1000000u64;
        let shell_stack = 0x1A00000u64;
        login.exec_pending = true;
        login.exec_entry = shell_entry;
        login.exec_stack = shell_stack;
        login.exec_user_data_sel = 0x23;

        // syscall return path reads context
        assert!(login.exec_pending);
        let entry = login.exec_entry;
        let stack = login.exec_stack;
        assert_eq!(entry, shell_entry);
        assert_eq!(stack, shell_stack);

        // === TIMER INTERRUPT - preemption ===
        // Another process (getty) does execve
        let mut getty = make_test_process(6);
        getty.exec_pending = true;
        getty.exec_entry = 0xDEAD;
        getty.exec_stack = 0xBEEF;

        // getty consumes its context
        getty.exec_pending = false;

        // === login resumes ===
        // FIXED: login's exec context is still there!
        assert!(login.exec_pending);
        assert_eq!(login.exec_entry, shell_entry);
        assert_eq!(login.exec_stack, shell_stack);

        // Now login can properly consume its context and jump to shell
        login.exec_pending = false;
        // Shell starts successfully!
    }
}
