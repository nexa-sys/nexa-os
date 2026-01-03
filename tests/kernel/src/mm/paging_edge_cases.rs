//! Paging subsystem edge case tests
//!
//! Tests for memory paging boundary conditions, address space management,
//! and user region allocation/deallocation.
//! Uses REAL kernel functions from crate::safety::paging

#[cfg(test)]
mod tests {
    use crate::process::{
        HEAP_BASE, HEAP_SIZE, INTERP_BASE, INTERP_REGION_SIZE,
        STACK_BASE, STACK_SIZE, USER_PHYS_BASE, USER_REGION_SIZE, USER_VIRT_BASE,
    };
    // Use REAL kernel functions
    use crate::safety::{
        page_table_indices, is_canonical_address, is_kernel_address, is_user_address,
        page_offset, page_frame_number, align_up, PAGE_SIZE,
    };
    use crate::mm::{size_to_order, order_to_size};

    // =========================================================================
    // Memory Layout Constant Tests
    // =========================================================================

    #[test]
    fn test_user_memory_layout_no_overlap() {
        // Verify that user memory regions don't overlap
        let user_code_end = USER_VIRT_BASE + 0x200000;
        
        assert!(user_code_end <= HEAP_BASE, 
            "Code region {:#x} overlaps with heap {:#x}", user_code_end, HEAP_BASE);
        
        let heap_end = HEAP_BASE + HEAP_SIZE;
        assert!(heap_end <= STACK_BASE,
            "Heap end {:#x} overlaps with stack {:#x}", heap_end, STACK_BASE);
        
        let stack_end = STACK_BASE + STACK_SIZE;
        assert!(stack_end <= INTERP_BASE,
            "Stack end {:#x} overlaps with interpreter {:#x}", stack_end, INTERP_BASE);
    }

    #[test]
    fn test_user_memory_layout_constants() {
        assert_eq!(USER_VIRT_BASE, 0x1000000, "USER_VIRT_BASE should be 16MB");
        assert_eq!(HEAP_BASE, USER_VIRT_BASE + 0x200000, "HEAP_BASE should be USER_VIRT_BASE + 2MB");
        assert_eq!(HEAP_SIZE, 0x800000, "HEAP_SIZE should be 8MB");
        assert_eq!(STACK_SIZE, 0x200000, "STACK_SIZE should be 2MB");
    }

    #[test]
    fn test_user_region_size_calculation() {
        let expected = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;
        assert_eq!(USER_REGION_SIZE, expected, "USER_REGION_SIZE calculation mismatch");
    }

    #[test]
    fn test_memory_alignment() {
        // All memory regions should be page-aligned (4KB)
        assert_eq!(USER_VIRT_BASE & (PAGE_SIZE - 1), 0, "USER_VIRT_BASE not page-aligned");
        assert_eq!(USER_PHYS_BASE & (PAGE_SIZE - 1), 0, "USER_PHYS_BASE not page-aligned");
        assert_eq!(HEAP_BASE & (PAGE_SIZE - 1), 0, "HEAP_BASE not page-aligned");
        assert_eq!(STACK_BASE & (PAGE_SIZE - 1), 0, "STACK_BASE not page-aligned");
        assert_eq!(INTERP_BASE & (PAGE_SIZE - 1), 0, "INTERP_BASE not page-aligned");
    }

    #[test]
    fn test_huge_page_alignment() {
        const HUGE_PAGE_SIZE: u64 = 2 * 1024 * 1024;
        
        assert_eq!(STACK_SIZE, HUGE_PAGE_SIZE, "STACK_SIZE should be 2MB");
        assert_eq!(STACK_BASE & (HUGE_PAGE_SIZE - 1), 0, 
            "STACK_BASE {:#x} not 2MB-aligned for huge pages", STACK_BASE);
    }

    // =========================================================================
    // Page Table Level Tests - Using REAL kernel page_table_indices
    // =========================================================================

    #[test]
    fn test_page_table_index_extraction() {
        // Using REAL kernel function page_table_indices
        let user_base = USER_VIRT_BASE;
        let (pml4, pdpt, pd, _pt) = page_table_indices(user_base);
        assert_eq!(pml4, 0, "PML4 index at USER_VIRT_BASE");
        assert_eq!(pdpt, 0, "PDPT index at USER_VIRT_BASE");
        assert_eq!(pd, 8, "PD index at USER_VIRT_BASE (16MB / 2MB = 8)");
        
        let kernel_addr: u64 = 0xFFFF_8000_0000_0000;
        let (pml4_k, _, _, _) = page_table_indices(kernel_addr);
        assert_eq!(pml4_k, 256, "PML4 index for kernel high half");
    }

    #[test]
    fn test_canonical_address_check() {
        // Using REAL kernel function is_canonical_address
        assert!(is_canonical_address(USER_VIRT_BASE), "USER_VIRT_BASE should be canonical");
        assert!(is_canonical_address(0x0000_7FFF_FFFF_FFFF), "Max user address should be canonical");
        
        assert!(is_canonical_address(0xFFFF_8000_0000_0000), "Kernel base should be canonical");
        assert!(is_canonical_address(0xFFFF_FFFF_FFFF_FFFF), "Max kernel address should be canonical");
        
        assert!(!is_canonical_address(0x0001_0000_0000_0000), "Non-canonical in hole");
        assert!(!is_canonical_address(0xFFFF_0000_0000_0000), "Non-canonical near hole");
    }

