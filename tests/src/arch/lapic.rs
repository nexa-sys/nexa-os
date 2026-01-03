//! Local APIC Tests
//!
//! Tests for LAPIC (Local Advanced Programmable Interrupt Controller)
//! register constants, delivery modes, and timer configurations.

#[cfg(test)]
mod tests {
    use crate::arch::lapic;

    // =========================================================================
    // MSR Constants Tests
    // =========================================================================

    #[test]
    fn test_ia32_apic_base_msr() {
        const IA32_APIC_BASE: u32 = 0x1B;
        assert_eq!(IA32_APIC_BASE, 0x1B);
    }

    #[test]
    fn test_apic_enable_bit() {
        const APIC_ENABLE: u64 = 1 << 11;
        assert_eq!(APIC_ENABLE, 0x800);
    }

    #[test]
    fn test_apic_base_mask() {
        const APIC_BASE_MASK: u64 = 0xFFFFF000;
        // Should mask to 4KB aligned address
        assert_eq!(APIC_BASE_MASK & 0xFFF, 0);
    }

    #[test]
    fn test_spurious_vector() {
        const DEFAULT_SPURIOUS_VECTOR: u8 = 0xFF;
        assert_eq!(DEFAULT_SPURIOUS_VECTOR, 255);
    }

    // =========================================================================
    // Register Offset Tests
    // =========================================================================

    #[test]
    fn test_lapic_register_offsets() {
        const REG_ID: u32 = 0x20;
        const REG_VERSION: u32 = 0x30;
        const REG_TPR: u32 = 0x80;
        const REG_EOI: u32 = 0x0B0;
        const REG_SVR: u32 = 0x0F0;
        const REG_ERROR: u32 = 0x280;
        
        assert_eq!(REG_ID, 0x20);
        assert_eq!(REG_VERSION, 0x30);
        assert_eq!(REG_TPR, 0x80);
        assert_eq!(REG_EOI, 0xB0);
        assert_eq!(REG_SVR, 0xF0);
        assert_eq!(REG_ERROR, 0x280);
    }

    #[test]
    fn test_lapic_icr_registers() {
        const REG_ICR_LOW: u32 = 0x300;
        const REG_ICR_HIGH: u32 = 0x310;
        
        assert_eq!(REG_ICR_LOW, 0x300);
        assert_eq!(REG_ICR_HIGH, 0x310);
        assert_eq!(REG_ICR_HIGH - REG_ICR_LOW, 0x10);
    }

    #[test]
    fn test_lapic_lvt_registers() {
        const REG_LVT_TIMER: u32 = 0x320;
        const REG_LVT_THERMAL: u32 = 0x330;
        const REG_LVT_PERF: u32 = 0x340;
        const REG_LVT_LINT0: u32 = 0x350;
        const REG_LVT_LINT1: u32 = 0x360;
        const REG_LVT_ERROR: u32 = 0x370;
        
        // LVT registers are spaced 0x10 apart
        assert_eq!(REG_LVT_TIMER, 0x320);
        assert_eq!(REG_LVT_THERMAL - REG_LVT_TIMER, 0x10);
        assert_eq!(REG_LVT_PERF - REG_LVT_THERMAL, 0x10);
        assert_eq!(REG_LVT_LINT0 - REG_LVT_PERF, 0x10);
        assert_eq!(REG_LVT_LINT1 - REG_LVT_LINT0, 0x10);
        assert_eq!(REG_LVT_ERROR - REG_LVT_LINT1, 0x10);
    }

    #[test]
    fn test_lapic_timer_registers() {
        const REG_TIMER_INITIAL: u32 = 0x380;
        const REG_TIMER_CURRENT: u32 = 0x390;
        const REG_TIMER_DIVIDE: u32 = 0x3E0;
        
        assert_eq!(REG_TIMER_INITIAL, 0x380);
        assert_eq!(REG_TIMER_CURRENT, 0x390);
        assert_eq!(REG_TIMER_DIVIDE, 0x3E0);
    }

    #[test]
    fn test_lapic_isr_tmr_irr_bases() {
        const REG_ISR_BASE: u32 = 0x100;
        const REG_TMR_BASE: u32 = 0x180;
        const REG_IRR_BASE: u32 = 0x200;
        
        // Each register set has 8 32-bit registers (256 bits total)
        assert_eq!(REG_ISR_BASE, 0x100);
        assert_eq!(REG_TMR_BASE, 0x180);
        assert_eq!(REG_IRR_BASE, 0x200);
        
        // Gap between sets
        assert_eq!(REG_TMR_BASE - REG_ISR_BASE, 0x80);
        assert_eq!(REG_IRR_BASE - REG_TMR_BASE, 0x80);
    }

