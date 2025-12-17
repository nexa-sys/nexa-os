//! VMA (Virtual Memory Area) Edge Case Tests
//!
//! Tests for VMA management including overlapping regions, permission handling,
//! and memory protection edge cases.

#[cfg(test)]
mod tests {
    use crate::mm::vma::{VMAFlags, VMAPermissions, PAGE_SIZE, MAX_VMAS};

    // =========================================================================
    // VMA Permission Tests
    // =========================================================================

    #[test]
    fn test_vma_permissions_none() {
        let perms = VMAPermissions::NONE;
        assert!(!perms.is_read());
        assert!(!perms.is_write());
        assert!(!perms.is_exec());
    }

    #[test]
    fn test_vma_permissions_read() {
        let perms = VMAPermissions::READ;
        assert!(perms.is_read());
        assert!(!perms.is_write());
        assert!(!perms.is_exec());
    }

    #[test]
    fn test_vma_permissions_write() {
        let perms = VMAPermissions::WRITE;
        assert!(!perms.is_read());
        assert!(perms.is_write());
        assert!(!perms.is_exec());
    }

    #[test]
    fn test_vma_permissions_exec() {
        let perms = VMAPermissions::EXEC;
        assert!(!perms.is_read());
        assert!(!perms.is_write());
        assert!(perms.is_exec());
    }

    #[test]
    fn test_vma_permissions_combined() {
        let perms = VMAPermissions::READ | VMAPermissions::WRITE;
        assert!(perms.is_read());
        assert!(perms.is_write());
        assert!(!perms.is_exec());
    }

    #[test]
    fn test_vma_permissions_all() {
        let perms = VMAPermissions::READ | VMAPermissions::WRITE | VMAPermissions::EXEC;
        assert!(perms.is_read());
        assert!(perms.is_write());
        assert!(perms.is_exec());
    }

    #[test]
    fn test_vma_permissions_from_prot() {
        // PROT_NONE = 0
        let perms = VMAPermissions::from_prot(0);
        assert!(!perms.is_read());
        assert!(!perms.is_write());
        assert!(!perms.is_exec());

        // PROT_READ = 1
        let perms = VMAPermissions::from_prot(1);
        assert!(perms.is_read());
        assert!(!perms.is_write());

        // PROT_WRITE = 2
        let perms = VMAPermissions::from_prot(2);
        assert!(!perms.is_read());
        assert!(perms.is_write());

        // PROT_EXEC = 4
        let perms = VMAPermissions::from_prot(4);
        assert!(!perms.is_read());
        assert!(perms.is_exec());

        // PROT_READ | PROT_WRITE | PROT_EXEC = 7
        let perms = VMAPermissions::from_prot(7);
        assert!(perms.is_read());
        assert!(perms.is_write());
        assert!(perms.is_exec());
    }

    #[test]
    fn test_vma_permissions_to_prot() {
        let perms = VMAPermissions::READ | VMAPermissions::EXEC;
        assert_eq!(perms.to_prot(), 5); // 1 + 4
    }

    #[test]
    fn test_vma_permissions_bitand() {
        let perms1 = VMAPermissions::READ | VMAPermissions::WRITE;
        let perms2 = VMAPermissions::READ | VMAPermissions::EXEC;
        let result = perms1 & perms2;
        
        assert!(result.is_read());
        assert!(!result.is_write());
        assert!(!result.is_exec());
    }

    // =========================================================================
    // VMA Flags Tests
    // =========================================================================

    #[test]
    fn test_vma_flags_none() {
        let flags = VMAFlags::NONE;
        assert!(!flags.contains(VMAFlags::SHARED));
        assert!(!flags.contains(VMAFlags::PRIVATE));
        assert!(!flags.contains(VMAFlags::ANONYMOUS));
    }

    #[test]
    fn test_vma_flags_shared() {
        let flags = VMAFlags::SHARED;
        assert!(flags.contains(VMAFlags::SHARED));
        assert!(!flags.contains(VMAFlags::PRIVATE));
    }

    #[test]
    fn test_vma_flags_private() {
        let flags = VMAFlags::PRIVATE;
        assert!(!flags.contains(VMAFlags::SHARED));
        assert!(flags.contains(VMAFlags::PRIVATE));
    }

    #[test]
    fn test_vma_flags_anonymous() {
        let flags = VMAFlags::ANONYMOUS;
        assert!(flags.contains(VMAFlags::ANONYMOUS));
    }

    #[test]
    fn test_vma_flags_from_mmap() {
        // MAP_SHARED = 0x01
        let flags = VMAFlags::from_mmap_flags(0x01);
        assert!(flags.contains(VMAFlags::SHARED));

        // MAP_PRIVATE = 0x02
        let flags = VMAFlags::from_mmap_flags(0x02);
        assert!(flags.contains(VMAFlags::PRIVATE));

        // MAP_FIXED = 0x10
        let flags = VMAFlags::from_mmap_flags(0x10);
        assert!(flags.contains(VMAFlags::FIXED));

        // MAP_ANONYMOUS = 0x20
        let flags = VMAFlags::from_mmap_flags(0x20);
        assert!(flags.contains(VMAFlags::ANONYMOUS));

        // MAP_POPULATE = 0x8000
        let flags = VMAFlags::from_mmap_flags(0x8000);
        assert!(flags.contains(VMAFlags::POPULATE));
    }

