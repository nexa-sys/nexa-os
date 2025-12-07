//! Big Integer Arithmetic
//!
//! Fixed-size big integer implementation for cryptographic operations.

#[allow(unused_imports)]
use std::vec::Vec;

use core::cmp::Ordering;

/// Maximum size in 64-bit limbs (supports up to 4096-bit numbers)
pub const MAX_LIMBS: usize = 64;

/// Big integer with fixed-size storage
#[derive(Clone, Debug)]
pub struct BigInt {
    /// Little-endian limbs
    limbs: [u64; MAX_LIMBS],
    /// Number of significant limbs
    len: usize,
    /// Negative flag
    negative: bool,
}

impl Default for BigInt {
    fn default() -> Self {
        Self::zero()
    }
}

impl PartialEq for BigInt {
    fn eq(&self, other: &Self) -> bool {
        if self.negative != other.negative {
            return false;
        }
        self.abs_cmp(other) == Ordering::Equal
    }
}

impl Eq for BigInt {}

impl PartialOrd for BigInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigInt {
    fn cmp(&self, other: &Self) -> Ordering {
        // Handle signs
        match (self.negative, other.negative) {
            (false, true) => return Ordering::Greater,
            (true, false) => return Ordering::Less,
            (true, true) => return self.abs_cmp(other).reverse(),
            (false, false) => {}
        }
        self.abs_cmp(other)
    }
}

impl BigInt {
    /// Compare absolute values
    pub fn abs_cmp(&self, other: &Self) -> Ordering {
        if self.len != other.len {
            return self.len.cmp(&other.len);
        }
        
        for i in (0..self.len).rev() {
            if self.limbs[i] != other.limbs[i] {
                return self.limbs[i].cmp(&other.limbs[i]);
            }
        }
        
        Ordering::Equal
    }

    /// Create a zero BigInt
    pub const fn zero() -> Self {
        Self {
            limbs: [0u64; MAX_LIMBS],
            len: 0,
            negative: false,
        }
    }

    /// Create a one BigInt
    pub const fn one() -> Self {
        let mut limbs = [0u64; MAX_LIMBS];
        limbs[0] = 1;
        Self {
            limbs,
            len: 1,
            negative: false,
        }
    }

    /// Create a BigInt from u64
    pub fn from_u64(val: u64) -> Self {
        let mut result = Self::zero();
        if val != 0 {
            result.limbs[0] = val;
            result.len = 1;
        }
        result
    }

    /// Check if negative
    pub fn is_negative(&self) -> bool {
        self.negative && !self.is_zero()
    }

    /// Check if odd
    pub fn is_odd(&self) -> bool {
        !self.is_even()
    }

    /// Get byte length
    pub fn byte_length(&self) -> usize {
        (self.bit_length() + 7) / 8
    }

    /// Create a BigInt from bytes (big-endian)
    pub fn from_bytes_be(bytes: &[u8]) -> Self {
        if bytes.len() > MAX_LIMBS * 8 {
            // Truncate if too large
            let start = bytes.len() - MAX_LIMBS * 8;
            return Self::from_bytes_be(&bytes[start..]);
        }

        let mut result = Self::zero();
        
        // Skip leading zeros
        let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
        let significant = &bytes[first_nonzero..];
        
        if significant.is_empty() {
            return result;
        }

        // Convert to little-endian limbs
        let num_limbs = (significant.len() + 7) / 8;
        result.len = num_limbs;

        for (i, chunk) in significant.rchunks(8).enumerate() {
            let mut limb_bytes = [0u8; 8];
            let offset = 8 - chunk.len();
            limb_bytes[offset..].copy_from_slice(chunk);
            result.limbs[i] = u64::from_be_bytes(limb_bytes);
        }

        result.normalize();
        result
    }

    /// Create modulo (alias for mod_reduce)
    pub fn modulo(&self, m: &Self) -> Self {
        self.mod_reduce(m)
    }

    /// Convert to bytes (big-endian)
    pub fn to_bytes_be(&self) -> Vec<u8> {
        if self.len == 0 {
            return vec![0];
        }

        let mut result = Vec::with_capacity(self.len * 8);
        let mut started = false;

        for i in (0..self.len).rev() {
            let bytes = self.limbs[i].to_be_bytes();
            for &byte in &bytes {
                if started || byte != 0 {
                    result.push(byte);
                    started = true;
                }
            }
        }

        if result.is_empty() {
            result.push(0);
        }

        result
    }

    /// Convert to bytes (big-endian) with fixed length
    pub fn to_bytes_be_padded(&self, len: usize) -> Vec<u8> {
        let bytes = self.to_bytes_be();
        if bytes.len() >= len {
            return bytes[bytes.len() - len..].to_vec();
        }
        
        let mut result = vec![0u8; len];
        result[len - bytes.len()..].copy_from_slice(&bytes);
        result
    }

    /// Normalize: remove leading zero limbs
    fn normalize(&mut self) {
        while self.len > 0 && self.limbs[self.len - 1] == 0 {
            self.len -= 1;
        }
    }