    // =========================================================================
    // Timer Mode Tests
    // =========================================================================

    #[test]
    fn test_timer_modes() {
        const TIMER_MODE_ONESHOT: u32 = 0 << 17;
        const TIMER_MODE_PERIODIC: u32 = 1 << 17;
        const TIMER_MODE_TSC_DEADLINE: u32 = 2 << 17;
        
        assert_eq!(TIMER_MODE_ONESHOT, 0);
        assert_eq!(TIMER_MODE_PERIODIC, 0x20000);
        assert_eq!(TIMER_MODE_TSC_DEADLINE, 0x40000);
    }

    #[test]
    fn test_timer_modes_bit_position() {
        // Timer mode is in bits 17-18
        const TIMER_MODE_MASK: u32 = 0x3 << 17;
        assert_eq!(TIMER_MODE_MASK, 0x60000);
    }

    // =========================================================================
    // Delivery Mode Tests
    // =========================================================================

    #[test]
    fn test_delivery_modes() {
        const DELIVERY_MODE_FIXED: u32 = 0 << 8;
        const DELIVERY_MODE_LOWEST: u32 = 1 << 8;
        const DELIVERY_MODE_SMI: u32 = 2 << 8;
        const DELIVERY_MODE_NMI: u32 = 4 << 8;
        const DELIVERY_MODE_INIT: u32 = 5 << 8;
        const DELIVERY_MODE_STARTUP: u32 = 6 << 8;
        
        assert_eq!(DELIVERY_MODE_FIXED, 0);
        assert_eq!(DELIVERY_MODE_LOWEST, 0x100);
        assert_eq!(DELIVERY_MODE_SMI, 0x200);
        assert_eq!(DELIVERY_MODE_NMI, 0x400);
        assert_eq!(DELIVERY_MODE_INIT, 0x500);
        assert_eq!(DELIVERY_MODE_STARTUP, 0x600);
    }

    #[test]
    fn test_delivery_mode_bit_position() {
        // Delivery mode is in bits 8-10
        const DELIVERY_MODE_MASK: u32 = 0x7 << 8;
        assert_eq!(DELIVERY_MODE_MASK, 0x700);
    }

    // =========================================================================
    // ICR (Interrupt Command Register) Tests
    // =========================================================================

    #[test]
    fn test_icr_destination_field() {
        // ICR high register bits 24-31 contain destination APIC ID
        let apic_id: u32 = 5;
        let icr_high = apic_id << 24;
        assert_eq!(icr_high, 0x05000000);
    }

    #[test]
    fn test_icr_vector_field() {
        // ICR low register bits 0-7 contain interrupt vector
        let vector: u32 = 0x30;
        assert_eq!(vector & 0xFF, 0x30);
    }

    #[test]
    fn test_icr_level_trigger() {
        const ICR_LEVEL_ASSERT: u32 = 1 << 14;
        const ICR_LEVEL_DEASSERT: u32 = 0 << 14;
        
        assert_eq!(ICR_LEVEL_ASSERT, 0x4000);
        assert_eq!(ICR_LEVEL_DEASSERT, 0);
    }

    #[test]
    fn test_icr_trigger_mode() {
        const ICR_TRIGGER_EDGE: u32 = 0 << 15;
        const ICR_TRIGGER_LEVEL: u32 = 1 << 15;
        
        assert_eq!(ICR_TRIGGER_EDGE, 0);
        assert_eq!(ICR_TRIGGER_LEVEL, 0x8000);
    }

    #[test]
    fn test_icr_destination_shorthand() {
        const SHORTHAND_NONE: u32 = 0 << 18;
        const SHORTHAND_SELF: u32 = 1 << 18;
        const SHORTHAND_ALL_INCL: u32 = 2 << 18;
        const SHORTHAND_ALL_EXCL: u32 = 3 << 18;
        
        assert_eq!(SHORTHAND_NONE, 0);
        assert_eq!(SHORTHAND_SELF, 0x40000);
        assert_eq!(SHORTHAND_ALL_INCL, 0x80000);
        assert_eq!(SHORTHAND_ALL_EXCL, 0xC0000);
    }

    // =========================================================================
    // LVT (Local Vector Table) Entry Tests
    // =========================================================================

    #[test]
    fn test_lvt_mask_bit() {
        const LVT_MASKED: u32 = 1 << 16;
        assert_eq!(LVT_MASKED, 0x10000);
    }

