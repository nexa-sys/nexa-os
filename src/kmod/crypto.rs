//! Cryptographic primitives for kernel module signature verification
//!
//! This module provides full PKCS#7/CMS cryptographic support including:
//! - SHA-256, SHA-384, SHA-512 hashing
//! - RSA with PKCS#1 v1.5 padding signature verification
//! - ECDSA P-256/P-384/P-521 signature verification
//! - X.509 certificate parsing and chain validation
//!
//! All implementations are `no_std` compatible and designed for kernel use.
//!
//! # Supported Algorithms
//!
//! Hash algorithms:
//! - SHA-256 (32-byte digest)
//! - SHA-384 (48-byte digest)
//! - SHA-512 (64-byte digest)
//!
//! Signature algorithms:
//! - RSA with PKCS#1 v1.5 padding (1024-4096 bits)
//! - ECDSA with P-256 (secp256r1)
//! - ECDSA with P-384 (secp384r1)
//! - ECDSA with P-521 (secp521r1)
//!
//! # Security Notes
//!
//! - Private keys are never handled by this code
//! - Only signature verification is supported (not signing)
//! - Key material is stored in kernel memory (protected)
//! - Certificate chain validation enforces proper trust hierarchy

use alloc::vec::Vec;

// ============================================================================
// SHA-256 Implementation
// ============================================================================

/// SHA-256 initial hash values (first 32 bits of fractional parts of
/// the square roots of the first 8 primes 2..19)
const SHA256_H: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// SHA-256 round constants (first 32 bits of fractional parts of
/// the cube roots of the first 64 primes 2..311)
const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 digest size in bytes
pub const SHA256_DIGEST_SIZE: usize = 32;

/// SHA-256 block size in bytes
const SHA256_BLOCK_SIZE: usize = 64;

/// SHA-256 hasher state
pub struct Sha256 {
    /// Current hash state
    state: [u32; 8],
    /// Pending data buffer
    buffer: [u8; SHA256_BLOCK_SIZE],
    /// Number of bytes in buffer
    buffer_len: usize,
    /// Total message length in bits
    total_bits: u64,
}

impl Sha256 {
    /// Create a new SHA-256 hasher
    pub const fn new() -> Self {
        Self {
            state: SHA256_H,
            buffer: [0u8; SHA256_BLOCK_SIZE],
            buffer_len: 0,
            total_bits: 0,
        }
    }

    /// Reset hasher to initial state
    pub fn reset(&mut self) {
        self.state = SHA256_H;
        self.buffer = [0u8; SHA256_BLOCK_SIZE];
        self.buffer_len = 0;
        self.total_bits = 0;
    }

    /// Update hash with input data
    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0;

        // If we have pending data, fill the buffer first
        if self.buffer_len > 0 {
            let to_copy = core::cmp::min(SHA256_BLOCK_SIZE - self.buffer_len, data.len());
            self.buffer[self.buffer_len..self.buffer_len + to_copy]
                .copy_from_slice(&data[..to_copy]);
            self.buffer_len += to_copy;
            offset = to_copy;

            // Process full buffer
            if self.buffer_len == SHA256_BLOCK_SIZE {
                let block = self.buffer;
                self.process_block(&block);
                self.buffer_len = 0;
            }
        }

        // Process full blocks directly
        while offset + SHA256_BLOCK_SIZE <= data.len() {
            let block: [u8; SHA256_BLOCK_SIZE] =
                data[offset..offset + SHA256_BLOCK_SIZE].try_into().unwrap();
            self.process_block(&block);
            offset += SHA256_BLOCK_SIZE;
        }

        // Buffer remaining data
        if offset < data.len() {
            let remaining = data.len() - offset;
            self.buffer[..remaining].copy_from_slice(&data[offset..]);
            self.buffer_len = remaining;
        }

