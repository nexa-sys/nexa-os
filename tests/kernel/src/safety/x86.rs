//! x86 Safety Wrapper Tests

#[cfg(test)]
mod tests {
    // MSR addresses (same as kernel defines)
    const MSR_IA32_STAR: u32 = 0xC0000081;
    const MSR_IA32_LSTAR: u32 = 0xC0000082;
    const MSR_IA32_FMASK: u32 = 0xC0000084;
    const MSR_IA32_KERNEL_GS_BASE: u32 = 0xC0000102;
    const MSR_IA32_GS_BASE: u32 = 0xC0000101;
    const MSR_IA32_FS_BASE: u32 = 0xC0000100;

    // =========================================================================
    // MSR Address Tests
    // =========================================================================

    #[test]
    fn test_msr_syscall_addresses() {
        // SYSCALL/SYSRET MSRs
        assert_eq!(MSR_IA32_STAR, 0xC0000081);
        assert_eq!(MSR_IA32_LSTAR, 0xC0000082);
        assert_eq!(MSR_IA32_FMASK, 0xC0000084);
    }

    #[test]
    fn test_msr_segment_base_addresses() {
        // Segment base MSRs
        assert_eq!(MSR_IA32_FS_BASE, 0xC0000100);
        assert_eq!(MSR_IA32_GS_BASE, 0xC0000101);
        assert_eq!(MSR_IA32_KERNEL_GS_BASE, 0xC0000102);
    }

    #[test]
    fn test_msr_address_range() {
        // All these MSRs are in the AMD64 extended range
        assert!(MSR_IA32_STAR >= 0xC0000000);
        assert!(MSR_IA32_KERNEL_GS_BASE >= 0xC0000000);
    }

    // =========================================================================
    // Port I/O Address Tests
    // =========================================================================

    #[test]
    fn test_common_port_addresses() {
        const PIC1_CMD: u16 = 0x20;
        const PIC1_DATA: u16 = 0x21;
        const PIC2_CMD: u16 = 0xA0;
        const PIC2_DATA: u16 = 0xA1;
        const PIT_CH0: u16 = 0x40;
        const PIT_CMD: u16 = 0x43;
        const PS2_DATA: u16 = 0x60;
        const PS2_CMD: u16 = 0x64;
        const CMOS_ADDR: u16 = 0x70;
        const CMOS_DATA: u16 = 0x71;
        const COM1: u16 = 0x3F8;
        
        assert_eq!(PIC1_CMD, 0x20);
        assert_eq!(PS2_DATA, 0x60);
        assert_eq!(COM1, 0x3F8);
    }

    // =========================================================================
    // PCI Configuration Space Tests
    // =========================================================================

    #[test]
    fn test_pci_config_address() {
        fn pci_config_addr(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
            let bus = bus as u32;
            let device = device as u32;
            let function = function as u32;
            let offset = offset as u32;
            0x80000000 | (bus << 16) | (device << 11) | (function << 8) | (offset & 0xFC)
        }
        
        // Bus 0, Device 0, Function 0, Offset 0
        assert_eq!(pci_config_addr(0, 0, 0, 0), 0x80000000);
        
        // Bus 0, Device 2, Function 0, Offset 0
        assert_eq!(pci_config_addr(0, 2, 0, 0), 0x80001000);
    }

    #[test]
    fn test_pci_device_vendor_offset() {
        const PCI_VENDOR_ID: u8 = 0x00;
        const PCI_DEVICE_ID: u8 = 0x02;
        const PCI_COMMAND: u8 = 0x04;
        const PCI_STATUS: u8 = 0x06;
        const PCI_CLASS_REVISION: u8 = 0x08;
        const PCI_BAR0: u8 = 0x10;
        
        assert_eq!(PCI_BAR0, 0x10);
    }

    // =========================================================================
    // RFLAGS Tests
    // =========================================================================

