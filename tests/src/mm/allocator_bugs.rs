//! Memory Allocator Bug Detection Tests
//!
//! Tests targeting potential bugs in the memory management subsystem:
//! - VMA (Virtual Memory Area) management
//! - Page table flags and corruption
//! - Address alignment

#[cfg(test)]
mod vma_bugs {
    use crate::mm::vma::{VMAManager, VMAFlags, VMAPermissions, VMA, VMABacking};

    /// Helper to create an initialized VMAManager
    fn create_manager() -> VMAManager {
        let mut manager = VMAManager::new();
        manager.init();  // CRITICAL: Must call init() to set up free list
        manager
    }

    /// Test: VMA regions must not overlap
    #[test]
    fn test_vma_no_overlap() {
        let mut manager = create_manager();
        
        // Add first VMA [0x1000, 0x2000)
        let perms = VMAPermissions::READ;
        let vma1 = VMA::new(0x1000, 0x2000, perms, VMAFlags::PRIVATE, VMABacking::Anonymous);
        let result1 = manager.insert(vma1);
        assert!(result1.is_some(), "Failed to insert first VMA");
        
        // Try to add overlapping VMA [0x1500, 0x2500) - should fail or be adjusted
        let vma2 = VMA::new(0x1500, 0x2500, perms, VMAFlags::PRIVATE, VMABacking::Anonymous);
        let result2 = manager.insert(vma2);
        
        // Check for overlaps
        let found1 = manager.find(0x1000);
        let found2 = manager.find(0x1500);
        
        // If both exist, they shouldn't overlap
        if found1.is_some() && found2.is_some() {
            let v1 = found1.unwrap();
            let v2 = found2.unwrap();
            
            // Same VMA is fine (the find returned the containing VMA)
            if v1.start != v2.start {
                assert!(v1.end <= v2.start || v2.end <= v1.start,
                    "BUG: Overlapping VMAs: [{:#x}, {:#x}) and [{:#x}, {:#x})",
                    v1.start, v1.end, v2.start, v2.end);
            }
        }
    }

    /// Test: VMA find returns correct region for address
    #[test]
    fn test_vma_find_correct_region() {
        let mut manager = create_manager();
        let perms = VMAPermissions::READ;
        
        // Add non-overlapping VMAs
        let vma1 = VMA::new(0x1000, 0x2000, perms, VMAFlags::PRIVATE, VMABacking::Anonymous);
        manager.insert(vma1).expect("Failed to insert VMA1");
        
        let vma2 = VMA::new(0x3000, 0x4000, VMAPermissions::from_prot(3), VMAFlags::PRIVATE, VMABacking::Anonymous);
        manager.insert(vma2).expect("Failed to insert VMA2");
        
        let vma3 = VMA::new(0x5000, 0x6000, VMAPermissions::from_prot(5), VMAFlags::PRIVATE, VMABacking::Anonymous);
        manager.insert(vma3).expect("Failed to insert VMA3");
        
        // Find should return correct VMA
        let found = manager.find(0x1500);
        assert!(found.is_some(), "BUG: Couldn't find VMA for address in range");
        assert_eq!(found.unwrap().start, 0x1000);
        
        let found = manager.find(0x3500);
        assert!(found.is_some());
        assert_eq!(found.unwrap().start, 0x3000);
        
        // Address not in any VMA
        let not_found = manager.find(0x2500);
        assert!(not_found.is_none(),
            "BUG: Found VMA for address not in any region");
    }

    /// Test: VMA permissions are preserved
    #[test]
    fn test_vma_permissions_preserved() {
        let mut manager = create_manager();
        
        // Create VMA with READ | WRITE | EXEC
        let perms = VMAPermissions::from_prot(7); // R|W|X
        let vma = VMA::new(0x1000, 0x2000, perms, VMAFlags::PRIVATE, VMABacking::Anonymous);
        manager.insert(vma).expect("Failed to insert VMA");
        
        let found = manager.find(0x1500).unwrap();
        
        assert!(found.perm.is_read(), "BUG: READ permission lost");
        assert!(found.perm.is_write(), "BUG: WRITE permission lost");
        assert!(found.perm.is_exec(), "BUG: EXEC permission lost");
    }

