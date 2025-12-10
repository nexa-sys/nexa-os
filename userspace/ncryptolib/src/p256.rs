//! P-256 (secp256r1) Elliptic Curve Operations
//!
//! Complete P-256 implementation for ECDH key exchange and ECDSA signatures.
//! NIST FIPS 186-4 compliant.

use std::vec::Vec;

use crate::bigint::BigInt;
use crate::hash::sha256;
use crate::random::random_bytes;

// ============================================================================
// Curve Constants
// ============================================================================

/// Field size in bytes
pub const P256_COORD_SIZE: usize = 32;
/// Signature size (r || s)
pub const P256_SIG_SIZE: usize = 64;
/// Private key size
pub const P256_PRIVATE_KEY_SIZE: usize = 32;
/// Uncompressed public key size (04 || x || y)
pub const P256_PUBLIC_KEY_SIZE: usize = 65;

/// Field prime p = 2^256 - 2^224 + 2^192 + 2^96 - 1
const P_BYTES: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
];

/// Curve order n
const N_BYTES: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
];

/// Generator point Gx
const GX_BYTES: [u8; 32] = [
    0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63, 0xa4, 0x40, 0xf2,
    0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb, 0x33, 0xa0, 0xf4, 0xa1, 0x39, 0x45, 0xd8, 0x98, 0xc2, 0x96,
];

/// Generator point Gy
const GY_BYTES: [u8; 32] = [
    0x4f, 0xe3, 0x42, 0xe2, 0xfe, 0x1a, 0x7f, 0x9b, 0x8e, 0xe7, 0xeb, 0x4a, 0x7c, 0x0f, 0x9e, 0x16,
    0x2b, 0xce, 0x33, 0x57, 0x6b, 0x31, 0x5e, 0xce, 0xcb, 0xb6, 0x40, 0x68, 0x37, 0xbf, 0x51, 0xf5,
];

/// Curve parameter a = -3 mod p (in Montgomery form this is p - 3)
const A_BYTES: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfc,
];

// ============================================================================
// Field Element
// ============================================================================

/// Field element modulo p
#[derive(Clone, Debug, PartialEq, Eq)]
struct FieldElement {
    value: BigInt,
}

impl FieldElement {
    fn zero() -> Self {
        Self {
            value: BigInt::zero(),
        }
    }

    fn one() -> Self {
        Self {
            value: BigInt::one(),
        }
    }

    fn p() -> BigInt {
        BigInt::from_bytes_be(&P_BYTES)
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > 32 {
            return None;
        }
        let value = BigInt::from_bytes_be(bytes);
        let p = Self::p();
        if value >= p {
            return None;
        }
        Some(Self { value })
    }

    fn from_bigint(value: BigInt) -> Self {
        let p = Self::p();
        Self {
            value: value.mod_reduce(&p),
        }
    }

    fn to_bytes(&self) -> [u8; 32] {
        let bytes = self.value.to_bytes_be_padded(32);
        let mut result = [0u8; 32];
        result.copy_from_slice(&bytes);
        result
    }

    fn add(&self, other: &Self) -> Self {
        let p = Self::p();
        let sum = self.value.add(&other.value);
        Self {
            value: if sum >= p { sum.sub(&p) } else { sum },
        }
    }

    fn sub(&self, other: &Self) -> Self {
        let p = Self::p();
        if self.value >= other.value {
            Self {
                value: self.value.sub(&other.value),
            }
        } else {
            Self {
                value: p.sub(&other.value.sub(&self.value)),
            }
        }
    }

    fn mul(&self, other: &Self) -> Self {
        let p = Self::p();
        Self {
            value: self.value.mul(&other.value).mod_reduce(&p),
        }
    }

    fn square(&self) -> Self {
        self.mul(self)
    }

    fn double(&self) -> Self {
        self.add(self)
    }

    fn negate(&self) -> Self {
        if self.is_zero() {
            Self::zero()
        } else {
            let p = Self::p();
            Self {
                value: p.sub(&self.value),
            }
        }
    }