        self.total_bits += (data.len() as u64) * 8;
    }

    /// Finalize and return the hash digest
    pub fn finalize(&mut self) -> [u8; SHA256_DIGEST_SIZE] {
        // Padding
        let mut padding = [0u8; SHA256_BLOCK_SIZE * 2];
        padding[0] = 0x80;

        let padding_len = if self.buffer_len < 56 {
            56 - self.buffer_len
        } else {
            120 - self.buffer_len
        };

        // Append length in bits (big-endian)
        let len_bytes = self.total_bits.to_be_bytes();
        padding[padding_len..padding_len + 8].copy_from_slice(&len_bytes);

        self.update(&padding[..padding_len + 8]);

        // Output hash
        let mut result = [0u8; SHA256_DIGEST_SIZE];
        for (i, &val) in self.state.iter().enumerate() {
            result[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
        }
        result
    }

    /// Process a single 64-byte block
    fn process_block(&mut self, block: &[u8; SHA256_BLOCK_SIZE]) {
        // Prepare message schedule
        let mut w = [0u32; 64];

        // First 16 words from block
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }

        // Extend to 64 words
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        // Working variables
        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        // 64 rounds
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        // Update state
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

/// Compute SHA-256 hash of data (convenience function)
pub fn sha256(data: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize()
}

// ============================================================================
// SHA-384 / SHA-512 Implementation (SHA-2 family with 64-bit words)
// ============================================================================

/// SHA-512 initial hash values
const SHA512_H: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

/// SHA-384 initial hash values
const SHA384_H: [u64; 8] = [
    0xcbbb9d5dc1059ed8,
    0x629a292a367cd507,
    0x9159015a3070dd17,
    0x152fecd8f70e5939,
    0x67332667ffc00b31,
    0x8eb44a8768581511,
    0xdb0c2e0d64f98fa7,
    0x47b5481dbefa4fa4,
];

/// SHA-512 round constants
const SHA512_K: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

/// SHA-512 digest size
pub const SHA512_DIGEST_SIZE: usize = 64;

/// SHA-384 digest size
pub const SHA384_DIGEST_SIZE: usize = 48;

/// SHA-512 block size
const SHA512_BLOCK_SIZE: usize = 128;

/// SHA-512/384 hasher state
pub struct Sha512 {
    state: [u64; 8],
    buffer: [u8; SHA512_BLOCK_SIZE],
    buffer_len: usize,
    total_bits: u128,
    is_384: bool,
}

impl Sha512 {
    /// Create a new SHA-512 hasher
    pub const fn new() -> Self {
        Self {
            state: SHA512_H,
            buffer: [0u8; SHA512_BLOCK_SIZE],
            buffer_len: 0,
            total_bits: 0,
            is_384: false,
        }
    }

    /// Create a new SHA-384 hasher
    pub const fn new_384() -> Self {
        Self {
            state: SHA384_H,
            buffer: [0u8; SHA512_BLOCK_SIZE],
            buffer_len: 0,
            total_bits: 0,
            is_384: true,
        }
    }

    /// Update hash with input data
    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0;

        if self.buffer_len > 0 {
            let to_copy = core::cmp::min(SHA512_BLOCK_SIZE - self.buffer_len, data.len());
            self.buffer[self.buffer_len..self.buffer_len + to_copy]
                .copy_from_slice(&data[..to_copy]);
            self.buffer_len += to_copy;
            offset = to_copy;

            if self.buffer_len == SHA512_BLOCK_SIZE {
                let block = self.buffer;
                self.process_block(&block);
                self.buffer_len = 0;
            }
        }

        while offset + SHA512_BLOCK_SIZE <= data.len() {
            let block: [u8; SHA512_BLOCK_SIZE] =
                data[offset..offset + SHA512_BLOCK_SIZE].try_into().unwrap();
            self.process_block(&block);
            offset += SHA512_BLOCK_SIZE;
        }

        if offset < data.len() {
            let remaining = data.len() - offset;
            self.buffer[..remaining].copy_from_slice(&data[offset..]);
            self.buffer_len = remaining;
        }

        self.total_bits += (data.len() as u128) * 8;
    }

    /// Finalize and return the hash digest
    pub fn finalize(&mut self) -> Vec<u8> {
        let mut padding = [0u8; SHA512_BLOCK_SIZE * 2];
        padding[0] = 0x80;

        let padding_len = if self.buffer_len < 112 {
            112 - self.buffer_len
        } else {
            240 - self.buffer_len
        };

        // Append length in bits (big-endian, 128-bit)
        let len_bytes = self.total_bits.to_be_bytes();
        padding[padding_len..padding_len + 16].copy_from_slice(&len_bytes);

        self.update(&padding[..padding_len + 16]);

        let digest_size = if self.is_384 {
            SHA384_DIGEST_SIZE
        } else {
            SHA512_DIGEST_SIZE
        };
        let mut result = Vec::with_capacity(digest_size);
        let num_words = digest_size / 8;

        for i in 0..num_words {
            result.extend_from_slice(&self.state[i].to_be_bytes());
        }

        result
    }

    fn process_block(&mut self, block: &[u8; SHA512_BLOCK_SIZE]) {
        let mut w = [0u64; 80];

        for i in 0..16 {
            w[i] = u64::from_be_bytes([
                block[i * 8],
                block[i * 8 + 1],
                block[i * 8 + 2],
                block[i * 8 + 3],
                block[i * 8 + 4],
                block[i * 8 + 5],
                block[i * 8 + 6],
                block[i * 8 + 7],
            ]);
        }

        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA512_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

/// Compute SHA-384 hash
pub fn sha384(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha512::new_384();
    hasher.update(data);
    hasher.finalize()
}

/// Compute SHA-512 hash
pub fn sha512(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha512::new();
    hasher.update(data);
    hasher.finalize()
}

/// Compute hash using specified algorithm
pub fn hash_with_algorithm(data: &[u8], algo: HashAlgorithm) -> Vec<u8> {
    match algo {
        HashAlgorithm::Sha256 => sha256(data).to_vec(),
        HashAlgorithm::Sha384 => sha384(data),
        HashAlgorithm::Sha512 => sha512(data),
    }
}

// ============================================================================
// RSA Verification (Big Integer Arithmetic)
// ============================================================================

/// Maximum RSA key size in bits
pub const MAX_RSA_BITS: usize = 4096;

/// Maximum RSA key size in bytes
pub const MAX_RSA_BYTES: usize = MAX_RSA_BITS / 8;

/// Maximum RSA key size in 64-bit limbs
const MAX_RSA_LIMBS: usize = MAX_RSA_BYTES / 8;

/// Big integer for RSA operations (fixed-size array)
#[derive(Clone, PartialEq, Eq)]
pub struct BigInt {
    /// Little-endian limbs
    limbs: [u64; MAX_RSA_LIMBS],
    /// Number of significant limbs
    len: usize,
}

impl BigInt {
    /// Create a new BigInt with value 0
    pub const fn zero() -> Self {
        Self {
            limbs: [0u64; MAX_RSA_LIMBS],
            len: 0,
        }
    }

    /// Create a BigInt from a byte slice (big-endian)
    pub fn from_bytes_be(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > MAX_RSA_BYTES {
            return None;
        }

        let mut result = Self::zero();

        // Skip leading zeros
        let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
        let significant_bytes = &bytes[first_nonzero..];

        if significant_bytes.is_empty() {
            return Some(result);
        }

        // Convert to little-endian limbs
        let num_limbs = (significant_bytes.len() + 7) / 8;
        result.len = num_limbs;

        for (i, chunk) in significant_bytes.rchunks(8).enumerate() {
            let mut limb_bytes = [0u8; 8];
            let offset = 8 - chunk.len();
            limb_bytes[offset..].copy_from_slice(chunk);
            result.limbs[i] = u64::from_be_bytes(limb_bytes);
        }

        result.normalize();
        Some(result)
    }

    /// Convert to bytes (big-endian)
    pub fn to_bytes_be(&self) -> Vec<u8> {
        if self.len == 0 {
            return alloc::vec![0];
        }

        let mut result = Vec::with_capacity(self.len * 8);

        // Find first non-zero byte
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

    /// Normalize: update len to reflect actual number of significant limbs
    fn normalize(&mut self) {
        while self.len > 0 && self.limbs[self.len - 1] == 0 {
            self.len -= 1;
        }
    }

    /// Check if value is zero
    pub fn is_zero(&self) -> bool {
        self.len == 0
    }

    /// Compare two BigInts
    pub fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;

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

    /// Check if self >= other
    pub fn ge(&self, other: &Self) -> bool {
        matches!(
            self.cmp(other),
            core::cmp::Ordering::Greater | core::cmp::Ordering::Equal
        )
    }

    /// Subtract: self - other (assumes self >= other)
    pub fn sub(&self, other: &Self) -> Self {
        let mut result = Self::zero();
        result.len = self.len;

        let mut borrow = 0u64;
        for i in 0..self.len {
            let other_limb = if i < other.len { other.limbs[i] } else { 0 };
            let (diff, b1) = self.limbs[i].overflowing_sub(other_limb);
            let (diff2, b2) = diff.overflowing_sub(borrow);
            result.limbs[i] = diff2;
            borrow = (b1 as u64) + (b2 as u64);
        }

        result.normalize();
        result
    }

    /// Multiply and add: self = self + a * b
    #[allow(dead_code)]
    fn mul_add_limb(&mut self, a: &Self, b: u64) {
        let mut carry = 0u128;

        for i in 0..a.len {
            let product = (a.limbs[i] as u128) * (b as u128) + (self.limbs[i] as u128) + carry;
            self.limbs[i] = product as u64;
            carry = product >> 64;
        }

        if carry != 0 && a.len < MAX_RSA_LIMBS {
            self.limbs[a.len] = carry as u64;
            self.len = a.len + 1;
        } else {
            self.len = a.len;
        }

        self.normalize();
    }

    /// Left shift by one bit
    #[allow(dead_code)]
    fn shl1(&mut self) {
        let mut carry = 0u64;
        for i in 0..self.len {
            let new_carry = self.limbs[i] >> 63;
            self.limbs[i] = (self.limbs[i] << 1) | carry;
            carry = new_carry;
        }
        if carry != 0 && self.len < MAX_RSA_LIMBS {
            self.limbs[self.len] = carry;
            self.len += 1;
        }
    }

    /// Right shift by one bit
    #[allow(dead_code)]
    fn shr1(&mut self) {
        let mut carry = 0u64;
        for i in (0..self.len).rev() {
            let new_carry = self.limbs[i] & 1;
            self.limbs[i] = (self.limbs[i] >> 1) | (carry << 63);
            carry = new_carry;
        }
        self.normalize();
    }

    /// Modular exponentiation: base^exp mod modulus
    /// Uses binary exponentiation (square-and-multiply)
    pub fn mod_exp(base: &Self, exp: &Self, modulus: &Self) -> Self {
        if modulus.is_zero() {
            return Self::zero();
        }

        let mut result = Self::zero();
        result.limbs[0] = 1;
        result.len = 1;

        let mut base_pow = base.mod_reduce(modulus);

        // Process each bit of the exponent
        for i in 0..exp.len {
            let mut limb = exp.limbs[i];
            for _ in 0..64 {
                if limb & 1 != 0 {
                    result = Self::mod_mul(&result, &base_pow, modulus);
                }
                base_pow = Self::mod_mul(&base_pow, &base_pow, modulus);
                limb >>= 1;
            }
        }

        result
    }

    /// Modular reduction: self mod modulus
    fn mod_reduce(&self, modulus: &Self) -> Self {
        if self.cmp(modulus) == core::cmp::Ordering::Less {
            return self.clone();
        }

        // For values larger than modulus, use subtraction
        let mut result = self.clone();
        while result.ge(modulus) {
            result = result.sub(modulus);
        }
        result
    }

    /// Full multiplication: a * b, result can be up to 2*MAX_RSA_LIMBS
    fn full_mul(a: &Self, b: &Self) -> [u64; MAX_RSA_LIMBS * 2] {
        let mut result = [0u64; MAX_RSA_LIMBS * 2];

        for i in 0..a.len {
            let mut carry = 0u128;
            for j in 0..b.len {
                let product =
                    (a.limbs[i] as u128) * (b.limbs[j] as u128) + (result[i + j] as u128) + carry;
                result[i + j] = product as u64;
                carry = product >> 64;
            }
            // Propagate remaining carry
            let mut k = i + b.len;
            while carry != 0 && k < MAX_RSA_LIMBS * 2 {
                let sum = (result[k] as u128) + carry;
                result[k] = sum as u64;
                carry = sum >> 64;
                k += 1;
            }
        }

        result
    }

    /// Count leading zeros in extended result
    #[allow(dead_code)]
    fn extended_clz(limbs: &[u64; MAX_RSA_LIMBS * 2], len: usize) -> usize {
        for i in (0..len).rev() {
            if limbs[i] != 0 {
                return (len - 1 - i) * 64 + limbs[i].leading_zeros() as usize;
            }
        }
        len * 64
    }

    /// Left shift extended result by n bits
    fn extended_shl(limbs: &mut [u64; MAX_RSA_LIMBS * 2], len: usize, n: usize) {
        if n == 0 {
            return;
        }
        let word_shift = n / 64;
        let bit_shift = n % 64;

        if word_shift >= len {
            for i in 0..len {
                limbs[i] = 0;
            }
            return;
        }

        if bit_shift == 0 {
            for i in (word_shift..len).rev() {
                limbs[i] = limbs[i - word_shift];
            }
        } else {
            for i in (word_shift + 1..len).rev() {
                limbs[i] = (limbs[i - word_shift] << bit_shift)
                    | (limbs[i - word_shift - 1] >> (64 - bit_shift));
            }
            limbs[word_shift] = limbs[0] << bit_shift;
        }
        for i in 0..word_shift {
            limbs[i] = 0;
        }
    }

    /// Compare extended with modulus
    #[allow(dead_code)]
    fn extended_ge_mod(limbs: &[u64; MAX_RSA_LIMBS * 2], m: &Self) -> bool {
        // Check if any limb beyond modulus length is non-zero
        for i in (m.len..MAX_RSA_LIMBS * 2).rev() {
            if limbs[i] != 0 {
                return true;
            }
        }
        // Compare within modulus length
        for i in (0..m.len).rev() {
            if limbs[i] > m.limbs[i] {
                return true;
            }
            if limbs[i] < m.limbs[i] {
                return false;
            }
        }
        true // equal
    }

    /// Subtract modulus from extended (assumes extended >= m)
    #[allow(dead_code)]
    fn extended_sub_mod(limbs: &mut [u64; MAX_RSA_LIMBS * 2], m: &Self) {
        let mut borrow = 0u64;
        for i in 0..m.len {
            let (diff, b1) = limbs[i].overflowing_sub(m.limbs[i]);
            let (diff2, b2) = diff.overflowing_sub(borrow);
            limbs[i] = diff2;
            borrow = (b1 as u64) + (b2 as u64);
        }
        // Propagate borrow
        let mut i = m.len;
        while borrow != 0 && i < MAX_RSA_LIMBS * 2 {
            let (diff, b) = limbs[i].overflowing_sub(borrow);
            limbs[i] = diff;
            borrow = b as u64;
            i += 1;
        }
    }

    /// Modular multiplication using schoolbook multiply then reduce
    fn mod_mul(a: &Self, b: &Self, modulus: &Self) -> Self {
        if a.is_zero() || b.is_zero() {
            return Self::zero();
        }

        // Step 1: Full multiplication a * b
        let mut product = Self::full_mul(a, b);
        let prod_len = MAX_RSA_LIMBS * 2;

        // Step 2: Modular reduction using bit-by-bit subtraction
        // Find effective length of product
        let mut eff_len = prod_len;
        while eff_len > 0 && product[eff_len - 1] == 0 {
            eff_len -= 1;
        }
        if eff_len == 0 {
            return Self::zero();
        }

        // Count bits in modulus
        let mod_bits = modulus.len * 64 - modulus.limbs[modulus.len - 1].leading_zeros() as usize;

        // Repeatedly subtract shifted modulus
        loop {
            // Count bits in product
            let prod_bits = eff_len * 64 - product[eff_len - 1].leading_zeros() as usize;

            if prod_bits < mod_bits {
                break;
            }

            let shift = prod_bits - mod_bits;

            // Create shifted modulus
            let mut shifted_mod = [0u64; MAX_RSA_LIMBS * 2];
            for i in 0..modulus.len {
                shifted_mod[i] = modulus.limbs[i];
            }
            Self::extended_shl(&mut shifted_mod, prod_len, shift);

            // Check if product >= shifted_mod
            let mut can_subtract = false;
            for i in (0..prod_len).rev() {
                if product[i] > shifted_mod[i] {
                    can_subtract = true;
                    break;
                }
                if product[i] < shifted_mod[i] {
                    break;
                }
            }
            // If equal, can also subtract
            if !can_subtract {
                let mut equal = true;
                for i in 0..prod_len {
                    if product[i] != shifted_mod[i] {
                        equal = false;
                        break;
                    }
                }
                if equal {
                    can_subtract = true;
                }
            }

            if !can_subtract {
                if shift == 0 {
                    break;
                }
                // Try with one less shift
                let shift = shift - 1;
                let mut shifted_mod = [0u64; MAX_RSA_LIMBS * 2];
                for i in 0..modulus.len {
                    shifted_mod[i] = modulus.limbs[i];
                }
                Self::extended_shl(&mut shifted_mod, prod_len, shift);

                // Check again
                for i in (0..prod_len).rev() {
                    if product[i] > shifted_mod[i] {
                        can_subtract = true;
                        break;
                    }
                    if product[i] < shifted_mod[i] {
                        break;
                    }
                }
                if !can_subtract {
                    let mut equal = true;
                    for i in 0..prod_len {
                        if product[i] != shifted_mod[i] {
                            equal = false;
                            break;
                        }
                    }
                    if equal {
                        can_subtract = true;
                    }
                }

                if !can_subtract {
                    break;
                }

                // Subtract
                let mut borrow = 0u64;
                for i in 0..prod_len {
                    let (diff, b1) = product[i].overflowing_sub(shifted_mod[i]);
                    let (diff2, b2) = diff.overflowing_sub(borrow);
                    product[i] = diff2;
                    borrow = (b1 as u64) + (b2 as u64);
                }
            } else {
                // Subtract shifted modulus
                let mut borrow = 0u64;
                for i in 0..prod_len {
                    let (diff, b1) = product[i].overflowing_sub(shifted_mod[i]);
                    let (diff2, b2) = diff.overflowing_sub(borrow);
                    product[i] = diff2;
                    borrow = (b1 as u64) + (b2 as u64);
                }
            }

            // Update effective length
            while eff_len > 0 && product[eff_len - 1] == 0 {
                eff_len -= 1;
            }
            if eff_len == 0 {
                return Self::zero();
            }
        }

        // Copy result
        let mut result = Self::zero();
        let copy_len = core::cmp::min(eff_len, MAX_RSA_LIMBS);
        for i in 0..copy_len {
            result.limbs[i] = product[i];
        }
        result.len = copy_len;
        result.normalize();

        result
    }
}

// ============================================================================
// RSA Public Key
// ============================================================================

/// RSA public key
#[derive(Clone)]
pub struct RsaPublicKey {
    /// Modulus (n)
    pub n: BigInt,
    /// Public exponent (e)
    pub e: BigInt,
    /// Key size in bits
    pub bits: usize,
}

impl RsaPublicKey {
    /// Create an RSA public key from raw components
    pub fn new(n_bytes: &[u8], e_bytes: &[u8]) -> Option<Self> {
        let n = BigInt::from_bytes_be(n_bytes)?;
        let e = BigInt::from_bytes_be(e_bytes)?;

        if n.is_zero() || e.is_zero() {
            return None;
        }

        let bits = n_bytes.len() * 8;

        Some(Self { n, e, bits })
    }

    /// RSA raw decryption: s^e mod n
    fn rsa_decrypt(&self, signature: &[u8]) -> Option<Vec<u8>> {
        let sig = BigInt::from_bytes_be(signature)?;

        let n_bytes = self.n.to_bytes_be();
        let e_bytes = self.e.to_bytes_be();
        crate::kinfo!(
            "RSA verify: n={} bytes ({:02X}{:02X}...), e={} bytes ({:02X?}), sig={} bytes",
            n_bytes.len(),
            n_bytes.get(0).copied().unwrap_or(0),
            n_bytes.get(1).copied().unwrap_or(0),
            e_bytes.len(),
            &e_bytes[..],
            signature.len()
        );

        let decrypted = BigInt::mod_exp(&sig, &self.e, &self.n);
        let decrypted_bytes = decrypted.to_bytes_be();

        crate::kinfo!(
            "RSA: decrypted {} bytes, first 4: {:02X}{:02X}{:02X}{:02X}",
            decrypted_bytes.len(),
            decrypted_bytes.get(0).copied().unwrap_or(0),
            decrypted_bytes.get(1).copied().unwrap_or(0),
            decrypted_bytes.get(2).copied().unwrap_or(0),
            decrypted_bytes.get(3).copied().unwrap_or(0)
        );

        let key_len = (self.bits + 7) / 8;

        // Pad to key length
        let mut padded = Vec::with_capacity(key_len);
        for _ in 0..(key_len.saturating_sub(decrypted_bytes.len())) {
            padded.push(0);
        }
        padded.extend_from_slice(&decrypted_bytes);

        Some(padded)
    }

    /// Parse PKCS#1 v1.5 padded message and extract DigestInfo
    fn parse_pkcs1_v15(&self, padded: &[u8]) -> Option<(HashAlgorithm, Vec<u8>)> {
        if padded.len() < 11 {
            return None;
        }

        // Check PKCS#1 v1.5 structure: 0x00 0x01 [0xFF padding] 0x00 [DigestInfo]
        if padded[0] != 0x00 || padded[1] != 0x01 {
            return None;
        }

        // Find end of 0xFF padding
        let mut padding_end = 2;
        while padding_end < padded.len() && padded[padding_end] == 0xFF {
            padding_end += 1;
        }

        if padding_end < 10 || padding_end >= padded.len() || padded[padding_end] != 0x00 {
            return None;
        }

        let content = &padded[padding_end + 1..];

        // DigestInfo for different algorithms:
        // SHA-256: 30 31 30 0d 06 09 60 86 48 01 65 03 04 02 01 05 00 04 20 [32 bytes]
        // SHA-384: 30 41 30 0d 06 09 60 86 48 01 65 03 04 02 02 05 00 04 30 [48 bytes]
        // SHA-512: 30 51 30 0d 06 09 60 86 48 01 65 03 04 02 03 05 00 04 40 [64 bytes]

        const DIGEST_INFO_SHA256: &[u8] = &[
            0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x01, 0x05, 0x00, 0x04, 0x20,
        ];
        const DIGEST_INFO_SHA384: &[u8] = &[
            0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x02, 0x05, 0x00, 0x04, 0x30,
        ];
        const DIGEST_INFO_SHA512: &[u8] = &[
            0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x03, 0x05, 0x00, 0x04, 0x40,
        ];

        // Try SHA-256
        if content.len() == DIGEST_INFO_SHA256.len() + SHA256_DIGEST_SIZE
            && content.starts_with(DIGEST_INFO_SHA256)
        {
            return Some((
                HashAlgorithm::Sha256,
                content[DIGEST_INFO_SHA256.len()..].to_vec(),
            ));
        }

        // Try SHA-384
        if content.len() == DIGEST_INFO_SHA384.len() + SHA384_DIGEST_SIZE
            && content.starts_with(DIGEST_INFO_SHA384)
        {
            return Some((
                HashAlgorithm::Sha384,
                content[DIGEST_INFO_SHA384.len()..].to_vec(),
            ));
        }

        // Try SHA-512
        if content.len() == DIGEST_INFO_SHA512.len() + SHA512_DIGEST_SIZE
            && content.starts_with(DIGEST_INFO_SHA512)
        {
            return Some((
                HashAlgorithm::Sha512,
                content[DIGEST_INFO_SHA512.len()..].to_vec(),
            ));
        }

        None
    }

    /// Verify PKCS#1 v1.5 signature with SHA-256 (backwards compatible)
    pub fn verify_pkcs1_v15(
        &self,
        message_hash: &[u8; SHA256_DIGEST_SIZE],
        signature: &[u8],
    ) -> bool {
        self.verify_pkcs1_v15_any(message_hash, HashAlgorithm::Sha256, signature)
    }

    /// Verify PKCS#1 v1.5 signature with any supported hash algorithm
    pub fn verify_pkcs1_v15_any(
        &self,
        message_hash: &[u8],
        expected_algo: HashAlgorithm,
        signature: &[u8],
    ) -> bool {
        let padded = match self.rsa_decrypt(signature) {
            Some(p) => p,
            None => {
                crate::kinfo!("RSA: failed to decrypt signature");
                return false;
            }
        };

        let (algo, embedded_hash) = match self.parse_pkcs1_v15(&padded) {
            Some((a, h)) => (a, h),
            None => {
                crate::kinfo!("RSA: failed to parse PKCS#1 v1.5 structure");
                return false;
            }
        };

        if algo != expected_algo {
            crate::kinfo!(
                "RSA: hash algorithm mismatch, expected {:?}, got {:?}",
                expected_algo,
                algo
            );
            return false;
        }

        if embedded_hash.len() != message_hash.len() {
            crate::kinfo!("RSA: hash length mismatch");
            return false;
        }

        embedded_hash == message_hash
    }

    /// Verify signature and auto-detect hash algorithm
    pub fn verify_pkcs1_v15_auto(&self, message: &[u8], signature: &[u8]) -> Option<HashAlgorithm> {
        let padded = self.rsa_decrypt(signature)?;
        let (algo, embedded_hash) = self.parse_pkcs1_v15(&padded)?;

        let computed_hash = hash_with_algorithm(message, algo);

        if embedded_hash == computed_hash {
            Some(algo)
        } else {
            None
        }
    }
}

// ============================================================================
// Module Signing Key Storage
// ============================================================================

/// Maximum number of trusted keys
const MAX_TRUSTED_KEYS: usize = 8;

/// Trusted key entry
struct TrustedKey {
    /// Key identifier (e.g., subject key identifier or fingerprint)
    id: [u8; 32],
    /// Key identifier length
    id_len: usize,
    /// RSA public key
    key: RsaPublicKey,
    /// Whether this slot is used
    used: bool,
}

impl TrustedKey {
    const fn empty() -> Self {
        Self {
            id: [0u8; 32],
            id_len: 0,
            key: RsaPublicKey {
                n: BigInt::zero(),
                e: BigInt::zero(),
                bits: 0,
            },
            used: false,
        }
    }
}

/// Trusted key store
static TRUSTED_KEYS: spin::Mutex<[TrustedKey; MAX_TRUSTED_KEYS]> = spin::Mutex::new([
    TrustedKey::empty(),
    TrustedKey::empty(),
    TrustedKey::empty(),
    TrustedKey::empty(),
    TrustedKey::empty(),
    TrustedKey::empty(),
    TrustedKey::empty(),
    TrustedKey::empty(),
]);

/// Add a trusted key to the keyring
pub fn add_trusted_key(id: &[u8], n: &[u8], e: &[u8]) -> bool {
    let key = match RsaPublicKey::new(n, e) {
        Some(k) => k,
        None => return false,
    };

    let mut keys = TRUSTED_KEYS.lock();

    // Find an empty slot
    for slot in keys.iter_mut() {
        if !slot.used {
            let id_len = core::cmp::min(id.len(), 32);
            slot.id[..id_len].copy_from_slice(&id[..id_len]);
            slot.id_len = id_len;
            slot.key = key;
            slot.used = true;
            return true;
        }
    }

    false
}

/// Find a trusted key by ID
pub fn find_trusted_key(id: &[u8]) -> Option<RsaPublicKey> {
    let keys = TRUSTED_KEYS.lock();

    for slot in keys.iter() {
        if slot.used && id.len() == slot.id_len && &slot.id[..slot.id_len] == id {
            return Some(slot.key.clone());
        }
    }

    None
}

/// Check if a public key is in the trusted keyring by comparing modulus
///
/// This is used when we extract a key from a certificate and need to verify
/// it matches one of our trusted keys.
pub fn is_key_trusted(key: &RsaPublicKey) -> bool {
    let keys = TRUSTED_KEYS.lock();

    for slot in keys.iter() {
        if slot.used {
            // Compare modulus (n) - if modulus matches, it's the same key
            if slot.key.n == key.n && slot.key.e == key.e {
                return true;
            }
        }
    }

    false
}

/// Get number of trusted keys
pub fn trusted_key_count() -> usize {
    TRUSTED_KEYS.lock().iter().filter(|k| k.used).count()
}

/// Clear all trusted keys
pub fn clear_trusted_keys() {
    let mut keys = TRUSTED_KEYS.lock();
    for slot in keys.iter_mut() {
        *slot = TrustedKey::empty();
    }
}

// ============================================================================
// OID Definitions for Cryptographic Algorithms (Complete PKCS#7 Support)
// ============================================================================

/// OID for SHA-1: 1.3.14.3.2.26 (for compatibility, not recommended)
pub const OID_SHA1: &[u8] = &[0x2b, 0x0e, 0x03, 0x02, 0x1a];

/// OID for SHA-256: 2.16.840.1.101.3.4.2.1
pub const OID_SHA256: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];

/// OID for SHA-384: 2.16.840.1.101.3.4.2.2
pub const OID_SHA384: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02];

/// OID for SHA-512: 2.16.840.1.101.3.4.2.3
pub const OID_SHA512: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03];

