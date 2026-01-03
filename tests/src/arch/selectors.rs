//! Segment Selector Tests
//!
//! Tests for x86_64 segment selectors and privilege levels.

#[cfg(test)]
mod tests {
    // =========================================================================
    // Selector Format Tests
    // =========================================================================

    #[test]
    fn test_selector_format() {
        // Selector format (16 bits):
        // Bits 0-1:   RPL (Requested Privilege Level)
        // Bit 2:      TI (Table Indicator: 0=GDT, 1=LDT)
        // Bits 3-15:  Index into descriptor table
        
        let rpl_mask = 0b11;
        let ti_mask = 0b100;
        let index_shift = 3;
        
        assert_eq!(rpl_mask, 3);
        assert_eq!(ti_mask, 4);
        assert_eq!(index_shift, 3);
    }

    #[test]
    fn test_selector_null() {
        // Null selector (index 0 in GDT)
        let null_selector: u16 = 0;
        
        assert_eq!(null_selector & 0b11, 0);      // RPL = 0
        assert_eq!(null_selector & 0b100, 0);     // TI = 0 (GDT)
        assert_eq!(null_selector >> 3, 0);        // Index = 0
    }

    // =========================================================================
    // Kernel Segment Tests
    // =========================================================================

    #[test]
    fn test_kernel_code_selector() {
        // Kernel code segment typically at GDT index 1
        let index = 1;
        let rpl = 0;  // Ring 0
        let selector: u16 = (index << 3) | rpl;
        
        assert_eq!(selector, 0x08);
        assert_eq!(selector & 3, 0);  // Kernel privilege
    }

    #[test]
    fn test_kernel_data_selector() {
        // Kernel data segment typically at GDT index 2
        let index = 2;
        let rpl = 0;  // Ring 0
        let selector: u16 = (index << 3) | rpl;
        
        assert_eq!(selector, 0x10);
        assert_eq!(selector & 3, 0);  // Kernel privilege
    }

    // =========================================================================
    // User Segment Tests
    // =========================================================================

    #[test]
    fn test_user_code_selector() {
        // User code segment - must have RPL 3
        // Typical layout: GDT[4] = user data, GDT[5] = user code (for syscall/sysret)
        let index = 5;
        let rpl = 3;  // Ring 3
        let selector: u16 = (index << 3) | rpl;
        
        assert_eq!(selector, 0x2B);  // 0x28 + 3
        assert_eq!(selector & 3, 3);  // User privilege
    }

    #[test]
    fn test_user_data_selector() {
        // User data segment
        let index = 4;
        let rpl = 3;  // Ring 3
        let selector: u16 = (index << 3) | rpl;
        
        assert_eq!(selector, 0x23);  // 0x20 + 3
        assert_eq!(selector & 3, 3);  // User privilege
    }

    // =========================================================================
    // RPL Tests
    // =========================================================================

    #[test]
    fn test_rpl_values() {
        // RPL 0 = Kernel (highest privilege)
        // RPL 1 = Unused in most OSes
        // RPL 2 = Unused in most OSes
        // RPL 3 = User (lowest privilege)
        
        assert_eq!(0 & 3, 0);  // Ring 0
        assert_eq!(1 & 3, 1);  // Ring 1
        assert_eq!(2 & 3, 2);  // Ring 2
        assert_eq!(3 & 3, 3);  // Ring 3
    }

    #[test]
    fn test_rpl_in_selector() {
        // Extract RPL from a selector
        fn get_rpl(selector: u16) -> u16 {
            selector & 0b11
        }
        
        assert_eq!(get_rpl(0x08), 0);  // Kernel code
        assert_eq!(get_rpl(0x10), 0);  // Kernel data
        assert_eq!(get_rpl(0x23), 3);  // User data
        assert_eq!(get_rpl(0x2B), 3);  // User code
    }

    // =========================================================================
    // Table Indicator Tests
    // =========================================================================

    #[test]
    fn test_ti_gdt() {
        // TI = 0 means selector references GDT
        let gdt_selector: u16 = 0x08;
        let ti = (gdt_selector & 0b100) != 0;
        
        assert!(!ti);  // GDT
    }

    #[test]
    fn test_ti_ldt() {
        // TI = 1 means selector references LDT
        let ldt_selector: u16 = 0x0C;  // Index 1, TI=1, RPL=0
        let ti = (ldt_selector & 0b100) != 0;
        
        assert!(ti);  // LDT
    }

    // =========================================================================
    // Index Extraction Tests
    // =========================================================================