    fn is_zero(&self) -> bool {
        self.value.is_zero()
    }

    /// Modular inverse using Fermat's little theorem: a^(-1) = a^(p-2) mod p
    fn invert(&self) -> Option<Self> {
        if self.is_zero() {
            return None;
        }

        let p = Self::p();
        let p_minus_2 = p.sub(&BigInt::from_u64(2));

        Some(Self {
            value: mod_exp(&self.value, &p_minus_2, &p),
        })
    }

    /// Multiply by a small integer
    fn mul_small(&self, n: u64) -> Self {
        let p = Self::p();
        Self {
            value: self.value.mul(&BigInt::from_u64(n)).mod_reduce(&p),
        }
    }
}

// ============================================================================
// Jacobian Point (for fast computation)
// ============================================================================

/// Jacobian point representation: (X, Y, Z) represents affine (X/Z², Y/Z³)
/// This allows point addition and doubling without modular inversion
#[derive(Clone, Debug)]
struct JacobianPoint {
    x: FieldElement,
    y: FieldElement,
    z: FieldElement,
}

impl JacobianPoint {
    /// Point at infinity
    fn infinity() -> Self {
        Self {
            x: FieldElement::one(),
            y: FieldElement::one(),
            z: FieldElement::zero(),
        }
    }

    /// Convert from affine point
    fn from_affine(p: &P256Point) -> Self {
        if p.infinity {
            Self::infinity()
        } else {
            Self {
                x: p.x.clone(),
                y: p.y.clone(),
                z: FieldElement::one(),
            }
        }
    }

    /// Convert to affine point (requires one modular inversion)
    fn to_affine(&self) -> P256Point {
        if self.z.is_zero() {
            return P256Point::infinity();
        }

        let z_inv = match self.z.invert() {
            Some(inv) => inv,
            None => return P256Point::infinity(),
        };

        let z_inv2 = z_inv.square();
        let z_inv3 = z_inv2.mul(&z_inv);

        P256Point {
            x: self.x.mul(&z_inv2),
            y: self.y.mul(&z_inv3),
            infinity: false,
        }
    }

    fn is_infinity(&self) -> bool {
        self.z.is_zero()
    }

    /// Point doubling in Jacobian coordinates
    /// Formula from https://hyperelliptic.org/EFD/g1p/auto-shortw-jacobian-3.html
    /// Cost: 4M + 4S (where M = multiplication, S = squaring)
    fn double(&self) -> Self {
        if self.is_infinity() {
            return Self::infinity();
        }

        // For curve y² = x³ + ax + b with a = -3:
        // δ = Z²
        // γ = Y²
        // β = X·γ
        // α = 3·(X-δ)·(X+δ) = 3·X² - 3·Z⁴ = 3·(X² - Z⁴)
        // X' = α² - 8·β
        // Z' = (Y+Z)² - γ - δ
        // Y' = α·(4·β - X') - 8·γ²

        let delta = self.z.square();
        let gamma = self.y.square();
        let beta = self.x.mul(&gamma);

        // Since a = -3 for P-256, we use: α = 3·(X - Z²)·(X + Z²)
        let x_minus_delta = self.x.sub(&delta);
        let x_plus_delta = self.x.add(&delta);
        let alpha = x_minus_delta.mul(&x_plus_delta).mul_small(3);

        // X' = α² - 8·β
        let alpha_sq = alpha.square();
        let beta8 = beta.mul_small(8);
        let x3 = alpha_sq.sub(&beta8);

        // Z' = (Y + Z)² - γ - δ
        let y_plus_z = self.y.add(&self.z);
        let z3 = y_plus_z.square().sub(&gamma).sub(&delta);

        // Y' = α·(4·β - X') - 8·γ²
        let beta4 = beta.mul_small(4);
        let gamma_sq = gamma.square();
        let gamma_sq8 = gamma_sq.mul_small(8);
        let y3 = alpha.mul(&beta4.sub(&x3)).sub(&gamma_sq8);

        Self {
            x: x3,
            y: y3,
            z: z3,
        }
    }

