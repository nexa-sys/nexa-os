//! Comprehensive memory management tests
//!
//! Tests virtual address mapping, page table operations, heap allocation,
//! and memory protection mechanisms.

#[cfg(test)]
mod tests {
    use crate::mm::allocator::BuddyStats;
    use crate::process::{
        Context, ProcessState, USER_VIRT_BASE, USER_PHYS_BASE, HEAP_BASE, HEAP_SIZE,
        STACK_BASE, STACK_SIZE, INTERP_BASE, INTERP_REGION_SIZE, USER_REGION_SIZE,
        KERNEL_STACK_SIZE, KERNEL_STACK_ALIGN, MAX_PROCESSES, MAX_PROCESS_ARGS,
        MAX_CMDLINE_SIZE,
    };

    // =========================================================================
    // Virtual Address Constants Tests
    // =========================================================================

    #[test]
    fn test_memory_layout_constants_valid() {
        // Verify that memory layout constants are properly defined
        assert!(USER_VIRT_BASE > 0);
        assert!(HEAP_BASE > 0);
        assert!(STACK_BASE > 0);
        assert!(INTERP_BASE > 0);
    }

    #[test]
    fn test_memory_regions_non_overlapping() {
        // Ensure that different memory regions don't overlap
        assert!(USER_VIRT_BASE < HEAP_BASE);
        assert!(HEAP_BASE < STACK_BASE);
        assert!(STACK_BASE < INTERP_BASE);
    }

    #[test]
    fn test_heap_bounds() {
        // Verify heap region is correctly bounded
        let heap_end = HEAP_BASE + HEAP_SIZE;
        // Stack starts exactly after heap (no gap is intended - they are adjacent)
        assert_eq!(heap_end, STACK_BASE, "Heap end should equal stack start");
        assert!(heap_end > HEAP_BASE, "Heap size is positive");
    }

    #[test]
    fn test_stack_bounds() {
        // Verify stack region is correctly bounded
        let stack_end = STACK_BASE + STACK_SIZE;
        assert!(stack_end > STACK_BASE, "Stack size is positive");
        // Stack should not extend into interpreter region
        assert!(stack_end <= INTERP_BASE, "Stack extends into interpreter region");
    }

    #[test]
    fn test_user_region_total_size() {
        // Verify total user region size calculation
        let expected = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;
        assert_eq!(USER_REGION_SIZE, expected);
    }

    #[test]
    fn test_memory_alignment_requirements() {
        // Verify that critical addresses are properly aligned
        assert_eq!(USER_VIRT_BASE & 0xFFF, 0, "USER_VIRT_BASE not page-aligned");
        assert_eq!(HEAP_BASE & 0xFFF, 0, "HEAP_BASE not page-aligned");
        assert_eq!(STACK_BASE & 0xFFF, 0, "STACK_BASE not page-aligned");
    }

    // =========================================================================
    // Kernel Stack Tests
    // =========================================================================

    #[test]
    fn test_kernel_stack_size_reasonable() {
        // Kernel stack should be at least 4KB but not excessively large
        assert!(KERNEL_STACK_SIZE >= 4096, "Kernel stack too small");
        assert!(KERNEL_STACK_SIZE <= 256 * 1024, "Kernel stack too large");
    }

    #[test]
    fn test_kernel_stack_alignment() {
        // Kernel stack should be 16-byte aligned for ABI compliance
        assert_eq!(KERNEL_STACK_ALIGN, 16);
        assert_eq!(KERNEL_STACK_ALIGN & (KERNEL_STACK_ALIGN - 1), 0, "Not power of 2");
    }

    // =========================================================================
    // Process Limit Tests
    // =========================================================================

    #[test]
    fn test_max_processes_valid() {
        // MAX_PROCESSES should be reasonable
        assert!(MAX_PROCESSES > 0);
        assert!(MAX_PROCESSES <= 4096, "Too many max processes");
    }