    // =========================================================================
    // Address Translation Tests - Using REAL kernel functions
    // =========================================================================

    #[test]
    fn test_page_offset_extraction() {
        // Using REAL kernel function page_offset
        assert_eq!(page_offset(0x1000), 0, "Page-aligned has 0 offset");
        assert_eq!(page_offset(0x1001), 1, "Page + 1 byte offset");
        assert_eq!(page_offset(0x1FFF), 0xFFF, "Last byte in page");
        assert_eq!(page_offset(0x12345678), 0x678, "Random address offset");
    }

    #[test]
    fn test_page_frame_number() {
        // Using REAL kernel function page_frame_number
        assert_eq!(page_frame_number(0x1000), 1, "Second page");
        assert_eq!(page_frame_number(0x1000000), 0x1000, "16MB mark");
        assert_eq!(page_frame_number(USER_VIRT_BASE), USER_VIRT_BASE >> 12);
    }

    // =========================================================================
    // Memory Region Boundary Tests - Using REAL kernel functions
    // =========================================================================

    #[test]
    fn test_address_within_heap() {
        // Helper function using memory constants
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
        // Using REAL kernel functions is_kernel_address, is_user_address
        assert!(is_user_address(USER_VIRT_BASE), "USER_VIRT_BASE is user address");
        assert!(is_user_address(INTERP_BASE + INTERP_REGION_SIZE - 1), "Top of user space");
        assert!(is_kernel_address(0xFFFF_8000_0000_0000), "Kernel base is kernel address");
        assert!(!is_kernel_address(USER_VIRT_BASE), "User address is not kernel");
        assert!(!is_user_address(0xFFFF_8000_0000_0000), "Kernel address is not user");
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
        
        let all_flags = [PRESENT, WRITABLE, USER_ACCESSIBLE, WRITE_THROUGH, 
                        NO_CACHE, ACCESSED, DIRTY, HUGE_PAGE, GLOBAL, NO_EXECUTE];
        
        for i in 0..all_flags.len() {
            for j in i+1..all_flags.len() {
                assert_eq!(all_flags[i] & all_flags[j], 0, "Flags {} and {} overlap", i, j);
            }
        }
        
        let user_page = PRESENT | WRITABLE | USER_ACCESSIBLE;
        assert_ne!(user_page & PRESENT, 0);
        assert_ne!(user_page & WRITABLE, 0);
        assert_ne!(user_page & USER_ACCESSIBLE, 0);
        assert_eq!(user_page & NO_EXECUTE, 0);
    }

    // =========================================================================
    // Physical Memory Allocation Tests - Using REAL kernel functions
    // =========================================================================

    #[test]
    fn test_order_to_pages() {
        // Using REAL kernel order_to_size and converting to pages
        assert_eq!(order_to_size(0) / PAGE_SIZE as usize, 1);
        assert_eq!(order_to_size(1) / PAGE_SIZE as usize, 2);
        assert_eq!(order_to_size(2) / PAGE_SIZE as usize, 4);
        assert_eq!(order_to_size(10) / PAGE_SIZE as usize, 1024);
    }

    #[test]
    fn test_pages_to_order() {
        // Using REAL kernel size_to_order
        assert_eq!(size_to_order(PAGE_SIZE as usize), 0);        // 1 page
        assert_eq!(size_to_order(PAGE_SIZE as usize * 2), 1);    // 2 pages
        assert_eq!(size_to_order(PAGE_SIZE as usize * 4), 2);    // 4 pages
        assert_eq!(size_to_order(PAGE_SIZE as usize * 1024), 10); // 1024 pages
    }

    // =========================================================================
    // Alignment Tests - Using REAL kernel functions
    // =========================================================================

    #[test]
    fn test_align_up() {
        // Using REAL kernel function align_up
        assert_eq!(align_up(0, PAGE_SIZE), 0);
        assert_eq!(align_up(1, PAGE_SIZE), PAGE_SIZE);
        assert_eq!(align_up(PAGE_SIZE, PAGE_SIZE), PAGE_SIZE);
        assert_eq!(align_up(PAGE_SIZE + 1, PAGE_SIZE), PAGE_SIZE * 2);
        assert_eq!(align_up(PAGE_SIZE * 2 - 1, PAGE_SIZE), PAGE_SIZE * 2);
    }

    #[test]
    fn test_validate_allocation_size() {
        const MAX_ALLOCATION: u64 = 8 * 1024 * 1024; // 8MB, same as MAX_ORDER budget
        
        fn validate_allocation_size(size: u64) -> bool {
            size > 0 && size <= MAX_ALLOCATION && size <= (1u64 << 32)
        }
        
        assert!(!validate_allocation_size(0), "Zero size invalid");
        assert!(validate_allocation_size(PAGE_SIZE), "Page size valid");
        assert!(validate_allocation_size(MAX_ALLOCATION), "Max allocation valid");
        assert!(!validate_allocation_size(MAX_ALLOCATION + 1), "Over max invalid");
    }
}
