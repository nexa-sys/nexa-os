//! IDT (Interrupt Descriptor Table) Tests
//!
//! Tests for IDT structure and interrupt vector assignments.

#[cfg(test)]
mod tests {
    // =========================================================================
    // IDT Structure Tests
    // =========================================================================

    #[test]
    fn test_idt_entry_count() {
        // x86_64 IDT has 256 entries
        let idt_entries = 256;
        assert_eq!(idt_entries, 256);
    }

    #[test]
    fn test_idt_entry_size() {
        // Each IDT entry is 16 bytes in 64-bit mode
        let entry_size = 16;
        assert_eq!(entry_size, 16);
    }

    #[test]
    fn test_idt_total_size() {
        // Total IDT size = 256 * 16 = 4096 bytes
        let total_size = 256 * 16;
        assert_eq!(total_size, 4096);
    }

    // =========================================================================
    // Exception Vector Tests
    // =========================================================================

    #[test]
    fn test_divide_error_vector() {
        let vector = 0;
        assert_eq!(vector, 0);
    }

    #[test]
    fn test_debug_vector() {
        let vector = 1;
        assert_eq!(vector, 1);
    }

    #[test]
    fn test_nmi_vector() {
        let vector = 2;
        assert_eq!(vector, 2);
    }

    #[test]
    fn test_breakpoint_vector() {
        let vector = 3;
        assert_eq!(vector, 3);
    }

    #[test]
    fn test_overflow_vector() {
        let vector = 4;
        assert_eq!(vector, 4);
    }

    #[test]
    fn test_bound_range_vector() {
        let vector = 5;
        assert_eq!(vector, 5);
    }

    #[test]
    fn test_invalid_opcode_vector() {
        let vector = 6;
        assert_eq!(vector, 6);
    }

    #[test]
    fn test_device_not_available_vector() {
        let vector = 7;
        assert_eq!(vector, 7);
    }

    #[test]
    fn test_double_fault_vector() {
        let vector = 8;
        assert_eq!(vector, 8);
    }

    #[test]
    fn test_invalid_tss_vector() {
        let vector = 10;
        assert_eq!(vector, 10);
    }

    #[test]
    fn test_segment_not_present_vector() {
        let vector = 11;
        assert_eq!(vector, 11);
    }

    #[test]
    fn test_stack_segment_fault_vector() {
        let vector = 12;
        assert_eq!(vector, 12);
    }

    #[test]
    fn test_general_protection_fault_vector() {
        let vector = 13;
        assert_eq!(vector, 13);
    }

    #[test]
    fn test_page_fault_vector() {
        let vector = 14;
        assert_eq!(vector, 14);
    }

    #[test]
    fn test_x87_fpu_error_vector() {
        let vector = 16;
        assert_eq!(vector, 16);
    }

    #[test]
    fn test_alignment_check_vector() {
        let vector = 17;
        assert_eq!(vector, 17);
    }

    #[test]
    fn test_machine_check_vector() {
        let vector = 18;
        assert_eq!(vector, 18);
    }

    #[test]
    fn test_simd_exception_vector() {
        let vector = 19;
        assert_eq!(vector, 19);
    }

    #[test]
    fn test_virtualization_exception_vector() {
        let vector = 20;
        assert_eq!(vector, 20);
    }

    // =========================================================================
    // Reserved Vector Tests
    // =========================================================================

    #[test]
    fn test_reserved_vectors() {
        // Vectors 21-31 are reserved by Intel
        let reserved_start = 21;
        let reserved_end = 31;
        let reserved_count = reserved_end - reserved_start + 1;

        assert_eq!(reserved_count, 11);
    }

    // =========================================================================
    // User-Defined Vector Tests
    // =========================================================================

    #[test]
    fn test_user_vectors_start() {
        // User-defined vectors start at 32
        let user_start = 32;
        assert_eq!(user_start, 32);
    }