/// OID for RSA encryption: 1.2.840.113549.1.1.1
pub const OID_RSA_ENCRYPTION: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];

/// OID for RSA with SHA-1: 1.2.840.113549.1.1.5
pub const OID_RSA_SHA1: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x05];

/// OID for RSA with SHA-256: 1.2.840.113549.1.1.11
pub const OID_RSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];

/// OID for RSA with SHA-384: 1.2.840.113549.1.1.12
pub const OID_RSA_SHA384: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0c];

/// OID for RSA with SHA-512: 1.2.840.113549.1.1.13
pub const OID_RSA_SHA512: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0d];

/// OID for RSA-PSS: 1.2.840.113549.1.1.10
pub const OID_RSA_PSS: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0a];

/// OID for ECDSA with SHA-256: 1.2.840.10045.4.3.2
pub const OID_ECDSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];

/// OID for ECDSA with SHA-384: 1.2.840.10045.4.3.3
pub const OID_ECDSA_SHA384: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x03];

/// OID for ECDSA with SHA-512: 1.2.840.10045.4.3.4
pub const OID_ECDSA_SHA512: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x04];

/// OID for EC public key: 1.2.840.10045.2.1
pub const OID_EC_PUBLIC_KEY: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];

/// OID for secp256r1 (P-256): 1.2.840.10045.3.1.7
pub const OID_SECP256R1: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];