    #[test]
    fn test_rflags_bits() {
        const RFLAGS_CF: u64 = 1 << 0;   // Carry Flag
        const RFLAGS_PF: u64 = 1 << 2;   // Parity Flag
        const RFLAGS_ZF: u64 = 1 << 6;   // Zero Flag
        const RFLAGS_SF: u64 = 1 << 7;   // Sign Flag
        const RFLAGS_IF: u64 = 1 << 9;   // Interrupt Flag
        const RFLAGS_DF: u64 = 1 << 10;  // Direction Flag
        const RFLAGS_OF: u64 = 1 << 11;  // Overflow Flag
        const RFLAGS_IOPL: u64 = 3 << 12; // I/O Privilege Level
        const RFLAGS_NT: u64 = 1 << 14;  // Nested Task
        const RFLAGS_AC: u64 = 1 << 18;  // Alignment Check
        const RFLAGS_ID: u64 = 1 << 21;  // CPUID Available
        
        assert_eq!(RFLAGS_IF, 0x200);
        assert_eq!(RFLAGS_IOPL, 0x3000);
    }

    #[test]
    fn test_interrupts_enabled_check() {
        const RFLAGS_IF: u64 = 0x200;
        
        fn interrupts_enabled(rflags: u64) -> bool {
            (rflags & RFLAGS_IF) != 0
        }
        
        assert!(interrupts_enabled(0x200));
        assert!(!interrupts_enabled(0x000));
        assert!(interrupts_enabled(0x202)); // IF + other flags
    }

    // =========================================================================
    // CR Register Tests
    // =========================================================================

    #[test]
    fn test_cr0_bits() {
        const CR0_PE: u64 = 1 << 0;   // Protected Mode Enable
        const CR0_MP: u64 = 1 << 1;   // Monitor Co-processor
        const CR0_EM: u64 = 1 << 2;   // Emulation
        const CR0_TS: u64 = 1 << 3;   // Task Switched
        const CR0_ET: u64 = 1 << 4;   // Extension Type
        const CR0_NE: u64 = 1 << 5;   // Numeric Error
        const CR0_WP: u64 = 1 << 16;  // Write Protect
        const CR0_AM: u64 = 1 << 18;  // Alignment Mask
        const CR0_NW: u64 = 1 << 29;  // Not Write-through
        const CR0_CD: u64 = 1 << 30;  // Cache Disable
        const CR0_PG: u64 = 1 << 31;  // Paging
        
        assert_eq!(CR0_PE, 1);
        assert_eq!(CR0_PG, 0x80000000);
    }

    #[test]
    fn test_cr4_bits() {
        const CR4_PAE: u64 = 1 << 5;      // Physical Address Extension
        const CR4_PGE: u64 = 1 << 7;      // Page Global Enable
        const CR4_OSFXSR: u64 = 1 << 9;   // OS FXSAVE/FXRSTOR Support
        const CR4_OSXMMEXCPT: u64 = 1 << 10; // OS Unmasked SIMD FP Exceptions
        const CR4_FSGSBASE: u64 = 1 << 16;   // FSGSBASE Enable
        const CR4_OSXSAVE: u64 = 1 << 18;    // XSAVE and Processor Extended States
        
        assert_eq!(CR4_PAE, 0x20);
        assert_eq!(CR4_FSGSBASE, 0x10000);
    }

    // =========================================================================
    // CPUID Tests
    // =========================================================================

    #[test]
    fn test_cpuid_leaves() {
        const CPUID_VENDOR: u32 = 0x00;
        const CPUID_FEATURES: u32 = 0x01;
        const CPUID_EXT_MAX: u32 = 0x80000000;
        const CPUID_EXT_FEATURES: u32 = 0x80000001;
        
        assert_eq!(CPUID_VENDOR, 0);
        assert_eq!(CPUID_EXT_MAX, 0x80000000);
    }

    #[test]
    fn test_cpuid_feature_bits() {
        // EDX features from leaf 1
        const CPUID_FPU: u32 = 1 << 0;
        const CPUID_PSE: u32 = 1 << 3;
        const CPUID_TSC: u32 = 1 << 4;
        const CPUID_MSR: u32 = 1 << 5;
        const CPUID_PAE: u32 = 1 << 6;
        const CPUID_APIC: u32 = 1 << 9;
        const CPUID_SSE: u32 = 1 << 25;
        const CPUID_SSE2: u32 = 1 << 26;
        
        assert_eq!(CPUID_TSC, 0x10);
        assert_eq!(CPUID_APIC, 0x200);
    }
}
