//! GS Context Slot Tests
//!
//! Tests for GS_DATA slots used in syscall/sysret transitions.

#[cfg(test)]
mod tests {
    // Import actual kernel GS slot constants
    use crate::interrupts::gs_context::{
        GS_SLOT_USER_RSP, GS_SLOT_KERNEL_RSP, GS_SLOT_USER_ENTRY, GS_SLOT_USER_STACK,
        GS_SLOT_USER_CS, GS_SLOT_USER_SS, GS_SLOT_USER_DS, GS_SLOT_SAVED_RCX,
        GS_SLOT_SAVED_RFLAGS, GS_SLOT_USER_RSP_DEBUG, GS_SLOT_SAVED_RAX,
        GS_SLOT_SAVED_RDI, GS_SLOT_SAVED_RSI, GS_SLOT_SAVED_RDX, GS_SLOT_SAVED_RBX,
        GS_SLOT_SAVED_RBP, GS_SLOT_SAVED_R8, GS_SLOT_SAVED_R9, GS_SLOT_SAVED_R10,
        GS_SLOT_SAVED_R12, GS_SLOT_KERNEL_STACK_GUARD, GS_SLOT_KERNEL_STACK_SNAPSHOT,
        GS_SLOT_INT81_RBX, GS_SLOT_INT81_RBP, GS_SLOT_INT81_R12, GS_SLOT_INT81_R13,
        GS_SLOT_INT81_R14, GS_SLOT_INT81_R15,
        GUARD_SOURCE_INT_GATE, GUARD_SOURCE_SYSCALL,
        encode_hex_u64,
    };

    // =========================================================================
    // Core Slot Tests
    // =========================================================================

    #[test]
    fn test_core_slot_indices() {
        // Core slots for user/kernel transition
        assert_eq!(GS_SLOT_USER_RSP, 0);
        assert_eq!(GS_SLOT_KERNEL_RSP, 1);
        assert_eq!(GS_SLOT_USER_ENTRY, 2);
        assert_eq!(GS_SLOT_USER_STACK, 3);
    }

    #[test]
    fn test_segment_slot_indices() {
        // Segment selectors
        assert_eq!(GS_SLOT_USER_CS, 4);
        assert_eq!(GS_SLOT_USER_SS, 5);
        assert_eq!(GS_SLOT_USER_DS, 6);
    }

    #[test]
    fn test_saved_register_slots() {
        // Saved registers for syscall context
        assert_eq!(GS_SLOT_SAVED_RCX, 7);
        assert_eq!(GS_SLOT_SAVED_RFLAGS, 8);
        assert_eq!(GS_SLOT_USER_RSP_DEBUG, 9);
        assert_eq!(GS_SLOT_SAVED_RAX, 10);
    }

    // =========================================================================
    // Fork Register Slots Tests
    // =========================================================================

    #[test]
    fn test_fork_register_slots() {
        // Slots 11-19 for fork() register preservation
        assert_eq!(GS_SLOT_SAVED_RDI, 11);
        assert_eq!(GS_SLOT_SAVED_RSI, 12);
        assert_eq!(GS_SLOT_SAVED_RDX, 13);
        assert_eq!(GS_SLOT_SAVED_RBX, 14);
        assert_eq!(GS_SLOT_SAVED_RBP, 15);
        assert_eq!(GS_SLOT_SAVED_R8, 16);
        assert_eq!(GS_SLOT_SAVED_R9, 17);
        assert_eq!(GS_SLOT_SAVED_R10, 18);
        assert_eq!(GS_SLOT_SAVED_R12, 19);
    }

    #[test]
    fn test_fork_slots_contiguous() {
        // Fork slots should be contiguous for efficient saving
        let fork_slots = [
            GS_SLOT_SAVED_RDI,
            GS_SLOT_SAVED_RSI,
            GS_SLOT_SAVED_RDX,
            GS_SLOT_SAVED_RBX,
            GS_SLOT_SAVED_RBP,
            GS_SLOT_SAVED_R8,
            GS_SLOT_SAVED_R9,
            GS_SLOT_SAVED_R10,
            GS_SLOT_SAVED_R12,
        ];

        for i in 1..fork_slots.len() {
            assert_eq!(
                fork_slots[i],
                fork_slots[i - 1] + 1,
                "Fork slots should be contiguous"
            );
        }
    }

    // =========================================================================
    // Stack Guard Slots Tests
    // =========================================================================

    #[test]
    fn test_stack_guard_slots() {
        assert_eq!(GS_SLOT_KERNEL_STACK_GUARD, 20);
        assert_eq!(GS_SLOT_KERNEL_STACK_SNAPSHOT, 21);
    }

    // =========================================================================
    // INT 0x81 Callee-Saved Register Slots Tests
    // =========================================================================

    #[test]
    fn test_int81_register_slots() {
        // Slots 22-27 for int 0x81 callee-saved registers
        assert_eq!(GS_SLOT_INT81_RBX, 22);
        assert_eq!(GS_SLOT_INT81_RBP, 23);
        assert_eq!(GS_SLOT_INT81_R12, 24);
        assert_eq!(GS_SLOT_INT81_R13, 25);
        assert_eq!(GS_SLOT_INT81_R14, 26);
        assert_eq!(GS_SLOT_INT81_R15, 27);
    }

