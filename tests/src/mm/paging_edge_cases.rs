//! Paging subsystem edge case tests
//!
//! Tests for memory paging boundary conditions, address space management,
//! and user region allocation/deallocation.

#[cfg(test)]
mod tests {
    use crate::process::{
        HEAP_BASE, HEAP_SIZE, INTERP_BASE, INTERP_REGION_SIZE,
        STACK_BASE, STACK_SIZE, USER_PHYS_BASE, USER_REGION_SIZE, USER_VIRT_BASE,
    };

    // =========================================================================
    // Memory Layout Constant Tests
    // =========================================================================

    #[test]
    fn test_user_memory_layout_no_overlap() {
        // Verify that user memory regions don't overlap
        let user_code_end = USER_VIRT_BASE + 0x200000; // Code section ends before HEAP_BASE
        
        // Code region should not overlap with heap
        assert!(user_code_end <= HEAP_BASE, 
            "Code region {:#x} overlaps with heap {:#x}", user_code_end, HEAP_BASE);
        
        // Heap should not overlap with stack
        let heap_end = HEAP_BASE + HEAP_SIZE;
        assert!(heap_end <= STACK_BASE,
            "Heap end {:#x} overlaps with stack {:#x}", heap_end, STACK_BASE);
        
        // Stack should not overlap with interpreter
        let stack_end = STACK_BASE + STACK_SIZE;
        assert!(stack_end <= INTERP_BASE,
            "Stack end {:#x} overlaps with interpreter {:#x}", stack_end, INTERP_BASE);
    }

    #[test]
    fn test_user_memory_layout_constants() {
        // Verify documented values match actual constants
        assert_eq!(USER_VIRT_BASE, 0x1000000, "USER_VIRT_BASE should be 16MB");
        assert_eq!(HEAP_BASE, USER_VIRT_BASE + 0x200000, "HEAP_BASE should be USER_VIRT_BASE + 2MB");
        assert_eq!(HEAP_SIZE, 0x800000, "HEAP_SIZE should be 8MB");
        assert_eq!(STACK_SIZE, 0x200000, "STACK_SIZE should be 2MB");
    }

    #[test]
    fn test_user_region_size_calculation() {
        // USER_REGION_SIZE should span from USER_VIRT_BASE to INTERP_BASE + INTERP_REGION_SIZE
        let expected = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;
        assert_eq!(USER_REGION_SIZE, expected, 
            "USER_REGION_SIZE calculation mismatch");
    }

    #[test]
    fn test_memory_alignment() {
        // All memory regions should be page-aligned (4KB)
        const PAGE_SIZE: u64 = 4096;
        
        assert_eq!(USER_VIRT_BASE & (PAGE_SIZE - 1), 0, "USER_VIRT_BASE not page-aligned");
        assert_eq!(USER_PHYS_BASE & (PAGE_SIZE - 1), 0, "USER_PHYS_BASE not page-aligned");
        assert_eq!(HEAP_BASE & (PAGE_SIZE - 1), 0, "HEAP_BASE not page-aligned");
        assert_eq!(STACK_BASE & (PAGE_SIZE - 1), 0, "STACK_BASE not page-aligned");
        assert_eq!(INTERP_BASE & (PAGE_SIZE - 1), 0, "INTERP_BASE not page-aligned");
    }

    #[test]
    fn test_huge_page_alignment() {
        // Stack should be 2MB aligned for huge pages
        const HUGE_PAGE_SIZE: u64 = 2 * 1024 * 1024;
        
        assert_eq!(STACK_SIZE, HUGE_PAGE_SIZE, "STACK_SIZE should be 2MB");
        assert_eq!(STACK_BASE & (HUGE_PAGE_SIZE - 1), 0, 
            "STACK_BASE {:#x} not 2MB-aligned for huge pages", STACK_BASE);
    }

    // =========================================================================
    // Page Table Level Tests
    // =========================================================================

    #[test]
    fn test_page_table_index_extraction() {
        // Test extracting page table indices from virtual addresses
        // x86_64 4-level paging: PML4 -> PDPT -> PD -> PT
        
        fn pml4_index(vaddr: u64) -> usize {
            ((vaddr >> 39) & 0x1FF) as usize
        }
        
        fn pdpt_index(vaddr: u64) -> usize {
            ((vaddr >> 30) & 0x1FF) as usize
        }
        
        fn pd_index(vaddr: u64) -> usize {
            ((vaddr >> 21) & 0x1FF) as usize
        }
        
        fn pt_index(vaddr: u64) -> usize {
            ((vaddr >> 12) & 0x1FF) as usize
        }
        
        // Test USER_VIRT_BASE (0x1000000 = 16MB)
        let user_base = USER_VIRT_BASE;
        assert_eq!(pml4_index(user_base), 0, "PML4 index at USER_VIRT_BASE");
        assert_eq!(pdpt_index(user_base), 0, "PDPT index at USER_VIRT_BASE");
        assert_eq!(pd_index(user_base), 8, "PD index at USER_VIRT_BASE (16MB / 2MB = 8)");
        
        // Test kernel high half address
        let kernel_addr: u64 = 0xFFFF_8000_0000_0000;
        assert_eq!(pml4_index(kernel_addr), 256, "PML4 index for kernel high half");
    }

