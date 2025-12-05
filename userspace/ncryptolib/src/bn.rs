//! Big Number (BN) OpenSSL Compatibility API
//!
//! Provides OpenSSL-compatible BN_* functions for arbitrary precision arithmetic.

use std::vec::Vec;
use crate::bigint::BigInt;

/// BIGNUM - Big Number structure
pub struct BIGNUM {
    /// Internal value
    value: BigInt,
    /// Negative flag
    negative: bool,
}

impl BIGNUM {
    /// Create new BIGNUM initialized to 0
    pub fn new() -> Self {
        Self {
            value: BigInt::zero(),
            negative: false,
        }
    }

    /// Create from bytes (big-endian unsigned)
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            value: BigInt::from_bytes_be(bytes),
            negative: false,
        }
    }

    /// Convert to bytes (big-endian unsigned)
    pub fn to_bytes(&self) -> Vec<u8> {
        self.value.to_bytes_be()
    }

    /// Get number of bits
    pub fn num_bits(&self) -> usize {
        self.value.bit_length()
    }

    /// Get number of bytes
    pub fn num_bytes(&self) -> usize {
        (self.num_bits() + 7) / 8
    }

    /// Check if zero
    pub fn is_zero(&self) -> bool {
        self.value.is_zero()
    }

    /// Check if one
    pub fn is_one(&self) -> bool {
        self.value.is_one()
    }

    /// Check if negative
    pub fn is_negative(&self) -> bool {
        self.negative && !self.is_zero()
    }

    /// Set to zero
    pub fn zero(&mut self) {
        self.value = BigInt::zero();
        self.negative = false;
    }

    /// Set to one
    pub fn one(&mut self) {
        self.value = BigInt::one();
        self.negative = false;
    }

    /// Set from u64
    pub fn set_word(&mut self, w: u64) {
        self.value = BigInt::from_u64(w);
        self.negative = false;
    }

    /// Get as u64 (truncates)
    pub fn get_word(&self) -> u64 {
        // Get the lowest 64 bits
        let bytes = self.value.to_bytes_be();
        if bytes.is_empty() {
            return 0;
        }
        // Take up to the last 8 bytes
        let start = if bytes.len() > 8 { bytes.len() - 8 } else { 0 };
        let mut result = 0u64;
        for &b in &bytes[start..] {
            result = (result << 8) | (b as u64);
        }
        result
    }

    /// Copy from another BIGNUM
    pub fn copy_from(&mut self, other: &BIGNUM) {
        self.value = other.value.clone();
        self.negative = other.negative;
    }

    /// Add two BIGNUMs
    pub fn add(&mut self, a: &BIGNUM, b: &BIGNUM) {
        // Simplified: assume both positive
        self.value = a.value.add(&b.value);
        self.negative = false;
    }

    /// Subtract two BIGNUMs
    pub fn sub(&mut self, a: &BIGNUM, b: &BIGNUM) {
        use core::cmp::Ordering;
        self.value = a.value.sub(&b.value);
        self.negative = matches!(a.value.abs_cmp(&b.value), Ordering::Less);
    }

    /// Multiply two BIGNUMs
    pub fn mul(&mut self, a: &BIGNUM, b: &BIGNUM) {
        self.value = a.value.mul(&b.value);
        self.negative = a.negative != b.negative;
    }

    /// Modular exponentiation: r = a^p mod m
    pub fn mod_exp(&mut self, a: &BIGNUM, p: &BIGNUM, m: &BIGNUM) {
        self.value = BigInt::mod_exp(&a.value, &p.value, &m.value);
        self.negative = false;
    }

    /// Compare two BIGNUMs
    pub fn cmp(&self, other: &BIGNUM) -> i32 {
        use core::cmp::Ordering;
        if self.negative && !other.negative {
            return -1;
        }
        if !self.negative && other.negative {
            return 1;
        }
        let r = match self.value.abs_cmp(&other.value) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        };
        if self.negative { -r } else { r }
    }
}

