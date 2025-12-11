//! SHA-3 Hash Functions (FIPS 202)
//!
//! Modern secure hash functions based on the Keccak sponge construction.
//! SHA-3 provides an alternative to SHA-2 with a completely different design.
//!
//! Supported variants:
//! - SHA3-256: 256-bit output (recommended for most uses)
//! - SHA3-384: 384-bit output
//! - SHA3-512: 512-bit output
//!
//! Also includes SHAKE extendable-output functions:
//! - SHAKE128: 128-bit security, variable output
//! - SHAKE256: 256-bit security, variable output

use core::ptr;
use std::vec::Vec;

// ============================================================================
// SHA-3 Constants
// ============================================================================

/// SHA3-256 digest size in bytes
pub const SHA3_256_DIGEST_SIZE: usize = 32;
/// SHA3-384 digest size in bytes
pub const SHA3_384_DIGEST_SIZE: usize = 48;
/// SHA3-512 digest size in bytes
pub const SHA3_512_DIGEST_SIZE: usize = 64;

/// Keccak state width in lanes (64-bit words)
const KECCAK_LANES: usize = 25;
/// Number of Keccak rounds
const KECCAK_ROUNDS: usize = 24;

/// Keccak round constants
const KECCAK_RC: [u64; 24] = [
    0x0000000000000001,
    0x0000000000008082,
    0x800000000000808a,
    0x8000000080008000,
    0x000000000000808b,
    0x0000000080000001,
    0x8000000080008081,
    0x8000000000008009,
    0x000000000000008a,
    0x0000000000000088,
    0x0000000080008009,
    0x000000008000000a,
    0x000000008000808b,
    0x800000000000008b,
    0x8000000000008089,
    0x8000000000008003,
    0x8000000000008002,
    0x8000000000000080,
    0x000000000000800a,
    0x800000008000000a,
    0x8000000080008081,
    0x8000000000008080,
    0x0000000080000001,
    0x8000000080008008,
];

/// Keccak rotation offsets
const KECCAK_ROTATION: [[u32; 5]; 5] = [
    [0, 36, 3, 41, 18],
    [1, 44, 10, 45, 2],
    [62, 6, 43, 15, 61],
    [28, 55, 25, 21, 56],
    [27, 20, 39, 8, 14],
];

// ============================================================================
// Keccak State
// ============================================================================

/// Keccak-f[1600] state
#[derive(Clone)]
struct KeccakState {
    state: [u64; KECCAK_LANES],
}

impl KeccakState {
    /// Create new zeroed state
    fn new() -> Self {
        Self {
            state: [0u64; KECCAK_LANES],
        }
    }

    /// Reset state to zero
    fn reset(&mut self) {
        self.state = [0u64; KECCAK_LANES];
    }

    /// XOR a block of data into the state
    fn absorb(&mut self, data: &[u8], rate_bytes: usize) {
        let rate_lanes = rate_bytes / 8;
        for i in 0..rate_lanes.min(data.len() / 8) {
            let lane = u64::from_le_bytes([
                data[i * 8],
                data[i * 8 + 1],
                data[i * 8 + 2],
                data[i * 8 + 3],
                data[i * 8 + 4],
                data[i * 8 + 5],
                data[i * 8 + 6],
                data[i * 8 + 7],
            ]);
            self.state[i] ^= lane;
        }
        // Handle remaining bytes
        let remaining = data.len() % 8;
        if remaining > 0 && data.len() / 8 < rate_lanes {
            let lane_idx = data.len() / 8;
            let mut bytes = [0u8; 8];
            bytes[..remaining].copy_from_slice(&data[lane_idx * 8..]);
            self.state[lane_idx] ^= u64::from_le_bytes(bytes);
        }
    }

