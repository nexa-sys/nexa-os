//! Futex (Fast Userspace Mutex) tests
//!
//! Tests for kernel futex operations used by pthread_join and condition variables.
//! Uses REAL kernel constants from syscalls/thread.rs

#[cfg(test)]
mod tests {
    // Import REAL kernel futex constants
    use crate::syscalls::{
        FUTEX_WAIT, FUTEX_WAKE, FUTEX_FD, FUTEX_REQUEUE, FUTEX_CMP_REQUEUE,
        FUTEX_WAKE_OP, FUTEX_LOCK_PI, FUTEX_UNLOCK_PI, FUTEX_TRYLOCK_PI,
        FUTEX_WAIT_BITSET, FUTEX_WAKE_BITSET,
        FUTEX_PRIVATE_FLAG, FUTEX_CLOCK_REALTIME, FUTEX_CMD_MASK,
    };
    
    // Import clone flags from kernel
    use crate::syscalls::{
        CLONE_VM, CLONE_THREAD, CLONE_CHILD_CLEARTID, CLONE_CHILD_SETTID,
        CLONE_SETTLS, CLONE_PARENT_SETTID,
    };

    // =========================================================================
    // Futex Constants Tests (using kernel constants)
    // =========================================================================

    #[test]
    fn test_futex_operations_defined() {
        // Verify futex operation constants from kernel
        assert_eq!(FUTEX_WAIT, 0);
        assert_eq!(FUTEX_WAKE, 1);
        assert_eq!(FUTEX_FD, 2);
        assert_eq!(FUTEX_REQUEUE, 3);
        assert_eq!(FUTEX_CMP_REQUEUE, 4);
    }

    #[test]
    fn test_futex_extended_operations() {
        // Extended futex operations
        assert_eq!(FUTEX_WAKE_OP, 5);
        assert_eq!(FUTEX_LOCK_PI, 6);
        assert_eq!(FUTEX_UNLOCK_PI, 7);
        assert_eq!(FUTEX_TRYLOCK_PI, 8);
        assert_eq!(FUTEX_WAIT_BITSET, 9);
        assert_eq!(FUTEX_WAKE_BITSET, 10);
    }

    #[test]
    fn test_futex_flags() {
        // Test FUTEX_PRIVATE_FLAG (128)
        assert_eq!(FUTEX_PRIVATE_FLAG, 128);
        
        // Test command extraction with private flag
        let private_wait = FUTEX_WAIT | FUTEX_PRIVATE_FLAG;
        assert_eq!(private_wait & FUTEX_CMD_MASK, FUTEX_WAIT);
        
        let private_wake = FUTEX_WAKE | FUTEX_PRIVATE_FLAG;
        assert_eq!(private_wake & FUTEX_CMD_MASK, FUTEX_WAKE);
    }

    #[test]
    fn test_futex_cmd_mask() {
        // FUTEX_CMD_MASK should clear PRIVATE and CLOCK_REALTIME flags
        assert_eq!(FUTEX_CMD_MASK, !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME));
        
