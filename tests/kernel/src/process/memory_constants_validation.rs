//! Memory Layout and Address Space Validation Tests
//!
//! Critical tests for memory layout constants to ensure consistency
//! and prevent memory corruption bugs.

#[cfg(test)]
mod tests {
    use crate::process::{
        USER_VIRT_BASE, USER_REGION_SIZE, USER_PHYS_BASE,
        HEAP_BASE, HEAP_SIZE,
        STACK_BASE, STACK_SIZE,
        INTERP_BASE, INTERP_REGION_SIZE,
    };

    // =========================================================================
    // Address Space Layout Invariants
    // =========================================================================

    #[test]
    fn test_user_virt_base_alignment() {
        // USER_VIRT_BASE must be page-aligned
        const PAGE_SIZE: u64 = 4096;
        assert_eq!(USER_VIRT_BASE % PAGE_SIZE, 0, 
            "USER_VIRT_BASE must be page-aligned");
    }

    #[test]
    fn test_user_virt_base_value() {
        // According to copilot-instructions.md: USER_VIRT_BASE = 0x1000000 (16MB)
        assert_eq!(USER_VIRT_BASE, 0x1000000, 
            "USER_VIRT_BASE should be 0x1000000 (16MB)");
    }

    #[test]
    fn test_heap_base_value() {
        // According to copilot-instructions.md: HEAP_BASE = 0x1200000
        assert_eq!(HEAP_BASE, 0x1200000, 
            "HEAP_BASE should be 0x1200000");
    }

    #[test]
    fn test_stack_base_value() {
        // According to copilot-instructions.md: STACK_BASE = 0x1A00000
        assert_eq!(STACK_BASE, 0x1A00000, 
            "STACK_BASE should be 0x1A00000");
    }

    #[test]
    fn test_interp_base_value() {
        // According to copilot-instructions.md: INTERP_BASE = 0x1C00000
        assert_eq!(INTERP_BASE, 0x1C00000, 
            "INTERP_BASE should be 0x1C00000");
    }

    // =========================================================================
    // Region Non-Overlap Tests (Critical for Memory Safety)
    // =========================================================================

    #[test]
    fn test_code_heap_no_overlap() {
        // Code region: USER_VIRT_BASE to HEAP_BASE
        // Heap: HEAP_BASE to HEAP_BASE + HEAP_SIZE
        
        let code_end = HEAP_BASE; // Code ends where heap begins
        let heap_end = HEAP_BASE + HEAP_SIZE;
        
        assert!(code_end <= HEAP_BASE, "Code region must end before heap starts");
        assert!(heap_end <= STACK_BASE, "Heap must end before stack starts");
    }

    #[test]
    fn test_heap_stack_no_overlap() {
        let heap_end = HEAP_BASE + HEAP_SIZE;
        let stack_end = STACK_BASE + STACK_SIZE;
        
        // Check non-overlap
        assert!(heap_end <= STACK_BASE || HEAP_BASE >= stack_end,
            "Heap and stack regions must not overlap!");
    }

    #[test]
    fn test_stack_interp_no_overlap() {
        let stack_end = STACK_BASE + STACK_SIZE;
        let interp_end = INTERP_BASE + INTERP_REGION_SIZE;
        
        // Stack should end before interpreter region
        assert!(stack_end <= INTERP_BASE || STACK_BASE >= interp_end,
            "Stack and interpreter regions must not overlap!");
    }

    #[test]
    fn test_all_regions_in_user_space() {
        let user_end = USER_VIRT_BASE + USER_REGION_SIZE;
        
        // All regions must be within user address space
        assert!(USER_VIRT_BASE >= USER_VIRT_BASE && USER_VIRT_BASE < user_end);
        
        // Heap in user space
        assert!(HEAP_BASE >= USER_VIRT_BASE);
        assert!(HEAP_BASE + HEAP_SIZE <= user_end);
        
        // Stack in user space
        assert!(STACK_BASE >= USER_VIRT_BASE);
        assert!(STACK_BASE + STACK_SIZE <= user_end);
        
        // Interpreter in user space
        assert!(INTERP_BASE >= USER_VIRT_BASE);
        assert!(INTERP_BASE + INTERP_REGION_SIZE <= user_end);
    }

    // =========================================================================
    // Layout Order Verification
    // =========================================================================

    #[test]
    fn test_layout_order() {
        // Expected order: Code < Heap < Stack < Interp
        // This is the standard layout documented in copilot-instructions.md
        
        assert!(USER_VIRT_BASE < HEAP_BASE, "Code should come before heap");
        assert!(HEAP_BASE < STACK_BASE, "Heap should come before stack");
        assert!(STACK_BASE < INTERP_BASE, "Stack should come before interpreter");
    }

    #[test]
    fn test_heap_after_code() {
        // Heap starts at HEAP_BASE (0x1200000)
        // Code is at USER_VIRT_BASE (0x1000000)
        // Gap between code and heap = 2MB for data segment
        
        let code_to_heap_gap = HEAP_BASE - USER_VIRT_BASE;
        assert!(code_to_heap_gap >= 0x100000, "Should have at least 1MB for code+data");
    }

    // =========================================================================
    // Size Sanity Checks
    // =========================================================================

    #[test]
    fn test_heap_size_reasonable() {
        // Heap should be at least 1MB and not exceed 1GB
        assert!(HEAP_SIZE >= 0x100000, "Heap should be at least 1MB");
        assert!(HEAP_SIZE <= 0x4000_0000, "Heap should not exceed 1GB");
    }