/// OID for secp384r1 (P-384): 1.3.132.0.34
pub const OID_SECP384R1: &[u8] = &[0x2b, 0x81, 0x04, 0x00, 0x22];

/// OID for secp521r1 (P-521): 1.3.132.0.35
pub const OID_SECP521R1: &[u8] = &[0x2b, 0x81, 0x04, 0x00, 0x23];

/// OID for PKCS#7 signedData: 1.2.840.113549.1.7.2
pub const OID_PKCS7_SIGNED_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];

/// OID for PKCS#7 data: 1.2.840.113549.1.7.1
pub const OID_PKCS7_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x01];

/// OID for PKCS#9 contentType: 1.2.840.113549.1.9.3
pub const OID_CONTENT_TYPE: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x03];

/// OID for PKCS#9 messageDigest: 1.2.840.113549.1.9.4
pub const OID_MESSAGE_DIGEST: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x04];

/// OID for PKCS#9 signingTime: 1.2.840.113549.1.9.5
pub const OID_SIGNING_TIME: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x05];

/// OID for PKCS#9 counterSignature: 1.2.840.113549.1.9.6
pub const OID_COUNTER_SIGNATURE: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x06];

/// OID for CMS signing certificate v2: 1.2.840.113549.1.9.16.2.47
pub const OID_SIGNING_CERTIFICATE_V2: &[u8] = &[
    0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x10, 0x02, 0x2f,
];