    #[test]
    fn test_canonical_address_check() {
        // x86_64 canonical address check: bits 48-63 must match bit 47
        
        fn is_canonical(addr: u64) -> bool {
            let sign_bit = (addr >> 47) & 1;
            let high_bits = addr >> 48;
            if sign_bit == 0 {
                high_bits == 0
            } else {
                high_bits == 0xFFFF
            }
        }
        
        // User addresses should be canonical (low half)
        assert!(is_canonical(USER_VIRT_BASE), "USER_VIRT_BASE should be canonical");
        assert!(is_canonical(0x0000_7FFF_FFFF_FFFF), "Max user address should be canonical");
        
        // Kernel addresses should be canonical (high half)
        assert!(is_canonical(0xFFFF_8000_0000_0000), "Kernel base should be canonical");
        assert!(is_canonical(0xFFFF_FFFF_FFFF_FFFF), "Max kernel address should be canonical");
        
        // Non-canonical addresses
        assert!(!is_canonical(0x0001_0000_0000_0000), "Non-canonical in hole");
        assert!(!is_canonical(0xFFFF_0000_0000_0000), "Non-canonical near hole");
    }

    // =========================================================================
    // Address Translation Tests
    // =========================================================================

    #[test]
    fn test_page_offset_extraction() {
        fn page_offset(addr: u64) -> u64 {
            addr & 0xFFF // Lower 12 bits
        }
        
        assert_eq!(page_offset(0x1000), 0, "Page-aligned has 0 offset");
        assert_eq!(page_offset(0x1001), 1, "Page + 1 byte offset");
        assert_eq!(page_offset(0x1FFF), 0xFFF, "Last byte in page");
        assert_eq!(page_offset(0x12345678), 0x678, "Random address offset");
    }

    #[test]
    fn test_page_frame_number() {
        fn page_frame_number(addr: u64) -> u64 {
            addr >> 12
        }
        
        assert_eq!(page_frame_number(0x1000), 1, "Second page");
        assert_eq!(page_frame_number(0x1000000), 0x1000, "16MB mark");
        assert_eq!(page_frame_number(USER_VIRT_BASE), USER_VIRT_BASE >> 12);
    }

    // =========================================================================
    // Memory Region Boundary Tests
    // =========================================================================

    #[test]
    fn test_address_within_heap() {
        fn is_in_heap(addr: u64) -> bool {
            addr >= HEAP_BASE && addr < HEAP_BASE + HEAP_SIZE
        }
        
        assert!(!is_in_heap(HEAP_BASE - 1), "Address before heap");
        assert!(is_in_heap(HEAP_BASE), "First byte of heap");
        assert!(is_in_heap(HEAP_BASE + HEAP_SIZE / 2), "Middle of heap");
        assert!(is_in_heap(HEAP_BASE + HEAP_SIZE - 1), "Last byte of heap");
        assert!(!is_in_heap(HEAP_BASE + HEAP_SIZE), "First byte after heap");
    }

    #[test]
    fn test_address_within_stack() {
        fn is_in_stack(addr: u64) -> bool {
            addr >= STACK_BASE && addr < STACK_BASE + STACK_SIZE
        }
        
        assert!(!is_in_stack(STACK_BASE - 1), "Address before stack");
        assert!(is_in_stack(STACK_BASE), "First byte of stack");
        assert!(is_in_stack(STACK_BASE + STACK_SIZE - 1), "Last byte of stack");
        assert!(!is_in_stack(STACK_BASE + STACK_SIZE), "First byte after stack");
    }

    #[test]
    fn test_kernel_user_boundary() {
        const KERNEL_HIGH_HALF: u64 = 0xFFFF_8000_0000_0000;
        
        fn is_kernel_address(addr: u64) -> bool {
            addr >= KERNEL_HIGH_HALF
        }
        
        fn is_user_address(addr: u64) -> bool {
            addr < KERNEL_HIGH_HALF && addr < 0x0000_8000_0000_0000
        }
        
        assert!(is_user_address(USER_VIRT_BASE), "USER_VIRT_BASE is user address");
        assert!(is_user_address(INTERP_BASE + INTERP_REGION_SIZE - 1), "Top of user space");
        assert!(is_kernel_address(KERNEL_HIGH_HALF), "Kernel base is kernel address");
    }

