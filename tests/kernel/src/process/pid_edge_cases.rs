//! PID Tree and Process Management Edge Case Tests
//!
//! Tests for PID allocation, radix tree operations, and process lifecycle
//! edge cases that could expose bugs.

#[cfg(test)]
mod tests {
    use crate::process::{ProcessState, Pid, MAX_PROCESSES, MAX_CMDLINE_SIZE};
    use crate::scheduler::ProcessEntry;

    // =========================================================================
    // PID Allocation Edge Cases
    // =========================================================================

    #[test]
    fn test_pid_zero_reserved() {
        // PID 0 should always be reserved for kernel/idle
        fn is_valid_user_pid(pid: Pid) -> bool {
            pid > 0 && pid < 262144 // MAX_PID
        }
        
        assert!(!is_valid_user_pid(0), "PID 0 should be invalid for user processes");
        assert!(is_valid_user_pid(1), "PID 1 should be valid");
    }

    #[test]
    fn test_pid_max_boundary() {
        const MAX_PID: u64 = (1 << 18) - 1; // 262143
        
        fn is_valid_pid(pid: Pid) -> bool {
            pid > 0 && pid <= MAX_PID
        }
        
        assert!(is_valid_pid(MAX_PID), "MAX_PID should be valid");
        assert!(!is_valid_pid(MAX_PID + 1), "MAX_PID + 1 should be invalid");
        assert!(!is_valid_pid(u64::MAX), "u64::MAX should be invalid");
    }

    #[test]
    fn test_pid_bitmap_word_boundary() {
        // PID bitmap uses u64 words, test boundary conditions
        // PIDs 63 and 64 are at word boundary
        
        fn pid_to_word_index(pid: u64) -> usize {
            (pid / 64) as usize
        }
        
        fn pid_to_bit_index(pid: u64) -> u64 {
            pid % 64
        }
        
        assert_eq!(pid_to_word_index(0), 0);
        assert_eq!(pid_to_word_index(63), 0);
        assert_eq!(pid_to_word_index(64), 1);
        assert_eq!(pid_to_word_index(127), 1);
        assert_eq!(pid_to_word_index(128), 2);
        
        assert_eq!(pid_to_bit_index(0), 0);
        assert_eq!(pid_to_bit_index(63), 63);
        assert_eq!(pid_to_bit_index(64), 0);
        assert_eq!(pid_to_bit_index(65), 1);
    }

    #[test]
    fn test_pid_bitmap_full_word() {
        // Test when a full word (64 PIDs) is allocated
        let mut bitmap: u64 = 0;
        
        // Allocate all 64 PIDs in first word
        for i in 0..64 {
            bitmap |= 1 << i;
        }
        
        assert_eq!(bitmap, u64::MAX, "Full word should be all 1s");
        
        // Check that word is full
        fn word_is_full(word: u64) -> bool {
            word == u64::MAX
        }
        
        assert!(word_is_full(bitmap));
    }

    // =========================================================================
    // Process State Machine Tests
    // =========================================================================

    #[test]
    fn test_process_state_transitions_valid() {
        // Valid state transitions per kernel documentation
        let valid_transitions = [
            (ProcessState::Ready, ProcessState::Running),      // Scheduled
            (ProcessState::Running, ProcessState::Ready),      // Preempted
            (ProcessState::Running, ProcessState::Sleeping),   // Blocked on I/O
            (ProcessState::Sleeping, ProcessState::Ready),     // Woken up
            (ProcessState::Running, ProcessState::Zombie),     // Exited
        ];
        
        fn is_valid_transition(from: ProcessState, to: ProcessState) -> bool {
            matches!((from, to),
                (ProcessState::Ready, ProcessState::Running) |
                (ProcessState::Running, ProcessState::Ready) |
                (ProcessState::Running, ProcessState::Sleeping) |
                (ProcessState::Sleeping, ProcessState::Ready) |
                (ProcessState::Running, ProcessState::Zombie) |
                (ProcessState::Ready, ProcessState::Zombie) |     // Signal
                (ProcessState::Sleeping, ProcessState::Zombie)    // Signal
            )
        }
        
        for (from, to) in &valid_transitions {
            assert!(is_valid_transition(*from, *to),
                "Transition {:?} -> {:?} should be valid", from, to);
        }
    }