impl Default for BIGNUM {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for BIGNUM {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            negative: self.negative,
        }
    }
}

/// BN_CTX - Context for temporary BIGNUMs
pub struct BN_CTX {
    stack: Vec<BIGNUM>,
}

impl BN_CTX {
    pub fn new() -> Self {
        Self {
            stack: Vec::with_capacity(32),
        }
    }
}

impl Default for BN_CTX {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// BN_new - Create new BIGNUM
#[no_mangle]
pub extern "C" fn BN_new() -> *mut BIGNUM {
    Box::into_raw(Box::new(BIGNUM::new()))
}

/// BN_free - Free BIGNUM
#[no_mangle]
pub extern "C" fn BN_free(bn: *mut BIGNUM) {
    if !bn.is_null() {
        unsafe { drop(Box::from_raw(bn)); }
    }
}

/// BN_clear_free - Clear and free BIGNUM
#[no_mangle]
pub extern "C" fn BN_clear_free(bn: *mut BIGNUM) {
    if !bn.is_null() {
        unsafe {
            (*bn).zero();
            drop(Box::from_raw(bn));
        }
    }
}

/// BN_dup - Duplicate BIGNUM
#[no_mangle]
pub extern "C" fn BN_dup(bn: *const BIGNUM) -> *mut BIGNUM {
    if bn.is_null() {
        return core::ptr::null_mut();
    }
    let bn = unsafe { &*bn };
    Box::into_raw(Box::new(bn.clone()))
}

/// BN_copy - Copy BIGNUM
#[no_mangle]
pub extern "C" fn BN_copy(dst: *mut BIGNUM, src: *const BIGNUM) -> *mut BIGNUM {
    if dst.is_null() || src.is_null() {
        return core::ptr::null_mut();
    }
    let dst = unsafe { &mut *dst };
    let src = unsafe { &*src };
    dst.copy_from(src);
    dst as *mut BIGNUM
}

/// BN_zero - Set BIGNUM to zero
#[no_mangle]
pub extern "C" fn BN_zero(bn: *mut BIGNUM) {
    if !bn.is_null() {
        unsafe { (*bn).zero(); }
    }
}

/// BN_one - Set BIGNUM to one
#[no_mangle]
pub extern "C" fn BN_one(bn: *mut BIGNUM) -> i32 {
    if bn.is_null() {
        return 0;
    }
    unsafe { (*bn).one(); }
    1
}

/// BN_set_word - Set BIGNUM from word
#[no_mangle]
pub extern "C" fn BN_set_word(bn: *mut BIGNUM, w: u64) -> i32 {
    if bn.is_null() {
        return 0;
    }
    unsafe { (*bn).set_word(w); }
    1
}

/// BN_get_word - Get BIGNUM as word
#[no_mangle]
pub extern "C" fn BN_get_word(bn: *const BIGNUM) -> u64 {
    if bn.is_null() {
        return u64::MAX;
    }
    unsafe { (*bn).get_word() }
}

/// BN_num_bits - Get number of bits
#[no_mangle]
pub extern "C" fn BN_num_bits(bn: *const BIGNUM) -> i32 {
    if bn.is_null() {
        return 0;
    }
    unsafe { (*bn).num_bits() as i32 }
}

/// BN_num_bytes - Get number of bytes
#[no_mangle]
pub extern "C" fn BN_num_bytes(bn: *const BIGNUM) -> i32 {
    if bn.is_null() {
        return 0;
    }
    unsafe { (*bn).num_bytes() as i32 }
}

/// BN_is_zero - Check if zero
#[no_mangle]
pub extern "C" fn BN_is_zero(bn: *const BIGNUM) -> i32 {
    if bn.is_null() {
        return 0;
    }
    if unsafe { (*bn).is_zero() } { 1 } else { 0 }
}

/// BN_is_one - Check if one
#[no_mangle]
pub extern "C" fn BN_is_one(bn: *const BIGNUM) -> i32 {
    if bn.is_null() {
        return 0;
    }
    if unsafe { (*bn).is_one() } { 1 } else { 0 }
}

/// BN_is_negative - Check if negative
#[no_mangle]
pub extern "C" fn BN_is_negative(bn: *const BIGNUM) -> i32 {
    if bn.is_null() {
        return 0;
    }
    if unsafe { (*bn).is_negative() } { 1 } else { 0 }
}

/// BN_cmp - Compare two BIGNUMs
#[no_mangle]
pub extern "C" fn BN_cmp(a: *const BIGNUM, b: *const BIGNUM) -> i32 {
    if a.is_null() || b.is_null() {
        return -2;
    }
    unsafe { (*a).cmp(&*b) }
}

/// BN_ucmp - Unsigned compare
#[no_mangle]
pub extern "C" fn BN_ucmp(a: *const BIGNUM, b: *const BIGNUM) -> i32 {
    if a.is_null() || b.is_null() {
        return -2;
    }
    use core::cmp::Ordering;
    match unsafe { (*a).value.abs_cmp(&(*b).value) } {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// BN_bin2bn - Convert binary to BIGNUM
#[no_mangle]
pub extern "C" fn BN_bin2bn(s: *const u8, len: i32, ret: *mut BIGNUM) -> *mut BIGNUM {
    if s.is_null() || len < 0 {
        return core::ptr::null_mut();
    }

    let data = unsafe { core::slice::from_raw_parts(s, len as usize) };
    let bn = BIGNUM::from_bytes(data);

    if ret.is_null() {
        Box::into_raw(Box::new(bn))
    } else {
        unsafe { *ret = bn; }
        ret
    }
}

/// BN_bn2bin - Convert BIGNUM to binary
#[no_mangle]
pub extern "C" fn BN_bn2bin(bn: *const BIGNUM, to: *mut u8) -> i32 {
    if bn.is_null() || to.is_null() {
        return 0;
    }

    let bn = unsafe { &*bn };
    let bytes = bn.to_bytes();

    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), to, bytes.len());
    }

