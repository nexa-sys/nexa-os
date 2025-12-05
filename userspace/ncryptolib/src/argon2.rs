//! Argon2 Password Hashing
//!
//! RFC 9106 compliant Argon2 implementation (Argon2d, Argon2i, Argon2id).
//! Argon2 is the winner of the Password Hashing Competition (PHC).
//!
//! Recommended parameters for 2024+:
//! - Argon2id (hybrid, recommended for most use cases)
//! - Memory: 64 MB (65536 KB)
//! - Iterations: 3
//! - Parallelism: 4

use std::vec::Vec;

use crate::blake2::blake2b_with_len;

// ============================================================================
// Constants
// ============================================================================

/// Argon2 variant
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Argon2Variant {
    /// Argon2d - data-dependent (vulnerable to side-channel attacks)
    Argon2d = 0,
    /// Argon2i - data-independent (resistant to side-channel attacks)
    Argon2i = 1,
    /// Argon2id - hybrid (recommended)
    Argon2id = 2,
}

/// Argon2 version
pub const ARGON2_VERSION: u32 = 0x13; // v1.3

/// Minimum and maximum parameters
pub const ARGON2_MIN_MEMORY: u32 = 8; // 8 KB
pub const ARGON2_MAX_MEMORY: u32 = 0x0FFFFFFF; // ~4 GB
pub const ARGON2_MIN_TIME: u32 = 1;
pub const ARGON2_MAX_TIME: u32 = 0xFFFFFFFF;
pub const ARGON2_MIN_LANES: u32 = 1;
pub const ARGON2_MAX_LANES: u32 = 0x00FFFFFF;
pub const ARGON2_MIN_SALT_LEN: usize = 8;
pub const ARGON2_MAX_SALT_LEN: usize = 0xFFFFFFFF;
pub const ARGON2_MIN_PASSWORD_LEN: usize = 0;
pub const ARGON2_MAX_PASSWORD_LEN: usize = 0xFFFFFFFF;
pub const ARGON2_MIN_TAG_LEN: usize = 4;
pub const ARGON2_MAX_TAG_LEN: usize = 0xFFFFFFFF;

/// Block size (1024 bytes)
const BLOCK_SIZE: usize = 1024;
/// Number of 64-bit words per block
const QWORDS_PER_BLOCK: usize = 128;
/// Sync points per pass
const SYNC_POINTS: u32 = 4;

// ============================================================================
// Argon2 Parameters
// ============================================================================

/// Argon2 parameters
#[derive(Clone, Debug)]
pub struct Argon2Params {
    /// Variant (Argon2d, Argon2i, Argon2id)
    pub variant: Argon2Variant,
    /// Memory size in KB
    pub memory_kb: u32,
    /// Number of iterations (time cost)
    pub iterations: u32,
    /// Degree of parallelism (lanes)
    pub parallelism: u32,
    /// Output length in bytes
    pub output_len: usize,
}

impl Default for Argon2Params {
    fn default() -> Self {
        Self {
            variant: Argon2Variant::Argon2id,
            memory_kb: 65536, // 64 MB
            iterations: 3,
            parallelism: 4,
            output_len: 32,
        }
    }
}

impl Argon2Params {
    /// Create new Argon2id parameters (recommended)
    pub fn argon2id(memory_kb: u32, iterations: u32, parallelism: u32) -> Self {
        Self {
            variant: Argon2Variant::Argon2id,
            memory_kb,
            iterations,
            parallelism,
            output_len: 32,
        }
    }

    /// Create new Argon2i parameters
    pub fn argon2i(memory_kb: u32, iterations: u32, parallelism: u32) -> Self {
        Self {
            variant: Argon2Variant::Argon2i,
            memory_kb,
            iterations,
            parallelism,
            output_len: 32,
        }
    }

    /// Create new Argon2d parameters
    pub fn argon2d(memory_kb: u32, iterations: u32, parallelism: u32) -> Self {
        Self {
            variant: Argon2Variant::Argon2d,
            memory_kb,
            iterations,
            parallelism,
            output_len: 32,
        }
    }

    /// Set output length
    pub fn with_output_len(mut self, len: usize) -> Self {
        self.output_len = len;
        self
    }
}

// ============================================================================
// Block Operations
// ============================================================================

/// A 1024-byte block (128 x 64-bit words)
#[derive(Clone)]
struct Block([u64; QWORDS_PER_BLOCK]);