    #[test]
    fn test_index_extraction() {
        fn get_index(selector: u16) -> u16 {
            selector >> 3
        }
        
        assert_eq!(get_index(0x00), 0);  // Null
        assert_eq!(get_index(0x08), 1);  // Kernel code
        assert_eq!(get_index(0x10), 2);  // Kernel data
        assert_eq!(get_index(0x18), 3);
        assert_eq!(get_index(0x20), 4);  // User data
        assert_eq!(get_index(0x28), 5);  // User code
        assert_eq!(get_index(0x30), 6);  // TSS
    }

    // =========================================================================
    // Selector Building Tests
    // =========================================================================

    #[test]
    fn test_selector_build() {
        fn build_selector(index: u16, is_ldt: bool, rpl: u16) -> u16 {
            let ti = if is_ldt { 1 } else { 0 };
            (index << 3) | (ti << 2) | (rpl & 0b11)
        }
        
        assert_eq!(build_selector(1, false, 0), 0x08);  // Kernel code
        assert_eq!(build_selector(2, false, 0), 0x10);  // Kernel data
        assert_eq!(build_selector(4, false, 3), 0x23);  // User data
        assert_eq!(build_selector(5, false, 3), 0x2B);  // User code
    }

    // =========================================================================
    // SYSCALL/SYSRET Selector Requirements
    // =========================================================================

    #[test]
    fn test_syscall_sysret_layout() {
        // SYSCALL/SYSRET has strict GDT ordering requirements:
        // STAR MSR encodes both kernel and user segment bases
        // 
        // On SYSCALL (going to kernel):
        //   CS = STAR[47:32]       (kernel code)
        //   SS = STAR[47:32] + 8   (kernel data)
        //
        // On SYSRET (going to user, 64-bit):
        //   CS = STAR[63:48] + 16  (user code)
        //   SS = STAR[63:48] + 8   (user data)
        
        let kernel_base = 0x08;  // GDT[1]
        let user_base = 0x18;    // GDT[3]
        
        // Verify kernel segments
        assert_eq!(kernel_base, 0x08);      // Kernel CS
        assert_eq!(kernel_base + 8, 0x10);  // Kernel SS
        
        // Verify user segments (with SYSRET offsets)
        assert_eq!(user_base + 8, 0x20);    // User SS (GDT[4])
        assert_eq!(user_base + 16, 0x28);   // User CS (GDT[5])
    }

    #[test]
    fn test_syscall_selectors_with_rpl() {
        // User selectors must have RPL 3 set
        let user_data = 0x20 | 3;  // 0x23
        let user_code = 0x28 | 3;  // 0x2B
        
        assert_eq!(user_data, 0x23);
        assert_eq!(user_code, 0x2B);
    }

    // =========================================================================
    // Segment Register Tests
    // =========================================================================

    #[test]
    fn test_segment_registers() {
        // x86_64 segment registers:
        // CS - Code Segment (affects execution privilege)
        // SS - Stack Segment (affects stack operations)
        // DS - Data Segment (general data access)
        // ES - Extra Segment (string operations)
        // FS - Extra Segment (TLS in user space)
        // GS - Extra Segment (per-CPU data in kernel)
        
        let segment_count = 6;
        assert_eq!(segment_count, 6);
    }

    #[test]
    fn test_fs_gs_base() {
        // FS and GS have hidden base registers accessible via MSR
        // FS_BASE = MSR 0xC0000100
        // GS_BASE = MSR 0xC0000101
        // KERNEL_GS_BASE = MSR 0xC0000102 (swapped on SWAPGS)
        
        let fs_base_msr: u32 = 0xC000_0100;
        let gs_base_msr: u32 = 0xC000_0101;
        let kernel_gs_base_msr: u32 = 0xC000_0102;
        
        assert_eq!(fs_base_msr, 0xC0000100);
        assert_eq!(gs_base_msr, 0xC0000101);
        assert_eq!(kernel_gs_base_msr, 0xC0000102);
    }

    // =========================================================================
    // CPL (Current Privilege Level) Tests
    // =========================================================================

    #[test]
    fn test_cpl_determination() {
        // CPL is determined by CS selector's RPL
        // Reading CS gives current privilege level in bits 0-1
        
        fn get_cpl(cs: u16) -> u16 {
            cs & 0b11
        }
        
        assert_eq!(get_cpl(0x08), 0);  // Kernel
        assert_eq!(get_cpl(0x2B), 3);  // User
    }

    #[test]
    fn test_privilege_check() {
        // CPU checks: max(CPL, RPL) <= DPL for access
        fn can_access(cpl: u8, rpl: u8, dpl: u8) -> bool {
            cpl.max(rpl) <= dpl
        }
        
        // Kernel accessing kernel segment (DPL=0)
        assert!(can_access(0, 0, 0));
        
        // User cannot access kernel segment
        assert!(!can_access(3, 3, 0));
        
        // User accessing user segment (DPL=3)
        assert!(can_access(3, 3, 3));
    }
}