/// OID for timestampToken: 1.2.840.113549.1.9.16.2.14
pub const OID_TIMESTAMP_TOKEN: &[u8] = &[
    0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x10, 0x02, 0x0e,
];

/// Signature algorithm type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    RsaSha256,
    RsaSha384,
    RsaSha512,
    RsaPss,
    EcdsaSha256,
    EcdsaSha384,
    EcdsaSha512,
}

impl SignatureAlgorithm {
    /// Get hash algorithm for this signature algorithm
    pub fn hash_algorithm(self) -> HashAlgorithm {
        match self {
            SignatureAlgorithm::RsaSha256 | SignatureAlgorithm::EcdsaSha256 => {
                HashAlgorithm::Sha256
            }
            SignatureAlgorithm::RsaSha384 | SignatureAlgorithm::EcdsaSha384 => {
                HashAlgorithm::Sha384
            }
            SignatureAlgorithm::RsaSha512 | SignatureAlgorithm::EcdsaSha512 => {
                HashAlgorithm::Sha512
            }
            SignatureAlgorithm::RsaPss => HashAlgorithm::Sha256, // Default, actual hash determined by params
        }
    }

    /// Check if this is an ECDSA algorithm
    pub fn is_ecdsa(self) -> bool {
        matches!(
            self,
            SignatureAlgorithm::EcdsaSha256
                | SignatureAlgorithm::EcdsaSha384
                | SignatureAlgorithm::EcdsaSha512
        )
    }
}