    bytes.len() as i32
}

/// BN_bn2binpad - Convert BIGNUM to binary with padding
#[no_mangle]
pub extern "C" fn BN_bn2binpad(bn: *const BIGNUM, to: *mut u8, tolen: i32) -> i32 {
    if bn.is_null() || to.is_null() || tolen < 0 {
        return -1;
    }

    let bn = unsafe { &*bn };
    let bytes = bn.to_bytes();
    let tolen = tolen as usize;

    if bytes.len() > tolen {
        return -1;
    }

    unsafe {
        // Zero-pad from start
        let pad = tolen - bytes.len();
        core::ptr::write_bytes(to, 0, pad);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), to.add(pad), bytes.len());
    }

    tolen as i32
}

/// BN_add - Add two BIGNUMs
#[no_mangle]
pub extern "C" fn BN_add(r: *mut BIGNUM, a: *const BIGNUM, b: *const BIGNUM) -> i32 {
    if r.is_null() || a.is_null() || b.is_null() {
        return 0;
    }
    unsafe {
        (*r).add(&*a, &*b);
    }
    1
}

/// BN_sub - Subtract two BIGNUMs
#[no_mangle]
pub extern "C" fn BN_sub(r: *mut BIGNUM, a: *const BIGNUM, b: *const BIGNUM) -> i32 {
    if r.is_null() || a.is_null() || b.is_null() {
        return 0;
    }
    unsafe {
        (*r).sub(&*a, &*b);
    }
    1
}