impl Block {
    fn new() -> Self {
        Block([0u64; QWORDS_PER_BLOCK])
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        let mut block = Block::new();
        for i in 0..QWORDS_PER_BLOCK {
            if i * 8 + 8 <= bytes.len() {
                block.0[i] = u64::from_le_bytes([
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
        }
        block
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(BLOCK_SIZE);
        for word in &self.0 {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    fn xor_with(&mut self, other: &Block) {
        for i in 0..QWORDS_PER_BLOCK {
            self.0[i] ^= other.0[i];
        }
    }
}

// ============================================================================
// Blake2b Long Hash
// ============================================================================

/// Variable-length hash function H' (Blake2b-based)
fn blake2b_long(output_len: usize, input: &[u8]) -> Vec<u8> {
    if output_len <= 64 {
        blake2b_with_len(input, output_len)
    } else {
        // For longer outputs, chain Blake2b calls
        let mut result = Vec::with_capacity(output_len);
        let mut v = blake2b_with_len(&[&(output_len as u32).to_le_bytes()[..], input].concat(), 64);
        
        while result.len() + 64 < output_len {
            result.extend_from_slice(&v[..32]);
            v = blake2b_with_len(&v, 64);
        }
        
        let remaining = output_len - result.len();
        let final_hash = blake2b_with_len(&v, remaining);
        result.extend_from_slice(&final_hash);
        
        result
    }
}

// ============================================================================
// G Function (Blake2b-based mixing)
// ============================================================================

#[inline]
fn rotr64(x: u64, n: u32) -> u64 {
    x.rotate_right(n)
}

#[inline]
fn g(a: &mut u64, b: &mut u64, c: &mut u64, d: &mut u64) {
    *a = a.wrapping_add(*b).wrapping_add(2u64.wrapping_mul((*a as u32 as u64).wrapping_mul(*b as u32 as u64)));
    *d = rotr64(*d ^ *a, 32);
    *c = c.wrapping_add(*d).wrapping_add(2u64.wrapping_mul((*c as u32 as u64).wrapping_mul(*d as u32 as u64)));
    *b = rotr64(*b ^ *c, 24);
    *a = a.wrapping_add(*b).wrapping_add(2u64.wrapping_mul((*a as u32 as u64).wrapping_mul(*b as u32 as u64)));
    *d = rotr64(*d ^ *a, 16);
    *c = c.wrapping_add(*d).wrapping_add(2u64.wrapping_mul((*c as u32 as u64).wrapping_mul(*d as u32 as u64)));
    *b = rotr64(*b ^ *c, 63);
}

/// G mixing function that works on array indices to avoid borrowing issues
#[inline]
fn g_idx(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(2u64.wrapping_mul((v[a] as u32 as u64).wrapping_mul(v[b] as u32 as u64)));
    v[d] = rotr64(v[d] ^ v[a], 32);
    v[c] = v[c].wrapping_add(v[d]).wrapping_add(2u64.wrapping_mul((v[c] as u32 as u64).wrapping_mul(v[d] as u32 as u64)));
    v[b] = rotr64(v[b] ^ v[c], 24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(2u64.wrapping_mul((v[a] as u32 as u64).wrapping_mul(v[b] as u32 as u64)));
    v[d] = rotr64(v[d] ^ v[a], 16);
    v[c] = v[c].wrapping_add(v[d]).wrapping_add(2u64.wrapping_mul((v[c] as u32 as u64).wrapping_mul(v[d] as u32 as u64)));
    v[b] = rotr64(v[b] ^ v[c], 63);
}

/// Permutation P - applies G function to 8 16-byte groups
fn permutation_p(v: &mut [u64; 16]) {
    // Column rounds
    g_idx(v, 0, 4, 8, 12);
    g_idx(v, 1, 5, 9, 13);
    g_idx(v, 2, 6, 10, 14);
    g_idx(v, 3, 7, 11, 15);
    // Diagonal rounds
    g_idx(v, 0, 5, 10, 15);
    g_idx(v, 1, 6, 11, 12);
    g_idx(v, 2, 7, 8, 13);
    g_idx(v, 3, 4, 9, 14);
}

/// Compression function G
fn compress_g(x: &Block, y: &Block) -> Block {
    let mut r = Block::new();
    
    // r = x XOR y
    for i in 0..QWORDS_PER_BLOCK {
        r.0[i] = x.0[i] ^ y.0[i];
    }
    
    let mut q = r.clone();
    
    // Apply P rowwise
    for row in 0..8 {
        let mut v = [0u64; 16];
        for i in 0..16 {
            v[i] = q.0[row * 16 + i];
        }
        permutation_p(&mut v);
        for i in 0..16 {
            q.0[row * 16 + i] = v[i];
        }
    }
    
    // Apply P columnwise
    for col in 0..8 {
        let mut v = [0u64; 16];
        for i in 0..16 {
            v[i] = q.0[(i / 2) * 16 + col * 2 + (i % 2)];
        }
        permutation_p(&mut v);
        for i in 0..16 {
            q.0[(i / 2) * 16 + col * 2 + (i % 2)] = v[i];
        }
    }
    
    // r = r XOR q
    r.xor_with(&q);
    r
}

// ============================================================================
// Indexing Functions
// ============================================================================

/// Compute reference block index for Argon2i/Argon2id
fn index_alpha(
    _r: u64,
    slice: u32,
    lane: u32,
    index: u32,
    lanes: u32,
    segment_length: u32,
    pass: u32,
    _variant: Argon2Variant,
    j1: u64,
    j2: u64,
) -> (u32, u32) {
    // Reference lane
    let ref_lane = if pass == 0 && slice == 0 {
        lane
    } else {
        (j2 as u32) % lanes
    };
    
    // Reference index calculation
    let total_blocks = lanes * segment_length * 4;
    let same_lane = ref_lane == lane;
    
    let reference_area_size = if pass == 0 {
        // First pass
        if slice == 0 {
            index.saturating_sub(1)
        } else if same_lane {
            slice * segment_length + index - 1
        } else {
            slice * segment_length + if index == 0 { 0 } else { index - 1 }
        }
    } else {
        // Later passes
        if same_lane {
            total_blocks - segment_length + index - 1
        } else {
            total_blocks - segment_length - if index == 0 { 1 } else { 0 }
        }
    };
    
    // Map j1 to reference index
    let relative_pos = (j1 as u128 * j1 as u128 / u64::MAX as u128) as u64;
    let ref_index = (reference_area_size as u64 - 1 - (reference_area_size as u64 * relative_pos / u64::MAX)) as u32;
    
    // Calculate actual position
    let start_position = if pass == 0 && slice == 0 {
        0
    } else if pass == 0 {
        slice * segment_length
    } else {
        ((slice + 1) % 4) * segment_length
    };
    
    let ref_block = (start_position + ref_index) % (4 * segment_length);
    
    (ref_lane, ref_block)
}

// ============================================================================
// Main Argon2 Implementation
// ============================================================================

/// Argon2 password hashing function
pub fn argon2(
    params: &Argon2Params,
    password: &[u8],
    salt: &[u8],
    secret: Option<&[u8]>,
    associated_data: Option<&[u8]>,
) -> Result<Vec<u8>, &'static str> {
    // Validate parameters
    if salt.len() < ARGON2_MIN_SALT_LEN {
        return Err("Salt too short");
    }
    if params.memory_kb < ARGON2_MIN_MEMORY {
        return Err("Memory too small");
    }
    if params.iterations < ARGON2_MIN_TIME {
        return Err("Too few iterations");
    }
    if params.parallelism < ARGON2_MIN_LANES {
        return Err("Too few lanes");
    }
    if params.output_len < ARGON2_MIN_TAG_LEN {
        return Err("Output too short");
    }

    let lanes = params.parallelism;
    let memory_blocks = (params.memory_kb as usize / (BLOCK_SIZE / 1024)).max(8 * lanes as usize);
    let segment_length = (memory_blocks / (4 * lanes as usize)) as u32;
    let memory_blocks = segment_length as usize * 4 * lanes as usize;

    // Initialize H0
    let secret_bytes = secret.unwrap_or(&[]);
    let ad_bytes = associated_data.unwrap_or(&[]);
    
    let mut h0_input = Vec::new();
    h0_input.extend_from_slice(&params.parallelism.to_le_bytes());
    h0_input.extend_from_slice(&(params.output_len as u32).to_le_bytes());
    h0_input.extend_from_slice(&params.memory_kb.to_le_bytes());
    h0_input.extend_from_slice(&params.iterations.to_le_bytes());
    h0_input.extend_from_slice(&ARGON2_VERSION.to_le_bytes());
    h0_input.extend_from_slice(&(params.variant as u32).to_le_bytes());
    h0_input.extend_from_slice(&(password.len() as u32).to_le_bytes());
    h0_input.extend_from_slice(password);
    h0_input.extend_from_slice(&(salt.len() as u32).to_le_bytes());
    h0_input.extend_from_slice(salt);
    h0_input.extend_from_slice(&(secret_bytes.len() as u32).to_le_bytes());
    h0_input.extend_from_slice(secret_bytes);
    h0_input.extend_from_slice(&(ad_bytes.len() as u32).to_le_bytes());
    h0_input.extend_from_slice(ad_bytes);
    
    let h0 = blake2b_with_len(&h0_input, 64);

    // Allocate memory
    let mut memory: Vec<Block> = vec![Block::new(); memory_blocks];

    // Initialize first two columns of each lane
    for lane in 0..lanes {
        let lane_start = (lane as usize) * segment_length as usize * 4;
        
        // Block[lane][0]
        let mut input0 = h0.clone();
        input0.extend_from_slice(&0u32.to_le_bytes());
        input0.extend_from_slice(&lane.to_le_bytes());
        let block0_bytes = blake2b_long(BLOCK_SIZE, &input0);
        memory[lane_start] = Block::from_bytes(&block0_bytes);
        
        // Block[lane][1]
        let mut input1 = h0.clone();
        input1.extend_from_slice(&1u32.to_le_bytes());
        input1.extend_from_slice(&lane.to_le_bytes());
        let block1_bytes = blake2b_long(BLOCK_SIZE, &input1);
        memory[lane_start + 1] = Block::from_bytes(&block1_bytes);
    }

    // Main iterations
    for pass in 0..params.iterations {
        for slice in 0..4u32 {
            for lane in 0..lanes {
                let start_index = if pass == 0 && slice == 0 { 2 } else { 0 };
                
                for index in start_index..segment_length {
                    // Current block position
                    let cur_lane = lane;
                    let cur_offset = slice * segment_length + index;
                    let cur_index = (cur_lane as usize) * (4 * segment_length as usize) + cur_offset as usize;
                    
                    // Previous block
                    let prev_offset = if cur_offset == 0 {
                        4 * segment_length - 1
                    } else {
                        cur_offset - 1
                    };
                    let prev_index = (cur_lane as usize) * (4 * segment_length as usize) + prev_offset as usize;
                    
                    // Generate pseudo-random values for indexing
                    let j1: u64;
                    let j2: u64;
                    
                    match params.variant {
                        Argon2Variant::Argon2d => {
                            // Data-dependent: use first 64 bits of previous block
                            j1 = memory[prev_index].0[0];
                            j2 = memory[prev_index].0[1];
                        }
                        Argon2Variant::Argon2i => {
                            // Data-independent: generate from counter
                            let counter = (pass as u64) << 32 | (slice as u64) << 16 | (lane as u64);
                            j1 = counter;
                            j2 = counter.wrapping_add(1);
                        }
                        Argon2Variant::Argon2id => {
                            // Hybrid: Argon2i for first two slices of first pass, Argon2d otherwise
                            if pass == 0 && slice < 2 {
                                let counter = (pass as u64) << 32 | (slice as u64) << 16 | (lane as u64);
                                j1 = counter;
                                j2 = counter.wrapping_add(1);
                            } else {
                                j1 = memory[prev_index].0[0];
                                j2 = memory[prev_index].0[1];
                            }
                        }
                    }
                    
                    // Get reference block
                    let (ref_lane, ref_offset) = index_alpha(
                        0,
                        slice,
                        lane,
                        index,
                        lanes,
                        segment_length,
                        pass,
                        params.variant,
                        j1,
                        j2,
                    );
                    let ref_index = (ref_lane as usize) * (4 * segment_length as usize) + ref_offset as usize;
                    
                    // Compute new block
                    let new_block = compress_g(&memory[prev_index], &memory[ref_index]);
                    
                    // XOR with existing block for passes > 0
                    if pass == 0 {
                        memory[cur_index] = new_block;
                    } else {
                        memory[cur_index].xor_with(&new_block);
                    }
                }
            }
        }
    }

    // Finalize: XOR last column of all lanes
    let mut final_block = Block::new();
    for lane in 0..lanes {
        let last_index = (lane as usize + 1) * (4 * segment_length as usize) - 1;
        final_block.xor_with(&memory[last_index]);
    }

    // Generate output tag
    let output = blake2b_long(params.output_len, &final_block.to_bytes());
    
    Ok(output)
}

// ============================================================================
// Convenience Functions
// ============================================================================

/// Hash password with Argon2id (recommended)
pub fn argon2id(
    password: &[u8],
    salt: &[u8],
    memory_kb: u32,
    iterations: u32,
    parallelism: u32,
    output_len: usize,
) -> Result<Vec<u8>, &'static str> {
    let params = Argon2Params::argon2id(memory_kb, iterations, parallelism)
        .with_output_len(output_len);
    argon2(&params, password, salt, None, None)
}

/// Hash password with Argon2i
pub fn argon2i(
    password: &[u8],
    salt: &[u8],
    memory_kb: u32,
    iterations: u32,
    parallelism: u32,
    output_len: usize,
) -> Result<Vec<u8>, &'static str> {
    let params = Argon2Params::argon2i(memory_kb, iterations, parallelism)
        .with_output_len(output_len);
    argon2(&params, password, salt, None, None)
}

/// Hash password with Argon2d
pub fn argon2d(
    password: &[u8],
    salt: &[u8],
    memory_kb: u32,
    iterations: u32,
    parallelism: u32,
    output_len: usize,
) -> Result<Vec<u8>, &'static str> {
    let params = Argon2Params::argon2d(memory_kb, iterations, parallelism)
        .with_output_len(output_len);
    argon2(&params, password, salt, None, None)
}

// ============================================================================
// C ABI Exports
// ============================================================================

use crate::{c_int, size_t};

/// Argon2id hash (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_argon2id(
    password: *const u8,
    password_len: size_t,
    salt: *const u8,
    salt_len: size_t,
    memory_kb: u32,
    iterations: u32,
    parallelism: u32,
    output: *mut u8,
    output_len: size_t,
) -> c_int {
    if password.is_null() || salt.is_null() || output.is_null() {
        return -1;
    }
    if salt_len < ARGON2_MIN_SALT_LEN {
        return -2;
    }

    let password_slice = core::slice::from_raw_parts(password, password_len);
    let salt_slice = core::slice::from_raw_parts(salt, salt_len);

    match argon2id(password_slice, salt_slice, memory_kb, iterations, parallelism, output_len) {
        Ok(hash) => {
            core::ptr::copy_nonoverlapping(hash.as_ptr(), output, hash.len());
            0
        }
        Err(_) => -1,
    }
}

/// Argon2i hash (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_argon2i(
    password: *const u8,
    password_len: size_t,
    salt: *const u8,
    salt_len: size_t,
    memory_kb: u32,
    iterations: u32,
    parallelism: u32,
    output: *mut u8,
    output_len: size_t,
) -> c_int {
    if password.is_null() || salt.is_null() || output.is_null() {
        return -1;
    }
    if salt_len < ARGON2_MIN_SALT_LEN {
        return -2;
    }

    let password_slice = core::slice::from_raw_parts(password, password_len);
    let salt_slice = core::slice::from_raw_parts(salt, salt_len);

    match argon2i(password_slice, salt_slice, memory_kb, iterations, parallelism, output_len) {
        Ok(hash) => {
            core::ptr::copy_nonoverlapping(hash.as_ptr(), output, hash.len());
            0
        }
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argon2id_basic() {
        let password = b"password";
        let salt = b"somesalt12345678";
        
        let result = argon2id(password, salt, 1024, 1, 1, 32);
        assert!(result.is_ok());
        let hash = result.unwrap();
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_argon2id_deterministic() {
        let password = b"test_password";
        let salt = b"random_salt_here";
        
        let hash1 = argon2id(password, salt, 1024, 1, 1, 32).unwrap();
        let hash2 = argon2id(password, salt, 1024, 1, 1, 32).unwrap();
        
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_argon2_different_passwords() {
        let salt = b"same_salt_value!";
        
        let hash1 = argon2id(b"password1", salt, 1024, 1, 1, 32).unwrap();
        let hash2 = argon2id(b"password2", salt, 1024, 1, 1, 32).unwrap();
        
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_argon2_salt_too_short() {
        let result = argon2id(b"password", b"short", 1024, 1, 1, 32);
        assert!(result.is_err());
    }
}