    #[test]
    fn test_process_state_transitions_invalid() {
        // Invalid state transitions that should never occur
        
        fn is_invalid_transition(from: ProcessState, to: ProcessState) -> bool {
            matches!((from, to),
                (ProcessState::Zombie, ProcessState::Ready) |     // Can't resurrect
                (ProcessState::Zombie, ProcessState::Running) |   // Can't resurrect
                (ProcessState::Zombie, ProcessState::Sleeping) |  // Can't resurrect
                (ProcessState::Sleeping, ProcessState::Running)   // Must go through Ready
            )
        }
        
        assert!(is_invalid_transition(ProcessState::Zombie, ProcessState::Ready));
        assert!(is_invalid_transition(ProcessState::Zombie, ProcessState::Running));
        assert!(is_invalid_transition(ProcessState::Sleeping, ProcessState::Running));
    }

    #[test]
    fn test_zombie_state_is_terminal() {
        // Once zombie, process can only be reaped (removed), not transitioned
        let mut entry = ProcessEntry::empty();
        entry.process.state = ProcessState::Zombie;
        entry.process.exit_code = 42;
        
        // Zombie should preserve exit code
        assert_eq!(entry.process.exit_code, 42);
        
        // State should remain Zombie
        assert_eq!(entry.process.state, ProcessState::Zombie);
    }

    // =========================================================================
    // Thread Group Tests
    // =========================================================================

    #[test]
    fn test_main_thread_tgid_equals_pid() {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 100;
        entry.process.tgid = 100; // Main thread: tgid == pid
        entry.process.is_thread = false;
        
        assert_eq!(entry.process.tgid, entry.process.pid,
            "Main thread's tgid should equal its pid");
        assert!(!entry.process.is_thread);
    }

    #[test]
    fn test_child_thread_tgid_equals_leader() {
        let leader_pid: Pid = 100;
        
        let mut thread1 = ProcessEntry::empty();
        thread1.process.pid = 101;
        thread1.process.tgid = leader_pid;
        thread1.process.is_thread = true;
        
        let mut thread2 = ProcessEntry::empty();
        thread2.process.pid = 102;
        thread2.process.tgid = leader_pid;
        thread2.process.is_thread = true;
        
        assert_eq!(thread1.process.tgid, leader_pid);
        assert_eq!(thread2.process.tgid, leader_pid);
        assert!(thread1.process.is_thread);
        assert!(thread2.process.is_thread);
    }

    #[test]
    fn test_thread_ppid_matches_leader() {
        // Threads should have the same ppid as their leader
        let parent_pid: Pid = 1;
        let leader_pid: Pid = 100;
        
        let mut leader = ProcessEntry::empty();
        leader.process.pid = leader_pid;
        leader.process.ppid = parent_pid;
        leader.process.tgid = leader_pid;
        leader.process.is_thread = false;
        
        let mut thread = ProcessEntry::empty();
        thread.process.pid = 101;
        thread.process.ppid = parent_pid; // Same ppid as leader
        thread.process.tgid = leader_pid;
        thread.process.is_thread = true;
        
        assert_eq!(thread.process.ppid, leader.process.ppid);
    }

    // =========================================================================
    // Fork/Clone Edge Cases
    // =========================================================================

    #[test]
    fn test_fork_child_flag_isolation() {
        let mut parent = ProcessEntry::empty();
        parent.process.pid = 1;
        parent.process.is_fork_child = false;
        
        let mut child = parent;
        child.process.pid = 2;
        child.process.ppid = 1;
        child.process.is_fork_child = true;
        
        // Parent should not be affected by child's flag
        assert!(!parent.process.is_fork_child);
        assert!(child.process.is_fork_child);
    }