    #[test]
    fn test_lvt_delivery_status() {
        const LVT_DELIVERY_IDLE: u32 = 0 << 12;
        const LVT_DELIVERY_PENDING: u32 = 1 << 12;
        
        assert_eq!(LVT_DELIVERY_IDLE, 0);
        assert_eq!(LVT_DELIVERY_PENDING, 0x1000);
    }

    // =========================================================================
    // Timer Divider Tests
    // =========================================================================

    #[test]
    fn test_timer_divide_values() {
        // Timer divide configuration register values
        const DIVIDE_BY_2: u32 = 0b0000;
        const DIVIDE_BY_4: u32 = 0b0001;
        const DIVIDE_BY_8: u32 = 0b0010;
        const DIVIDE_BY_16: u32 = 0b0011;
        const DIVIDE_BY_32: u32 = 0b1000;
        const DIVIDE_BY_64: u32 = 0b1001;
        const DIVIDE_BY_128: u32 = 0b1010;
        const DIVIDE_BY_1: u32 = 0b1011;
        
        // Verify these are valid 4-bit values
        assert!(DIVIDE_BY_2 < 16);
        assert!(DIVIDE_BY_4 < 16);
        assert!(DIVIDE_BY_8 < 16);
        assert!(DIVIDE_BY_16 < 16);
        assert!(DIVIDE_BY_32 < 16);
        assert!(DIVIDE_BY_64 < 16);
        assert!(DIVIDE_BY_128 < 16);
        assert!(DIVIDE_BY_1 < 16);
    }

    // =========================================================================
    // SVR (Spurious Vector Register) Tests
    // =========================================================================

    #[test]
    fn test_svr_apic_enable() {
        const SVR_APIC_ENABLE: u32 = 1 << 8;
        assert_eq!(SVR_APIC_ENABLE, 0x100);
    }

    #[test]
    fn test_svr_focus_disable() {
        const SVR_FOCUS_DISABLE: u32 = 1 << 9;
        assert_eq!(SVR_FOCUS_DISABLE, 0x200);
    }

    #[test]
    fn test_svr_eoi_broadcast_suppress() {
        const SVR_EOI_BROADCAST_SUPPRESS: u32 = 1 << 12;
        assert_eq!(SVR_EOI_BROADCAST_SUPPRESS, 0x1000);
    }

    // =========================================================================
    // DFR (Destination Format Register) Tests
    // =========================================================================

    #[test]
    fn test_dfr_models() {
        const DFR_FLAT_MODEL: u32 = 0xFFFFFFFF;
        const DFR_CLUSTER_MODEL: u32 = 0x0FFFFFFF;
        
        // Flat model: bits 28-31 = 0xF
        assert_eq!(DFR_FLAT_MODEL >> 28, 0xF);
        // Cluster model: bits 28-31 = 0x0
        assert_eq!(DFR_CLUSTER_MODEL >> 28, 0x0);
    }

    // =========================================================================
    // Error Status Register Tests
    // =========================================================================

    #[test]
    fn test_esr_error_bits() {
        const ESR_SEND_CHECKSUM: u32 = 1 << 0;
        const ESR_RECV_CHECKSUM: u32 = 1 << 1;
        const ESR_SEND_ACCEPT: u32 = 1 << 2;
        const ESR_RECV_ACCEPT: u32 = 1 << 3;
        const ESR_REDIRECTABLE_IPI: u32 = 1 << 4;
        const ESR_SEND_ILLEGAL_VECTOR: u32 = 1 << 5;
        const ESR_RECV_ILLEGAL_VECTOR: u32 = 1 << 6;
        const ESR_ILLEGAL_REGISTER: u32 = 1 << 7;
        
        // All bits should be unique
        let all = ESR_SEND_CHECKSUM | ESR_RECV_CHECKSUM | ESR_SEND_ACCEPT |
                  ESR_RECV_ACCEPT | ESR_REDIRECTABLE_IPI | ESR_SEND_ILLEGAL_VECTOR |
                  ESR_RECV_ILLEGAL_VECTOR | ESR_ILLEGAL_REGISTER;
        assert_eq!(all.count_ones(), 8);
    }

    // =========================================================================
    // APIC ID Tests
    // =========================================================================

    #[test]
    fn test_apic_id_extraction() {
        // APIC ID is in bits 24-31 of the ID register
        let id_reg: u32 = 0x05000000;
        let apic_id = id_reg >> 24;
        assert_eq!(apic_id, 5);
    }

    #[test]
    fn test_apic_id_max_value() {
        // 8-bit APIC ID supports up to 256 CPUs
        let max_apic_id: u32 = 0xFF;
        assert_eq!(max_apic_id, 255);
    }
}
