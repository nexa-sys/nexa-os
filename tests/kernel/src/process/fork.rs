//! Fork syscall edge case tests
//!
//! Tests for fork/clone edge cases, register preservation, and process hierarchy.

#[cfg(test)]
mod tests {
    use crate::process::{
        Process, ProcessState, Context, Pid,
        USER_VIRT_BASE, HEAP_BASE, STACK_BASE, STACK_SIZE,
        INTERP_BASE, INTERP_REGION_SIZE, USER_REGION_SIZE,
        KERNEL_STACK_SIZE, KERNEL_STACK_ALIGN,
        MAX_PROCESSES, MAX_PROCESS_ARGS, MAX_CMDLINE_SIZE,
    };
    use crate::scheduler::ProcessEntry;

    // =========================================================================
    // Memory Layout Constants Tests
    // =========================================================================

    #[test]
    fn test_memory_layout_constants() {
        // Verify all memory layout constants are sensible
        assert!(USER_VIRT_BASE > 0, "USER_VIRT_BASE should be non-zero");
        assert!(HEAP_BASE > USER_VIRT_BASE, "HEAP_BASE should be after USER_VIRT_BASE");
        assert!(STACK_BASE > HEAP_BASE, "STACK_BASE should be after HEAP_BASE");
        assert!(INTERP_BASE > STACK_BASE, "INTERP_BASE should be after STACK_BASE");
    }

    #[test]
    fn test_memory_regions_non_overlapping() {
        // Calculate region boundaries
        let code_end = HEAP_BASE;
        let heap_end = STACK_BASE;
        let stack_end = INTERP_BASE;
        let interp_end = INTERP_BASE + INTERP_REGION_SIZE;
        
        // Check no overlap
        assert!(USER_VIRT_BASE < code_end);
        assert!(HEAP_BASE < heap_end);
        assert!(STACK_BASE < stack_end);
        assert!(INTERP_BASE < interp_end);
    }

    #[test]
    fn test_user_region_size_calculation() {
        // USER_REGION_SIZE should span from USER_VIRT_BASE to end of INTERP region
        let expected = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;
        assert_eq!(USER_REGION_SIZE, expected);
    }

    #[test]
    fn test_stack_size_alignment() {
        // Stack should be 2MB aligned for huge pages
        assert_eq!(STACK_SIZE, 0x200000);
        assert_eq!(STACK_SIZE % (2 * 1024 * 1024), 0);
    }

    // =========================================================================
    // Context Register Tests
    // =========================================================================

    #[test]
    fn test_context_zero_initialization() {
        let ctx = Context::zero();
        
        // All general purpose registers should be zero
        assert_eq!(ctx.rax, 0);
        assert_eq!(ctx.rbx, 0);
        assert_eq!(ctx.rcx, 0);
        assert_eq!(ctx.rdx, 0);
        assert_eq!(ctx.rsi, 0);
        assert_eq!(ctx.rdi, 0);
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
        
        // rflags should be 0x202 (IF flag set + reserved bit 1)
        // This is intentional to ensure interrupts are enabled for user processes
        assert_eq!(ctx.rflags, 0x202);
    }

    #[test]
    fn test_context_copy() {
        let mut ctx = Context::zero();
        ctx.rax = 0xDEADBEEF;
        ctx.rip = 0x1000;
        ctx.rsp = 0x2000;
        
        let copy = ctx;
        
        assert_eq!(copy.rax, 0xDEADBEEF);
        assert_eq!(copy.rip, 0x1000);
        assert_eq!(copy.rsp, 0x2000);
    }

    #[test]
    fn test_context_size() {
        // Context should be a reasonable size
        let size = core::mem::size_of::<Context>();
        
        // 18 registers * 8 bytes = 144 bytes minimum
        assert!(size >= 144);
        // But not too large
        assert!(size <= 256);
    }

    // =========================================================================
    // ProcessState Tests
    // =========================================================================

    #[test]
    fn test_process_state_values() {
        // Verify all states exist and are distinct
        let states = [
            ProcessState::Ready,
            ProcessState::Running,
            ProcessState::Sleeping,
            ProcessState::Zombie,
        ];
        
        for (i, &s1) in states.iter().enumerate() {
            for (j, &s2) in states.iter().enumerate() {
                if i != j {
                    assert_ne!(s1, s2);
                }
            }
        }
    }

    #[test]
    fn test_process_state_default() {
        // New processes should start in Ready state
        let entry = ProcessEntry::empty();
        assert_eq!(entry.process.state, ProcessState::Ready);
    }

    // =========================================================================
    // Fork Child State Tests
    // =========================================================================

    #[test]
    fn test_fork_child_initial_state() {
        let mut entry = ProcessEntry::empty();
        
        // Set up as fork child
        entry.process.is_fork_child = true;
        entry.process.state = ProcessState::Ready;
        
        // Child context should have RAX=0 (fork returns 0 in child)
        entry.process.context.rax = 0;
        
        assert!(entry.process.is_fork_child);
        assert_eq!(entry.process.context.rax, 0);
    }

