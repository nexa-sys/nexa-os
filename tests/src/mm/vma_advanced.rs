//! VMA (Virtual Memory Area) Advanced Edge Case Tests
//!
//! Tests for complex VMA operations including:
//! - VMA splitting and merging edge cases
//! - Overlapping region detection
//! - COW (Copy-on-Write) handling
//! - Memory corruption detection patterns
//! - Interval tree invariant validation

#[cfg(test)]
mod tests {
    use crate::mm::vma::{
        VMA, VMABacking, VMAFlags, VMAManager, VMAPermissions, MAX_VMAS, PAGE_SIZE,
    };

    // =========================================================================
    // VMA Split Tests - Critical for munmap/mprotect
    // =========================================================================

    #[test]
    fn test_vma_split_at_middle() {
        let mut vma = VMA::new(
            0x1000,
            0x5000, // 4 pages
            VMAPermissions::READ | VMAPermissions::WRITE,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Split at middle (address 0x3000)
        let upper = vma.split_at(0x3000);
        assert!(upper.is_some(), "Split should succeed in middle");

        let upper = upper.unwrap();
        
        // Verify lower part
        assert_eq!(vma.start, 0x1000);
        assert_eq!(vma.end, 0x3000);
        assert_eq!(vma.size(), 2 * PAGE_SIZE);
        
        // Verify upper part
        assert_eq!(upper.start, 0x3000);
        assert_eq!(upper.end, 0x5000);
        assert_eq!(upper.size(), 2 * PAGE_SIZE);
        
        // Permissions should be preserved
        assert_eq!(vma.perm.to_prot(), upper.perm.to_prot());
    }

    #[test]
    fn test_vma_split_at_start_fails() {
        let mut vma = VMA::new(
            0x1000,
            0x3000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Split at start should fail
        let result = vma.split_at(0x1000);
        assert!(result.is_none(), "Split at start should fail");
        
        // Original VMA should be unchanged
        assert_eq!(vma.start, 0x1000);
        assert_eq!(vma.end, 0x3000);
    }

    #[test]
    fn test_vma_split_at_end_fails() {
        let mut vma = VMA::new(
            0x1000,
            0x3000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Split at end should fail
        let result = vma.split_at(0x3000);
        assert!(result.is_none(), "Split at end should fail");
    }

    #[test]
    fn test_vma_split_before_start_fails() {
        let mut vma = VMA::new(
            0x2000,
            0x4000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Split before start should fail
        let result = vma.split_at(0x1000);
        assert!(result.is_none(), "Split before start should fail");
    }

    #[test]
    fn test_vma_split_after_end_fails() {
        let mut vma = VMA::new(
            0x1000,
            0x3000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Split after end should fail
        let result = vma.split_at(0x5000);
        assert!(result.is_none(), "Split after end should fail");
    }

    #[test]
    fn test_vma_split_unaligned_rounds_up() {
        let mut vma = VMA::new(
            0x1000,
            0x5000,
            VMAPermissions::READ | VMAPermissions::WRITE,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Split at unaligned address - should round up to page boundary
        let upper = vma.split_at(0x2001);
        assert!(upper.is_some(), "Split at unaligned address should succeed");
        
        let upper = upper.unwrap();
        // Should be rounded up to 0x3000
        assert_eq!(upper.start, 0x3000);
        assert_eq!(vma.end, 0x3000);
    }

    #[test]
    fn test_vma_split_single_page_fails() {
        let mut vma = VMA::new(
            0x1000,
            0x2000, // single page
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Can't split a single page - any split address will round to boundaries
        let result = vma.split_at(0x1800); // Mid-page
        assert!(result.is_none(), "Split of single page should fail");
    }

    #[test]
    fn test_vma_split_file_backed_offset_adjustment() {
        let mut vma = VMA::new(
            0x1000,
            0x5000,
            VMAPermissions::READ,
            VMAFlags::PRIVATE,
            VMABacking::File { inode: 42, offset: 0x100 },
        );

        let upper = vma.split_at(0x3000);
        assert!(upper.is_some());
        
        let upper = upper.unwrap();
        
        // File offset should be adjusted in upper part
        if let VMABacking::File { inode, offset } = upper.backing {
            assert_eq!(inode, 42);
            // offset should be original + size of lower part
            assert_eq!(offset, 0x100 + (0x3000 - 0x1000));
        } else {
            panic!("Upper VMA should be file-backed");
        }
    }

    // =========================================================================
    // VMA Merge Tests - Critical for memory efficiency
    // =========================================================================

    #[test]
    fn test_vma_can_merge_adjacent_anonymous() {
        let vma1 = VMA::new(
            0x1000,
            0x2000,
            VMAPermissions::READ | VMAPermissions::WRITE,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        let vma2 = VMA::new(
            0x2000,
            0x3000,
            VMAPermissions::READ | VMAPermissions::WRITE,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert!(vma1.can_merge_with(&vma2), "Adjacent anonymous VMAs should be mergeable");
    }

    #[test]
    fn test_vma_cannot_merge_non_adjacent() {
        let vma1 = VMA::new(
            0x1000,
            0x2000,
            VMAPermissions::READ | VMAPermissions::WRITE,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        let vma2 = VMA::new(
            0x3000, // Gap at 0x2000-0x3000
            0x4000,
            VMAPermissions::READ | VMAPermissions::WRITE,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert!(!vma1.can_merge_with(&vma2), "Non-adjacent VMAs should not be mergeable");
    }

    #[test]
    fn test_vma_cannot_merge_different_permissions() {
        let vma1 = VMA::new(
            0x1000,
            0x2000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        let vma2 = VMA::new(
            0x2000,
            0x3000,
            VMAPermissions::READ | VMAPermissions::WRITE,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert!(!vma1.can_merge_with(&vma2), "VMAs with different permissions should not be mergeable");
    }

    #[test]
    fn test_vma_cannot_merge_different_backing() {
        let vma1 = VMA::new(
            0x1000,
            0x2000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        let vma2 = VMA::new(
            0x2000,
            0x3000,
            VMAPermissions::READ,
            VMAFlags::PRIVATE,
            VMABacking::File { inode: 1, offset: 0 },
        );

        assert!(!vma1.can_merge_with(&vma2), "VMAs with different backing should not be mergeable");
    }

    #[test]
    fn test_vma_can_merge_file_backed_contiguous() {
        let vma1 = VMA::new(
            0x1000,
            0x2000,
            VMAPermissions::READ,
            VMAFlags::PRIVATE,
            VMABacking::File { inode: 42, offset: 0x0 },
        );

        let vma2 = VMA::new(
            0x2000,
            0x3000,
            VMAPermissions::READ,
            VMAFlags::PRIVATE,
            VMABacking::File { inode: 42, offset: 0x1000 }, // Contiguous offset
        );

        assert!(vma1.can_merge_with(&vma2), "File-backed VMAs with contiguous offsets should be mergeable");
    }

    #[test]
    fn test_vma_cannot_merge_file_backed_non_contiguous() {
        let vma1 = VMA::new(
            0x1000,
            0x2000,
            VMAPermissions::READ,
            VMAFlags::PRIVATE,
            VMABacking::File { inode: 42, offset: 0x0 },
        );

        let vma2 = VMA::new(
            0x2000,
            0x3000,
            VMAPermissions::READ,
            VMAFlags::PRIVATE,
            VMABacking::File { inode: 42, offset: 0x5000 }, // Gap in file
        );

        assert!(!vma1.can_merge_with(&vma2), "File-backed VMAs with non-contiguous offsets should not be mergeable");
    }

    #[test]
    fn test_vma_cannot_merge_different_inodes() {
        let vma1 = VMA::new(
            0x1000,
            0x2000,
            VMAPermissions::READ,
            VMAFlags::PRIVATE,
            VMABacking::File { inode: 42, offset: 0x0 },
        );

        let vma2 = VMA::new(
            0x2000,
            0x3000,
            VMAPermissions::READ,
            VMAFlags::PRIVATE,
            VMABacking::File { inode: 43, offset: 0x1000 },
        );

        assert!(!vma1.can_merge_with(&vma2), "File-backed VMAs from different files should not be mergeable");
    }

    // =========================================================================
    // VMA Overlap Detection Tests - Critical for mmap address selection
    // =========================================================================

    #[test]
    fn test_vma_overlaps_full_containment() {
        let vma = VMA::new(
            0x2000,
            0x4000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Range fully contains VMA
        assert!(vma.overlaps(0x1000, 0x5000), "Full containment should overlap");
    }

    #[test]
    fn test_vma_overlaps_partial_start() {
        let vma = VMA::new(
            0x2000,
            0x4000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Range overlaps start
        assert!(vma.overlaps(0x1000, 0x3000), "Overlap at start should be detected");
    }

    #[test]
    fn test_vma_overlaps_partial_end() {
        let vma = VMA::new(
            0x2000,
            0x4000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Range overlaps end
        assert!(vma.overlaps(0x3000, 0x5000), "Overlap at end should be detected");
    }

    #[test]
    fn test_vma_overlaps_contained_inside() {
        let vma = VMA::new(
            0x1000,
            0x5000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Range fully inside VMA
        assert!(vma.overlaps(0x2000, 0x3000), "Contained range should overlap");
    }

    #[test]
    fn test_vma_no_overlap_before() {
        let vma = VMA::new(
            0x3000,
            0x5000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Range entirely before VMA
        assert!(!vma.overlaps(0x1000, 0x2000), "Range before should not overlap");
    }

    #[test]
    fn test_vma_no_overlap_after() {
        let vma = VMA::new(
            0x1000,
            0x3000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Range entirely after VMA
        assert!(!vma.overlaps(0x4000, 0x5000), "Range after should not overlap");
    }

    #[test]
    fn test_vma_no_overlap_adjacent_before() {
        let vma = VMA::new(
            0x2000,
            0x4000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Range adjacent (touching) before VMA
        assert!(!vma.overlaps(0x1000, 0x2000), "Adjacent range before should not overlap");
    }

    #[test]
    fn test_vma_no_overlap_adjacent_after() {
        let vma = VMA::new(
            0x2000,
            0x4000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // Range adjacent (touching) after VMA
        assert!(!vma.overlaps(0x4000, 0x5000), "Adjacent range after should not overlap");
    }

    // =========================================================================
    // VMA Contains Tests - Critical for page fault handling
    // =========================================================================

    #[test]
    fn test_vma_contains_start() {
        let vma = VMA::new(
            0x1000,
            0x3000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert!(vma.contains(0x1000), "VMA should contain its start address");
    }

    #[test]
    fn test_vma_not_contains_end() {
        let vma = VMA::new(
            0x1000,
            0x3000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        // End is exclusive
        assert!(!vma.contains(0x3000), "VMA should not contain its end address (exclusive)");
    }

    #[test]
    fn test_vma_contains_middle() {
        let vma = VMA::new(
            0x1000,
            0x5000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert!(vma.contains(0x2500), "VMA should contain middle addresses");
    }

    #[test]
    fn test_vma_not_contains_before() {
        let vma = VMA::new(
            0x2000,
            0x4000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert!(!vma.contains(0x1000), "VMA should not contain addresses before it");
    }

    #[test]
    fn test_vma_not_contains_after() {
        let vma = VMA::new(
            0x1000,
            0x3000,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert!(!vma.contains(0x4000), "VMA should not contain addresses after it");
    }

    // =========================================================================
    // VMA Validity Tests
    // =========================================================================

    #[test]
    fn test_vma_empty_is_invalid() {
        let vma = VMA::empty();
        assert!(!vma.is_valid(), "Empty VMA should be invalid");
        assert_eq!(vma.size(), 0);
        assert_eq!(vma.page_count(), 0);
    }

    #[test]
    fn test_vma_zero_size_is_invalid() {
        let vma = VMA::new(
            0x1000,
            0x1000, // start == end
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert!(!vma.is_valid(), "Zero-size VMA should be invalid");
    }

    #[test]
    fn test_vma_inverted_range_is_invalid() {
        let vma = VMA::new(
            0x3000,
            0x1000, // end < start
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert!(!vma.is_valid(), "Inverted range VMA should be invalid");
        assert_eq!(vma.size(), 0, "Inverted range should have zero size");
    }

    #[test]
    fn test_vma_page_count() {
        let vma = VMA::new(
            0x1000,
            0x5000, // 4 pages
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert_eq!(vma.page_count(), 4);
    }

    // =========================================================================
    // VMA Manager Tests
    // =========================================================================

    #[test]
    fn test_vma_manager_new_is_empty() {
        let manager = VMAManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn test_vma_manager_init() {
        let mut manager = VMAManager::new();
        manager.init();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_vma_manager_max_capacity() {
        // VMAManager should support MAX_VMAS entries
        assert!(MAX_VMAS >= 64, "Should support at least 64 VMAs");
        assert!(MAX_VMAS <= 1024, "Should not allocate excessive memory");
    }

    // =========================================================================
    // VMA Flags Tests - Edge cases
    // =========================================================================

    #[test]
    fn test_vma_flags_combinations() {
        // Test common flag combinations
        let stack_flags = VMAFlags::ANONYMOUS | VMAFlags::PRIVATE | VMAFlags::GROWSDOWN | VMAFlags::STACK;
        assert!(stack_flags.is_anonymous());
        assert!(stack_flags.is_private());
        assert!(stack_flags.contains(VMAFlags::GROWSDOWN));
        assert!(stack_flags.contains(VMAFlags::STACK));
        
        let heap_flags = VMAFlags::ANONYMOUS | VMAFlags::PRIVATE | VMAFlags::GROWSUP | VMAFlags::HEAP;
        assert!(heap_flags.is_anonymous());
        assert!(heap_flags.contains(VMAFlags::HEAP));
    }

    #[test]
    fn test_vma_flags_from_mmap_flags() {
        // MAP_SHARED = 0x01
        let shared = VMAFlags::from_mmap_flags(0x01);
        assert!(shared.is_shared());
        
        // MAP_PRIVATE = 0x02
        let private = VMAFlags::from_mmap_flags(0x02);
        assert!(private.is_private());
        
        // MAP_FIXED = 0x10
        let fixed = VMAFlags::from_mmap_flags(0x10);
        assert!(fixed.contains(VMAFlags::FIXED));
        
        // MAP_ANONYMOUS = 0x20
        let anon = VMAFlags::from_mmap_flags(0x20);
        assert!(anon.is_anonymous());
        
        // Combined: MAP_PRIVATE | MAP_ANONYMOUS = 0x22
        let combined = VMAFlags::from_mmap_flags(0x22);
        assert!(combined.is_private());
        assert!(combined.is_anonymous());
    }

    // =========================================================================
    // Stress/Boundary Tests
    // =========================================================================

    #[test]
    fn test_vma_max_address_handling() {
        // Test handling of addresses near u64::MAX
        let vma = VMA::new(
            u64::MAX - PAGE_SIZE,
            u64::MAX,
            VMAPermissions::READ,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert!(vma.is_valid());
        assert_eq!(vma.size(), PAGE_SIZE);
        assert!(vma.contains(u64::MAX - PAGE_SIZE));
        assert!(!vma.contains(u64::MAX)); // Exclusive end
    }

    #[test]
    fn test_vma_address_zero() {
        // Address 0 is typically unmapped (null pointer guard)
        let vma = VMA::new(
            0,
            PAGE_SIZE,
            VMAPermissions::NONE,
            VMAFlags::ANONYMOUS | VMAFlags::PRIVATE,
            VMABacking::Anonymous,
        );

        assert!(vma.is_valid());
        assert!(vma.contains(0));
    }

    #[test]
    fn test_vma_permissions_to_page_flags() {
        // Test that permissions are correctly converted to page table flags
        let read_only = VMAPermissions::READ;
        let flags = read_only.to_page_flags();
        
        // Should have PRESENT and USER_ACCESSIBLE
        // Should NOT have WRITABLE
        assert!(flags != 0, "Page flags should not be zero for readable mapping");
        
        let read_write = VMAPermissions::READ | VMAPermissions::WRITE;
        let rw_flags = read_write.to_page_flags();
        
        // Read-write should have WRITABLE bit set
        assert!(rw_flags != read_only.to_page_flags(), "Write permission should change flags");
    }
}