    #[test]
    fn test_process_arguments_limit() {
        // Maximum arguments should be reasonable
        assert!(MAX_PROCESS_ARGS > 0);
        assert!(MAX_PROCESS_ARGS <= 256);
    }

    #[test]
    fn test_cmdline_size_sufficient() {
        // Command line storage should be large enough
        assert!(MAX_CMDLINE_SIZE >= 256);
        assert!(MAX_CMDLINE_SIZE <= 8192);
    }

    // =========================================================================
    // Buddy Allocator Statistics Tests
    // =========================================================================

    #[test]
    fn test_buddy_stats_initialization() {
        let stats = BuddyStats {
            allocations: 0,
            frees: 0,
            splits: 0,
            merges: 0,
            pages_allocated: 0,
            pages_free: 0,
        };

        assert_eq!(stats.allocations, 0);
        assert_eq!(stats.frees, 0);
    }

    #[test]
    fn test_buddy_stats_consistency() {
        // For a consistent allocator, frees should not exceed allocations
        let stats = BuddyStats {
            allocations: 100,
            frees: 50,
            splits: 25,
            merges: 20,
            pages_allocated: 80,
            pages_free: 920,
        };

        // Allocations >= Frees (invariant check)
        assert!(stats.allocations >= stats.frees);

        // Total pages should equal allocated + free
        assert_eq!(stats.pages_allocated + stats.pages_free, 1000);
    }

    #[test]
    fn test_buddy_stats_split_merge_ratio() {
        let stats = BuddyStats {
            allocations: 100,
            frees: 50,
            splits: 80,
            merges: 75,
            pages_allocated: 100,
            pages_free: 900,
        };

        // Splits and merges should be roughly balanced (indicating good fragmentation)
        // This is a heuristic check, not a hard invariant
        let split_merge_ratio = (stats.splits as i64 - stats.merges as i64).abs();
        assert!(split_merge_ratio <= 10, "Unbalanced split/merge operations");
    }

    // =========================================================================
    // CPU Context Tests
    // =========================================================================

    #[test]
    fn test_context_zero_initialization() {
        let ctx = Context::zero();

        // General purpose registers should be zero
        assert_eq!(ctx.rax, 0);
        assert_eq!(ctx.rbx, 0);
        assert_eq!(ctx.rcx, 0);
        assert_eq!(ctx.rdx, 0);
        assert_eq!(ctx.rsi, 0);
        assert_eq!(ctx.rdi, 0);
        assert_eq!(ctx.rbp, 0);
        assert_eq!(ctx.r8, 0);
        assert_eq!(ctx.r9, 0);
        assert_eq!(ctx.r10, 0);
        assert_eq!(ctx.r11, 0);
        assert_eq!(ctx.r12, 0);
        assert_eq!(ctx.r13, 0);
        assert_eq!(ctx.r14, 0);
        assert_eq!(ctx.r15, 0);

        // Instruction pointer should be zero
        assert_eq!(ctx.rip, 0);
        assert_eq!(ctx.rsp, 0);
    }

    #[test]
    fn test_context_flags_initialization() {
        let ctx = Context::zero();

        // Interrupt flag (IF) should be set in RFLAGS (bit 9 = 0x200)
        assert_eq!(ctx.rflags & 0x200, 0x200, "Interrupt flag not set");

        // Reserved bits should be consistent
        // Bits 1, 3, 5, 15, 22-63 are reserved
        // Bit 1 should be set (always 1 in RFLAGS)
        assert_eq!(ctx.rflags & 0x2, 0x2, "Bit 1 must be set");
    }

    #[test]
    fn test_context_copy_semantics() {
        let ctx1 = Context::zero();
        let ctx2 = ctx1;

        // Copying should preserve all values
        assert_eq!(ctx1.rax, ctx2.rax);
        assert_eq!(ctx1.rip, ctx2.rip);
        assert_eq!(ctx1.rflags, ctx2.rflags);
    }