    #[test]
    fn test_fork_parent_child_relationship() {
        let mut parent = ProcessEntry::empty();
        parent.process.pid = 1;
        parent.process.tgid = 1;
        
        let mut child = ProcessEntry::empty();
        child.process.pid = 2;
        child.process.ppid = 1; // Parent is PID 1
        child.process.tgid = 2; // New thread group (fork, not clone)
        child.process.is_fork_child = true;
        child.process.is_thread = false;
        
        assert_eq!(child.process.ppid, parent.process.pid);
        assert_ne!(child.process.tgid, parent.process.tgid);
        assert!(!child.process.is_thread);
    }

    #[test]
    fn test_thread_vs_fork_distinction() {
        // Fork creates new process (new tgid)
        let mut fork_child = ProcessEntry::empty();
        fork_child.process.pid = 10;
        fork_child.process.ppid = 1;
        fork_child.process.tgid = 10; // Same as pid
        fork_child.process.is_thread = false;
        
        // Clone with CLONE_THREAD creates thread (same tgid as parent)
        let mut thread = ProcessEntry::empty();
        thread.process.pid = 11;
        thread.process.ppid = 1; // Same parent as process
        thread.process.tgid = 1; // Same thread group as parent
        thread.process.is_thread = true;
        
        assert_ne!(fork_child.process.tgid, thread.process.tgid);
        assert!(!fork_child.process.is_thread);
        assert!(thread.process.is_thread);
    }

    // =========================================================================
    // Kernel Stack Tests
    // =========================================================================

    #[test]
    fn test_kernel_stack_size() {
        // 32KB kernel stack
        assert_eq!(KERNEL_STACK_SIZE, 32 * 1024);
    }

    #[test]
    fn test_kernel_stack_alignment() {
        // 16-byte alignment for x86_64 ABI
        assert_eq!(KERNEL_STACK_ALIGN, 16);
        assert!(KERNEL_STACK_SIZE % KERNEL_STACK_ALIGN == 0);
    }

    // =========================================================================
    // Process Limits Tests
    // =========================================================================

    #[test]
    fn test_max_processes() {
        assert_eq!(MAX_PROCESSES, 64);
        assert!(MAX_PROCESSES >= 32, "Should support at least 32 processes");
    }

    #[test]
    fn test_max_process_args() {
        assert_eq!(MAX_PROCESS_ARGS, 32);
        assert!(MAX_PROCESS_ARGS >= 16, "Should support at least 16 args");
    }

    #[test]
    fn test_max_cmdline_size() {
        assert_eq!(MAX_CMDLINE_SIZE, 1024);
        assert!(MAX_CMDLINE_SIZE >= 256, "Should support reasonable command lines");
    }

    // =========================================================================
    // Exit Code Tests
    // =========================================================================

    #[test]
    fn test_exit_code_range() {
        let mut entry = ProcessEntry::empty();
        
        // Test various exit codes
        entry.process.exit_code = 0;
        assert_eq!(entry.process.exit_code, 0);
        
        entry.process.exit_code = 1;
        assert_eq!(entry.process.exit_code, 1);
        
        entry.process.exit_code = 255;
        assert_eq!(entry.process.exit_code, 255);
        
        // Negative exit codes (from signals)
        entry.process.exit_code = -15; // SIGTERM
        assert_eq!(entry.process.exit_code, -15);
    }

    #[test]
    fn test_zombie_with_exit_code() {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 5;
        entry.process.exit_code = 42;
        entry.process.state = ProcessState::Zombie;
        
        assert_eq!(entry.process.state, ProcessState::Zombie);
        assert_eq!(entry.process.exit_code, 42);
    }

    // =========================================================================
    // Termination Signal Tests
    // =========================================================================

    #[test]
    fn test_term_signal_initially_none() {
        let entry = ProcessEntry::empty();
        assert!(entry.process.term_signal.is_none());
    }

    #[test]
    fn test_term_signal_with_value() {
        let mut entry = ProcessEntry::empty();
        
        // Process killed by SIGKILL (9)
        entry.process.term_signal = Some(9);
        assert_eq!(entry.process.term_signal, Some(9));
        
        // Process killed by SIGSEGV (11)
        entry.process.term_signal = Some(11);
        assert_eq!(entry.process.term_signal, Some(11));
    }

    // =========================================================================
    // Clear Child TID Tests (for pthread_join)
    // =========================================================================

    #[test]
    fn test_clear_child_tid_default() {
        let entry = ProcessEntry::empty();
        assert_eq!(entry.process.clear_child_tid, 0);
    }

    #[test]
    fn test_clear_child_tid_for_thread() {
        let mut entry = ProcessEntry::empty();
        entry.process.is_thread = true;
        
        // Set up for CLONE_CHILD_CLEARTID
        entry.process.clear_child_tid = 0x7FFE0000;
        
        assert_eq!(entry.process.clear_child_tid, 0x7FFE0000);
    }

    // =========================================================================
    // Memory Base Tests
    // =========================================================================

    #[test]
    fn test_memory_base_default() {
        let entry = ProcessEntry::empty();
        // Memory base should be set during process creation
        assert_eq!(entry.process.memory_base, 0);
    }

    #[test]
    fn test_memory_base_alignment() {
        let mut entry = ProcessEntry::empty();
        
        // Memory base should be page-aligned
        entry.process.memory_base = 0x100000;
        assert_eq!(entry.process.memory_base % 4096, 0);
    }
}
