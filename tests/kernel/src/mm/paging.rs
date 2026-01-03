//! Paging Tests
//!
//! Tests for memory paging constants, structures, and address calculations.

#[cfg(test)]
mod tests {
    // =========================================================================
    // Page Table Constants
    // =========================================================================

    #[test]
    fn test_page_size() {
        const PAGE_SIZE: u64 = 4096;
        assert_eq!(PAGE_SIZE, 0x1000);
        assert_eq!(PAGE_SIZE, 1 << 12);
    }

    #[test]
    fn test_large_page_size() {
        const PAGE_SIZE_2M: u64 = 2 * 1024 * 1024;
        assert_eq!(PAGE_SIZE_2M, 0x200000);
        assert_eq!(PAGE_SIZE_2M, 1 << 21);
    }

    #[test]
    fn test_huge_page_size() {
        const PAGE_SIZE_1G: u64 = 1024 * 1024 * 1024;
        assert_eq!(PAGE_SIZE_1G, 0x40000000);
        assert_eq!(PAGE_SIZE_1G, 1 << 30);
    }

    // =========================================================================
    // Page Table Entry Flags
    // =========================================================================

    #[test]
    fn test_pte_present_flag() {
        const PTE_PRESENT: u64 = 1 << 0;
        assert_eq!(PTE_PRESENT, 0x1);
    }

    #[test]
    fn test_pte_writable_flag() {
        const PTE_WRITABLE: u64 = 1 << 1;
        assert_eq!(PTE_WRITABLE, 0x2);
    }

    #[test]
    fn test_pte_user_flag() {
        const PTE_USER: u64 = 1 << 2;
        assert_eq!(PTE_USER, 0x4);
    }

    #[test]
    fn test_pte_write_through() {
        const PTE_WRITE_THROUGH: u64 = 1 << 3;
        assert_eq!(PTE_WRITE_THROUGH, 0x8);
    }

    #[test]
    fn test_pte_cache_disable() {
        const PTE_CACHE_DISABLE: u64 = 1 << 4;
        assert_eq!(PTE_CACHE_DISABLE, 0x10);
    }

    #[test]
    fn test_pte_accessed() {
        const PTE_ACCESSED: u64 = 1 << 5;
        assert_eq!(PTE_ACCESSED, 0x20);
    }

    #[test]
    fn test_pte_dirty() {
        const PTE_DIRTY: u64 = 1 << 6;
        assert_eq!(PTE_DIRTY, 0x40);
    }

    #[test]
    fn test_pte_huge_page() {
        const PTE_HUGE_PAGE: u64 = 1 << 7;
        assert_eq!(PTE_HUGE_PAGE, 0x80);
    }

    #[test]
    fn test_pte_global() {
        const PTE_GLOBAL: u64 = 1 << 8;
        assert_eq!(PTE_GLOBAL, 0x100);
    }

    #[test]
    fn test_pte_no_execute() {
        const PTE_NO_EXECUTE: u64 = 1 << 63;
        assert_eq!(PTE_NO_EXECUTE, 0x8000_0000_0000_0000);
    }

    // =========================================================================
    // Page Address Calculations
    // =========================================================================

    #[test]
    fn test_page_align_down() {
        const PAGE_SIZE: u64 = 4096;

        fn page_align_down(addr: u64) -> u64 {
            addr & !(PAGE_SIZE - 1)
        }

        assert_eq!(page_align_down(0), 0);
        assert_eq!(page_align_down(100), 0);
        assert_eq!(page_align_down(4096), 4096);
        assert_eq!(page_align_down(4097), 4096);
        assert_eq!(page_align_down(8191), 4096);
        assert_eq!(page_align_down(8192), 8192);
    }