    /// Point addition in Jacobian coordinates (mixed: Jacobian + Affine)
    /// This is faster when adding an affine point (Z = 1)
    /// Cost: 8M + 3S
    fn add_affine(&self, other: &P256Point) -> Self {
        if self.is_infinity() {
            return Self::from_affine(other);
        }
        if other.infinity {
            return self.clone();
        }

        // Z1² and Z1³
        let z1_sq = self.z.square();
        let z1_cu = z1_sq.mul(&self.z);

        // U1 = X1, U2 = X2·Z1²
        let u2 = other.x.mul(&z1_sq);

        // S1 = Y1, S2 = Y2·Z1³
        let s2 = other.y.mul(&z1_cu);

        // H = U2 - U1 = U2 - X1
        let h = u2.sub(&self.x);

        // R = S2 - S1 = S2 - Y1
        let r = s2.sub(&self.y);

        if h.is_zero() {
            if r.is_zero() {
                // P1 == P2, do doubling
                return self.double();
            } else {
                // P1 == -P2
                return Self::infinity();
            }
        }

        let h_sq = h.square();
        let h_cu = h_sq.mul(&h);

        // X3 = R² - H³ - 2·U1·H²
        let u1_h_sq = self.x.mul(&h_sq);
        let x3 = r.square().sub(&h_cu).sub(&u1_h_sq.double());

        // Y3 = R·(U1·H² - X3) - S1·H³
        let y3 = r.mul(&u1_h_sq.sub(&x3)).sub(&self.y.mul(&h_cu));

        // Z3 = Z1·H
        let z3 = self.z.mul(&h);

        Self {
            x: x3,
            y: y3,
            z: z3,
        }
    }

    /// Full Jacobian addition (both points in Jacobian coordinates)
    /// Cost: 12M + 4S
    fn add(&self, other: &Self) -> Self {
        if self.is_infinity() {
            return other.clone();
        }
        if other.is_infinity() {
            return self.clone();
        }

        let z1_sq = self.z.square();
        let z2_sq = other.z.square();
        let z1_cu = z1_sq.mul(&self.z);
        let z2_cu = z2_sq.mul(&other.z);

        // U1 = X1·Z2², U2 = X2·Z1²
        let u1 = self.x.mul(&z2_sq);
        let u2 = other.x.mul(&z1_sq);

        // S1 = Y1·Z2³, S2 = Y2·Z1³
        let s1 = self.y.mul(&z2_cu);
        let s2 = other.y.mul(&z1_cu);

        // H = U2 - U1
        let h = u2.sub(&u1);

        // R = S2 - S1
        let r = s2.sub(&s1);

        if h.is_zero() {
            if r.is_zero() {
                return self.double();
            } else {
                return Self::infinity();
            }
        }

        let h_sq = h.square();
        let h_cu = h_sq.mul(&h);

        // X3 = R² - H³ - 2·U1·H²
        let u1_h_sq = u1.mul(&h_sq);
        let x3 = r.square().sub(&h_cu).sub(&u1_h_sq.double());

        // Y3 = R·(U1·H² - X3) - S1·H³
        let y3 = r.mul(&u1_h_sq.sub(&x3)).sub(&s1.mul(&h_cu));

        // Z3 = Z1·Z2·H
        let z3 = self.z.mul(&other.z).mul(&h);

        Self {
            x: x3,
            y: y3,
            z: z3,
        }
    }
}

// ============================================================================
// Point Operations (Affine)
// ============================================================================

/// Affine point on the P-256 curve
#[derive(Clone, Debug)]
pub struct P256Point {
    x: FieldElement,
    y: FieldElement,
    infinity: bool,
}

impl P256Point {
    /// Point at infinity
    pub fn infinity() -> Self {
        Self {
            x: FieldElement::zero(),
            y: FieldElement::zero(),
            infinity: true,
        }
    }