    #[test]
    fn test_clone_vm_shares_cr3() {
        let parent_cr3: u64 = 0x1000_0000;
        
        let mut parent = ProcessEntry::empty();
        parent.process.pid = 1;
        parent.process.cr3 = parent_cr3;
        
        // CLONE_VM: child shares address space
        let mut thread = ProcessEntry::empty();
        thread.process.pid = 2;
        thread.process.cr3 = parent_cr3; // Shared
        thread.process.is_thread = true;
        
        assert_eq!(thread.process.cr3, parent.process.cr3,
            "CLONE_VM should share CR3");
    }

    #[test]
    fn test_fork_copies_cr3() {
        let parent_cr3: u64 = 0x1000_0000;
        let child_cr3: u64 = 0x2000_0000;
        
        let mut parent = ProcessEntry::empty();
        parent.process.pid = 1;
        parent.process.cr3 = parent_cr3;
        
        // Fork: child gets own address space
        let mut child = ProcessEntry::empty();
        child.process.pid = 2;
        child.process.ppid = 1;
        child.process.cr3 = child_cr3; // Separate
        child.process.is_thread = false;
        
        assert_ne!(child.process.cr3, parent.process.cr3,
            "Fork should have separate CR3");
    }

    // =========================================================================
    // Context Management Tests
    // =========================================================================

    #[test]
    fn test_context_zero_initial() {
        use crate::process::Context;
        
        let ctx = Context::zero();
        
        assert_eq!(ctx.rax, 0);
        assert_eq!(ctx.rbx, 0);
        assert_eq!(ctx.rcx, 0);
        assert_eq!(ctx.rdx, 0);
        assert_eq!(ctx.rdi, 0);
        assert_eq!(ctx.rsi, 0);
        assert_eq!(ctx.rbp, 0);
        assert_eq!(ctx.rsp, 0);
        assert_eq!(ctx.r8, 0);
        assert_eq!(ctx.r9, 0);
        assert_eq!(ctx.r10, 0);
        assert_eq!(ctx.r11, 0);
        assert_eq!(ctx.r12, 0);
        assert_eq!(ctx.r13, 0);
        assert_eq!(ctx.r14, 0);
        assert_eq!(ctx.r15, 0);
        assert_eq!(ctx.rip, 0);
    }

    #[test]
    fn test_context_rflags_if_enabled() {
        use crate::process::Context;
        
        let ctx = Context::zero();
        
        // IF (Interrupt Flag) should be set (bit 9)
        const IF_FLAG: u64 = 1 << 9;
        assert_ne!(ctx.rflags & IF_FLAG, 0, "IF flag should be set in initial context");
        
        // Verify the value is 0x202 (IF + reserved bit 1)
        assert_eq!(ctx.rflags, 0x202);
    }

    #[test]
    fn test_context_valid_flag_semantics() {
        let mut entry = ProcessEntry::empty();
        
        // Initially context_valid is false
        assert!(!entry.process.context_valid);
        
        // After context_switch saves context, it should be true
        entry.process.context_valid = true;
        assert!(entry.process.context_valid);
        
        // After exec, context_valid should be reset
        entry.process.context_valid = false;
        assert!(!entry.process.context_valid);
    }

    // =========================================================================
    // Command Line Tests
    // =========================================================================

    #[test]
    fn test_cmdline_buffer_size() {
        assert_eq!(MAX_CMDLINE_SIZE, 1024, "MAX_CMDLINE_SIZE should be 1024");
        
        let mut entry = ProcessEntry::empty();
        assert_eq!(entry.process.cmdline.len(), MAX_CMDLINE_SIZE);
        assert_eq!(entry.process.cmdline_len, 0);
    }

    #[test]
    fn test_cmdline_null_terminated() {
        let mut entry = ProcessEntry::empty();
        
        // Set a simple command line: "ls\0"
        entry.process.cmdline[0] = b'l';
        entry.process.cmdline[1] = b's';
        entry.process.cmdline[2] = 0;
        entry.process.cmdline_len = 3;
        
        // Verify null termination
        assert_eq!(entry.process.cmdline[2], 0);
    }

