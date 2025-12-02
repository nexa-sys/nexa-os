//! Cryptographic primitives for kernel module signature verification
//!
//! This module provides SHA-256 hashing and RSA signature verification
//! for PKCS#7 signed kernel modules. All implementations are `no_std`
//! compatible and do not require heap allocation for core operations.
//!
//! # Supported Algorithms
//!
//! - SHA-256: For message digest computation
//! - RSA with PKCS#1 v1.5 padding: For signature verification
//!
//! # Security Notes
//!
//! - Private keys are never handled by this code
//! - Only signature verification is supported (not signing)
//! - Key material is stored in kernel memory (protected)

use alloc::vec::Vec;

// ============================================================================
// SHA-256 Implementation
// ============================================================================

/// SHA-256 initial hash values (first 32 bits of fractional parts of
/// the square roots of the first 8 primes 2..19)
const SHA256_H: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
    0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// SHA-256 round constants (first 32 bits of fractional parts of
/// the cube roots of the first 64 primes 2..311)
const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
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
            let block: [u8; SHA256_BLOCK_SIZE] = data[offset..offset + SHA256_BLOCK_SIZE]
                .try_into()
                .unwrap();
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
// RSA Verification (Big Integer Arithmetic)
// ============================================================================

/// Maximum RSA key size in bits
pub const MAX_RSA_BITS: usize = 4096;

/// Maximum RSA key size in bytes
pub const MAX_RSA_BYTES: usize = MAX_RSA_BITS / 8;

/// Maximum RSA key size in 64-bit limbs
const MAX_RSA_LIMBS: usize = MAX_RSA_BYTES / 8;

/// Big integer for RSA operations (fixed-size array)
#[derive(Clone)]
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
        matches!(self.cmp(other), core::cmp::Ordering::Greater | core::cmp::Ordering::Equal)
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
        
        // Simple repeated subtraction for now
        // (A proper implementation would use Montgomery reduction)
        let mut result = self.clone();
        while result.ge(modulus) {
            result = result.sub(modulus);
        }
        result
    }

    /// Modular multiplication: (a * b) mod modulus
    fn mod_mul(a: &Self, b: &Self, modulus: &Self) -> Self {
        // Use schoolbook multiplication followed by reduction
        let mut result = Self::zero();
        
        for i in (0..b.len).rev() {
            let limb = b.limbs[i];
            for bit in (0..64).rev() {
                result.shl1();
                if result.ge(modulus) {
                    result = result.sub(modulus);
                }
                
                if (limb >> bit) & 1 != 0 {
                    // Add a
                    let mut carry = 0u64;
                    let max_len = core::cmp::max(result.len, a.len);
                    for j in 0..max_len {
                        let r = if j < result.len { result.limbs[j] } else { 0 };
                        let av = if j < a.len { a.limbs[j] } else { 0 };
                        let (sum, c1) = r.overflowing_add(av);
                        let (sum2, c2) = sum.overflowing_add(carry);
                        result.limbs[j] = sum2;
                        carry = (c1 as u64) + (c2 as u64);
                    }
                    if carry != 0 && max_len < MAX_RSA_LIMBS {
                        result.limbs[max_len] = carry;
                        result.len = max_len + 1;
                    } else {
                        result.len = max_len;
                    }
                    result.normalize();
                    
                    if result.ge(modulus) {
                        result = result.sub(modulus);
                    }
                }
            }
        }
        
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

    /// Verify PKCS#1 v1.5 signature
    pub fn verify_pkcs1_v15(&self, message_hash: &[u8; SHA256_DIGEST_SIZE], signature: &[u8]) -> bool {
        // Convert signature to BigInt
        let sig = match BigInt::from_bytes_be(signature) {
            Some(s) => s,
            None => return false,
        };
        
        // RSA verification: m = s^e mod n
        let decrypted = BigInt::mod_exp(&sig, &self.e, &self.n);
        let decrypted_bytes = decrypted.to_bytes_be();
        
        // Expected PKCS#1 v1.5 structure for SHA-256:
        // 0x00 0x01 [padding 0xFF] 0x00 [DigestInfo] [hash]
        //
        // DigestInfo for SHA-256:
        // 30 31 30 0d 06 09 60 86 48 01 65 03 04 02 01 05 00 04 20
        
        let digest_info_sha256: &[u8] = &[
            0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01,
            0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20,
        ];
        
        let key_len = (self.bits + 7) / 8;
        
        // Pad decrypted bytes to key length
        let mut padded = Vec::with_capacity(key_len);
        for _ in 0..(key_len - decrypted_bytes.len()) {
            padded.push(0);
        }
        padded.extend_from_slice(&decrypted_bytes);
        
        if padded.len() < 11 + digest_info_sha256.len() + SHA256_DIGEST_SIZE {
            return false;
        }
        
        // Check PKCS#1 v1.5 structure
        if padded[0] != 0x00 || padded[1] != 0x01 {
            return false;
        }
        
        // Check padding (0xFF bytes)
        let mut padding_end = 2;
        while padding_end < padded.len() && padded[padding_end] == 0xFF {
            padding_end += 1;
        }
        
        if padding_end < 10 {
            // Minimum 8 bytes of 0xFF padding
            return false;
        }
        
        if padding_end >= padded.len() || padded[padding_end] != 0x00 {
            return false;
        }
        
        let content_start = padding_end + 1;
        let remaining = &padded[content_start..];
        
        // Check DigestInfo
        if remaining.len() != digest_info_sha256.len() + SHA256_DIGEST_SIZE {
            return false;
        }
        
        if &remaining[..digest_info_sha256.len()] != digest_info_sha256 {
            return false;
        }
        
        // Check hash
        let embedded_hash = &remaining[digest_info_sha256.len()..];
        embedded_hash == message_hash
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
static TRUSTED_KEYS: spin::Mutex<[TrustedKey; MAX_TRUSTED_KEYS]> = 
    spin::Mutex::new([
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
// OID Definitions for Cryptographic Algorithms
// ============================================================================

/// OID for SHA-256: 2.16.840.1.101.3.4.2.1
pub const OID_SHA256: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];

/// OID for SHA-384: 2.16.840.1.101.3.4.2.2
pub const OID_SHA384: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02];

/// OID for SHA-512: 2.16.840.1.101.3.4.2.3
pub const OID_SHA512: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03];

/// OID for RSA encryption: 1.2.840.113549.1.1.1
pub const OID_RSA_ENCRYPTION: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];

/// OID for RSA with SHA-256: 1.2.840.113549.1.1.11
pub const OID_RSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];

/// OID for RSA with SHA-384: 1.2.840.113549.1.1.12
pub const OID_RSA_SHA384: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0c];

/// OID for RSA with SHA-512: 1.2.840.113549.1.1.13
pub const OID_RSA_SHA512: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0d];

/// OID for PKCS#7 signedData: 1.2.840.113549.1.7.2
pub const OID_PKCS7_SIGNED_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];

/// OID for PKCS#7 data: 1.2.840.113549.1.7.1
pub const OID_PKCS7_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x01];

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_empty() {
        let hash = sha256(b"");
        let expected: [u8; 32] = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14,
            0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
            0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
            0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_sha256_hello() {
        let hash = sha256(b"hello");
        let expected: [u8; 32] = [
            0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e,
            0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9, 0xe2, 0x9e,
            0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e,
            0x73, 0x04, 0x33, 0x62, 0x93, 0x8b, 0x98, 0x24,
        ];
        assert_eq!(hash, expected);
    }
}
