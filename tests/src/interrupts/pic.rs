//! PIC (Programmable Interrupt Controller) Tests
//!
//! Tests for PIC configuration and IRQ vector numbers.

#[cfg(test)]
mod tests {
    // Import actual kernel PIC constants
    use crate::interrupts::handlers::{PIC_1_OFFSET, PIC_2_OFFSET};

    // =========================================================================
    // PIC Offset Tests
    // =========================================================================

    #[test]
    fn test_pic1_offset() {
        // PIC1 starts at vector 32 (after CPU exceptions 0-31)
        assert_eq!(PIC_1_OFFSET, 32);
    }

    #[test]
    fn test_pic2_offset() {
        // PIC2 follows PIC1 at offset + 8
        assert_eq!(PIC_2_OFFSET, PIC_1_OFFSET + 8);
        assert_eq!(PIC_2_OFFSET, 40);
    }

    #[test]
    fn test_pic_offsets_not_conflicting() {
        // PIC1 and PIC2 should not overlap
        assert!(PIC_2_OFFSET >= PIC_1_OFFSET + 8);
    }

    // =========================================================================
    // IRQ Vector Calculation Tests
    // =========================================================================

    #[test]
    fn test_timer_irq_vector() {
        // Timer is IRQ 0 on PIC1
        let timer_irq = 0;
        let timer_vector = PIC_1_OFFSET + timer_irq;
        assert_eq!(timer_vector, 32);
    }

    #[test]
    fn test_keyboard_irq_vector() {
        // Keyboard is IRQ 1 on PIC1
        let keyboard_irq = 1;
        let keyboard_vector = PIC_1_OFFSET + keyboard_irq;
        assert_eq!(keyboard_vector, 33);
    }

    #[test]
    fn test_cascade_irq_vector() {
        // Cascade (PIC2 connection) is IRQ 2 on PIC1
        let cascade_irq = 2;
        let cascade_vector = PIC_1_OFFSET + cascade_irq;
        assert_eq!(cascade_vector, 34);
    }

    #[test]
    fn test_com2_irq_vector() {
        // COM2/COM4 is IRQ 3 on PIC1
        let com2_irq = 3;
        let com2_vector = PIC_1_OFFSET + com2_irq;
        assert_eq!(com2_vector, 35);
    }

    #[test]
    fn test_com1_irq_vector() {
        // COM1/COM3 is IRQ 4 on PIC1
        let com1_irq = 4;
        let com1_vector = PIC_1_OFFSET + com1_irq;
        assert_eq!(com1_vector, 36);
    }

    #[test]
    fn test_pic2_rtc_irq_vector() {
        // RTC is IRQ 0 on PIC2 (IRQ 8 in total)
        let rtc_irq = 0;
        let rtc_vector = PIC_2_OFFSET + rtc_irq;
        assert_eq!(rtc_vector, 40);
    }

    // =========================================================================
    // PIC IRQ Range Tests
    // =========================================================================

    #[test]
    fn test_pic1_irq_range() {
        // PIC1 handles IRQ 0-7 (vectors 32-39)
        let pic1_start = PIC_1_OFFSET;
        let pic1_end = PIC_1_OFFSET + 7;

        assert_eq!(pic1_start, 32);
        assert_eq!(pic1_end, 39);
    }

    #[test]
    fn test_pic2_irq_range() {
        // PIC2 handles IRQ 8-15 (vectors 40-47)
        let pic2_start = PIC_2_OFFSET;
        let pic2_end = PIC_2_OFFSET + 7;

        assert_eq!(pic2_start, 40);
        assert_eq!(pic2_end, 47);
    }

    #[test]
    fn test_total_pic_irqs() {
        // Total 16 IRQs from both PICs
        let pic1_count = 8;
        let pic2_count = 8;
        let total = pic1_count + pic2_count;

        assert_eq!(total, 16);
    }

    // =========================================================================
    // Vector Validation Tests
    // =========================================================================

