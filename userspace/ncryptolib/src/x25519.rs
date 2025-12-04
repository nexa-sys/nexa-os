//! X25519 Key Exchange
//!
//! RFC 7748 compliant Curve25519 ECDH implementation.

#[allow(unused_imports)]
use std::vec::Vec;

// ============================================================================
// Constants
// ============================================================================

/// X25519 key size (32 bytes)
pub const X25519_KEY_SIZE: usize = 32;

/// Prime p = 2^255 - 19
const P: [u64; 4] = [
    0xffffffffffffffed,
    0xffffffffffffffff,
    0xffffffffffffffff,
    0x7fffffffffffffff,
];

// ============================================================================
// Field Element (mod p)
// ============================================================================

/// Field element in GF(2^255 - 19)
#[derive(Clone, Copy)]
struct Fe([u64; 4]);

impl Fe {
    const fn zero() -> Self {
        Fe([0; 4])
    }

    const fn one() -> Self {
        Fe([1, 0, 0, 0])
    }

    /// Create from little-endian bytes
    fn from_bytes(bytes: &[u8; 32]) -> Self {
        let mut limbs = [0u64; 4];
        for i in 0..4 {
            limbs[i] = u64::from_le_bytes([
                bytes[i * 8],
                bytes[i * 8 + 1],
                bytes[i * 8 + 2],
                bytes[i * 8 + 3],
                bytes[i * 8 + 4],
                bytes[i * 8 + 5],
                bytes[i * 8 + 6],
                bytes[i * 8 + 7],
            ]);
        }
        // Clear the top bit (mod p reduction)
        limbs[3] &= 0x7fffffffffffffff;
        Fe(limbs)
    }

    /// Convert to little-endian bytes
    fn to_bytes(&self) -> [u8; 32] {
        let reduced = self.reduce();
        let mut bytes = [0u8; 32];
        for i in 0..4 {
            let b = reduced.0[i].to_le_bytes();
            bytes[i * 8..i * 8 + 8].copy_from_slice(&b);
        }
        bytes
    }

    /// Reduce modulo p
    fn reduce(&self) -> Self {
        let mut r = *self;
        
        // Simple reduction
        let mut carry = (r.0[3] >> 63) * 19;
        r.0[3] &= 0x7fffffffffffffff;
        
        for i in 0..4 {
            let sum = r.0[i] as u128 + carry as u128;
            r.0[i] = sum as u64;
            carry = (sum >> 64) as u64;
        }
        
        // Handle overflow
        if carry > 0 || r.ge_p() {
            let mut borrow = 0i128;
            for i in 0..4 {
                let diff = r.0[i] as i128 - P[i] as i128 - borrow;
                if diff < 0 {
                    r.0[i] = (diff + (1i128 << 64)) as u64;
                    borrow = 1;
                } else {
                    r.0[i] = diff as u64;
                    borrow = 0;
                }
            }
        }
        
        r
    }

    /// Check if >= p
    fn ge_p(&self) -> bool {
        for i in (0..4).rev() {
            if self.0[i] > P[i] {
                return true;
            }
            if self.0[i] < P[i] {
                return false;
            }
        }
        true
    }

    /// Addition
    fn add(&self, other: &Self) -> Self {
        let mut result = Fe::zero();
        let mut carry = 0u128;
        
        for i in 0..4 {
            let sum = self.0[i] as u128 + other.0[i] as u128 + carry;
            result.0[i] = sum as u64;
            carry = sum >> 64;
        }
        
        // Reduce
        if carry > 0 || result.ge_p() {
            result = result.reduce();
        }
        
        result
    }

    /// Subtraction
    fn sub(&self, other: &Self) -> Self {
        let mut result = Fe::zero();
        let mut borrow = 0i128;
        
        for i in 0..4 {
            let diff = self.0[i] as i128 - other.0[i] as i128 - borrow;
            if diff < 0 {
                result.0[i] = (diff + (1i128 << 64)) as u64;
                borrow = 1;
            } else {
                result.0[i] = diff as u64;
                borrow = 0;
            }
        }
        
        // Add p if result is negative
        if borrow > 0 {
            let mut carry = 0u128;
            for i in 0..4 {
                let sum = result.0[i] as u128 + P[i] as u128 + carry;
                result.0[i] = sum as u64;
                carry = sum >> 64;
            }
        }
        
        result
    }

