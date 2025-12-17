//! Process Memory Layout Tests
//!
//! Tests for critical memory layout constants and their consistency.
//! Per copilot-instructions.md: Memory constant changes require coordinated updates
//! in paging.rs + loader.rs + elf.rs
//!
//! These tests catch misconfigurations that could cause memory corruption or segfaults.

#[cfg(test)]
mod tests {
    use crate::process::{
        USER_VIRT_BASE, USER_PHYS_BASE, HEAP_BASE, HEAP_SIZE,
        STACK_BASE, STACK_SIZE, INTERP_BASE, INTERP_REGION_SIZE, USER_REGION_SIZE,
        KERNEL_STACK_SIZE, KERNEL_STACK_ALIGN, MAX_PROCESSES,
    };

    // =========================================================================
    // Basic Constant Value Tests
    // =========================================================================

    #[test]
    fn test_user_virt_base_after_kernel() {
        // USER_VIRT_BASE must be after kernel's BSS section (~0x840000)
        // We use 16MB to be safe
        assert_eq!(USER_VIRT_BASE, 0x1000000, "USER_VIRT_BASE should be 16MB");
        assert!(USER_VIRT_BASE > 0x840000, "USER_VIRT_BASE must be after kernel BSS");
    }

    #[test]
    fn test_user_phys_base_matches_virt() {
        // For identity-mapped userspace, physical and virtual should match
        assert_eq!(USER_PHYS_BASE, USER_VIRT_BASE, 
            "USER_PHYS_BASE should equal USER_VIRT_BASE for identity mapping");
    }

    #[test]
    fn test_heap_base_calculation() {
        // HEAP_BASE = USER_VIRT_BASE + 0x200000 (2MB for code/data)
        assert_eq!(HEAP_BASE, USER_VIRT_BASE + 0x200000);
        assert_eq!(HEAP_BASE, 0x1200000);
    }

    #[test]
    fn test_heap_size() {
        // HEAP_SIZE should be 8MB
        assert_eq!(HEAP_SIZE, 0x800000);
        assert_eq!(HEAP_SIZE, 8 * 1024 * 1024);
    }

    #[test]
    fn test_stack_base_after_heap() {
        // STACK_BASE = HEAP_BASE + HEAP_SIZE
        assert_eq!(STACK_BASE, HEAP_BASE + HEAP_SIZE);
        assert_eq!(STACK_BASE, 0x1A00000);
    }

    #[test]
    fn test_stack_size_alignment() {
        // STACK_SIZE must be 2MB aligned for huge pages
        assert_eq!(STACK_SIZE, 0x200000);
        assert_eq!(STACK_SIZE & 0x1FFFFF, 0, "STACK_SIZE must be 2MB aligned");
    }

    #[test]
    fn test_interp_base_after_stack() {
        // INTERP_BASE = STACK_BASE + STACK_SIZE
        assert_eq!(INTERP_BASE, STACK_BASE + STACK_SIZE);
        assert_eq!(INTERP_BASE, 0x1C00000);
    }

    #[test]
    fn test_interp_region_size() {
        // INTERP_REGION_SIZE should be 16MB
        assert_eq!(INTERP_REGION_SIZE, 0x1000000);
        assert_eq!(INTERP_REGION_SIZE, 16 * 1024 * 1024);
    }

    // =========================================================================
    // Memory Layout Consistency Tests
    // =========================================================================

    #[test]
    fn test_user_region_size_calculation() {
        // USER_REGION_SIZE should equal total span
        let expected = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;
        assert_eq!(USER_REGION_SIZE, expected, 
            "USER_REGION_SIZE calculation mismatch");
    }

    #[test]
    fn test_regions_do_not_overlap() {
        // Code region: USER_VIRT_BASE to HEAP_BASE
        let code_end = HEAP_BASE;
        
        // Heap region: HEAP_BASE to STACK_BASE
        let heap_start = HEAP_BASE;
        let heap_end = STACK_BASE;
        
        // Stack region: STACK_BASE to INTERP_BASE
        let stack_start = STACK_BASE;
        let stack_end = INTERP_BASE;
        
        // Interp region: INTERP_BASE to end
        let interp_start = INTERP_BASE;
        
        // Verify no overlaps
        assert!(code_end <= heap_start, "Code and heap regions overlap");
        assert!(heap_end <= stack_start, "Heap and stack regions overlap");
        assert!(stack_end <= interp_start, "Stack and interp regions overlap");
    }

    #[test]
    fn test_heap_region_bounds() {
        let heap_end = HEAP_BASE + HEAP_SIZE;
        assert_eq!(heap_end, STACK_BASE, "Heap end should equal stack start");
        assert_eq!(heap_end, 0x1A00000);
    }