    /// Base point (generator)
    pub fn generator() -> Self {
        Self {
            x: FieldElement::from_bytes(&GX_BYTES).unwrap(),
            y: FieldElement::from_bytes(&GY_BYTES).unwrap(),
            infinity: false,
        }
    }

    /// Create from coordinates
    pub fn from_affine(x: &[u8], y: &[u8]) -> Option<Self> {
        let x_fe = FieldElement::from_bytes(x)?;
        let y_fe = FieldElement::from_bytes(y)?;

        let point = Self {
            x: x_fe,
            y: y_fe,
            infinity: false,
        };

        // Verify point is on curve
        if !point.is_on_curve() {
            return None;
        }

        Some(point)
    }

    /// Create from uncompressed format (04 || x || y)
    pub fn from_uncompressed(data: &[u8]) -> Option<Self> {
        if data.len() != 65 || data[0] != 0x04 {
            return None;
        }
        Self::from_affine(&data[1..33], &data[33..65])
    }

    /// Convert to uncompressed format
    pub fn to_uncompressed(&self) -> Vec<u8> {
        if self.infinity {
            return vec![0];
        }

        let mut result = Vec::with_capacity(65);
        result.push(0x04);
        result.extend_from_slice(&self.x.to_bytes());
        result.extend_from_slice(&self.y.to_bytes());
        result
    }

    /// Check if point is on the curve: y^2 = x^3 + ax + b
    fn is_on_curve(&self) -> bool {
        if self.infinity {
            return true;
        }

        let a = FieldElement::from_bytes(&A_BYTES).unwrap();
        let b = FieldElement::from_bytes(&[
            0x5a, 0xc6, 0x35, 0xd8, 0xaa, 0x3a, 0x93, 0xe7, 0xb3, 0xeb, 0xbd, 0x55, 0x76, 0x98,
            0x86, 0xbc, 0x65, 0x1d, 0x06, 0xb0, 0xcc, 0x53, 0xb0, 0xf6, 0x3b, 0xce, 0x3c, 0x3e,
            0x27, 0xd2, 0x60, 0x4b,
        ])
        .unwrap();

        // y^2 = x^3 + ax + b
        let y_squared = self.y.square();
        let x_cubed = self.x.square().mul(&self.x);
        let ax = a.mul(&self.x);
        let rhs = x_cubed.add(&ax).add(&b);

        y_squared == rhs
    }

    /// Point doubling (affine)
    fn double(&self) -> Self {
        if self.infinity || self.y.is_zero() {
            return Self::infinity();
        }

        let a = FieldElement::from_bytes(&A_BYTES).unwrap();

        // lambda = (3*x^2 + a) / (2*y)
        let three = FieldElement {
            value: BigInt::from_u64(3),
        };
        let two = FieldElement {
            value: BigInt::from_u64(2),
        };

        let x_squared = self.x.square();
        let numerator = three.mul(&x_squared).add(&a);
        let denominator = two.mul(&self.y);

        let lambda = match denominator.invert() {
            Some(inv) => numerator.mul(&inv),
            None => return Self::infinity(),
        };

        // x3 = lambda^2 - 2*x
        let lambda_squared = lambda.square();
        let two_x = two.mul(&self.x);
        let x3 = lambda_squared.sub(&two_x);

        // y3 = lambda*(x - x3) - y
        let y3 = lambda.mul(&self.x.sub(&x3)).sub(&self.y);

        Self {
            x: x3,
            y: y3,
            infinity: false,
        }
    }