    /// Check if zero
    pub fn is_zero(&self) -> bool {
        self.len == 0
    }

    /// Check if one
    pub fn is_one(&self) -> bool {
        self.len == 1 && self.limbs[0] == 1
    }

    /// Check if even
    pub fn is_even(&self) -> bool {
        self.len == 0 || (self.limbs[0] & 1) == 0
    }

    /// Get bit length
    pub fn bit_length(&self) -> usize {
        if self.len == 0 {
            return 0;
        }
        (self.len - 1) * 64 + (64 - self.limbs[self.len - 1].leading_zeros() as usize)
    }

    /// Get specific bit (0 = LSB)
    pub fn get_bit(&self, n: usize) -> bool {
        let limb_idx = n / 64;
        if limb_idx >= self.len {
            return false;
        }
        let bit_idx = n % 64;
        (self.limbs[limb_idx] >> bit_idx) & 1 == 1
    }

    /// Add two BigInts
    pub fn add(&self, other: &Self) -> Self {
        let mut result = Self::zero();
        let max_len = core::cmp::max(self.len, other.len);
        
        let mut carry = 0u64;
        for i in 0..max_len {
            let a = if i < self.len { self.limbs[i] } else { 0 };
            let b = if i < other.len { other.limbs[i] } else { 0 };
            
            let (sum1, c1) = a.overflowing_add(b);
            let (sum2, c2) = sum1.overflowing_add(carry);
            
            result.limbs[i] = sum2;
            carry = (c1 as u64) + (c2 as u64);
        }
        
        if carry != 0 && max_len < MAX_LIMBS {
            result.limbs[max_len] = carry;
            result.len = max_len + 1;
        } else {
            result.len = max_len;
        }
        
        result.normalize();
        result
    }

    /// Subtract: self - other (assumes self >= other)
    pub fn sub(&self, other: &Self) -> Self {
        let mut result = Self::zero();
        result.len = self.len;
        
        let mut borrow = 0u64;
        for i in 0..self.len {
            let a = self.limbs[i];
            let b = if i < other.len { other.limbs[i] } else { 0 };
            
            let (diff1, b1) = a.overflowing_sub(b);
            let (diff2, b2) = diff1.overflowing_sub(borrow);
            
            result.limbs[i] = diff2;
            borrow = (b1 as u64) + (b2 as u64);
        }
        
        result.normalize();
        result
    }

    /// Multiply two BigInts
    pub fn mul(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Self::zero();
        }

        let mut result = Self::zero();
        
        for i in 0..self.len {
            let mut carry = 0u128;
            for j in 0..other.len {
                if i + j >= MAX_LIMBS {
                    break;
                }
                
                let product = (self.limbs[i] as u128) * (other.limbs[j] as u128)
                    + (result.limbs[i + j] as u128)
                    + carry;
                
                result.limbs[i + j] = product as u64;
                carry = product >> 64;
            }
            
            if i + other.len < MAX_LIMBS && carry != 0 {
                result.limbs[i + other.len] = carry as u64;
            }
        }
        