    /// Apply Keccak-f[1600] permutation
    fn permute(&mut self) {
        for round in 0..KECCAK_ROUNDS {
            // θ (theta) step
            let mut c = [0u64; 5];
            for x in 0..5 {
                c[x] = self.state[x]
                    ^ self.state[x + 5]
                    ^ self.state[x + 10]
                    ^ self.state[x + 15]
                    ^ self.state[x + 20];
            }
            let mut d = [0u64; 5];
            for x in 0..5 {
                d[x] = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            }
            for y in 0..5 {
                for x in 0..5 {
                    self.state[x + 5 * y] ^= d[x];
                }
            }

            // ρ (rho) and π (pi) steps
            let mut b = [0u64; KECCAK_LANES];
            for y in 0..5 {
                for x in 0..5 {
                    let new_x = y;
                    let new_y = (2 * x + 3 * y) % 5;
                    b[new_x + 5 * new_y] = self.state[x + 5 * y].rotate_left(KECCAK_ROTATION[y][x]);
                }
            }

            // χ (chi) step
            for y in 0..5 {
                for x in 0..5 {
                    self.state[x + 5 * y] =
                        b[x + 5 * y] ^ ((!b[(x + 1) % 5 + 5 * y]) & b[(x + 2) % 5 + 5 * y]);
                }
            }

            // ι (iota) step
            self.state[0] ^= KECCAK_RC[round];
        }
    }

    /// Squeeze output from the state
    fn squeeze(&self, output: &mut [u8], rate_bytes: usize) {
        let rate_lanes = rate_bytes / 8;
        let mut offset = 0;
        for i in 0..rate_lanes {
            if offset >= output.len() {
                break;
            }
            let bytes = self.state[i].to_le_bytes();
            let to_copy = (output.len() - offset).min(8);
            output[offset..offset + to_copy].copy_from_slice(&bytes[..to_copy]);
            offset += 8;
        }
    }
}

// ============================================================================
// SHA-3 Implementation
// ============================================================================

/// SHA-3 hasher state
#[derive(Clone)]
pub struct Sha3 {
    state: KeccakState,
    buffer: Vec<u8>,
    rate: usize,       // Rate in bytes
    output_len: usize, // Output length in bytes
}

impl Sha3 {
    /// Create a new SHA3-256 hasher
    pub fn new_256() -> Self {
        Self {
            state: KeccakState::new(),
            buffer: Vec::new(),
            rate: 136, // 1088 bits = 136 bytes for SHA3-256
            output_len: SHA3_256_DIGEST_SIZE,
        }
    }

    /// Create a new SHA3-384 hasher
    pub fn new_384() -> Self {
        Self {
            state: KeccakState::new(),
            buffer: Vec::new(),
            rate: 104, // 832 bits = 104 bytes for SHA3-384
            output_len: SHA3_384_DIGEST_SIZE,
        }
    }

    /// Create a new SHA3-512 hasher
    pub fn new_512() -> Self {
        Self {
            state: KeccakState::new(),
            buffer: Vec::new(),
            rate: 72, // 576 bits = 72 bytes for SHA3-512
            output_len: SHA3_512_DIGEST_SIZE,
        }
    }

    /// Reset hasher to initial state
    pub fn reset(&mut self) {
        self.state.reset();
        self.buffer.clear();
    }

    /// Update hash with input data
    pub fn update(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);

        // Process complete blocks
        while self.buffer.len() >= self.rate {
            let block: Vec<u8> = self.buffer.drain(..self.rate).collect();
            self.state.absorb(&block, self.rate);
            self.state.permute();
        }
    }

    /// Finalize and return the hash digest
    pub fn finalize(&mut self) -> Vec<u8> {
        // Pad the message (SHA-3 domain separation: 0x06)
        let mut padded = self.buffer.clone();
        padded.push(0x06);
        while padded.len() < self.rate - 1 {
            padded.push(0x00);
        }
        // Set the last bit
        if padded.len() < self.rate {
            padded.push(0x80);
        } else {
            let last = padded.last_mut().unwrap();
            *last |= 0x80;
        }

        // Absorb final block
        self.state.absorb(&padded, self.rate);
        self.state.permute();

        // Squeeze output
        let mut output = vec![0u8; self.output_len];
        let mut remaining = self.output_len;
        let mut offset = 0;

        while remaining > 0 {
            let to_squeeze = remaining.min(self.rate);
            self.state
                .squeeze(&mut output[offset..offset + to_squeeze], self.rate);
            remaining -= to_squeeze;
            offset += to_squeeze;
            if remaining > 0 {
                self.state.permute();
            }
        }

        output
    }
}