    /// Point addition (affine)
    fn add(&self, other: &Self) -> Self {
        if self.infinity {
            return other.clone();
        }
        if other.infinity {
            return self.clone();
        }

        // If points are equal, use doubling
        if self.x == other.x {
            if self.y == other.y {
                return self.double();
            } else {
                // P + (-P) = O
                return Self::infinity();
            }
        }

        // lambda = (y2 - y1) / (x2 - x1)
        let numerator = other.y.sub(&self.y);
        let denominator = other.x.sub(&self.x);

        let lambda = match denominator.invert() {
            Some(inv) => numerator.mul(&inv),
            None => return Self::infinity(),
        };

        // x3 = lambda^2 - x1 - x2
        let lambda_squared = lambda.square();
        let x3 = lambda_squared.sub(&self.x).sub(&other.x);

        // y3 = lambda*(x1 - x3) - y1
        let y3 = lambda.mul(&self.x.sub(&x3)).sub(&self.y);

        Self {
            x: x3,
            y: y3,
            infinity: false,
        }
    }

    /// Scalar multiplication using Jacobian coordinates
    /// This is MUCH faster than affine as it only requires ONE modular inversion
    /// at the end instead of one per point operation.
    pub fn scalar_mul(&self, scalar: &[u8]) -> Self {
        let k = BigInt::from_bytes_be(scalar);

        if k.is_zero() {
            return Self::infinity();
        }

        if self.infinity {
            return Self::infinity();
        }

        // Use Jacobian coordinates for all intermediate computations
        let mut result = JacobianPoint::infinity();

        // Process bits from most significant to least significant (left-to-right)
        // This allows us to use add_affine which is faster than full Jacobian add
        let bits = k.bit_length();
        for i in (0..bits).rev() {
            result = result.double();
            if k.get_bit(i) {
                result = result.add_affine(self);
            }
        }

        // Single conversion back to affine (single modular inversion)
        result.to_affine()
    }

    /// Scalar multiplication optimized for the generator point
    /// Uses a simple double-and-add with Jacobian coordinates
    pub fn scalar_mul_generator(scalar: &[u8]) -> Self {
        Self::generator().scalar_mul(scalar)
    }
}

// ============================================================================
// ECDH Key Exchange
// ============================================================================

/// P-256 ECDH Key Pair
pub struct P256KeyPair {
    /// Private key (scalar)
    pub private_key: [u8; 32],
    /// Public key point
    pub public_key: P256Point,
}

impl P256KeyPair {
    /// Generate a new random key pair
    pub fn generate() -> Option<Self> {
        let n = BigInt::from_bytes_be(&N_BYTES);

        // Generate random scalar in [1, n-1]
        let mut private_key = [0u8; 32];
        loop {
            random_bytes(&mut private_key).ok()?;

            let k = BigInt::from_bytes_be(&private_key);
            if !k.is_zero() && k < n {
                break;
            }
        }

        // Compute public key Q = k * G
        let g = P256Point::generator();
        let public_key = g.scalar_mul(&private_key);

        if public_key.infinity {
            return None;
        }

        Some(Self {
            private_key,
            public_key,
        })
    }