        result.len = core::cmp::min(self.len + other.len, MAX_LIMBS);
        result.normalize();
        result
    }

    /// Modular reduction: self mod m
    /// Uses shift-and-subtract algorithm for reasonable performance
    pub fn mod_reduce(&self, m: &Self) -> Self {
        if m.is_zero() {
            return Self::zero();
        }
        if self < m {
            return self.clone();
        }
        
        // Use division to get remainder
        let (_q, r) = self.div(m);
        r
    }

    /// Division: returns (quotient, remainder)
    pub fn div(&self, divisor: &Self) -> (Self, Self) {
        if divisor.is_zero() {
            // Division by zero - return zero
            return (Self::zero(), Self::zero());
        }
        
        if self < divisor {
            return (Self::zero(), self.clone());
        }
        
        if self == divisor {
            return (Self::from_u64(1), Self::zero());
        }

        // Binary long division
        let mut quotient = Self::zero();
        let mut remainder = Self::zero();
        
        let self_bits = self.bit_length();
        
        // Process bits from most significant to least
        for i in (0..self_bits).rev() {
            // Shift remainder left by 1
            remainder = remainder.shl(1);
            
            // Bring down next bit from dividend
            if self.get_bit(i) {
                remainder.limbs[0] |= 1;
                remainder.len = core::cmp::max(remainder.len, 1);
            }
            
            // If remainder >= divisor, subtract and set quotient bit
            if &remainder >= divisor {
                remainder = remainder.sub(divisor);
                // Set bit i in quotient
                let limb_idx = i / 64;
                let bit_idx = i % 64;
                if limb_idx < MAX_LIMBS {
                    quotient.limbs[limb_idx] |= 1u64 << bit_idx;
                    quotient.len = core::cmp::max(quotient.len, limb_idx + 1);
                }
            }
        }
        
        quotient.normalize();
        remainder.normalize();
        (quotient, remainder)
    }

    /// Left shift by n bits
    pub fn shl(&self, n: usize) -> Self {
        if n == 0 || self.is_zero() {
            return self.clone();
        }
        
        let limb_shift = n / 64;
        let bit_shift = n % 64;
        
        let mut result = Self::zero();
        
        if bit_shift == 0 {
            // Simple limb copy
            for i in 0..self.len {
                if i + limb_shift < MAX_LIMBS {
                    result.limbs[i + limb_shift] = self.limbs[i];
                }
            }
            result.len = core::cmp::min(self.len + limb_shift, MAX_LIMBS);
        } else {
            // Need to handle bit shifting
            let mut carry = 0u64;
            for i in 0..self.len {
                if i + limb_shift < MAX_LIMBS {
                    result.limbs[i + limb_shift] = (self.limbs[i] << bit_shift) | carry;
                    carry = self.limbs[i] >> (64 - bit_shift);
                }
            }
            if carry != 0 && self.len + limb_shift < MAX_LIMBS {
                result.limbs[self.len + limb_shift] = carry;
            }
            result.len = core::cmp::min(self.len + limb_shift + 1, MAX_LIMBS);
        }
        
        result.normalize();
        result
    }

    /// Modular exponentiation: base^exp mod m
    pub fn mod_exp(base: &Self, exp: &Self, m: &Self) -> Self {
        if m.is_zero() {
            return Self::zero();
        }
        if m.is_one() {
            return Self::zero();
        }
        if exp.is_zero() {
            return Self::from_u64(1);
        }

        let mut result = Self::from_u64(1);
        let mut base = base.mod_reduce(m);
        let exp_bits = exp.bit_length();

        for i in 0..exp_bits {
            if exp.get_bit(i) {
                result = result.mul(&base).mod_reduce(m);
            }
            base = base.mul(&base).mod_reduce(m);
        }

        result
    }

    /// Right shift by n bits
    pub fn shr(&self, n: usize) -> Self {
        if self.is_zero() || n == 0 {
            return self.clone();
        }

        let limb_shift = n / 64;
        let bit_shift = n % 64;

        if limb_shift >= self.len {
            return Self::zero();
        }

        let mut result = Self::zero();
        
        if bit_shift == 0 {
            for i in limb_shift..self.len {
                result.limbs[i - limb_shift] = self.limbs[i];
            }
        } else {
            for i in limb_shift..self.len {
                result.limbs[i - limb_shift] = self.limbs[i] >> bit_shift;
                if i + 1 < self.len {
                    result.limbs[i - limb_shift] |= self.limbs[i + 1] << (64 - bit_shift);
                }
            }
        }

        result.len = self.len - limb_shift;
        result.normalize();
        result
    }

    /// Modular inverse using extended Euclidean algorithm
    pub fn mod_inverse(&self, m: &Self) -> Option<Self> {
        if self.is_zero() || m.is_zero() || m.is_one() {
            return None;
        }

        // Extended GCD
        let mut old_r = m.clone();
        let mut r = self.mod_reduce(m);
        let mut old_s = Self::zero();
        let mut s = Self::from_u64(1);

        while !r.is_zero() {
            // Compute quotient using our fast binary division
            let (q, _) = old_r.div(&r);
            
            let temp_r = r.clone();
            r = old_r.sub(&q.mul(&r));
            old_r = temp_r;

            let temp_s = s.clone();
            let qs = q.mul(&s);
            if old_s >= qs {
                s = old_s.sub(&qs);
            } else {
                s = m.sub(&qs.sub(&old_s).mod_reduce(m));
            }
            old_s = temp_s;
        }

        // GCD must be 1
        if !old_r.is_one() {
            return None;
        }

        Some(old_s.mod_reduce(m))
    }

    /// Check if greater than or equal to p (for modular reduction)
    pub fn ge_p(&self, p: &Self) -> bool {
        self >= p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_bytes() {
        let bytes = [0x01, 0x02, 0x03, 0x04];
        let n = BigInt::from_bytes_be(&bytes);
        assert_eq!(n.to_bytes_be(), bytes.to_vec());
    }

    #[test]
    fn test_add() {
        let a = BigInt::from_u64(100);
        let b = BigInt::from_u64(200);
        let c = a.add(&b);
        assert_eq!(c.limbs[0], 300);
    }

    #[test]
    fn test_mul() {
        let a = BigInt::from_u64(100);
        let b = BigInt::from_u64(200);
        let c = a.mul(&b);
        assert_eq!(c.limbs[0], 20000);
    }

    #[test]
    fn test_mod_exp() {
        let base = BigInt::from_u64(2);
        let exp = BigInt::from_u64(10);
        let m = BigInt::from_u64(1000);
        let result = BigInt::mod_exp(&base, &exp, &m);
        assert_eq!(result.limbs[0], 24); // 2^10 = 1024 mod 1000 = 24
    }

    #[test]
    fn test_div() {
        let a = BigInt::from_u64(100);
        let b = BigInt::from_u64(30);
        let (q, r) = a.div(&b);
        assert_eq!(q.limbs[0], 3);
        assert_eq!(r.limbs[0], 10);
    }
}