    #[test]
    fn test_total_userspace_size() {
        // Total userspace: from USER_VIRT_BASE to end of INTERP_REGION
        let total = USER_REGION_SIZE;
        
        // Should be code(2MB) + heap(8MB) + stack(2MB) + interp(16MB) = 28MB
        let expected = 0x200000 + 0x800000 + 0x200000 + 0x1000000;
        assert_eq!(total, expected, "Total userspace size mismatch");
        assert_eq!(total, 28 * 1024 * 1024);
    }

    // =========================================================================
    // Kernel Stack Tests
    // =========================================================================

    #[test]
    fn test_kernel_stack_size() {
        // Kernel stack should be 32KB
        assert_eq!(KERNEL_STACK_SIZE, 32 * 1024);
    }

    #[test]
    fn test_kernel_stack_alignment() {
        // Kernel stack must be 16-byte aligned (x86_64 ABI)
        assert_eq!(KERNEL_STACK_ALIGN, 16);
        assert!(KERNEL_STACK_SIZE % KERNEL_STACK_ALIGN == 0,
            "KERNEL_STACK_SIZE must be a multiple of KERNEL_STACK_ALIGN");
    }

    // =========================================================================
    // Process Limits Tests
    // =========================================================================

    #[test]
    fn test_max_processes() {
        assert_eq!(MAX_PROCESSES, 64);
        assert!(MAX_PROCESSES > 0);
    }

    // =========================================================================
    // Address Validity Tests
    // =========================================================================

    #[test]
    fn test_addresses_page_aligned() {
        const PAGE_SIZE: u64 = 4096;
        
        assert_eq!(USER_VIRT_BASE & (PAGE_SIZE - 1), 0, "USER_VIRT_BASE not page aligned");
        assert_eq!(USER_PHYS_BASE & (PAGE_SIZE - 1), 0, "USER_PHYS_BASE not page aligned");
        assert_eq!(HEAP_BASE & (PAGE_SIZE - 1), 0, "HEAP_BASE not page aligned");
        assert_eq!(STACK_BASE & (PAGE_SIZE - 1), 0, "STACK_BASE not page aligned");
        assert_eq!(INTERP_BASE & (PAGE_SIZE - 1), 0, "INTERP_BASE not page aligned");
    }

    #[test]
    fn test_no_address_below_1mb() {
        // First 1MB often reserved for BIOS/bootloader
        const RESERVED_END: u64 = 0x100000;
        
        assert!(USER_VIRT_BASE >= RESERVED_END, "USER_VIRT_BASE in reserved region");
        assert!(USER_PHYS_BASE >= RESERVED_END, "USER_PHYS_BASE in reserved region");
    }

    #[test]
    fn test_addresses_in_canonical_form() {
        // x86_64 canonical addresses: bits 48-63 must be copies of bit 47
        fn is_canonical(addr: u64) -> bool {
            let sign_ext = (addr as i64) >> 47;
            sign_ext == 0 || sign_ext == -1
        }
        
        assert!(is_canonical(USER_VIRT_BASE), "USER_VIRT_BASE not canonical");
        assert!(is_canonical(HEAP_BASE), "HEAP_BASE not canonical");
        assert!(is_canonical(STACK_BASE), "STACK_BASE not canonical");
        assert!(is_canonical(INTERP_BASE), "INTERP_BASE not canonical");
        assert!(is_canonical(INTERP_BASE + INTERP_REGION_SIZE), "Interp region end not canonical");
    }

    // =========================================================================
    // ELF Loading Compatibility Tests
    // =========================================================================

    #[test]
    fn test_user_virt_base_elf_compatible() {
        // Most ELF executables expect to load at low addresses
        // USER_VIRT_BASE at 16MB is compatible with typical ELF expectations
        assert!(USER_VIRT_BASE < 0x10000000, "USER_VIRT_BASE too high for typical ELF");
        assert!(USER_VIRT_BASE >= 0x1000, "USER_VIRT_BASE too low (null page protection)");
    }

    #[test]
    fn test_interp_region_sufficient() {
        // Dynamic linker needs space for:
        // - ld-nrlib itself (~1MB)
        // - libc equivalent (~2MB)
        // - Other shared libraries
        // 16MB should be sufficient
        assert!(INTERP_REGION_SIZE >= 4 * 1024 * 1024,
            "INTERP_REGION_SIZE may be too small for shared libraries");
    }

    #[test]
    fn test_stack_grows_down_room() {
        // Stack starts at STACK_BASE and grows down
        // Ensure there's a guard page possibility
        assert!(STACK_SIZE >= 0x10000, "Stack too small for practical use");
    }
}