    /// Create from private key
    pub fn from_private_key(private_key: &[u8]) -> Option<Self> {
        if private_key.len() != 32 {
            return None;
        }

        let n = BigInt::from_bytes_be(&N_BYTES);
        let k = BigInt::from_bytes_be(private_key);

        if k.is_zero() || k >= n {
            return None;
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(private_key);

        let g = P256Point::generator();
        let public_key = g.scalar_mul(&key);

        Some(Self {
            private_key: key,
            public_key,
        })
    }

    /// Compute ECDH shared secret
    pub fn ecdh(&self, peer_public: &P256Point) -> Option<[u8; 32]> {
        if peer_public.infinity || !peer_public.is_on_curve() {
            return None;
        }

        let shared = peer_public.scalar_mul(&self.private_key);
        if shared.infinity {
            return None;
        }

        Some(shared.x.to_bytes())
    }

    /// Get public key in uncompressed format
    pub fn public_key_uncompressed(&self) -> Vec<u8> {
        self.public_key.to_uncompressed()
    }
}

// ============================================================================
// ECDSA Signatures
// ============================================================================

/// ECDSA signature (r, s)
#[derive(Clone, Debug)]
pub struct P256Signature {
    pub r: [u8; 32],
    pub s: [u8; 32],
}

impl P256Signature {
    /// Sign a message hash
    pub fn sign(private_key: &[u8], hash: &[u8]) -> Option<Self> {
        if private_key.len() != 32 || hash.len() != 32 {
            return None;
        }

        let n = BigInt::from_bytes_be(&N_BYTES);
        let d = BigInt::from_bytes_be(private_key);
        let z = BigInt::from_bytes_be(hash);

        // Generate random k
        let mut k_bytes = [0u8; 32];
        loop {
            random_bytes(&mut k_bytes).ok()?;

            let k = BigInt::from_bytes_be(&k_bytes);
            if k.is_zero() || k >= n {
                continue;
            }

            // R = k * G
            let g = P256Point::generator();
            let r_point = g.scalar_mul(&k_bytes);
            if r_point.infinity {
                continue;
            }

            // r = R.x mod n
            let r = BigInt::from_bytes_be(&r_point.x.to_bytes()).mod_reduce(&n);
            if r.is_zero() {
                continue;
            }

            // s = k^(-1) * (z + r*d) mod n
            let k_inv = match k.mod_inverse(&n) {
                Some(inv) => inv,
                None => continue,
            };

            let rd = r.mul(&d).mod_reduce(&n);
            let z_rd = z.add(&rd).mod_reduce(&n);
            let s = k_inv.mul(&z_rd).mod_reduce(&n);

            if s.is_zero() {
                continue;
            }

            // Low-S normalization: if s > n/2, use n - s
            let n_half = n.div(&BigInt::from_u64(2)).0;
            let s_normalized = if s > n_half { n.sub(&s) } else { s };

            let mut sig = P256Signature {
                r: [0u8; 32],
                s: [0u8; 32],
            };
            sig.r.copy_from_slice(&r.to_bytes_be_padded(32));
            sig.s.copy_from_slice(&s_normalized.to_bytes_be_padded(32));

            return Some(sig);
        }
    }

    /// Sign a message (hash first with SHA-256)
    pub fn sign_message(private_key: &[u8], message: &[u8]) -> Option<Self> {
        let hash = sha256(message);
        Self::sign(private_key, &hash)
    }

    /// Verify signature against message hash
    pub fn verify(&self, public_key: &P256Point, hash: &[u8]) -> bool {
        if hash.len() != 32 || public_key.infinity {
            return false;
        }

        let n = BigInt::from_bytes_be(&N_BYTES);
        let r = BigInt::from_bytes_be(&self.r);
        let s = BigInt::from_bytes_be(&self.s);
        let z = BigInt::from_bytes_be(hash);

        // Check r, s in [1, n-1]
        if r.is_zero() || r >= n || s.is_zero() || s >= n {
            return false;
        }

        // w = s^(-1) mod n
        let w = match s.mod_inverse(&n) {
            Some(inv) => inv,
            None => return false,
        };

        // u1 = z * w mod n
        let u1 = z.mul(&w).mod_reduce(&n);

        // u2 = r * w mod n
        let u2 = r.mul(&w).mod_reduce(&n);

        // R = u1*G + u2*Q
        let g = P256Point::generator();
        let u1_g = g.scalar_mul(&u1.to_bytes_be_padded(32));
        let u2_q = public_key.scalar_mul(&u2.to_bytes_be_padded(32));
        let r_point = u1_g.add(&u2_q);

        if r_point.infinity {
            return false;
        }

        // r == R.x mod n
        let rx = BigInt::from_bytes_be(&r_point.x.to_bytes()).mod_reduce(&n);
        rx == r
    }

    /// Verify signature against message (hash first)
    pub fn verify_message(&self, public_key: &P256Point, message: &[u8]) -> bool {
        let hash = sha256(message);
        self.verify(public_key, &hash)
    }

