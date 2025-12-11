//! X25519 Key Exchange
//!
//! RFC 7748 compliant Curve25519 ECDH implementation.
//! Uses radix-2^51 representation for field elements.

#[allow(unused_imports)]
use std::vec::Vec;

// ============================================================================
// Constants
// ============================================================================

/// X25519 key size (32 bytes)
pub const X25519_KEY_SIZE: usize = 32;

// ============================================================================
// Field Element (mod 2^255 - 19)
//
// Uses 5 limbs of 51 bits each (255 bits total)
// This representation avoids carry overflow in multiplication
// ============================================================================

/// Field element in GF(2^255 - 19)
/// Represented as 5 limbs, each holding up to 51 bits
#[derive(Clone, Copy, Debug)]
struct Fe([u64; 5]);

const MASK51: u64 = (1u64 << 51) - 1;

impl Fe {
    const fn zero() -> Self {
        Fe([0; 5])
    }

    const fn one() -> Self {
        Fe([1, 0, 0, 0, 0])
    }

    /// Create from little-endian bytes
    fn from_bytes(bytes: &[u8; 32]) -> Self {
        let mut h = [0u64; 5];

        // Load bytes as u64s
        let load64 = |b: &[u8]| -> u64 {
            let mut tmp = [0u8; 8];
            let len = b.len().min(8);
            tmp[..len].copy_from_slice(&b[..len]);
            u64::from_le_bytes(tmp)
        };

        h[0] = load64(&bytes[0..]) & MASK51;
        h[1] = (load64(&bytes[6..]) >> 3) & MASK51;
        h[2] = (load64(&bytes[12..]) >> 6) & MASK51;
        h[3] = (load64(&bytes[19..]) >> 1) & MASK51;
        h[4] = (load64(&bytes[24..]) >> 12) & MASK51;

        Fe(h)
    }

    /// Convert to little-endian bytes
    fn to_bytes(&self) -> [u8; 32] {
        let mut h = self.reduce();

        // Second reduction to ensure fully reduced
        h = h.reduce();

        let mut bytes = [0u8; 32];

        // Pack into bytes
        let h0 = h.0[0];
        let h1 = h.0[1];
        let h2 = h.0[2];
        let h3 = h.0[3];
        let h4 = h.0[4];

        bytes[0] = h0 as u8;
        bytes[1] = (h0 >> 8) as u8;
        bytes[2] = (h0 >> 16) as u8;
        bytes[3] = (h0 >> 24) as u8;
        bytes[4] = (h0 >> 32) as u8;
        bytes[5] = (h0 >> 40) as u8;
        bytes[6] = ((h0 >> 48) | (h1 << 3)) as u8;
        bytes[7] = (h1 >> 5) as u8;
        bytes[8] = (h1 >> 13) as u8;
        bytes[9] = (h1 >> 21) as u8;
        bytes[10] = (h1 >> 29) as u8;
        bytes[11] = (h1 >> 37) as u8;
        bytes[12] = ((h1 >> 45) | (h2 << 6)) as u8;
        bytes[13] = (h2 >> 2) as u8;
        bytes[14] = (h2 >> 10) as u8;
        bytes[15] = (h2 >> 18) as u8;
        bytes[16] = (h2 >> 26) as u8;
        bytes[17] = (h2 >> 34) as u8;
        bytes[18] = (h2 >> 42) as u8;
        bytes[19] = ((h2 >> 50) | (h3 << 1)) as u8;
        bytes[20] = (h3 >> 7) as u8;
        bytes[21] = (h3 >> 15) as u8;
        bytes[22] = (h3 >> 23) as u8;
        bytes[23] = (h3 >> 31) as u8;
        bytes[24] = (h3 >> 39) as u8;
        bytes[25] = ((h3 >> 47) | (h4 << 4)) as u8;
        bytes[26] = (h4 >> 4) as u8;
        bytes[27] = (h4 >> 12) as u8;
        bytes[28] = (h4 >> 20) as u8;
        bytes[29] = (h4 >> 28) as u8;
        bytes[30] = (h4 >> 36) as u8;
        bytes[31] = (h4 >> 44) as u8;

        bytes
    }

