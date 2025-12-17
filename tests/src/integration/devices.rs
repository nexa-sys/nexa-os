//! Device integration tests
//!
//! Tests kernel interaction with emulated hardware devices.

use crate::mock::vm::{VirtualMachine, VmConfig, TestHarness};
use crate::mock::hal;
use crate::mock::devices::{Device, DeviceId, IoAccess};

/// Test UART (Serial) device interaction
mod uart_tests {
    use super::*;
    
    #[test]
    fn test_serial_port_detection() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // COM1 base port is 0x3F8
            // Reading the Line Status Register (offset 5) should return valid status
            let lsr = hal::inb(0x3F8 + 5);
            // THRE (bit 5) and TEMT (bit 6) should be set when TX is empty
            assert!(lsr & 0x60 != 0, "UART should have empty TX buffer");
        });
    }
    
    #[test]
    fn test_serial_write() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Write a character to COM1 THR (offset 0)
            hal::outb(0x3F8, b'H');
            hal::outb(0x3F8, b'i');
            
            // Check LSR - TX should still be ready after write
            let lsr = hal::inb(0x3F8 + 5);
            assert!(lsr & 0x20 != 0, "UART TX should be ready");
        });
    }
    
    #[test]
    fn test_serial_interrupt_enable() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Enable TX empty interrupt (IER bit 1)
            hal::outb(0x3F8 + 1, 0x02);
            
            // Read back IER
            let ier = hal::inb(0x3F8 + 1);
            assert_eq!(ier & 0x02, 0x02, "THRE interrupt should be enabled");
        });
    }
    
    #[test]
    fn test_serial_dlab_access() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Set DLAB (bit 7 of LCR at offset 3)
            hal::outb(0x3F8 + 3, 0x80);
            
            // Write divisor latch (baud rate)
            hal::outb(0x3F8 + 0, 0x01); // Low byte
            hal::outb(0x3F8 + 1, 0x00); // High byte (115200 baud)
            
            // Read back divisor
            let div_lo = hal::inb(0x3F8 + 0);
            assert_eq!(div_lo, 0x01, "Divisor low byte should be 1");
            
            // Clear DLAB, set 8N1
            hal::outb(0x3F8 + 3, 0x03);
            let lcr = hal::inb(0x3F8 + 3);
            assert_eq!(lcr & 0x03, 0x03, "LCR should be set to 8N1");
        });
    }
}

/// Test PIC (Programmable Interrupt Controller) device
mod pic_tests {
    use super::*;
    
    #[test]
    fn test_pic_initialization() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Standard PIC initialization sequence
            // ICW1: edge triggered, cascade mode, ICW4 needed
            hal::outb(0x20, 0x11);
            hal::outb(0xA0, 0x11);
            
            // ICW2: interrupt vector offset
            hal::outb(0x21, 0x20); // Master: IRQ 0-7 -> INT 0x20-0x27
            hal::outb(0xA1, 0x28); // Slave: IRQ 8-15 -> INT 0x28-0x2F
            
            // ICW3: master/slave wiring
            hal::outb(0x21, 0x04); // Master: slave on IRQ2
            hal::outb(0xA1, 0x02); // Slave: cascade identity
            
            // ICW4: 8086 mode
            hal::outb(0x21, 0x01);
            hal::outb(0xA1, 0x01);
            
            // Read ISR - should be 0 (no interrupts being serviced)
            hal::outb(0x20, 0x0B); // Read ISR command
            let isr = hal::inb(0x20);
            assert_eq!(isr, 0, "ISR should be empty after init");
        });
    }
    
    #[test]
    fn test_pic_masking() {
        let harness = TestHarness::with_config(VmConfig::default());
        harness.run(|vm| {
            // Initialize PIC first
            hal::outb(0x20, 0x11);
            hal::outb(0x21, 0x20);
            hal::outb(0x21, 0x04);
            hal::outb(0x21, 0x01);
            
            // Mask all interrupts except IRQ0 (timer)
            hal::outb(0x21, 0xFE);
            
            // Read back IMR
            let imr = hal::inb(0x21);
            assert_eq!(imr, 0xFE, "IMR should mask all but IRQ0");
        });
    }
}