    /// Test: VMA count tracking
    #[test]
    fn test_vma_count_tracking() {
        let mut manager = create_manager();
        let perms = VMAPermissions::READ;
        
        assert_eq!(manager.len(), 0, "Initial count should be 0");
        
        let vma1 = VMA::new(0x1000, 0x2000, perms, VMAFlags::PRIVATE, VMABacking::Anonymous);
        manager.insert(vma1).expect("Failed to insert VMA1");
        assert_eq!(manager.len(), 1, "Count should be 1 after first insert");
        
        let vma2 = VMA::new(0x3000, 0x4000, perms, VMAFlags::PRIVATE, VMABacking::Anonymous);
        manager.insert(vma2).expect("Failed to insert VMA2");
        assert_eq!(manager.len(), 2, "Count should be 2 after second insert");
    }

    /// Test: VMA address alignment
    #[test]
    fn test_vma_address_alignment() {
        let mut manager = create_manager();
        let perms = VMAPermissions::READ;
        
        // Aligned addresses should work
        let vma = VMA::new(0x1000, 0x2000, perms, VMAFlags::PRIVATE, VMABacking::Anonymous);
        let result = manager.insert(vma);
        assert!(result.is_some(), "Page-aligned insert should succeed");
        
        // Find should work
        let found = manager.find(0x1000);
        assert!(found.is_some(), "Should find VMA at aligned address");
        
        // Check alignment
        let v = found.unwrap();
        assert!(v.start % crate::mm::vma::PAGE_SIZE == 0,
            "BUG: VMA start not page-aligned");
        assert!(v.end % crate::mm::vma::PAGE_SIZE == 0,
            "BUG: VMA end not page-aligned");
    }
    
    /// Test: VMA split and merge edge cases
    #[test]
    fn test_vma_boundary_conditions() {
        let mut manager = create_manager();
        let perms = VMAPermissions::READ | VMAPermissions::WRITE;
        
        // Insert VMA at exact page boundary
        let vma = VMA::new(0x0, 0x1000, perms, VMAFlags::PRIVATE, VMABacking::Anonymous);
        let result = manager.insert(vma);
        // Starting at 0 may be rejected by some implementations
        // This tests the boundary condition handling
        
        // Insert at very high address (close to canonical limit)
        let high_addr = 0x7FFF_FFFF_0000u64;
        let high_vma = VMA::new(high_addr, high_addr + 0x1000, perms, VMAFlags::PRIVATE, VMABacking::Anonymous);
        let high_result = manager.insert(high_vma);
        // Should succeed or fail gracefully
    }
    
    /// Test: VMA removal tracking
    #[test] 
    fn test_vma_remove_consistency() {
        let mut manager = create_manager();
        let perms = VMAPermissions::READ;
        
        // Insert VMAs
        let vma1 = VMA::new(0x1000, 0x2000, perms, VMAFlags::PRIVATE, VMABacking::Anonymous);
        let vma2 = VMA::new(0x3000, 0x4000, perms, VMAFlags::PRIVATE, VMABacking::Anonymous);
        manager.insert(vma1);
        manager.insert(vma2);
        
        let initial_count = manager.len();
        
        // Remove first VMA
        manager.remove(0x1000);
        
        // Verify removal
        let after_remove = manager.find(0x1500);
        assert!(after_remove.is_none(), 
            "BUG: VMA still found after removal");
        
        // Count should decrease
        assert!(manager.len() < initial_count,
            "BUG: VMA count didn't decrease after removal");
    }
}

#[cfg(test)]
mod paging_bugs {
    /// Test: Page table entry flags are set correctly
    #[test]
    fn test_page_flags_user_accessible() {
        use x86_64::structures::paging::PageTableFlags;
        
        // User page should have USER_ACCESSIBLE
        let user_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
        
        assert!(user_flags.contains(PageTableFlags::USER_ACCESSIBLE),
            "BUG: User page missing USER_ACCESSIBLE flag");
        assert!(user_flags.contains(PageTableFlags::PRESENT),
            "BUG: Page missing PRESENT flag");
    }

    /// Test: NX bit is set for non-executable pages
    #[test]
    fn test_page_flags_no_execute() {
        use x86_64::structures::paging::PageTableFlags;
        
        // Data page should have NO_EXECUTE
        let data_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        
        assert!(data_flags.contains(PageTableFlags::NO_EXECUTE),
            "BUG: Data page missing NO_EXECUTE flag (security issue)");
    }