    /// Carry propagation and reduction
    fn reduce(&self) -> Self {
        let mut h = *self;

        // First pass: carry propagation
        let mut carry = h.0[0] >> 51;
        h.0[0] &= MASK51;
        h.0[1] += carry;
        carry = h.0[1] >> 51;
        h.0[1] &= MASK51;
        h.0[2] += carry;
        carry = h.0[2] >> 51;
        h.0[2] &= MASK51;
        h.0[3] += carry;
        carry = h.0[3] >> 51;
        h.0[3] &= MASK51;
        h.0[4] += carry;
        carry = h.0[4] >> 51;
        h.0[4] &= MASK51;

        // Reduce mod p: multiply carry by 19
        h.0[0] += carry * 19;

        // Second pass
        carry = h.0[0] >> 51;
        h.0[0] &= MASK51;
        h.0[1] += carry;
        carry = h.0[1] >> 51;
        h.0[1] &= MASK51;
        h.0[2] += carry;
        carry = h.0[2] >> 51;
        h.0[2] &= MASK51;
        h.0[3] += carry;
        carry = h.0[3] >> 51;
        h.0[3] &= MASK51;
        h.0[4] += carry;
        carry = h.0[4] >> 51;
        h.0[4] &= MASK51;
        h.0[0] += carry * 19;

        h
    }

    /// Addition
    fn add(&self, other: &Self) -> Self {
        Fe([
            self.0[0] + other.0[0],
            self.0[1] + other.0[1],
            self.0[2] + other.0[2],
            self.0[3] + other.0[3],
            self.0[4] + other.0[4],
        ])
    }

    /// Subtraction
    fn sub(&self, other: &Self) -> Self {
        // Add 8*p to ensure positive result (this fits in 5 limbs of 54 bits each)
        // 8*p = 8*(2^255 - 19) = 2^258 - 152
        // Donna uses: limb[0] = 2^54 - 152, limbs[1-4] = 2^54 - 8
        // These sum to 8*p ≡ 0 (mod p)
        const ADJ: [u64; 5] = [
            0x3fffffffffff68, // 2^54 - 152
            0x3ffffffffffff8, // 2^54 - 8
            0x3ffffffffffff8,
            0x3ffffffffffff8,
            0x3ffffffffffff8,
        ];

        Fe([
            (self.0[0] + ADJ[0]) - other.0[0],
            (self.0[1] + ADJ[1]) - other.0[1],
            (self.0[2] + ADJ[2]) - other.0[2],
            (self.0[3] + ADJ[3]) - other.0[3],
            (self.0[4] + ADJ[4]) - other.0[4],
        ])
    }

    /// Multiplication using 128-bit intermediates
    fn mul(&self, other: &Self) -> Self {
        let a0 = self.0[0] as u128;
        let a1 = self.0[1] as u128;
        let a2 = self.0[2] as u128;
        let a3 = self.0[3] as u128;
        let a4 = self.0[4] as u128;

        let b0 = other.0[0] as u128;
        let b1 = other.0[1] as u128;
        let b2 = other.0[2] as u128;
        let b3 = other.0[3] as u128;
        let b4 = other.0[4] as u128;

        // Pre-multiply by 19 for reduction
        let b1_19 = b1 * 19;
        let b2_19 = b2 * 19;
        let b3_19 = b3 * 19;
        let b4_19 = b4 * 19;

        // Schoolbook multiplication with modular reduction
        let h0 = a0 * b0 + a1 * b4_19 + a2 * b3_19 + a3 * b2_19 + a4 * b1_19;
        let h1 = a0 * b1 + a1 * b0 + a2 * b4_19 + a3 * b3_19 + a4 * b2_19;
        let h2 = a0 * b2 + a1 * b1 + a2 * b0 + a3 * b4_19 + a4 * b3_19;
        let h3 = a0 * b3 + a1 * b2 + a2 * b1 + a3 * b0 + a4 * b4_19;
        let h4 = a0 * b4 + a1 * b3 + a2 * b2 + a3 * b1 + a4 * b0;

        // Carry propagation
        let mut r0 = h0 as u64 & MASK51;
        let mut c = h0 >> 51;

        let h1 = h1 + c;
        let mut r1 = h1 as u64 & MASK51;
        c = h1 >> 51;

        let h2 = h2 + c;
        let mut r2 = h2 as u64 & MASK51;
        c = h2 >> 51;

        let h3 = h3 + c;
        let mut r3 = h3 as u64 & MASK51;
        c = h3 >> 51;

        let h4 = h4 + c;
        let mut r4 = h4 as u64 & MASK51;
        c = h4 >> 51;

        // Final reduction
        r0 += (c as u64) * 19;
        c = (r0 >> 51) as u128;
        r0 &= MASK51;
        r1 += c as u64;

        Fe([r0, r1, r2, r3, r4])
    }