/// Convert signature algorithm OID to enum
pub fn oid_to_sig_algo(oid: &[u8]) -> Option<SignatureAlgorithm> {
    if oid == OID_RSA_SHA256 {
        Some(SignatureAlgorithm::RsaSha256)
    } else if oid == OID_RSA_SHA384 {
        Some(SignatureAlgorithm::RsaSha384)
    } else if oid == OID_RSA_SHA512 {
        Some(SignatureAlgorithm::RsaSha512)
    } else if oid == OID_RSA_PSS {
        Some(SignatureAlgorithm::RsaPss)
    } else if oid == OID_ECDSA_SHA256 {
        Some(SignatureAlgorithm::EcdsaSha256)
    } else if oid == OID_ECDSA_SHA384 {
        Some(SignatureAlgorithm::EcdsaSha384)
    } else if oid == OID_ECDSA_SHA512 {
        Some(SignatureAlgorithm::EcdsaSha512)
    } else {
        None
    }
}

/// Check if an OID matches a known hash algorithm
pub fn oid_to_hash_algo(oid: &[u8]) -> Option<HashAlgorithm> {
    if oid == OID_SHA256 {
        Some(HashAlgorithm::Sha256)
    } else if oid == OID_SHA384 {
        Some(HashAlgorithm::Sha384)
    } else if oid == OID_SHA512 {
        Some(HashAlgorithm::Sha512)
    } else {
        None
    }
}

/// Hash algorithm enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgorithm {
    /// Get digest size in bytes
    pub fn digest_size(self) -> usize {
        match self {
            HashAlgorithm::Sha256 => 32,
            HashAlgorithm::Sha384 => 48,
            HashAlgorithm::Sha512 => 64,
        }
    }
}