    #[test]
    fn test_vma_flags_combined_mmap() {
        // MAP_PRIVATE | MAP_ANONYMOUS = 0x02 | 0x20 = 0x22
        let flags = VMAFlags::from_mmap_flags(0x22);
        assert!(flags.contains(VMAFlags::PRIVATE));
        assert!(flags.contains(VMAFlags::ANONYMOUS));
        assert!(!flags.contains(VMAFlags::SHARED));
    }

    #[test]
    fn test_vma_flags_special_types() {
        let stack = VMAFlags::STACK;
        let heap = VMAFlags::HEAP;
        let code = VMAFlags::CODE;
        
        assert!(stack.contains(VMAFlags::STACK));
        assert!(heap.contains(VMAFlags::HEAP));
        assert!(code.contains(VMAFlags::CODE));
        
        assert!(!stack.contains(VMAFlags::HEAP));
        assert!(!heap.contains(VMAFlags::CODE));
    }

    #[test]
    fn test_vma_flags_cow() {
        let flags = VMAFlags::COW;
        assert!(flags.contains(VMAFlags::COW));
    }

    #[test]
    fn test_vma_flags_demand_paging() {
        let flags = VMAFlags::DEMAND;
        assert!(flags.contains(VMAFlags::DEMAND));
        assert!(!flags.contains(VMAFlags::POPULATE));
    }

    // =========================================================================
    // VMA Constants Tests
    // =========================================================================

    #[test]
    fn test_page_size() {
        assert_eq!(PAGE_SIZE, 4096);
        assert!(PAGE_SIZE.is_power_of_two());
    }

    #[test]
    fn test_max_vmas() {
        assert_eq!(MAX_VMAS, 256);
        assert!(MAX_VMAS >= 64, "Should support at least 64 VMAs per process");
    }

    // =========================================================================
    // VMA Range Tests (Simulated)
    // =========================================================================

    #[derive(Debug, Clone, Copy)]
    struct VMARange {
        start: u64,
        end: u64,
        perms: u64,
        flags: u64,
    }

    impl VMARange {
        fn new(start: u64, end: u64) -> Self {
            Self {
                start,
                end,
                perms: 0,
                flags: 0,
            }
        }

        fn len(&self) -> u64 {
            self.end - self.start
        }

        fn contains(&self, addr: u64) -> bool {
            addr >= self.start && addr < self.end
        }

        fn overlaps(&self, other: &VMARange) -> bool {
            self.start < other.end && other.start < self.end
        }

        fn adjacent_before(&self, other: &VMARange) -> bool {
            self.end == other.start
        }

        fn adjacent_after(&self, other: &VMARange) -> bool {
            other.end == self.start
        }

        fn can_merge(&self, other: &VMARange) -> bool {
            (self.adjacent_before(other) || self.adjacent_after(other))
                && self.perms == other.perms
                && self.flags == other.flags
        }
    }

    #[test]
    fn test_vma_range_contains() {
        let vma = VMARange::new(0x1000, 0x2000);
        
        assert!(!vma.contains(0x0FFF));
        assert!(vma.contains(0x1000));
        assert!(vma.contains(0x1500));
        assert!(vma.contains(0x1FFF));
        assert!(!vma.contains(0x2000)); // End is exclusive
    }

    #[test]
    fn test_vma_range_overlaps() {
        let vma1 = VMARange::new(0x1000, 0x2000);
        let vma2 = VMARange::new(0x1500, 0x2500);
        let vma3 = VMARange::new(0x2000, 0x3000);
        let vma4 = VMARange::new(0x0500, 0x0F00);
        
        assert!(vma1.overlaps(&vma2));
        assert!(!vma1.overlaps(&vma3)); // Adjacent, not overlapping
        assert!(!vma1.overlaps(&vma4));
    }

    #[test]
    fn test_vma_range_adjacent() {
        let vma1 = VMARange::new(0x1000, 0x2000);
        let vma2 = VMARange::new(0x2000, 0x3000);
        
        assert!(vma1.adjacent_before(&vma2));
        assert!(vma2.adjacent_after(&vma1));
    }

    #[test]
    fn test_vma_range_merge_check() {
        let mut vma1 = VMARange::new(0x1000, 0x2000);
        let mut vma2 = VMARange::new(0x2000, 0x3000);
        
        // Same permissions - can merge
        vma1.perms = 3; // RW
        vma2.perms = 3;
        assert!(vma1.can_merge(&vma2));
        
        // Different permissions - cannot merge
        vma2.perms = 5; // RX
        assert!(!vma1.can_merge(&vma2));
    }