    #[test]
    fn test_stack_size_reasonable() {
        // Stack should be at least 64KB and not exceed 64MB
        assert!(STACK_SIZE >= 0x10000, "Stack should be at least 64KB");
        assert!(STACK_SIZE <= 0x400_0000, "Stack should not exceed 64MB");
    }

    #[test]
    fn test_interp_region_reasonable() {
        // Interpreter region should be at least 1MB (for ld.so)
        assert!(INTERP_REGION_SIZE >= 0x100000, 
            "Interpreter region should be at least 1MB");
    }

    #[test]
    fn test_user_region_fits_all() {
        // User region must be large enough to contain all sub-regions
        let total_needed = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;
        assert!(USER_REGION_SIZE >= total_needed,
            "USER_REGION_SIZE must accommodate all regions");
    }

    // =========================================================================
    // Alignment Checks
    // =========================================================================

    const PAGE_SIZE: u64 = 4096;
    const MB: u64 = 1024 * 1024;

    #[test]
    fn test_all_regions_page_aligned() {
        assert_eq!(USER_VIRT_BASE % PAGE_SIZE, 0, "USER_VIRT_BASE not page-aligned");
        assert_eq!(HEAP_BASE % PAGE_SIZE, 0, "HEAP_BASE not page-aligned");
        assert_eq!(STACK_BASE % PAGE_SIZE, 0, "STACK_BASE not page-aligned");
        assert_eq!(INTERP_BASE % PAGE_SIZE, 0, "INTERP_BASE not page-aligned");
    }

    #[test]
    fn test_all_sizes_page_aligned() {
        assert_eq!(USER_REGION_SIZE % PAGE_SIZE, 0, "USER_REGION_SIZE not page-aligned");
        assert_eq!(HEAP_SIZE % PAGE_SIZE, 0, "HEAP_SIZE not page-aligned");
        assert_eq!(STACK_SIZE % PAGE_SIZE, 0, "STACK_SIZE not page-aligned");
        assert_eq!(INTERP_REGION_SIZE % PAGE_SIZE, 0, "INTERP_REGION_SIZE not page-aligned");
    }

    // =========================================================================
    // Overflow Detection
    // =========================================================================

    #[test]
    fn test_heap_end_no_overflow() {
        let heap_end = HEAP_BASE.checked_add(HEAP_SIZE);
        assert!(heap_end.is_some(), "HEAP_BASE + HEAP_SIZE overflows!");
    }

    #[test]
    fn test_stack_end_no_overflow() {
        let stack_end = STACK_BASE.checked_add(STACK_SIZE);
        assert!(stack_end.is_some(), "STACK_BASE + STACK_SIZE overflows!");
    }

    #[test]
    fn test_interp_end_no_overflow() {
        let interp_end = INTERP_BASE.checked_add(INTERP_REGION_SIZE);
        assert!(interp_end.is_some(), "INTERP_BASE + INTERP_REGION_SIZE overflows!");
    }

    #[test]
    fn test_user_region_end_no_overflow() {
        let user_end = USER_VIRT_BASE.checked_add(USER_REGION_SIZE);
        assert!(user_end.is_some(), "USER_VIRT_BASE + USER_REGION_SIZE overflows!");
    }

    // =========================================================================
    // Physical Address Checks
    // =========================================================================

    #[test]
    fn test_user_phys_base_defined() {
        // USER_PHYS_BASE should be defined and reasonable
        assert!(USER_PHYS_BASE > 0, "USER_PHYS_BASE should be non-zero");
    }

    #[test]
    fn test_phys_virt_mapping_sensible() {
        // Physical and virtual base can be different (allows ASLR, etc.)
        // But both should be aligned
        assert_eq!(USER_PHYS_BASE % PAGE_SIZE, 0, "USER_PHYS_BASE not page-aligned");
    }

    // =========================================================================
    // Documentation Cross-Reference Tests
    // =========================================================================

    #[test]
    fn test_documented_heap_range() {
        // From copilot-instructions.md:
        // HEAP_BASE: 0x1200000 (User heap, 8MB: 0x1200000–0x1A00000)
        
        assert_eq!(HEAP_BASE, 0x1200000);
        
        // 8MB = 0x800000
        // End should be 0x1200000 + 0x800000 = 0x1A00000
        let expected_heap_end = 0x1A00000u64;
        let actual_heap_end = HEAP_BASE + HEAP_SIZE;
        
        // Check heap ends where stack begins
        assert_eq!(actual_heap_end, STACK_BASE, 
            "Heap should end exactly where stack begins");
    }

    #[test]
    fn test_documented_stack_size() {
        // From copilot-instructions.md:
        // STACK_BASE: 0x1A00000 (User stack, 2MB, placed after heap)
        
        // 2MB = 0x200000
        let expected_stack_size = 0x200000u64;
        assert_eq!(STACK_SIZE, expected_stack_size,
            "Stack size should be 2MB as documented");
    }

    #[test]
    fn test_documented_interp_region() {
        // From copilot-instructions.md:
        // INTERP_BASE: 0x1C00000 (Dynamic linker region, 16MB reserved)
        
        // Stack end: 0x1A00000 + 0x200000 = 0x1C00000
        let stack_end = STACK_BASE + STACK_SIZE;
        assert_eq!(stack_end, INTERP_BASE,
            "Interpreter should start right after stack");
        
        // 16MB reserved = 0x1000000
        let expected_interp_size = 0x1000000u64;
        assert_eq!(INTERP_REGION_SIZE, expected_interp_size,
            "Interpreter region should be 16MB as documented");
    }
}
