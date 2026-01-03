//! Pointer Safety Tests

#[cfg(test)]
mod tests {
    use crate::safety::ptr::{UserSlice, UserSliceMut};

    // =========================================================================
    // UserSlice Tests
    // =========================================================================

    #[test]
    fn test_userslice_null_rejected() {
        let result = UserSlice::<u8>::new(core::ptr::null(), 10);
        assert!(result.is_none());
    }

    #[test]
    fn test_userslice_valid_pointer() {
        let data = [1u8, 2, 3, 4, 5];
        let result = UserSlice::new(data.as_ptr(), data.len());
        assert!(result.is_some());
    }

    #[test]
    fn test_userslice_len() {
        let data = [1u8, 2, 3, 4, 5];
        let slice = UserSlice::new(data.as_ptr(), data.len()).unwrap();
        assert_eq!(slice.len(), 5);
    }

    #[test]
    fn test_userslice_is_empty() {
        let data: [u8; 0] = [];
        let slice = UserSlice::new(data.as_ptr(), 0).unwrap();
        assert!(slice.is_empty());
    }

    // =========================================================================
    // UserSliceMut Tests
    // =========================================================================

    #[test]
    fn test_userslicemut_null_rejected() {
        let result = UserSliceMut::<u8>::new(core::ptr::null_mut(), 10);
        assert!(result.is_none());
    }

    #[test]
    fn test_userslicemut_valid_pointer() {
        let mut data = [1u8, 2, 3, 4, 5];
        let result = UserSliceMut::new(data.as_mut_ptr(), data.len());
        assert!(result.is_some());
    }

    // =========================================================================
    // Address Space Boundary Tests
    // =========================================================================

    #[test]
    fn test_kernel_user_boundary() {
        // Typical kernel/user space boundary on x86_64
        const USER_SPACE_END: usize = 0x0000_7FFF_FFFF_FFFF;
        const KERNEL_SPACE_START: usize = 0xFFFF_8000_0000_0000;
        
        fn is_user_address(addr: usize) -> bool {
            addr <= USER_SPACE_END
        }
        
        fn is_kernel_address(addr: usize) -> bool {
            addr >= KERNEL_SPACE_START
        }
        
        assert!(is_user_address(0x1000));
        assert!(is_user_address(USER_SPACE_END));
        assert!(!is_user_address(KERNEL_SPACE_START));
        
        assert!(is_kernel_address(KERNEL_SPACE_START));
        assert!(!is_kernel_address(0x1000));
    }

    #[test]
    fn test_canonical_address_check() {
        // x86_64 canonical address check (48-bit virtual addresses, sign-extended)
        fn is_canonical(addr: u64) -> bool {
            // Bits 63:48 must all be copies of bit 47
            let sign_bit = (addr >> 47) & 1;
            let top_bits = addr >> 48;
            if sign_bit == 0 {
                top_bits == 0
            } else {
                top_bits == 0xFFFF
            }
        }
        
        // Valid canonical addresses (lower half: 0x0000_0000_0000_0000 - 0x0000_7FFF_FFFF_FFFF)
        assert!(is_canonical(0x0000_0000_0000_1000));
        assert!(is_canonical(0x0000_7FFF_FFFF_FFFF));
        
        // Valid canonical addresses (upper half: 0xFFFF_8000_0000_0000 - 0xFFFF_FFFF_FFFF_FFFF)
        assert!(is_canonical(0xFFFF_8000_0000_0000));
        assert!(is_canonical(0xFFFF_FFFF_FFFF_FFFF));
        
        // Non-canonical addresses (hole in the middle)
        assert!(!is_canonical(0x0000_8000_0000_0000)); // Just above lower canonical
        assert!(!is_canonical(0x0001_0000_0000_0000));
        assert!(!is_canonical(0x8000_0000_0000_0000));
        assert!(!is_canonical(0xFFFF_7FFF_FFFF_FFFF)); // Just below upper canonical
    }

    // =========================================================================
    // Alignment Tests
    // =========================================================================

    #[test]
    fn test_pointer_alignment() {
        fn is_aligned(ptr: usize, align: usize) -> bool {
            (ptr & (align - 1)) == 0
        }
        
        assert!(is_aligned(0x1000, 4096)); // Page aligned
        assert!(is_aligned(0x1000, 8));    // 8-byte aligned
        assert!(!is_aligned(0x1001, 4096));
        assert!(!is_aligned(0x1001, 2));
    }

    #[test]
    fn test_align_up() {
        fn align_up(addr: usize, align: usize) -> usize {
            (addr + align - 1) & !(align - 1)
        }
        
        assert_eq!(align_up(0x1001, 4096), 0x2000);
        assert_eq!(align_up(0x1000, 4096), 0x1000);
        assert_eq!(align_up(5, 8), 8);
        assert_eq!(align_up(8, 8), 8);
    }

    #[test]
    fn test_align_down() {
        fn align_down(addr: usize, align: usize) -> usize {
            addr & !(align - 1)
        }
        
        assert_eq!(align_down(0x1FFF, 4096), 0x1000);
        assert_eq!(align_down(0x1000, 4096), 0x1000);
        assert_eq!(align_down(15, 8), 8);
        assert_eq!(align_down(8, 8), 8);
    }

    // =========================================================================
    // Overflow Protection Tests
    // =========================================================================

    #[test]
    fn test_size_overflow_check() {
        fn safe_size_check(ptr: usize, len: usize, elem_size: usize) -> bool {
            let total_size = len.checked_mul(elem_size);
            if total_size.is_none() {
                return false;
            }
            let end = ptr.checked_add(total_size.unwrap());
            end.is_some()
        }
        
        // Valid case
        assert!(safe_size_check(0x1000, 100, 8));
        
        // Overflow in multiplication
        assert!(!safe_size_check(0x1000, usize::MAX, 2));
        
        // Overflow in addition
        assert!(!safe_size_check(usize::MAX - 10, 100, 1));
    }
}