/// Test PIT (Programmable Interval Timer) device
mod pit_tests {
    use super::*;
    
    #[test]
    fn test_pit_counter_read() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Read counter 0 value
            // First send latch command (counter 0, latch)
            hal::outb(0x43, 0x00);
            
            // Read low and high bytes
            let lo = hal::inb(0x40);
            let hi = hal::inb(0x40);
            let _count = (hi as u16) << 8 | lo as u16;
            
            // Counter should be non-zero (default 65535 in mode 2)
            // Just verify we can read it without panic
        });
    }
    
    #[test]
    fn test_pit_mode_set() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Set counter 0 to mode 2 (rate generator), LSB/MSB access
            hal::outb(0x43, 0x34); // 00 11 010 0 = counter 0, LSB/MSB, mode 2, binary
            
            // Set count to 11932 (100 Hz at 1.193182 MHz)
            hal::outb(0x40, 0x9C); // Low byte
            hal::outb(0x40, 0x2E); // High byte
            
            // Latch and read back
            hal::outb(0x43, 0x00);
            let lo = hal::inb(0x40);
            let hi = hal::inb(0x40);
            let count = (hi as u16) << 8 | lo as u16;
            
            // Count should be around 11932 (may have decremented)
            assert!(count <= 11932, "PIT counter should be <= initial value");
        });
    }
}

/// Test RTC (Real Time Clock) device
mod rtc_tests {
    use super::*;
    
    #[test]
    fn test_rtc_read_time() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Read seconds
            hal::outb(0x70, 0x00);
            let seconds = hal::inb(0x71);
            
            // Read minutes
            hal::outb(0x70, 0x02);
            let minutes = hal::inb(0x71);
            
            // Read hours
            hal::outb(0x70, 0x04);
            let hours = hal::inb(0x71);
            
            // Values should be valid BCD (0x00-0x59 for sec/min, 0x00-0x23 for hours)
            // In emulation, may start at 00:00:00
            assert!(seconds <= 0x59, "RTC seconds should be valid");
            assert!(minutes <= 0x59, "RTC minutes should be valid");
            assert!(hours <= 0x23, "RTC hours should be valid");
        });
    }
    
    #[test]
    fn test_rtc_status_register() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Read Status Register A
            hal::outb(0x70, 0x0A);
            let status_a = hal::inb(0x71);
            
            // Read Status Register B
            hal::outb(0x70, 0x0B);
            let status_b = hal::inb(0x71);
            
            // UIP bit (7) in status A should not always be set
            // Status B default values vary by implementation
            
            // Just verify we can read without panic
        });
    }
}

/// Test PCI device enumeration
mod pci_tests {
    use super::*;
    
    #[test]
    fn test_pci_host_bridge_present() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Read vendor ID of device at bus 0, device 0, function 0
            // PCI config space address: 0x80000000 | (bus << 16) | (dev << 11) | (func << 8) | reg
            let addr: u32 = 0x80000000;
            
            hal::outl(0xCF8, addr);
            let vendor_device = hal::inl(0xCFC);
            
            let vendor_id = (vendor_device & 0xFFFF) as u16;
            let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;
            
            // Should have a host bridge (vendor ID != 0xFFFF)
            assert_ne!(vendor_id, 0xFFFF, "PCI host bridge should be present");
        });
    }
    
    #[test]
    fn test_pci_config_space_access() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // Read class code of host bridge
            let addr: u32 = 0x80000000 | 0x08; // Offset 0x08: revision/class
            
            hal::outl(0xCF8, addr);
            let rev_class = hal::inl(0xCFC);
            
            let class_code = ((rev_class >> 24) & 0xFF) as u8;
            let subclass = ((rev_class >> 16) & 0xFF) as u8;
            
            // Host bridge: class 0x06, subclass 0x00
            assert_eq!(class_code, 0x06, "PCI host bridge class should be 0x06");
            assert_eq!(subclass, 0x00, "PCI host bridge subclass should be 0x00");
        });
    }
}