    #[test]
    fn test_pic_avoids_exceptions() {
        // PIC vectors should not conflict with CPU exceptions (0-31)
        assert!(PIC_1_OFFSET >= 32, "PIC1 must start after exceptions");
        assert!(PIC_2_OFFSET >= 32, "PIC2 must start after exceptions");
    }

    #[test]
    fn test_pic_in_valid_range() {
        // IDT has 256 entries (0-255)
        let max_pic_vector = PIC_2_OFFSET + 7;
        assert!(max_pic_vector <= 255, "PIC vectors must fit in IDT");
    }

    // =========================================================================
    // Spurious IRQ Tests
    // =========================================================================

    #[test]
    fn test_pic1_spurious_vector() {
        // PIC1 spurious IRQ is IRQ 7
        let spurious_vector = PIC_1_OFFSET + 7;
        assert_eq!(spurious_vector, 39);
    }

    #[test]
    fn test_pic2_spurious_vector() {
        // PIC2 spurious IRQ is IRQ 15
        let spurious_vector = PIC_2_OFFSET + 7;
        assert_eq!(spurious_vector, 47);
    }

    // =========================================================================
    // IRQ to Vector Conversion Tests
    // =========================================================================

    #[test]
    fn test_irq_to_vector_conversion() {
        fn irq_to_vector(irq: u8) -> u8 {
            if irq < 8 {
                PIC_1_OFFSET + irq
            } else {
                PIC_2_OFFSET + (irq - 8)
            }
        }

        // PIC1 IRQs
        assert_eq!(irq_to_vector(0), 32); // Timer
        assert_eq!(irq_to_vector(1), 33); // Keyboard
        assert_eq!(irq_to_vector(7), 39); // Spurious

        // PIC2 IRQs
        assert_eq!(irq_to_vector(8), 40);  // RTC
        assert_eq!(irq_to_vector(12), 44); // PS/2 Mouse
        assert_eq!(irq_to_vector(15), 47); // Secondary ATA / Spurious
    }

    #[test]
    fn test_vector_to_irq_conversion() {
        fn vector_to_irq(vector: u8) -> Option<u8> {
            if vector >= PIC_1_OFFSET && vector < PIC_1_OFFSET + 8 {
                Some(vector - PIC_1_OFFSET)
            } else if vector >= PIC_2_OFFSET && vector < PIC_2_OFFSET + 8 {
                Some(vector - PIC_2_OFFSET + 8)
            } else {
                None
            }
        }

        assert_eq!(vector_to_irq(32), Some(0));  // Timer
        assert_eq!(vector_to_irq(33), Some(1));  // Keyboard
        assert_eq!(vector_to_irq(40), Some(8));  // RTC
        assert_eq!(vector_to_irq(0), None);      // Exception, not IRQ
        assert_eq!(vector_to_irq(255), None);    // Out of PIC range
    }

    // =========================================================================
    // EOI Requirement Tests
    // =========================================================================

    #[test]
    fn test_pic1_needs_eoi() {
        // IRQs 0-7 need EOI to PIC1 only
        fn needs_pic1_eoi(irq: u8) -> bool {
            irq < 16 // All IRQs need PIC1 EOI
        }

        assert!(needs_pic1_eoi(0));  // Timer
        assert!(needs_pic1_eoi(7));  // PIC1 spurious
        assert!(needs_pic1_eoi(8));  // PIC2 IRQ also needs PIC1 EOI (cascade)
    }

    #[test]
    fn test_pic2_needs_eoi() {
        // IRQs 8-15 need EOI to both PICs
        fn needs_pic2_eoi(irq: u8) -> bool {
            irq >= 8 && irq < 16
        }

        assert!(!needs_pic2_eoi(0)); // Timer - PIC1 only
        assert!(!needs_pic2_eoi(7)); // PIC1 spurious - PIC1 only
        assert!(needs_pic2_eoi(8));  // RTC - needs both
        assert!(needs_pic2_eoi(15)); // Secondary ATA - needs both
    }
}