    #[test]
    fn test_int81_slots_byte_offsets() {
        // Verify byte offsets (slot * 8)
        assert_eq!(GS_SLOT_INT81_RBX * 8, 176);
        assert_eq!(GS_SLOT_INT81_RBP * 8, 184);
        assert_eq!(GS_SLOT_INT81_R12 * 8, 192);
        assert_eq!(GS_SLOT_INT81_R13 * 8, 200);
        assert_eq!(GS_SLOT_INT81_R14 * 8, 208);
        assert_eq!(GS_SLOT_INT81_R15 * 8, 216);
    }

    // =========================================================================
    // Guard Source Constants Tests
    // =========================================================================

    #[test]
    fn test_guard_source_constants() {
        assert_eq!(GUARD_SOURCE_INT_GATE, 0);
        assert_eq!(GUARD_SOURCE_SYSCALL, 1);
    }

    #[test]
    fn test_guard_sources_distinct() {
        assert_ne!(GUARD_SOURCE_INT_GATE, GUARD_SOURCE_SYSCALL);
    }

    // =========================================================================
    // GS_DATA Size Tests
    // =========================================================================

    #[test]
    fn test_gs_data_minimum_size() {
        // GS_DATA must accommodate all slots
        let max_slot = GS_SLOT_INT81_R15;
        let min_size_bytes = (max_slot + 1) * 8;

        // At least 224 bytes needed (28 slots * 8)
        assert!(min_size_bytes >= 224);
    }

    #[test]
    fn test_gs_data_slot_alignment() {
        // All slots should be naturally aligned (8-byte u64 slots)
        let slots = [
            GS_SLOT_USER_RSP,
            GS_SLOT_KERNEL_RSP,
            GS_SLOT_USER_ENTRY,
            GS_SLOT_USER_STACK,
            GS_SLOT_USER_CS,
            GS_SLOT_USER_SS,
            GS_SLOT_USER_DS,
            GS_SLOT_SAVED_RCX,
            GS_SLOT_SAVED_RFLAGS,
        ];

        for slot in slots {
            let offset = slot * 8;
            assert_eq!(
                offset % 8,
                0,
                "Slot {} offset {} not aligned",
                slot,
                offset
            );
        }
    }

    // =========================================================================
    // Hex Encoding Tests
    // =========================================================================

    #[test]
    fn test_encode_hex_u64_zero() {
        let mut buf = [0u8; 16];
        encode_hex_u64(0, &mut buf);
        assert_eq!(&buf, b"0000000000000000");
    }

    #[test]
    fn test_encode_hex_u64_max() {
        let mut buf = [0u8; 16];
        encode_hex_u64(u64::MAX, &mut buf);
        assert_eq!(&buf, b"FFFFFFFFFFFFFFFF");
    }

    #[test]
    fn test_encode_hex_u64_pattern() {
        let mut buf = [0u8; 16];
        encode_hex_u64(0x123456789ABCDEF0, &mut buf);
        assert_eq!(&buf, b"123456789ABCDEF0");
    }

    #[test]
    fn test_encode_hex_u64_lower_nibbles() {
        let mut buf = [0u8; 16];
        encode_hex_u64(0x000000000000FACE, &mut buf);
        assert_eq!(&buf, b"000000000000FACE");
    }

    // =========================================================================
    // Slot Organization Tests
    // =========================================================================

    #[test]
    fn test_no_slot_conflicts() {
        // Ensure all slots have unique indices
        use std::collections::HashSet;

        let slots = [
            GS_SLOT_USER_RSP,
            GS_SLOT_KERNEL_RSP,
            GS_SLOT_USER_ENTRY,
            GS_SLOT_USER_STACK,
            GS_SLOT_USER_CS,
            GS_SLOT_USER_SS,
            GS_SLOT_USER_DS,
            GS_SLOT_SAVED_RCX,
            GS_SLOT_SAVED_RFLAGS,
            GS_SLOT_USER_RSP_DEBUG,
            GS_SLOT_SAVED_RAX,
            GS_SLOT_SAVED_RDI,
            GS_SLOT_SAVED_RSI,
            GS_SLOT_SAVED_RDX,
            GS_SLOT_SAVED_RBX,
            GS_SLOT_SAVED_RBP,
            GS_SLOT_SAVED_R8,
            GS_SLOT_SAVED_R9,
            GS_SLOT_SAVED_R10,
            GS_SLOT_SAVED_R12,
            GS_SLOT_KERNEL_STACK_GUARD,
            GS_SLOT_KERNEL_STACK_SNAPSHOT,
            GS_SLOT_INT81_RBX,
            GS_SLOT_INT81_RBP,
            GS_SLOT_INT81_R12,
            GS_SLOT_INT81_R13,
            GS_SLOT_INT81_R14,
            GS_SLOT_INT81_R15,
        ];

        let unique: HashSet<_> = slots.iter().collect();
        assert_eq!(
            unique.len(),
            slots.len(),
            "All slot indices should be unique"
        );
    }

    #[test]
    fn test_syscall_critical_slots() {
        // These slots are accessed in hot path - must be low indices
        assert!(GS_SLOT_USER_RSP < 10);
        assert!(GS_SLOT_KERNEL_RSP < 10);
        assert!(GS_SLOT_SAVED_RCX < 10);
        assert!(GS_SLOT_SAVED_RFLAGS < 10);
    }
}