    #[test]
    fn test_vma_split() {
        let vma = VMARange::new(0x1000, 0x4000);
        let split_point = 0x2000;
        
        let left = VMARange::new(vma.start, split_point);
        let right = VMARange::new(split_point, vma.end);
        
        assert_eq!(left.len() + right.len(), vma.len());
        assert!(left.adjacent_before(&right));
    }

    // =========================================================================
    // Address Space Layout Tests
    // =========================================================================

    #[test]
    fn test_address_space_layout() {
        // Typical Linux-like address space layout
        const USER_START: u64 = 0x0000_0001_0000_0000; // 4GB
        const USER_END: u64 = 0x0000_7FFF_FFFF_FFFF;
        const KERNEL_START: u64 = 0xFFFF_8000_0000_0000;
        
        // User space should be in lower half
        assert!(USER_END < KERNEL_START);
        
        // Kernel space should be in higher half
        assert!(KERNEL_START > USER_END);
    }

    #[test]
    fn test_mmap_region_finding() {
        // Simulate finding a free region in address space
        struct AddressSpace {
            vmas: Vec<VMARange>,
        }

        impl AddressSpace {
            fn find_free_region(&self, size: u64, hint: u64) -> Option<u64> {
                let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
                let mut start = hint;
                
                // Check each VMA to find a gap
                for vma in &self.vmas {
                    if start + aligned_size <= vma.start {
                        return Some(start);
                    }
                    start = vma.end;
                }
                
                Some(start) // After all VMAs
            }
        }
        
        let mut space = AddressSpace { vmas: Vec::new() };
        space.vmas.push(VMARange::new(0x1000, 0x2000));
        space.vmas.push(VMARange::new(0x4000, 0x5000));
        
        // Should find gap between 0x2000 and 0x4000
        let addr = space.find_free_region(0x1000, 0x1000);
        assert_eq!(addr, Some(0x2000));
    }

    // =========================================================================
    // Page Table Flag Conversion Tests
    // =========================================================================

    #[test]
    fn test_permissions_to_page_flags() {
        use x86_64::structures::paging::PageTableFlags;
        
        // Read-only
        let perms = VMAPermissions::READ;
        let flags = perms.to_page_flags();
        assert!(flags & PageTableFlags::PRESENT.bits() != 0);
        assert!(flags & PageTableFlags::USER_ACCESSIBLE.bits() != 0);
        assert!(flags & PageTableFlags::WRITABLE.bits() == 0);
        assert!(flags & PageTableFlags::NO_EXECUTE.bits() != 0);
        
        // Read-write
        let perms = VMAPermissions::READ | VMAPermissions::WRITE;
        let flags = perms.to_page_flags();
        assert!(flags & PageTableFlags::WRITABLE.bits() != 0);
        
        // Read-execute
        let perms = VMAPermissions::READ | VMAPermissions::EXEC;
        let flags = perms.to_page_flags();
        assert!(flags & PageTableFlags::NO_EXECUTE.bits() == 0);
    }

    // =========================================================================
    // Edge Cases and Bug Detection
    // =========================================================================

    #[test]
    fn test_zero_size_vma() {
        let vma = VMARange::new(0x1000, 0x1000);
        assert_eq!(vma.len(), 0);
        assert!(!vma.contains(0x1000)); // Empty VMA contains nothing
    }

    #[test]
    fn test_large_vma() {
        let vma = VMARange::new(0, 0x1_0000_0000); // 4GB
        assert_eq!(vma.len(), 0x1_0000_0000);
        assert!(vma.contains(0));
        assert!(vma.contains(0xFFFF_FFFF));
    }

    #[test]
    fn test_vma_at_page_boundary() {
        let vma = VMARange::new(0x1000, 0x2000);
        
        // Start should be page-aligned
        assert_eq!(vma.start % PAGE_SIZE, 0);
        // End should be page-aligned
        assert_eq!(vma.end % PAGE_SIZE, 0);
    }

    #[test]
    fn test_non_aligned_vma() {
        // This would be a bug in real code - VMAs should be page-aligned
        let vma = VMARange::new(0x1001, 0x1FFF);
        
        // Detect misalignment
        assert_ne!(vma.start % PAGE_SIZE, 0);
        assert_ne!(vma.end % PAGE_SIZE, 0);
    }

    #[test]
    fn test_overlapping_vma_detection() {
        let vmas = vec![
            VMARange::new(0x1000, 0x2000),
            VMARange::new(0x3000, 0x4000),
            VMARange::new(0x1500, 0x2500), // Overlaps with first!
        ];
        
        // Check for overlaps
        let mut has_overlap = false;
        for i in 0..vmas.len() {
            for j in (i + 1)..vmas.len() {
                if vmas[i].overlaps(&vmas[j]) {
                    has_overlap = true;
                }
            }
        }
        
        assert!(has_overlap, "Should detect overlapping VMAs");
    }
}