    /// Convert to DER format
    pub fn to_der(&self) -> Vec<u8> {
        let r = BigInt::from_bytes_be(&self.r);
        let s = BigInt::from_bytes_be(&self.s);

        let r_bytes = r.to_bytes_be();
        let s_bytes = s.to_bytes_be();

        // Add leading zero if high bit is set
        let r_needs_pad = !r_bytes.is_empty() && r_bytes[0] >= 0x80;
        let s_needs_pad = !s_bytes.is_empty() && s_bytes[0] >= 0x80;

        let r_len = r_bytes.len() + r_needs_pad as usize;
        let s_len = s_bytes.len() + s_needs_pad as usize;
        let seq_len = 2 + r_len + 2 + s_len;

        let mut result = Vec::with_capacity(2 + seq_len);
        result.push(0x30); // SEQUENCE
        result.push(seq_len as u8);

        result.push(0x02); // INTEGER
        result.push(r_len as u8);
        if r_needs_pad {
            result.push(0x00);
        }
        result.extend_from_slice(&r_bytes);

        result.push(0x02); // INTEGER
        result.push(s_len as u8);
        if s_needs_pad {
            result.push(0x00);
        }
        result.extend_from_slice(&s_bytes);

        result
    }

    /// Parse from DER format
    pub fn from_der(data: &[u8]) -> Option<Self> {
        if data.len() < 8 || data[0] != 0x30 {
            return None;
        }

        let _seq_len = data[1] as usize;
        let mut pos = 2;

        // Parse r
        if data[pos] != 0x02 {
            return None;
        }
        pos += 1;
        let r_len = data[pos] as usize;
        pos += 1;

        let r_start = if data[pos] == 0x00 { pos + 1 } else { pos };
        let r_bytes = &data[r_start..pos + r_len];
        pos += r_len;

        // Parse s
        if pos >= data.len() || data[pos] != 0x02 {
            return None;
        }
        pos += 1;
        let s_len = data[pos] as usize;
        pos += 1;

        let s_start = if data[pos] == 0x00 { pos + 1 } else { pos };
        let s_bytes = &data[s_start..pos + s_len];

        let mut sig = P256Signature {
            r: [0u8; 32],
            s: [0u8; 32],
        };

        // Pad to 32 bytes
        let r_offset = 32 - r_bytes.len().min(32);
        let s_offset = 32 - s_bytes.len().min(32);

        sig.r[r_offset..].copy_from_slice(&r_bytes[r_bytes.len().saturating_sub(32)..]);
        sig.s[s_offset..].copy_from_slice(&s_bytes[s_bytes.len().saturating_sub(32)..]);

        Some(sig)
    }

    /// Convert to fixed (r || s) format
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut result = [0u8; 64];
        result[..32].copy_from_slice(&self.r);
        result[32..].copy_from_slice(&self.s);
        result
    }