    #[test]
    fn test_page_align_up() {
        const PAGE_SIZE: u64 = 4096;

        fn page_align_up(addr: u64) -> u64 {
            (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
        }

        assert_eq!(page_align_up(0), 0);
        assert_eq!(page_align_up(1), 4096);
        assert_eq!(page_align_up(4096), 4096);
        assert_eq!(page_align_up(4097), 8192);
    }

    #[test]
    fn test_page_offset() {
        const PAGE_SIZE: u64 = 4096;

        fn page_offset(addr: u64) -> u64 {
            addr & (PAGE_SIZE - 1)
        }

        assert_eq!(page_offset(0), 0);
        assert_eq!(page_offset(100), 100);
        assert_eq!(page_offset(4095), 4095);
        assert_eq!(page_offset(4096), 0);
        assert_eq!(page_offset(4200), 104);
    }

    // =========================================================================
    // Virtual Address Structure Tests (4-level paging)
    // =========================================================================

    #[test]
    fn test_pml4_index() {
        fn pml4_index(addr: u64) -> usize {
            ((addr >> 39) & 0x1FF) as usize
        }

        assert_eq!(pml4_index(0), 0);
        assert_eq!(pml4_index(0x0000_8000_0000_0000), 256);
        assert_eq!(pml4_index(0xFFFF_FFFF_FFFF_FFFF), 511);
    }

    #[test]
    fn test_pdpt_index() {
        fn pdpt_index(addr: u64) -> usize {
            ((addr >> 30) & 0x1FF) as usize
        }

        assert_eq!(pdpt_index(0), 0);
        assert_eq!(pdpt_index(0x4000_0000), 1);
        assert_eq!(pdpt_index(0x8000_0000), 2);
    }

    #[test]
    fn test_pd_index() {
        fn pd_index(addr: u64) -> usize {
            ((addr >> 21) & 0x1FF) as usize
        }

        assert_eq!(pd_index(0), 0);
        assert_eq!(pd_index(0x0020_0000), 1);
        assert_eq!(pd_index(0x0040_0000), 2);
    }

    #[test]
    fn test_pt_index() {
        fn pt_index(addr: u64) -> usize {
            ((addr >> 12) & 0x1FF) as usize
        }

        assert_eq!(pt_index(0), 0);
        assert_eq!(pt_index(0x1000), 1);
        assert_eq!(pt_index(0x2000), 2);
    }

    // =========================================================================
    // Canonical Address Tests
    // =========================================================================

    #[test]
    fn test_canonical_lower_half() {
        fn is_canonical(addr: u64) -> bool {
            let top_bits = addr >> 47;
            top_bits == 0 || top_bits == 0x1FFFF
        }

        // Lower canonical half: 0x0000_0000_0000_0000 - 0x0000_7FFF_FFFF_FFFF
        assert!(is_canonical(0));
        assert!(is_canonical(0x0000_7FFF_FFFF_FFFF));
    }

    #[test]
    fn test_canonical_upper_half() {
        fn is_canonical(addr: u64) -> bool {
            let top_bits = addr >> 47;
            top_bits == 0 || top_bits == 0x1FFFF
        }

        // Upper canonical half: 0xFFFF_8000_0000_0000 - 0xFFFF_FFFF_FFFF_FFFF
        assert!(is_canonical(0xFFFF_8000_0000_0000));
        assert!(is_canonical(0xFFFF_FFFF_FFFF_FFFF));
    }

    #[test]
    fn test_non_canonical_address() {
        fn is_canonical(addr: u64) -> bool {
            let top_bits = addr >> 47;
            top_bits == 0 || top_bits == 0x1FFFF
        }

        // Non-canonical: between 0x0000_8000_0000_0000 and 0xFFFF_7FFF_FFFF_FFFF
        assert!(!is_canonical(0x0000_8000_0000_0000));
        assert!(!is_canonical(0xFFFF_7FFF_FFFF_FFFF));
    }

    // =========================================================================
    // Page Table Size Tests
    // =========================================================================

    #[test]
    fn test_page_table_entries() {
        const ENTRIES_PER_TABLE: usize = 512;
        assert_eq!(ENTRIES_PER_TABLE, 1 << 9);
    }

    #[test]
    fn test_page_table_size() {
        const PAGE_TABLE_SIZE: usize = 4096;
        const ENTRY_SIZE: usize = 8;
        const ENTRIES: usize = PAGE_TABLE_SIZE / ENTRY_SIZE;
        assert_eq!(ENTRIES, 512);
    }

    // =========================================================================
    // Physical Address Extraction
    // =========================================================================

    #[test]
    fn test_extract_phys_addr_from_pte() {
        const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

        fn extract_phys_addr(pte: u64) -> u64 {
            pte & ADDR_MASK
        }

        let pte = 0x0000_0000_1234_5007; // Present, Writable, User
        assert_eq!(extract_phys_addr(pte), 0x0000_0000_1234_5000);
    }

    // =========================================================================
    // CR3 Register Tests
    // =========================================================================

    #[test]
    fn test_cr3_pml4_addr() {
        fn cr3_to_pml4_phys(cr3: u64) -> u64 {
            cr3 & 0xFFFF_FFFF_FFFF_F000
        }

        let cr3 = 0x0000_0000_1234_5018; // With PCID bits set
        assert_eq!(cr3_to_pml4_phys(cr3), 0x0000_0000_1234_5000);
    }

    #[test]
    fn test_cr3_pcid_extraction() {
        fn cr3_to_pcid(cr3: u64) -> u16 {
            (cr3 & 0xFFF) as u16
        }

        let cr3 = 0x0000_0000_1234_5ABC;
        assert_eq!(cr3_to_pcid(cr3), 0xABC);
    }
}