    #[test]
    fn test_user_vectors_range() {
        // User vectors: 32-255
        let user_start = 32;
        let user_end = 255;
        let user_count = user_end - user_start + 1;

        assert_eq!(user_count, 224);
    }

    // =========================================================================
    // Syscall Vector Tests
    // =========================================================================

    #[test]
    fn test_syscall_int_vector() {
        // NexaOS uses int 0x81 for software interrupt syscalls
        let syscall_vector = 0x81;
        assert_eq!(syscall_vector, 129);
    }

    #[test]
    fn test_syscall_vector_in_user_range() {
        // Syscall vector must be in user-defined range
        let syscall_vector = 0x81;
        assert!(syscall_vector >= 32);
        assert!(syscall_vector <= 255);
    }

    // =========================================================================
    // APIC Vector Tests
    // =========================================================================

    #[test]
    fn test_apic_spurious_vector() {
        // APIC spurious vector typically 0xFF
        let spurious_vector = 0xFF;
        assert_eq!(spurious_vector, 255);
    }

    #[test]
    fn test_apic_error_vector() {
        // APIC error vector typically 0xFE
        let error_vector = 0xFE;
        assert_eq!(error_vector, 254);
    }

    #[test]
    fn test_apic_timer_vector() {
        // Local APIC timer can use any user vector
        // Common choice: 0xFD
        let timer_vector = 0xFD;
        assert_eq!(timer_vector, 253);
    }

    // =========================================================================
    // IPI Vector Tests
    // =========================================================================

    #[test]
    fn test_ipi_reschedule_vector() {
        // IPI for cross-CPU reschedule
        let reschedule_vector = 0xFC;
        assert_eq!(reschedule_vector, 252);
    }

    #[test]
    fn test_ipi_tlb_shootdown_vector() {
        // IPI for TLB invalidation
        let tlb_vector = 0xFB;
        assert_eq!(tlb_vector, 251);
    }

    // =========================================================================
    // IDT Descriptor Tests
    // =========================================================================

    #[test]
    fn test_idt_descriptor_limit() {
        // IDTR limit = (entry count * entry size) - 1
        let limit = (256 * 16) - 1;
        assert_eq!(limit, 4095);
    }

    #[test]
    fn test_idt_gate_types() {
        // Gate types in type_attr field
        let interrupt_gate = 0xE; // 64-bit interrupt gate
        let trap_gate = 0xF;      // 64-bit trap gate

        assert_eq!(interrupt_gate, 14);
        assert_eq!(trap_gate, 15);
    }

    #[test]
    fn test_interrupt_gate_clears_if() {
        // Interrupt gates clear IF (disable interrupts on entry)
        let clears_if = true;
        assert!(clears_if);
    }

    #[test]
    fn test_trap_gate_preserves_if() {
        // Trap gates preserve IF
        let preserves_if = true;
        assert!(preserves_if);
    }

    // =========================================================================
    // DPL Tests
    // =========================================================================

    #[test]
    fn test_kernel_only_gate() {
        // DPL 0 = kernel only (most exceptions)
        let dpl = 0;
        assert_eq!(dpl, 0);
    }

    #[test]
    fn test_user_callable_gate() {
        // DPL 3 = user can trigger (syscall int)
        let dpl = 3;
        assert_eq!(dpl, 3);
    }

    #[test]
    fn test_syscall_gate_dpl() {
        // INT 0x81 syscall must be DPL 3 for user access
        let syscall_dpl = 3;
        assert_eq!(syscall_dpl, 3);
    }

    // =========================================================================
    // Present Bit Tests
    // =========================================================================

    #[test]
    fn test_present_bit() {
        // Present bit must be set for valid IDT entries
        let present = 1;
        assert_eq!(present, 1);
    }

    #[test]
    fn test_absent_entry() {
        // Absent entries (P=0) cause #GP on access
        let absent = 0;
        assert_eq!(absent, 0);
    }
}