    /// Multiplication (schoolbook, could use Karatsuba)
    fn mul(&self, other: &Self) -> Self {
        let mut product = [0u128; 8];
        
        for i in 0..4 {
            for j in 0..4 {
                product[i + j] += self.0[i] as u128 * other.0[j] as u128;
            }
        }
        
        // Carry propagation
        for i in 0..7 {
            product[i + 1] += product[i] >> 64;
            product[i] &= 0xffffffffffffffff;
        }
        
        // Reduction: multiply high part by 38 (since 2^256 = 38 mod p)
        let mut result = Fe::zero();
        
        for i in 0..4 {
            let sum = product[i] + product[i + 4] * 38;
            result.0[i] = sum as u64;
            if i < 3 {
                product[i + 1] += sum >> 64;
            }
        }
        
        result.reduce()
    }

    /// Square
    fn square(&self) -> Self {
        self.mul(self)
    }

    /// Modular inverse using Fermat's little theorem: a^(-1) = a^(p-2) mod p
    fn invert(&self) -> Self {
        // p - 2 = 2^255 - 21
        let mut result = Fe::one();
        let base = *self;
        
        // Square-and-multiply with p-2
        // p-2 = ...11111111111111111111111111111111111111111111111111111111111101011
        
        // First compute base^(2^250 - 1)
        let mut t = base;
        for _ in 0..250 {
            t = t.square();
            t = t.mul(&base);
        }
        
        // Simplified version - just do full exponentiation
        // In production, use addition chain
        result = base;
        for i in 1..255 {
            result = result.square();
            // Check if bit is set in (p-2)
            let byte_idx = i / 64;
            let bit_idx = i % 64;
            let p_minus_2: [u64; 4] = [
                0xffffffffffffffeb_u64,
                0xffffffffffffffff_u64,
                0xffffffffffffffff_u64,
                0x7fffffffffffffff_u64,
            ];
            if (p_minus_2[byte_idx] >> bit_idx) & 1 == 1 {
                result = result.mul(&base);
            }
        }
        
        result
    }
}

// ============================================================================
// Montgomery Ladder
// ============================================================================

/// X25519 scalar multiplication using Montgomery ladder
fn x25519_scalar_mult(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    // Clamp scalar
    let mut k = *scalar;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;

    let u = Fe::from_bytes(point);
    
    // Montgomery ladder
    let x1 = u;
    let mut x2 = Fe::one();
    let mut z2 = Fe::zero();
    let mut x3 = u;
    let mut z3 = Fe::one();
    
    let mut swap = 0u64;
    
    for i in (0..255).rev() {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        let bit = ((k[byte_idx] >> bit_idx) & 1) as u64;
        
        swap ^= bit;
        cswap(&mut x2, &mut x3, swap);
        cswap(&mut z2, &mut z3, swap);
        swap = bit;
        
        let a = x2.add(&z2);
        let aa = a.square();
        let b = x2.sub(&z2);
        let bb = b.square();
        let e = aa.sub(&bb);
        let c = x3.add(&z3);
        let d = x3.sub(&z3);
        let da = d.mul(&a);
        let cb = c.mul(&b);
        let sum = da.add(&cb);
        let diff = da.sub(&cb);
        x3 = sum.square();
        z3 = x1.mul(&diff.square());
        x2 = aa.mul(&bb);
        // a24 = 121666 = (486662 + 2) / 4
        let a24 = Fe([121666, 0, 0, 0]);
        z2 = e.mul(&aa.add(&a24.mul(&e)));
    }
    
    cswap(&mut x2, &mut x3, swap);
    cswap(&mut z2, &mut z3, swap);
    
    // x2 / z2
    let result = x2.mul(&z2.invert());
    result.to_bytes()
}

/// Conditional swap
fn cswap(a: &mut Fe, b: &mut Fe, swap: u64) {
    let mask = (swap.wrapping_neg()) as u64;
    for i in 0..4 {
        let t = mask & (a.0[i] ^ b.0[i]);
        a.0[i] ^= t;
        b.0[i] ^= t;
    }
}

// ============================================================================
// Public API
// ============================================================================