    /// Parse from fixed (r || s) format
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() != 64 {
            return None;
        }
        let mut sig = P256Signature {
            r: [0u8; 32],
            s: [0u8; 32],
        };
        sig.r.copy_from_slice(&data[..32]);
        sig.s.copy_from_slice(&data[32..]);
        Some(sig)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Modular exponentiation
fn mod_exp(base: &BigInt, exp: &BigInt, modulus: &BigInt) -> BigInt {
    if modulus == &BigInt::one() {
        return BigInt::zero();
    }

    let mut result = BigInt::one();
    let mut base = base.mod_reduce(modulus);
    let mut exp = exp.clone();

    while exp > BigInt::zero() {
        if exp.is_odd() {
            result = result.mul(&base).mod_reduce(modulus);
        }
        exp = exp.div(&BigInt::from_u64(2)).0;
        base = base.mul(&base).mod_reduce(modulus);
    }

    result
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// Generate P-256 key pair
#[no_mangle]
pub extern "C" fn p256_keygen(private_key: *mut u8, public_key: *mut u8) -> i32 {
    if private_key.is_null() || public_key.is_null() {
        return -1;
    }

    match P256KeyPair::generate() {
        Some(kp) => {
            unsafe {
                core::ptr::copy_nonoverlapping(kp.private_key.as_ptr(), private_key, 32);
                let pk = kp.public_key_uncompressed();
                core::ptr::copy_nonoverlapping(pk.as_ptr(), public_key, 65);
            }
            0
        }
        None => -1,
    }
}

/// P-256 ECDH
#[no_mangle]
pub extern "C" fn p256_ecdh(
    private_key: *const u8,
    peer_public_key: *const u8,
    shared_secret: *mut u8,
) -> i32 {
    if private_key.is_null() || peer_public_key.is_null() || shared_secret.is_null() {
        return -1;
    }

    let priv_slice = unsafe { core::slice::from_raw_parts(private_key, 32) };
    let pub_slice = unsafe { core::slice::from_raw_parts(peer_public_key, 65) };

    let kp = match P256KeyPair::from_private_key(priv_slice) {
        Some(k) => k,
        None => return -1,
    };

    let peer = match P256Point::from_uncompressed(pub_slice) {
        Some(p) => p,
        None => return -1,
    };

    match kp.ecdh(&peer) {
        Some(secret) => {
            unsafe {
                core::ptr::copy_nonoverlapping(secret.as_ptr(), shared_secret, 32);
            }
            0
        }
        None => -1,
    }
}

/// P-256 ECDSA sign
#[no_mangle]
pub extern "C" fn p256_sign(
    private_key: *const u8,
    message: *const u8,
    message_len: usize,
    signature: *mut u8,
) -> i32 {
    if private_key.is_null() || message.is_null() || signature.is_null() {
        return -1;
    }

    let priv_slice = unsafe { core::slice::from_raw_parts(private_key, 32) };
    let msg_slice = unsafe { core::slice::from_raw_parts(message, message_len) };

    match P256Signature::sign_message(priv_slice, msg_slice) {
        Some(sig) => {
            let sig_bytes = sig.to_bytes();
            unsafe {
                core::ptr::copy_nonoverlapping(sig_bytes.as_ptr(), signature, 64);
            }
            64
        }
        None => -1,
    }
}

/// P-256 ECDSA verify
#[no_mangle]
pub extern "C" fn p256_verify(
    public_key: *const u8,
    message: *const u8,
    message_len: usize,
    signature: *const u8,
) -> i32 {
    if public_key.is_null() || message.is_null() || signature.is_null() {
        return -1;
    }

    let pub_slice = unsafe { core::slice::from_raw_parts(public_key, 65) };
    let msg_slice = unsafe { core::slice::from_raw_parts(message, message_len) };
    let sig_slice = unsafe { core::slice::from_raw_parts(signature, 64) };

    let pk = match P256Point::from_uncompressed(pub_slice) {
        Some(p) => p,
        None => return 0,
    };

    let sig = match P256Signature::from_bytes(sig_slice) {
        Some(s) => s,
        None => return 0,
    };

    if sig.verify_message(&pk, msg_slice) {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keygen_and_ecdh() {
        let alice = P256KeyPair::generate().unwrap();
        let bob = P256KeyPair::generate().unwrap();

        let alice_shared = alice.ecdh(&bob.public_key).unwrap();
        let bob_shared = bob.ecdh(&alice.public_key).unwrap();

        assert_eq!(alice_shared, bob_shared);
    }

    #[test]
    fn test_sign_and_verify() {
        let kp = P256KeyPair::generate().unwrap();
        let message = b"test message";

        let sig = P256Signature::sign_message(&kp.private_key, message).unwrap();
        assert!(sig.verify_message(&kp.public_key, message));

        // Wrong message should fail
        assert!(!sig.verify_message(&kp.public_key, b"wrong message"));
    }

    #[test]
    fn test_signature_der_roundtrip() {
        let kp = P256KeyPair::generate().unwrap();
        let message = b"test";

        let sig = P256Signature::sign_message(&kp.private_key, message).unwrap();
        let der = sig.to_der();
        let sig2 = P256Signature::from_der(&der).unwrap();

        assert_eq!(sig.r, sig2.r);
        assert_eq!(sig.s, sig2.s);
    }
}