        // All commands should be extractable
        for cmd in [FUTEX_WAIT, FUTEX_WAKE, FUTEX_REQUEUE, FUTEX_CMP_REQUEUE] {
            let with_private = cmd | FUTEX_PRIVATE_FLAG;
            assert_eq!(with_private & FUTEX_CMD_MASK, cmd);
        }
    }

    #[test]
    fn test_futex_clock_realtime_flag() {
        assert_eq!(FUTEX_CLOCK_REALTIME, 256);
        
        // Combined flags
        let op_with_both = FUTEX_WAIT | FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME;
        assert_eq!(op_with_both & FUTEX_CMD_MASK, FUTEX_WAIT);
    }

    // =========================================================================
    // Clone Flags Tests (used by pthread_create)
    // =========================================================================

    #[test]
    fn test_clone_flags_values() {
        // Verify clone flags match Linux ABI
        assert_eq!(CLONE_VM, 0x00000100);
        assert_eq!(CLONE_THREAD, 0x00010000);
        assert_eq!(CLONE_CHILD_CLEARTID, 0x00200000);
        assert_eq!(CLONE_CHILD_SETTID, 0x01000000);
        assert_eq!(CLONE_SETTLS, 0x00080000);
        assert_eq!(CLONE_PARENT_SETTID, 0x00100000);
    }

    #[test]
    fn test_clone_thread_combination() {
        // pthread_create typically uses this flag combination
        let pthread_flags = CLONE_VM | CLONE_THREAD | CLONE_SETTLS 
            | CLONE_PARENT_SETTID | CLONE_CHILD_SETTID | CLONE_CHILD_CLEARTID;
        
        // All flags should be set
        assert_ne!(pthread_flags & CLONE_VM, 0);
        assert_ne!(pthread_flags & CLONE_THREAD, 0);
        assert_ne!(pthread_flags & CLONE_SETTLS, 0);
        assert_ne!(pthread_flags & CLONE_CHILD_CLEARTID, 0);
    }

    #[test]
    fn test_clone_flags_no_overlap() {
        // Verify flags don't overlap
        let all_flags = [
            CLONE_VM, CLONE_THREAD, CLONE_CHILD_CLEARTID,
            CLONE_CHILD_SETTID, CLONE_SETTLS, CLONE_PARENT_SETTID,
        ];
        
        for (i, &flag1) in all_flags.iter().enumerate() {
            for (j, &flag2) in all_flags.iter().enumerate() {
                if i != j {
                    assert_eq!(flag1 & flag2, 0, "Flags {:x} and {:x} overlap", flag1, flag2);
                }
            }
        }
    }

    // =========================================================================
    // Futex Operation Decoding Tests
    // =========================================================================

    #[test]
    fn test_futex_op_decoding() {
        // Test that we can properly decode operations with flags
        fn decode_futex_op(op: i32) -> (i32, bool, bool) {
            let cmd = op & FUTEX_CMD_MASK;
            let is_private = (op & FUTEX_PRIVATE_FLAG) != 0;
            let use_realtime = (op & FUTEX_CLOCK_REALTIME) != 0;
            (cmd, is_private, use_realtime)
        }
        
        let (cmd, is_private, _) = decode_futex_op(FUTEX_WAIT | FUTEX_PRIVATE_FLAG);
        assert_eq!(cmd, FUTEX_WAIT);
        assert!(is_private);
        
        let (cmd, is_private, use_rt) = decode_futex_op(
            FUTEX_WAKE | FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME
        );
        assert_eq!(cmd, FUTEX_WAKE);
        assert!(is_private);
        assert!(use_rt);
    }

    #[test]
    fn test_futex_all_operations_distinct() {
        // All base operations should have unique values
        let ops = [
            FUTEX_WAIT, FUTEX_WAKE, FUTEX_FD, FUTEX_REQUEUE, FUTEX_CMP_REQUEUE,
            FUTEX_WAKE_OP, FUTEX_LOCK_PI, FUTEX_UNLOCK_PI, FUTEX_TRYLOCK_PI,
            FUTEX_WAIT_BITSET, FUTEX_WAKE_BITSET,
        ];
        
        for (i, &op1) in ops.iter().enumerate() {
            for (j, &op2) in ops.iter().enumerate() {
                if i != j {
                    assert_ne!(op1, op2, "Operations {} and {} should be distinct", i, j);
                }
            }
        }
    }

    // =========================================================================
    // Futex Address Alignment Tests
    // =========================================================================

    #[test]
    fn test_futex_address_alignment() {
        // Futex addresses must be 4-byte aligned (u32 atomic)
        fn is_futex_aligned(addr: u64) -> bool {
            addr % 4 == 0
        }
        
        assert!(is_futex_aligned(0x1000));
        assert!(is_futex_aligned(0x1004));
        assert!(!is_futex_aligned(0x1001));
        assert!(!is_futex_aligned(0x1002));
        assert!(!is_futex_aligned(0x1003));
    }

    #[test]
    fn test_futex_value_range() {
        // Futex values are i32
        let max_val: i32 = i32::MAX;
        let min_val: i32 = i32::MIN;
        
        // Both should be valid for FUTEX_WAIT comparison
        assert_ne!(max_val, min_val);
        assert_eq!(max_val.wrapping_add(1), min_val);
    }
}