    // =========================================================================
    // Page Table Entry Flag Tests
    // =========================================================================

    #[test]
    fn test_page_entry_flags() {
        const PRESENT: u64 = 1 << 0;
        const WRITABLE: u64 = 1 << 1;
        const USER_ACCESSIBLE: u64 = 1 << 2;
        const WRITE_THROUGH: u64 = 1 << 3;
        const NO_CACHE: u64 = 1 << 4;
        const ACCESSED: u64 = 1 << 5;
        const DIRTY: u64 = 1 << 6;
        const HUGE_PAGE: u64 = 1 << 7;
        const GLOBAL: u64 = 1 << 8;
        const NO_EXECUTE: u64 = 1 << 63;
        
        // Test that flags don't overlap
        let all_flags = [PRESENT, WRITABLE, USER_ACCESSIBLE, WRITE_THROUGH, 
                        NO_CACHE, ACCESSED, DIRTY, HUGE_PAGE, GLOBAL, NO_EXECUTE];
        
        for i in 0..all_flags.len() {
            for j in i+1..all_flags.len() {
                assert_eq!(all_flags[i] & all_flags[j], 0, 
                    "Flags {} and {} overlap", i, j);
            }
        }
        
        // Test typical user page flags
        let user_page = PRESENT | WRITABLE | USER_ACCESSIBLE;
        assert_ne!(user_page & PRESENT, 0);
        assert_ne!(user_page & WRITABLE, 0);
        assert_ne!(user_page & USER_ACCESSIBLE, 0);
        assert_eq!(user_page & NO_EXECUTE, 0); // Execute allowed by default
    }

    // =========================================================================
    // Physical Memory Allocation Tests
    // =========================================================================

    #[test]
    fn test_order_to_pages() {
        fn order_to_pages(order: usize) -> usize {
            1 << order
        }
        
        assert_eq!(order_to_pages(0), 1);
        assert_eq!(order_to_pages(1), 2);
        assert_eq!(order_to_pages(2), 4);
        assert_eq!(order_to_pages(10), 1024);
    }

    #[test]
    fn test_pages_to_order() {
        fn pages_to_order(pages: usize) -> usize {
            if pages == 0 { return 0; }
            (usize::BITS - (pages - 1).leading_zeros()) as usize
        }
        
        assert_eq!(pages_to_order(1), 0);
        assert_eq!(pages_to_order(2), 1);
        assert_eq!(pages_to_order(3), 2); // Rounds up
        assert_eq!(pages_to_order(4), 2);
        assert_eq!(pages_to_order(5), 3); // Rounds up
    }

    // =========================================================================
    // Overflow and Edge Case Tests
    // =========================================================================

    #[test]
    fn test_address_overflow_protection() {
        // Test that address calculations don't overflow
        let max_user_addr = INTERP_BASE + INTERP_REGION_SIZE;
        
        // This should not overflow
        assert!(max_user_addr < u64::MAX);
        assert!(max_user_addr < 0x0000_8000_0000_0000, "User space within valid range");
    }

    #[test]
    fn test_size_alignment_rounding() {
        const PAGE_SIZE: u64 = 4096;
        
        fn align_up(size: u64, alignment: u64) -> u64 {
            (size + alignment - 1) & !(alignment - 1)
        }
        
        assert_eq!(align_up(0, PAGE_SIZE), 0);
        assert_eq!(align_up(1, PAGE_SIZE), PAGE_SIZE);
        assert_eq!(align_up(PAGE_SIZE, PAGE_SIZE), PAGE_SIZE);
        assert_eq!(align_up(PAGE_SIZE + 1, PAGE_SIZE), PAGE_SIZE * 2);
        
        // Edge case: very large size
        let large_size: u64 = u64::MAX - PAGE_SIZE + 1;
        // This would overflow with naive implementation
        // Safe alignment should handle this
    }

    #[test]
    fn test_zero_size_allocation() {
        // Zero-size allocations should fail or be handled gracefully
        const PAGE_SIZE: u64 = 4096;
        
        fn validate_allocation_size(size: u64) -> bool {
            size > 0 && size <= 0x1_0000_0000_0000 // 256TB max
        }
        
        assert!(!validate_allocation_size(0), "Zero size should fail");
        assert!(validate_allocation_size(PAGE_SIZE), "One page should succeed");
    }
}