/// BN_mul - Multiply two BIGNUMs
#[no_mangle]
pub extern "C" fn BN_mul(r: *mut BIGNUM, a: *const BIGNUM, b: *const BIGNUM, _ctx: *mut BN_CTX) -> i32 {
    if r.is_null() || a.is_null() || b.is_null() {
        return 0;
    }
    unsafe {
        (*r).mul(&*a, &*b);
    }
    1
}

/// BN_mod_exp - Modular exponentiation
#[no_mangle]
pub extern "C" fn BN_mod_exp(r: *mut BIGNUM, a: *const BIGNUM, p: *const BIGNUM, m: *const BIGNUM, _ctx: *mut BN_CTX) -> i32 {
    if r.is_null() || a.is_null() || p.is_null() || m.is_null() {
        return 0;
    }
    unsafe {
        (*r).mod_exp(&*a, &*p, &*m);
    }
    1
}

/// BN_CTX_new - Create new context
#[no_mangle]
pub extern "C" fn BN_CTX_new() -> *mut BN_CTX {
    Box::into_raw(Box::new(BN_CTX::new()))
}

/// BN_CTX_free - Free context
#[no_mangle]
pub extern "C" fn BN_CTX_free(ctx: *mut BN_CTX) {
    if !ctx.is_null() {
        unsafe { drop(Box::from_raw(ctx)); }
    }
}

/// BN_CTX_start - Start scope
#[no_mangle]
pub extern "C" fn BN_CTX_start(_ctx: *mut BN_CTX) {
    // No-op in simplified implementation
}

/// BN_CTX_end - End scope
#[no_mangle]
pub extern "C" fn BN_CTX_end(_ctx: *mut BN_CTX) {
    // No-op in simplified implementation
}

/// BN_CTX_get - Get temporary BIGNUM
#[no_mangle]
pub extern "C" fn BN_CTX_get(ctx: *mut BN_CTX) -> *mut BIGNUM {
    if ctx.is_null() {
        return core::ptr::null_mut();
    }
    // Return a new BIGNUM (simplified)
    BN_new()
}

/// BN_hex2bn - Convert hex string to BIGNUM
#[no_mangle]
pub extern "C" fn BN_hex2bn(bn: *mut *mut BIGNUM, str: *const i8) -> i32 {
    if str.is_null() {
        return 0;
    }

    let hex_str = unsafe {
        match core::ffi::CStr::from_ptr(str).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        }
    };

    // Parse hex string
    let bytes = match crate::encoding::hex_decode(hex_str) {
        Ok(b) => b,
        Err(_) => return 0,
    };

    let new_bn = BIGNUM::from_bytes(&bytes);

    if bn.is_null() {
        return hex_str.len() as i32;
    }

    unsafe {
        if (*bn).is_null() {
            *bn = Box::into_raw(Box::new(new_bn));
        } else {
            **bn = new_bn;
        }
    }

    hex_str.len() as i32
}

/// BN_bn2hex - Convert BIGNUM to hex string
#[no_mangle]
pub extern "C" fn BN_bn2hex(bn: *const BIGNUM) -> *mut i8 {
    if bn.is_null() {
        return core::ptr::null_mut();
    }

    let bn = unsafe { &*bn };
    let bytes = bn.to_bytes();
    let hex = crate::encoding::hex_encode(&bytes);

    // Allocate and copy
    let mut result = hex.into_bytes();
    result.push(0); // Null terminator
    let ptr = result.as_mut_ptr() as *mut i8;
    std::mem::forget(result);
    ptr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bn_basic() {
        let mut bn = BIGNUM::new();
        assert!(bn.is_zero());

        bn.one();
        assert!(bn.is_one());
        assert!(!bn.is_zero());

        bn.set_word(12345);
        assert_eq!(bn.get_word(), 12345);
    }

    #[test]
    fn test_bn_bytes() {
        let bytes = [0x12, 0x34, 0x56, 0x78];
        let bn = BIGNUM::from_bytes(&bytes);
        let result = bn.to_bytes();
        assert_eq!(result, bytes);
    }
}