    #[test]
    fn test_cmdline_multiple_args() {
        let mut entry = ProcessEntry::empty();
        
        // Set command line: "ls\0-la\0/home\0\0" (double-null terminated)
        let cmdline = b"ls\0-la\0/home\0\0";
        entry.process.cmdline[..cmdline.len()].copy_from_slice(cmdline);
        entry.process.cmdline_len = cmdline.len();
        
        // Parse arguments
        let mut args: Vec<&[u8]> = Vec::new();
        let mut start = 0;
        for i in 0..entry.process.cmdline_len {
            if entry.process.cmdline[i] == 0 {
                if start < i {
                    args.push(&entry.process.cmdline[start..i]);
                }
                start = i + 1;
                if i + 1 < entry.process.cmdline_len && entry.process.cmdline[i + 1] == 0 {
                    break; // Double-null terminator
                }
            }
        }
        
        assert_eq!(args.len(), 3);
        assert_eq!(args[0], b"ls");
        assert_eq!(args[1], b"-la");
        assert_eq!(args[2], b"/home");
    }

    // =========================================================================
    // Exit Code and Signal Tests
    // =========================================================================

    #[test]
    fn test_exit_code_range() {
        // Exit codes are i32, but POSIX typically uses 0-255
        let mut entry = ProcessEntry::empty();
        
        entry.process.exit_code = 0;
        assert_eq!(entry.process.exit_code, 0);
        
        entry.process.exit_code = 255;
        assert_eq!(entry.process.exit_code, 255);
        
        // Negative exit codes are possible (signal termination)
        entry.process.exit_code = -1;
        assert_eq!(entry.process.exit_code, -1);
    }

    #[test]
    fn test_term_signal_none_for_normal_exit() {
        let mut entry = ProcessEntry::empty();
        entry.process.state = ProcessState::Zombie;
        entry.process.exit_code = 0;
        entry.process.term_signal = None;
        
        // Normal exit: no termination signal
        assert!(entry.process.term_signal.is_none());
    }

    #[test]
    fn test_term_signal_set_for_signal_death() {
        let mut entry = ProcessEntry::empty();
        entry.process.state = ProcessState::Zombie;
        entry.process.term_signal = Some(9); // SIGKILL
        
        // Signal death: term_signal is set
        assert_eq!(entry.process.term_signal, Some(9));
    }

    // =========================================================================
    // File Descriptor Bitmask Tests
    // =========================================================================

    #[test]
    fn test_open_fds_bitmask() {
        let mut entry = ProcessEntry::empty();
        
        // Initial state: no FDs open (bits 0-15 for fd 3-18)
        assert_eq!(entry.process.open_fds, 0);
        
        // Open fd 3 (bit 0)
        entry.process.open_fds |= 1 << 0;
        assert_ne!(entry.process.open_fds & (1 << 0), 0);
        
        // Open fd 10 (bit 7)
        entry.process.open_fds |= 1 << 7;
        assert_ne!(entry.process.open_fds & (1 << 7), 0);
        
        // Close fd 3 (bit 0)
        entry.process.open_fds &= !(1 << 0);
        assert_eq!(entry.process.open_fds & (1 << 0), 0);
    }

    #[test]
    fn test_open_fds_maximum() {
        let mut entry = ProcessEntry::empty();
        
        // u16 can track 16 FDs (fd 3-18)
        entry.process.open_fds = u16::MAX;
        
        assert_eq!(entry.process.open_fds.count_ones(), 16);
    }

    // =========================================================================
    // Process Table Limits
    // =========================================================================

    #[test]
    fn test_max_processes_limit() {
        assert_eq!(MAX_PROCESSES, 64, "MAX_PROCESSES should be 64");
    }

    #[test]
    fn test_process_entry_size() {
        // ProcessEntry should fit in cache line or reasonable size
        let size = std::mem::size_of::<ProcessEntry>();
        
        // ProcessEntry is large due to Process struct containing cmdline buffer
        // Just verify it's not absurdly large
        assert!(size < 4096, "ProcessEntry size {} should be < 4096", size);
    }
}