    /// Square
    fn square(&self) -> Self {
        let a0 = self.0[0] as u128;
        let a1 = self.0[1] as u128;
        let a2 = self.0[2] as u128;
        let a3 = self.0[3] as u128;
        let a4 = self.0[4] as u128;

        // Double products
        let a0_2 = a0 * 2;
        let a1_2 = a1 * 2;
        let a2_2 = a2 * 2;
        let a3_2 = a3 * 2;

        // Pre-multiply by 19
        let a1_38 = a1 * 38;
        let a2_38 = a2 * 38;
        let a3_38 = a3 * 38;
        let a4_38 = a4 * 38;
        let a4_19 = a4 * 19;

        let h0 = a0 * a0 + a1_38 * a4 + a2_38 * a3;
        let h1 = a0_2 * a1 + a2_38 * a4 + a3_38 * (a3 / 2); // a3^2 * 19
        let h2 = a0_2 * a2 + a1 * a1 + a3_38 * a4;
        let h3 = a0_2 * a3 + a1_2 * a2 + a4_19 * a4;
        let h4 = a0_2 * a4 + a1_2 * a3 + a2 * a2;

        // Hmm, the squaring formula is more complex. Let me use mul instead
        self.mul(self)
    }

    /// Modular inverse using Fermat's little theorem
    /// a^(-1) = a^(p-2) mod p where p = 2^255 - 19
    fn invert(&self) -> Self {
        // Use addition chain for p-2 = 2^255 - 21
        let z1 = *self;
        let z2 = z1.square();
        let z4 = z2.square();
        let z8 = z4.square();
        let z9 = z8.mul(&z1);
        let z11 = z9.mul(&z2);
        let z22 = z11.square();
        let z_5_0 = z22.mul(&z9); // z^(2^5 - 1) = z^31

        let mut z_10_5 = z_5_0;
        for _ in 0..5 {
            z_10_5 = z_10_5.square();
        }
        let z_10_0 = z_10_5.mul(&z_5_0); // z^(2^10 - 1)

        let mut z_20_10 = z_10_0;
        for _ in 0..10 {
            z_20_10 = z_20_10.square();
        }
        let z_20_0 = z_20_10.mul(&z_10_0); // z^(2^20 - 1)

        let mut z_40_20 = z_20_0;
        for _ in 0..20 {
            z_40_20 = z_40_20.square();
        }
        let z_40_0 = z_40_20.mul(&z_20_0); // z^(2^40 - 1)

        let mut z_50_10 = z_40_0;
        for _ in 0..10 {
            z_50_10 = z_50_10.square();
        }
        let z_50_0 = z_50_10.mul(&z_10_0); // z^(2^50 - 1)

        let mut z_100_50 = z_50_0;
        for _ in 0..50 {
            z_100_50 = z_100_50.square();
        }
        let z_100_0 = z_100_50.mul(&z_50_0); // z^(2^100 - 1)

        let mut z_200_100 = z_100_0;
        for _ in 0..100 {
            z_200_100 = z_200_100.square();
        }
        let z_200_0 = z_200_100.mul(&z_100_0); // z^(2^200 - 1)

        let mut z_250_50 = z_200_0;
        for _ in 0..50 {
            z_250_50 = z_250_50.square();
        }
        let z_250_0 = z_250_50.mul(&z_50_0); // z^(2^250 - 1)

        let mut z_255_5 = z_250_0;
        for _ in 0..5 {
            z_255_5 = z_255_5.square();
        } // z^(2^255 - 2^5)

        z_255_5.mul(&z11) // z^(2^255 - 2^5 + 11) = z^(2^255 - 21) = z^(p-2)
    }
}

// ============================================================================
// Montgomery Ladder
// ============================================================================