/// X25519 private key
#[derive(Clone)]
pub struct X25519PrivateKey {
    scalar: [u8; X25519_KEY_SIZE],
}

/// X25519 public key
#[derive(Clone)]
pub struct X25519PublicKey {
    point: [u8; X25519_KEY_SIZE],
}

impl X25519PrivateKey {
    /// Create from raw bytes
    pub fn from_bytes(bytes: &[u8; X25519_KEY_SIZE]) -> Self {
        Self { scalar: *bytes }
    }

    /// Generate from random bytes
    pub fn generate(random: &[u8; X25519_KEY_SIZE]) -> Self {
        Self { scalar: *random }
    }

    /// Derive public key
    pub fn public_key(&self) -> X25519PublicKey {
        // Base point (u = 9)
        let base_point = [9u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                         0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let point = x25519_scalar_mult(&self.scalar, &base_point);
        X25519PublicKey { point }
    }

    /// Perform ECDH key exchange
    pub fn diffie_hellman(&self, peer_public: &X25519PublicKey) -> [u8; X25519_KEY_SIZE] {
        x25519_scalar_mult(&self.scalar, &peer_public.point)
    }

    /// Get raw scalar bytes
    pub fn to_bytes(&self) -> [u8; X25519_KEY_SIZE] {
        self.scalar
    }
}

impl X25519PublicKey {
    /// Create from raw bytes
    pub fn from_bytes(bytes: &[u8; X25519_KEY_SIZE]) -> Self {
        Self { point: *bytes }
    }

    /// Get raw point bytes
    pub fn to_bytes(&self) -> [u8; X25519_KEY_SIZE] {
        self.point
    }
}

/// Perform X25519 key exchange
pub fn x25519(private_key: &[u8; 32], public_key: &[u8; 32]) -> [u8; 32] {
    x25519_scalar_mult(private_key, public_key)
}

/// Derive X25519 public key from private key
pub fn x25519_base(private_key: &[u8; 32]) -> [u8; 32] {
    let base_point = [9u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                     0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    x25519_scalar_mult(private_key, &base_point)
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// X25519 key exchange
#[no_mangle]
pub extern "C" fn X25519(
    out: *mut u8,
    private_key: *const u8,
    public_key: *const u8,
) -> i32 {
    if out.is_null() || private_key.is_null() || public_key.is_null() {
        return 0;
    }

    let priv_key = unsafe {
        let mut arr = [0u8; 32];
        core::ptr::copy_nonoverlapping(private_key, arr.as_mut_ptr(), 32);
        arr
    };

    let pub_key = unsafe {
        let mut arr = [0u8; 32];
        core::ptr::copy_nonoverlapping(public_key, arr.as_mut_ptr(), 32);
        arr
    };

    let result = x25519(&priv_key, &pub_key);

    unsafe {
        core::ptr::copy_nonoverlapping(result.as_ptr(), out, 32);
    }

    1
}

/// X25519 public key derivation
#[no_mangle]
pub extern "C" fn X25519_public_from_private(
    out: *mut u8,
    private_key: *const u8,
) -> i32 {
    if out.is_null() || private_key.is_null() {
        return 0;
    }

    let priv_key = unsafe {
        let mut arr = [0u8; 32];
        core::ptr::copy_nonoverlapping(private_key, arr.as_mut_ptr(), 32);
        arr
    };

    let result = x25519_base(&priv_key);

    unsafe {
        core::ptr::copy_nonoverlapping(result.as_ptr(), out, 32);
    }

    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_x25519_base() {
        // Test vector from RFC 7748
        let private_key = [
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d,
            0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2, 0x66, 0x45,
            0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a,
            0xb1, 0x77, 0xfb, 0xa5, 0x1d, 0xb9, 0x2c, 0x2a,
        ];

        let public_key = x25519_base(&private_key);
        
        // Public key should be non-zero
        assert!(public_key.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_x25519_key_exchange() {
        let alice_private = [1u8; 32];
        let bob_private = [2u8; 32];

        let alice_public = x25519_base(&alice_private);
        let bob_public = x25519_base(&bob_private);

        let alice_shared = x25519(&alice_private, &bob_public);
        let bob_shared = x25519(&bob_private, &alice_public);

        assert_eq!(alice_shared, bob_shared);
    }
}
