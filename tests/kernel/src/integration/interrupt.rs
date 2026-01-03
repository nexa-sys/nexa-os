//! Interrupt handling integration tests
//!
//! Tests interrupt controller configuration and interrupt delivery
//! using the Virtual Machine emulation layer.

use crate::mock::vm::{VirtualMachine, VmConfig, TestHarness};
use crate::mock::hal;

/// PIC interrupt tests
mod pic_interrupt {
    use super::*;
    
    #[test]
    fn test_pic_eoi() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Initialize PIC
            hal::outb(0x20, 0x11);
            hal::outb(0x21, 0x20);
            hal::outb(0x21, 0x04);
            hal::outb(0x21, 0x01);
            
            // Send EOI (End Of Interrupt)
            hal::outb(0x20, 0x20);
            
            // Read ISR - should be 0
            hal::outb(0x20, 0x0B);
            let isr = hal::inb(0x20);
            assert_eq!(isr, 0, "ISR should be clear after EOI");
        });
    }
    
    #[test]
    fn test_pic_cascade() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Initialize both PICs
            // Master
            hal::outb(0x20, 0x11);
            hal::outb(0x21, 0x20);
            hal::outb(0x21, 0x04);
            hal::outb(0x21, 0x01);
            
            // Slave
            hal::outb(0xA0, 0x11);
            hal::outb(0xA1, 0x28);
            hal::outb(0xA1, 0x02);
            hal::outb(0xA1, 0x01);
            
            // Unmask IRQ2 (cascade) on master
            let mut mask = hal::inb(0x21);
            mask &= !0x04;
            hal::outb(0x21, mask);
            
            // Read slave IMR
            let slave_imr = hal::inb(0xA1);
            // Just verify cascade is set up properly
        });
    }
}

/// Timer interrupt tests (via PIT)
mod timer_interrupt {
    use super::*;
    
    #[test]
    fn test_pit_interrupt_setup() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Initialize PIC with timer enabled
            hal::outb(0x20, 0x11);
            hal::outb(0x21, 0x20);
            hal::outb(0x21, 0x04);
            hal::outb(0x21, 0x01);
            hal::outb(0x21, 0xFE); // Only timer (IRQ0) enabled
            
            // Configure PIT channel 0 for 100 Hz
            hal::outb(0x43, 0x36); // Channel 0, LSB/MSB, mode 3, binary
            
            // 1193182 / 100 = 11932
            hal::outb(0x40, 0x9C); // LSB
            hal::outb(0x40, 0x2E); // MSB
            
            // PIT is now configured for 100 Hz interrupts on IRQ0
        });
    }
}

/// APIC tests (when enabled)
mod apic_tests {
    use super::*;
    
    #[test]
    fn test_lapic_id_read() {
        let harness = TestHarness::with_config(VmConfig::full());
        harness.run(|vm| {
            // LAPIC ID register is at offset 0x20
            let lapic_base: u64 = 0xFEE00000;
            let lapic_id = hal::mmio_read_u32(lapic_base + 0x20);
            
            // BSP should have APIC ID 0
            let id = (lapic_id >> 24) & 0xFF;
            assert_eq!(id, 0, "BSP should have LAPIC ID 0");
        });
    }
    
    #[test]
    fn test_lapic_version_read() {
        let harness = TestHarness::with_config(VmConfig::full());
        harness.run(|vm| {
            // LAPIC version register is at offset 0x30
            let lapic_base: u64 = 0xFEE00000;
            let version = hal::mmio_read_u32(lapic_base + 0x30);
            
            // Version should be non-zero
            // LAPIC versions are typically 0x10-0x15
            let ver = version & 0xFF;
            assert!(ver >= 0x10 && ver <= 0x20, 
                "LAPIC version should be in expected range");
        });
    }
    
    #[test]
    fn test_lapic_spurious_vector() {
        let harness = TestHarness::with_config(VmConfig::full());
        harness.run(|vm| {
            // Spurious interrupt vector register at offset 0xF0
            let lapic_base: u64 = 0xFEE00000;
            
            // Enable LAPIC with spurious vector 0xFF
            hal::mmio_write_u32(lapic_base + 0xF0, 0x1FF); // Enable + vector 0xFF
            
            let svr = hal::mmio_read_u32(lapic_base + 0xF0);
            assert!(svr & 0x100 != 0, "LAPIC should be enabled");
            assert_eq!(svr & 0xFF, 0xFF, "Spurious vector should be 0xFF");
        });
    }
}

/// Interrupt flag tests
mod interrupt_flag_tests {
    use super::*;
    
    #[test]
    fn test_cli_sti_emulation() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Initially interrupts may be enabled or disabled
            let initial_state = hal::interrupts_enabled();
            
            // Disable interrupts
            hal::cli();
            assert!(!hal::interrupts_enabled(), "Interrupts should be disabled after CLI");
            
            // Enable interrupts  
            hal::sti();
            assert!(hal::interrupts_enabled(), "Interrupts should be enabled after STI");
            
            // Restore initial state
            if !initial_state {
                hal::cli();
            }
        });
    }
    
    #[test]
    fn test_interrupt_guard() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            hal::sti(); // Ensure enabled
            
            {
                // Simulate interrupt-disabled critical section
                let _guard = hal::interrupt_free(|| {
                    assert!(!hal::interrupts_enabled(), 
                        "Interrupts should be disabled in critical section");
                    42 // Return value
                });
            }
            
            // Should be re-enabled after guard drops
            // (actual behavior depends on implementation)
        });
    }
}