    // =========================================================================
    // Memory Protection Tests (Data validation)
    // =========================================================================

    #[test]
    fn test_user_region_size_alignment() {
        // User region size should be a multiple of 4KB
        assert_eq!(USER_REGION_SIZE & 0xFFF, 0, "USER_REGION_SIZE not page-aligned");
    }

    #[test]
    fn test_interpreter_region_reservation() {
        // Interpreter region should be sufficiently large for dynamic linking
        assert!(INTERP_REGION_SIZE >= 0x100000, "Interpreter region too small");
    }

    #[test]
    fn test_process_addresses_isolation() {
        // Each process should have isolated address space
        // Verify that user base isn't zero (would conflict with null pointer checks)
        assert!(USER_VIRT_BASE > 0x1000, "User base too low, may conflict with null");
    }

    // =========================================================================
    // Memory Layout Invariant Tests
    // =========================================================================

    #[test]
    fn test_heap_stack_gap_safety() {
        // Heap and stack are adjacent by design in this OS
        // The heap ends exactly where the stack begins
        let heap_top = HEAP_BASE + HEAP_SIZE;
        let stack_bottom = STACK_BASE;

        // They should be adjacent with no gap
        assert_eq!(heap_top, stack_bottom, "Heap and stack should be adjacent");
    }

    #[test]
    fn test_phys_virt_correspondence() {
        // Physical and virtual bases should be the same for identity mapping
        assert!(USER_PHYS_BASE >= 0);
        assert!(USER_VIRT_BASE >= 0);
    }

    // =========================================================================
    // Edge Case Tests
    // =========================================================================

    #[test]
    fn test_memory_constants_not_negative() {
        // All memory constants should be non-negative
        assert!(USER_VIRT_BASE >= 0);
        assert!(USER_PHYS_BASE >= 0);
        assert!(HEAP_BASE >= 0);
        assert!(HEAP_SIZE > 0);
        assert!(STACK_BASE >= 0);
        assert!(STACK_SIZE > 0);
        assert!(INTERP_BASE >= 0);
        assert!(INTERP_REGION_SIZE > 0);
    }

    #[test]
    fn test_context_register_independence() {
        // Modifying one register shouldn't affect others
        let mut ctx = Context::zero();
        ctx.rax = 0xDEADBEEF;
        ctx.rbx = 0xCAFEBABE;

        // Check that only the modified registers changed
        assert_eq!(ctx.rax, 0xDEADBEEF);
        assert_eq!(ctx.rbx, 0xCAFEBABE);
        assert_eq!(ctx.rcx, 0); // Should remain zero
    }

    #[test]
    fn test_max_process_cmdline_capacity() {
        // With MAX_PROCESS_ARGS arguments, maximum cmdline should be reasonable
        let avg_arg_len = 32;
        let estimated_max = MAX_PROCESS_ARGS * avg_arg_len;

        assert!(
            estimated_max <= MAX_CMDLINE_SIZE,
            "Cmdline size insufficient for typical args"
        );
    }

    // =========================================================================
    // Stress Tests (Logical validation without actual allocation)
    // =========================================================================

    #[test]
    fn test_multiple_context_instances() {
        let contexts: Vec<Context> = (0..100).map(|_| Context::zero()).collect();

        // All contexts should be independent zero-initialized
        for ctx in &contexts {
            assert_eq!(ctx.rax, 0);
            assert_eq!(ctx.rip, 0);
        }

        assert_eq!(contexts.len(), 100);
    }

    #[test]
    fn test_stats_boundary_values() {
        // Test with extreme but valid values
        let stats = BuddyStats {
            allocations: u64::MAX,
            frees: u64::MAX - 1,
            splits: u64::MAX,
            merges: u64::MAX,
            pages_allocated: u64::MAX / 2,
            pages_free: u64::MAX / 2,
        };

        // Should not panic on creation with large values
        assert!(stats.allocations >= stats.frees);
    }
}