/// X25519 scalar multiplication using Montgomery ladder
fn x25519_scalar_mult(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    // Clamp scalar per RFC 7748
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

    let mut swap: u64 = 0;

    for i in (0..255).rev() {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        let bit = ((k[byte_idx] >> bit_idx) & 1) as u64;

        swap ^= bit;
        cswap(&mut x2, &mut x3, swap);
        cswap(&mut z2, &mut z3, swap);
        swap = bit;

        let a = x2.add(&z2);
        let aa = a.mul(&a); // (x2+z2)^2
        let b = x2.sub(&z2);
        let bb = b.mul(&b); // (x2-z2)^2
        let e = aa.sub(&bb); // (x2+z2)^2 - (x2-z2)^2 = 4*x2*z2
        let c = x3.add(&z3);
        let d = x3.sub(&z3);
        let da = d.mul(&a); // (x3-z3)(x2+z2)
        let cb = c.mul(&b); // (x3+z3)(x2-z2)
        let sum = da.add(&cb);
        let diff = da.sub(&cb);
        x3 = sum.mul(&sum); // ((x3-z3)(x2+z2) + (x3+z3)(x2-z2))^2
        z3 = x1.mul(&diff.mul(&diff)); // x1 * ((x3-z3)(x2+z2) - (x3+z3)(x2-z2))^2
        x2 = aa.mul(&bb); // (x2+z2)^2 * (x2-z2)^2

        // a24 = (A+2)/4 = (486662+2)/4 = 121666
        let a24 = Fe([121666, 0, 0, 0, 0]);
        let a24_e = a24.mul(&e);
        z2 = e.mul(&bb.add(&a24_e)); // 4*x2*z2 * ((x2-z2)^2 + 121666*4*x2*z2)  -- FIXED: bb not aa
    }

    cswap(&mut x2, &mut x3, swap);
    cswap(&mut z2, &mut z3, swap);

    // x2 / z2
    let result = x2.mul(&z2.invert());
    result.to_bytes()
}

/// Constant-time conditional swap
fn cswap(a: &mut Fe, b: &mut Fe, swap: u64) {
    let mask = (swap.wrapping_neg()) as u64;
    for i in 0..5 {
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
        let base_point = [
            9u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ];
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
    let base_point = [
        9u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];
    x25519_scalar_mult(private_key, &base_point)
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// X25519 key exchange
#[no_mangle]
pub extern "C" fn X25519(out: *mut u8, private_key: *const u8, public_key: *const u8) -> i32 {
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
pub extern "C" fn X25519_public_from_private(out: *mut u8, private_key: *const u8) -> i32 {
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
    fn test_x25519_rfc7748() {
        // Test vector from RFC 7748
        let private_key = [
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d, 0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2,
            0x66, 0x45, 0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a, 0xb1, 0x77, 0xfb, 0xa5,
            0x1d, 0xb9, 0x2c, 0x2a,
        ];
        let expected_public = [
            0x85, 0x20, 0xf0, 0x09, 0x89, 0x30, 0xa7, 0x54, 0x74, 0x8b, 0x7d, 0xdc, 0xb4, 0x3e,
            0xf7, 0x5a, 0x0d, 0xbf, 0x3a, 0x0d, 0x26, 0x38, 0x1a, 0xf4, 0xeb, 0xa4, 0xa9, 0x8e,
            0xaa, 0x9b, 0x4e, 0x6a,
        ];

        let public_key = x25519_base(&private_key);
        assert_eq!(public_key, expected_public);
    }

    #[test]
    fn test_x25519_key_exchange() {
        // Test key exchange from RFC 7748
        let alice_private = [
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d, 0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2,
            0x66, 0x45, 0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a, 0xb1, 0x77, 0xfb, 0xa5,
            0x1d, 0xb9, 0x2c, 0x2a,
        ];
        let bob_private = [
            0x5d, 0xab, 0x08, 0x7e, 0x62, 0x4a, 0x8a, 0x4b, 0x79, 0xe1, 0x7f, 0x8b, 0x83, 0x80,
            0x0e, 0xe6, 0x6f, 0x3b, 0xb1, 0x29, 0x26, 0x18, 0xb6, 0xfd, 0x1c, 0x2f, 0x8b, 0x27,
            0xff, 0x88, 0xe0, 0xeb,
        ];

        let alice_public = x25519_base(&alice_private);
        let bob_public = x25519_base(&bob_private);

        let alice_shared = x25519(&alice_private, &bob_public);
        let bob_shared = x25519(&bob_private, &alice_public);

        // Both should compute the same shared secret
        assert_eq!(alice_shared, bob_shared);

        // Expected shared secret from RFC 7748
        let expected_shared = [
            0x4a, 0x5d, 0x9d, 0x5b, 0xa4, 0xce, 0x2d, 0xe1, 0x72, 0x8e, 0x3b, 0xf4, 0x80, 0x35,
            0x0f, 0x25, 0xe0, 0x7e, 0x21, 0xc9, 0x47, 0xd1, 0x9e, 0x33, 0x76, 0xf0, 0x9b, 0x3c,
            0x1e, 0x16, 0x17, 0x42,
        ];
        assert_eq!(alice_shared, expected_shared);
    }
}
