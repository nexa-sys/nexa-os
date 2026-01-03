//! IPI (Inter-Processor Interrupt) Tests
//!
//! Tests for IPI vector constants and related structures.

#[cfg(test)]
mod tests {
    use crate::interrupts::ipi::{
        IPI_CALL_FUNCTION, IPI_HALT, IPI_RESCHEDULE, IPI_TLB_FLUSH,
    };

    // =========================================================================
    // IPI Vector Constants Tests
    // =========================================================================

    #[test]
    fn test_ipi_reschedule_vector() {
        assert_eq!(IPI_RESCHEDULE, 0xF0);
    }

    #[test]
    fn test_ipi_tlb_flush_vector() {
        assert_eq!(IPI_TLB_FLUSH, 0xF1);
    }

    #[test]
    fn test_ipi_call_function_vector() {
        assert_eq!(IPI_CALL_FUNCTION, 0xF2);
    }

    #[test]
    fn test_ipi_halt_vector() {
        assert_eq!(IPI_HALT, 0xF3);
    }

    #[test]
    fn test_ipi_vectors_unique() {
        let vectors = [IPI_RESCHEDULE, IPI_TLB_FLUSH, IPI_CALL_FUNCTION, IPI_HALT];
        for i in 0..vectors.len() {
            for j in (i + 1)..vectors.len() {
                assert_ne!(vectors[i], vectors[j]);
            }
        }
    }

    #[test]
    fn test_ipi_vectors_in_range() {
        // IPI vectors should be in the user-defined range (0x20-0xFF)
        assert!(IPI_RESCHEDULE >= 0x20);
        assert!(IPI_TLB_FLUSH >= 0x20);
        assert!(IPI_CALL_FUNCTION >= 0x20);
        assert!(IPI_HALT >= 0x20);
    }

    #[test]
    fn test_ipi_vectors_above_exceptions() {
        // First 32 vectors (0x00-0x1F) are reserved for CPU exceptions
        const EXCEPTION_END: u8 = 0x20;
        assert!(IPI_RESCHEDULE >= EXCEPTION_END);
        assert!(IPI_TLB_FLUSH >= EXCEPTION_END);
        assert!(IPI_CALL_FUNCTION >= EXCEPTION_END);
        assert!(IPI_HALT >= EXCEPTION_END);
    }

    #[test]
    fn test_ipi_vectors_high_priority() {
        // IPI vectors are typically in the high range for priority
        const HIGH_VECTOR_START: u8 = 0xF0;
        assert!(IPI_RESCHEDULE >= HIGH_VECTOR_START);
        assert!(IPI_TLB_FLUSH >= HIGH_VECTOR_START);
        assert!(IPI_CALL_FUNCTION >= HIGH_VECTOR_START);
        assert!(IPI_HALT >= HIGH_VECTOR_START);
    }

    // =========================================================================
    // IPI Destination Tests (Conceptual)
    // =========================================================================

    #[test]
    fn test_ipi_destination_modes() {
        // APIC destination modes
        const DEST_NO_SHORTHAND: u32 = 0;
        const DEST_SELF: u32 = 1;
        const DEST_ALL_INCLUDING_SELF: u32 = 2;
        const DEST_ALL_EXCLUDING_SELF: u32 = 3;

        assert_eq!(DEST_NO_SHORTHAND, 0);
        assert_eq!(DEST_SELF, 1);
        assert_eq!(DEST_ALL_INCLUDING_SELF, 2);
        assert_eq!(DEST_ALL_EXCLUDING_SELF, 3);
    }

    #[test]
    fn test_ipi_delivery_modes() {
        // APIC delivery modes
        const DELIVERY_FIXED: u32 = 0;
        const DELIVERY_LOWEST: u32 = 1;
        const DELIVERY_SMI: u32 = 2;
        const DELIVERY_NMI: u32 = 4;
        const DELIVERY_INIT: u32 = 5;
        const DELIVERY_SIPI: u32 = 6;

        assert_eq!(DELIVERY_FIXED, 0);
        assert_eq!(DELIVERY_INIT, 5);
        assert_eq!(DELIVERY_SIPI, 6);
    }

    // =========================================================================
    // LAPIC ICR Encoding Tests
    // =========================================================================

    #[test]
    fn test_icr_vector_field() {
        fn encode_icr_low(vector: u8, delivery: u32, dest_mode: u32) -> u32 {
            (vector as u32) | (delivery << 8) | (dest_mode << 18)
        }

        let icr = encode_icr_low(IPI_RESCHEDULE, 0, 3);
        assert_eq!(icr & 0xFF, IPI_RESCHEDULE as u32);
    }

    #[test]
    fn test_icr_dest_field() {
        fn encode_icr_high(dest_apic_id: u8) -> u32 {
            (dest_apic_id as u32) << 24
        }

        let icr_hi = encode_icr_high(1);
        assert_eq!(icr_hi, 0x01000000);
    }
}
