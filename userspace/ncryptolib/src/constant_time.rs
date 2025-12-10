//! Constant-Time Comparison and Utility Functions
//!
//! Provides timing-safe comparison functions to prevent timing attacks.

// ============================================================================
// Constant-Time Operations
// ============================================================================

/// Constant-time comparison of two byte slices
///
/// Returns true if slices are equal, false otherwise.
/// Timing is independent of the actual content of the slices.
#[inline(never)]
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }

    diff == 0
}

/// Constant-time comparison that returns a u8 (0 for unequal, 1 for equal)
#[inline(never)]
pub fn ct_eq_u8(a: &[u8], b: &[u8]) -> u8 {
    if a.len() != b.len() {
        return 0;
    }

    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }

    // Convert to 0 or 1
    (((diff as u16).wrapping_sub(1)) >> 8) as u8
}

/// Constant-time select: returns a if select == 1, b if select == 0
#[inline(never)]
pub fn ct_select(a: u8, b: u8, select: u8) -> u8 {
    // select must be 0 or 1
    let mask = 0u8.wrapping_sub(select); // 0x00 if select=0, 0xFF if select=1
    (a & mask) | (b & !mask)
}

/// Constant-time conditional swap
#[inline(never)]
pub fn ct_swap(a: &mut [u8], b: &mut [u8], swap: u8) {
    let mask = 0u8.wrapping_sub(swap);
    for i in 0..a.len().min(b.len()) {
        let t = (a[i] ^ b[i]) & mask;
        a[i] ^= t;
        b[i] ^= t;
    }
}

/// Zero out a byte slice (secure zeroization)
#[inline(never)]
pub fn secure_zero(data: &mut [u8]) {
    // Use volatile writes to prevent compiler optimization
    for byte in data.iter_mut() {
        unsafe {
            core::ptr::write_volatile(byte, 0);
        }
    }
    // Memory barrier to ensure writes complete
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Zero out a Vec (secure zeroization)
pub fn secure_zero_vec(data: &mut Vec<u8>) {
    secure_zero(data.as_mut_slice());
}

/// Constant-time less-than comparison for u64
#[inline(never)]
pub fn ct_lt_u64(a: u64, b: u64) -> u64 {
    // Returns 1 if a < b, 0 otherwise
    let x = a ^ ((a ^ b) | ((a.wrapping_sub(b)) ^ b));
    x >> 63
}

/// Constant-time greater-than comparison for u64
#[inline(never)]
pub fn ct_gt_u64(a: u64, b: u64) -> u64 {
    ct_lt_u64(b, a)
}

/// Constant-time equality for u64
#[inline(never)]
pub fn ct_eq_u64(a: u64, b: u64) -> u64 {
    let x = a ^ b;
    // Returns 1 if equal, 0 otherwise
    ((x | x.wrapping_neg()) >> 63) ^ 1
}

// ============================================================================
// C ABI Exports
// ============================================================================

use crate::{c_int, size_t};

/// Constant-time comparison (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_ct_eq(a: *const u8, b: *const u8, len: size_t) -> c_int {
    if a.is_null() || b.is_null() {
        return -1;
    }

    let a_slice = core::slice::from_raw_parts(a, len);
    let b_slice = core::slice::from_raw_parts(b, len);

    if ct_eq(a_slice, b_slice) {
        1
    } else {
        0
    }
}

/// Secure zero (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_secure_zero(data: *mut u8, len: size_t) -> c_int {
    if data.is_null() {
        return -1;
    }

    let slice = core::slice::from_raw_parts_mut(data, len);
    secure_zero(slice);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ct_eq() {
        assert!(ct_eq(b"hello", b"hello"));
        assert!(!ct_eq(b"hello", b"world"));
        assert!(!ct_eq(b"hello", b"hell"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn test_ct_eq_u8() {
        assert_eq!(ct_eq_u8(b"test", b"test"), 1);
        assert_eq!(ct_eq_u8(b"test", b"TEST"), 0);
    }

    #[test]
    fn test_ct_select() {
        assert_eq!(ct_select(0xAA, 0xBB, 1), 0xAA);
        assert_eq!(ct_select(0xAA, 0xBB, 0), 0xBB);
    }

    #[test]
    fn test_ct_swap() {
        let mut a = [1, 2, 3];
        let mut b = [4, 5, 6];

        ct_swap(&mut a, &mut b, 0);
        assert_eq!(a, [1, 2, 3]);
        assert_eq!(b, [4, 5, 6]);

        ct_swap(&mut a, &mut b, 1);
        assert_eq!(a, [4, 5, 6]);
        assert_eq!(b, [1, 2, 3]);
    }

    #[test]
    fn test_secure_zero() {
        let mut data = vec![1, 2, 3, 4, 5];
        secure_zero(&mut data);
        assert_eq!(data, vec![0, 0, 0, 0, 0]);
    }

    #[test]
    fn test_ct_lt_u64() {
        assert_eq!(ct_lt_u64(1, 2), 1);
        assert_eq!(ct_lt_u64(2, 1), 0);
        assert_eq!(ct_lt_u64(1, 1), 0);
    }

    #[test]
    fn test_ct_eq_u64() {
        assert_eq!(ct_eq_u64(42, 42), 1);
        assert_eq!(ct_eq_u64(42, 43), 0);
    }
}