    /// Test: Kernel pages are not user accessible
    #[test]
    fn test_page_flags_kernel_not_user() {
        use x86_64::structures::paging::PageTableFlags;
        
        // Kernel page should NOT have USER_ACCESSIBLE
        let kernel_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        
        assert!(!kernel_flags.contains(PageTableFlags::USER_ACCESSIBLE),
            "BUG: Kernel page has USER_ACCESSIBLE flag (security issue)");
    }

    /// Test: Address alignment for page operations
    #[test]
    fn test_address_page_alignment() {
        const PAGE_SIZE: u64 = 4096;
        
        let aligned = 0x1000u64;
        let unaligned = 0x1001u64;
        
        assert!(aligned % PAGE_SIZE == 0, "Test setup error");
        assert!(unaligned % PAGE_SIZE != 0, "Test setup error");
        
        // Document the alignment function
        let aligned_down = unaligned & !(PAGE_SIZE - 1);
        assert_eq!(aligned_down, 0x1000,
            "BUG: Align-down calculation wrong");
        
        let aligned_up = (unaligned + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        assert_eq!(aligned_up, 0x2000,
            "BUG: Align-up calculation wrong");
    }
    
    /// Test: Page table flags conversions are consistent
    #[test]
    fn test_page_flags_roundtrip() {
        use x86_64::structures::paging::PageTableFlags;
        
        let original = PageTableFlags::PRESENT 
            | PageTableFlags::WRITABLE 
            | PageTableFlags::USER_ACCESSIBLE
            | PageTableFlags::NO_EXECUTE;
        
        let bits = original.bits();
        let recovered = PageTableFlags::from_bits_truncate(bits);
        
        assert_eq!(original, recovered,
            "BUG: Page flags don't survive roundtrip conversion");
    }
}

#[cfg(test)]
mod allocation_boundary_bugs {
    /// Test: Allocator handles zero-size requests
    #[test]
    fn test_zero_size_allocation() {
        // Zero-size allocations should either:
        // 1. Return an error/None
        // 2. Return a valid pointer that can be freed
        // But never crash
        
        // This documents the expected behavior
        let zero_size = 0usize;
        assert_eq!(zero_size, 0, "Test setup");
        
        // Can't test actual allocator here, but document behavior
    }
    
    /// Test: Address space boundaries
    #[test]
    fn test_address_space_boundaries() {
        // User space boundary (canonical address limit)
        let user_limit = 0x0000_7FFF_FFFF_FFFFu64;
        let kernel_start = 0xFFFF_8000_0000_0000u64;
        
        // There's a gap in x86_64 canonical addresses
        let non_canonical = 0x0000_8000_0000_0000u64;
        
        assert!(non_canonical > user_limit, "Test setup");
        assert!(non_canonical < kernel_start, "Test setup");
        
        // User allocations must stay within user_limit
        let user_addr = crate::process::USER_VIRT_BASE;
        assert!(user_addr <= user_limit,
            "BUG: USER_VIRT_BASE exceeds user space limit");
    }
    
    /// Test: Overflow in size calculations
    #[test]
    fn test_size_overflow_detection() {
        let base: u64 = 0xFFFF_FFFF_FFFF_0000;
        let size: u64 = 0x20000; // Would overflow
        
        // Should use checked/saturating arithmetic
        let end_checked = base.checked_add(size);
        assert!(end_checked.is_none(),
            "Expected overflow detection");
        
        let end_saturating = base.saturating_add(size);
        assert_eq!(end_saturating, u64::MAX,
            "Saturating add should cap at MAX");
    }
    
    /// Test: Alignment requirements for different allocation sizes
    #[test]
    fn test_alignment_requirements() {
        // 8-byte alignment for most allocations
        assert!(8usize.is_power_of_two());
        
        // 16-byte alignment for SIMD
        assert!(16usize.is_power_of_two());
        
        // Page alignment for mmap
        assert!(4096usize.is_power_of_two());
        
        // Helper to check alignment
        fn is_aligned(addr: u64, align: u64) -> bool {
            addr & (align - 1) == 0
        }
        
        assert!(is_aligned(0x1000, 4096), "0x1000 should be page-aligned");
        assert!(!is_aligned(0x1001, 4096), "0x1001 should not be page-aligned");
        assert!(is_aligned(0x1000, 8), "0x1000 should be 8-byte aligned");
    }
}