/// SHA3-256 type alias
pub type Sha3_256 = Sha3;
/// SHA3-384 type alias
pub type Sha3_384 = Sha3;
/// SHA3-512 type alias
pub type Sha3_512 = Sha3;

/// Compute SHA3-256 hash
pub fn sha3_256(data: &[u8]) -> [u8; SHA3_256_DIGEST_SIZE] {
    let mut hasher = Sha3::new_256();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; SHA3_256_DIGEST_SIZE];
    output.copy_from_slice(&result);
    output
}

/// Compute SHA3-384 hash
pub fn sha3_384(data: &[u8]) -> [u8; SHA3_384_DIGEST_SIZE] {
    let mut hasher = Sha3::new_384();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; SHA3_384_DIGEST_SIZE];
    output.copy_from_slice(&result);
    output
}

/// Compute SHA3-512 hash
pub fn sha3_512(data: &[u8]) -> [u8; SHA3_512_DIGEST_SIZE] {
    let mut hasher = Sha3::new_512();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; SHA3_512_DIGEST_SIZE];
    output.copy_from_slice(&result);
    output
}

// ============================================================================
// SHAKE (Extendable Output Functions)
// ============================================================================

/// SHAKE128 hasher
pub struct Shake128 {
    state: KeccakState,
    buffer: Vec<u8>,
    rate: usize,
}

impl Shake128 {
    /// Create a new SHAKE128 hasher
    pub fn new() -> Self {
        Self {
            state: KeccakState::new(),
            buffer: Vec::new(),
            rate: 168, // 1344 bits = 168 bytes
        }
    }

    /// Update with data
    pub fn update(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
        while self.buffer.len() >= self.rate {
            let block: Vec<u8> = self.buffer.drain(..self.rate).collect();
            self.state.absorb(&block, self.rate);
            self.state.permute();
        }
    }

    /// Finalize and squeeze output
    pub fn finalize(&mut self, output_len: usize) -> Vec<u8> {
        // SHAKE domain separation: 0x1F
        let mut padded = self.buffer.clone();
        padded.push(0x1F);
        while padded.len() < self.rate - 1 {
            padded.push(0x00);
        }
        if padded.len() < self.rate {
            padded.push(0x80);
        } else {
            let last = padded.last_mut().unwrap();
            *last |= 0x80;
        }

        self.state.absorb(&padded, self.rate);
        self.state.permute();

        let mut output = vec![0u8; output_len];
        let mut remaining = output_len;
        let mut offset = 0;

        while remaining > 0 {
            let to_squeeze = remaining.min(self.rate);
            self.state
                .squeeze(&mut output[offset..offset + to_squeeze], self.rate);
            remaining -= to_squeeze;
            offset += to_squeeze;
            if remaining > 0 {
                self.state.permute();
            }
        }

        output
    }
}

impl Default for Shake128 {
    fn default() -> Self {
        Self::new()
    }
}

/// SHAKE256 hasher
pub struct Shake256 {
    state: KeccakState,
    buffer: Vec<u8>,
    rate: usize,
}

impl Shake256 {
    /// Create a new SHAKE256 hasher
    pub fn new() -> Self {
        Self {
            state: KeccakState::new(),
            buffer: Vec::new(),
            rate: 136, // 1088 bits = 136 bytes
        }
    }

    /// Update with data
    pub fn update(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
        while self.buffer.len() >= self.rate {
            let block: Vec<u8> = self.buffer.drain(..self.rate).collect();
            self.state.absorb(&block, self.rate);
            self.state.permute();
        }
    }

    /// Finalize and squeeze output
    pub fn finalize(&mut self, output_len: usize) -> Vec<u8> {
        let mut padded = self.buffer.clone();
        padded.push(0x1F);
        while padded.len() < self.rate - 1 {
            padded.push(0x00);
        }
        if padded.len() < self.rate {
            padded.push(0x80);
        } else {
            let last = padded.last_mut().unwrap();
            *last |= 0x80;
        }

        self.state.absorb(&padded, self.rate);
        self.state.permute();

        let mut output = vec![0u8; output_len];
        let mut remaining = output_len;
        let mut offset = 0;

        while remaining > 0 {
            let to_squeeze = remaining.min(self.rate);
            self.state
                .squeeze(&mut output[offset..offset + to_squeeze], self.rate);
            remaining -= to_squeeze;
            offset += to_squeeze;
            if remaining > 0 {
                self.state.permute();
            }
        }

        output
    }
}

impl Default for Shake256 {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// One-shot SHA3-256
#[no_mangle]
pub extern "C" fn SHA3_256(data: *const u8, len: usize, md: *mut u8) -> *mut u8 {
    if data.is_null() || md.is_null() {
        return ptr::null_mut();
    }

    let input = unsafe { core::slice::from_raw_parts(data, len) };
    let hash = sha3_256(input);

    unsafe {
        ptr::copy_nonoverlapping(hash.as_ptr(), md, SHA3_256_DIGEST_SIZE);
    }

    md
}

/// One-shot SHA3-384
#[no_mangle]
pub extern "C" fn SHA3_384(data: *const u8, len: usize, md: *mut u8) -> *mut u8 {
    if data.is_null() || md.is_null() {
        return ptr::null_mut();
    }

    let input = unsafe { core::slice::from_raw_parts(data, len) };
    let hash = sha3_384(input);

    unsafe {
        ptr::copy_nonoverlapping(hash.as_ptr(), md, SHA3_384_DIGEST_SIZE);
    }

    md
}

/// One-shot SHA3-512
#[no_mangle]
pub extern "C" fn SHA3_512(data: *const u8, len: usize, md: *mut u8) -> *mut u8 {
    if data.is_null() || md.is_null() {
        return ptr::null_mut();
    }

    let input = unsafe { core::slice::from_raw_parts(data, len) };
    let hash = sha3_512(input);

    unsafe {
        ptr::copy_nonoverlapping(hash.as_ptr(), md, SHA3_512_DIGEST_SIZE);
    }

    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha3_256_empty() {
        let hash = sha3_256(b"");
        // SHA3-256("") = a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a
        let expected = [
            0xa7, 0xff, 0xc6, 0xf8, 0xbf, 0x1e, 0xd7, 0x66, 0x51, 0xc1, 0x47, 0x56, 0xa0, 0x61,
            0xd6, 0x62, 0xf5, 0x80, 0xff, 0x4d, 0xe4, 0x3b, 0x49, 0xfa, 0x82, 0xd8, 0x0a, 0x4b,
            0x80, 0xf8, 0x43, 0x4a,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_sha3_256_abc() {
        let hash = sha3_256(b"abc");
        // SHA3-256("abc") = 3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532
        let expected = [
            0x3a, 0x98, 0x5d, 0xa7, 0x4f, 0xe2, 0x25, 0xb2, 0x04, 0x5c, 0x17, 0x2d, 0x6b, 0xd3,
            0x90, 0xbd, 0x85, 0x5f, 0x08, 0x6e, 0x3e, 0x9d, 0x52, 0x5b, 0x46, 0xbf, 0xe2, 0x45,
            0x11, 0x43, 0x15, 0x32,
        ];
        assert_eq!(hash, expected);
    }
}
