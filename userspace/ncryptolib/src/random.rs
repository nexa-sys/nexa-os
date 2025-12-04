//! Random Number Generation
//!
//! CSPRNG implementation using the getrandom syscall.

use std::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Syscall Interface
// ============================================================================

/// NexaOS getrandom syscall number
const SYS_GETRANDOM: usize = 318;

/// getrandom flags
pub const GRND_NONBLOCK: u32 = 0x0001;
pub const GRND_RANDOM: u32 = 0x0002;
pub const GRND_INSECURE: u32 = 0x0004;

/// Get random bytes from the kernel
#[inline]
pub fn getrandom(buf: &mut [u8], flags: u32) -> Result<usize, i32> {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            inout("rax") SYS_GETRANDOM => ret,
            in("rdi") buf.as_mut_ptr(),
            in("rsi") buf.len(),
            in("rdx") flags,
            out("rcx") _,
            out("r11") _,
            options(nostack, preserves_flags)
        );
    }
    
    if ret < 0 {
        Err(ret as i32)
    } else {
        Ok(ret as usize)
    }
}

/// Fill buffer with random bytes (blocking)
pub fn random_bytes(buf: &mut [u8]) -> Result<(), i32> {
    let mut filled = 0;
    while filled < buf.len() {
        match getrandom(&mut buf[filled..], 0) {
            Ok(n) => filled += n,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

// ============================================================================
// ChaCha20 PRNG (for userspace CSPRNG with kernel seed)
// ============================================================================

/// ChaCha20 quarter round
#[inline]
fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);
    
    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);
    
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);
    
    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

/// ChaCha20 block function
fn chacha20_block(key: &[u32; 8], counter: u64, nonce: &[u32; 2]) -> [u8; 64] {
    let mut state = [0u32; 16];
    
    // Constants "expand 32-byte k"
    state[0] = 0x61707865;
    state[1] = 0x3320646e;
    state[2] = 0x79622d32;
    state[3] = 0x6b206574;
    
    // Key
    state[4..12].copy_from_slice(key);
    
    // Counter
    state[12] = counter as u32;
    state[13] = (counter >> 32) as u32;
    
    // Nonce
    state[14] = nonce[0];
    state[15] = nonce[1];
    
    let mut working = state;
    
    // 20 rounds (10 double rounds)
    for _ in 0..10 {
        // Column rounds
        quarter_round(&mut working, 0, 4, 8, 12);
        quarter_round(&mut working, 1, 5, 9, 13);
        quarter_round(&mut working, 2, 6, 10, 14);
        quarter_round(&mut working, 3, 7, 11, 15);
        // Diagonal rounds
        quarter_round(&mut working, 0, 5, 10, 15);
        quarter_round(&mut working, 1, 6, 11, 12);
        quarter_round(&mut working, 2, 7, 8, 13);
        quarter_round(&mut working, 3, 4, 9, 14);
    }
    
    // Add original state
    for i in 0..16 {
        working[i] = working[i].wrapping_add(state[i]);
    }
    
    // Convert to bytes
    let mut output = [0u8; 64];
    for (i, word) in working.iter().enumerate() {
        output[i * 4..(i + 1) * 4].copy_from_slice(&word.to_le_bytes());
    }
    
    output
}

// ============================================================================
// RNG State
// ============================================================================

/// Cryptographically secure random number generator state
pub struct RngState {
    key: [u32; 8],
    nonce: [u32; 2],
    counter: AtomicU64,
    buffer: [u8; 64],
    buffer_pos: usize,
}

impl RngState {
    /// Create a new RNG with random seed from kernel
    pub fn new() -> Result<Self, i32> {
        let mut seed = [0u8; 40]; // 32 bytes key + 8 bytes nonce
        random_bytes(&mut seed)?;
        
        let mut key = [0u32; 8];
        for i in 0..8 {
            key[i] = u32::from_le_bytes([
                seed[i * 4],
                seed[i * 4 + 1],
                seed[i * 4 + 2],
                seed[i * 4 + 3],
            ]);
        }
        
        let nonce = [
            u32::from_le_bytes([seed[32], seed[33], seed[34], seed[35]]),
            u32::from_le_bytes([seed[36], seed[37], seed[38], seed[39]]),
        ];
        
        Ok(Self {
            key,
            nonce,
            counter: AtomicU64::new(0),
            buffer: [0u8; 64],
            buffer_pos: 64, // Empty buffer
        })
    }

    /// Create RNG with explicit seed (for testing)
    pub fn with_seed(seed: &[u8; 40]) -> Self {
        let mut key = [0u32; 8];
        for i in 0..8 {
            key[i] = u32::from_le_bytes([
                seed[i * 4],
                seed[i * 4 + 1],
                seed[i * 4 + 2],
                seed[i * 4 + 3],
            ]);
        }
        
        let nonce = [
            u32::from_le_bytes([seed[32], seed[33], seed[34], seed[35]]),
            u32::from_le_bytes([seed[36], seed[37], seed[38], seed[39]]),
        ];
        
        Self {
            key,
            nonce,
            counter: AtomicU64::new(0),
            buffer: [0u8; 64],
            buffer_pos: 64,
        }
    }

    /// Fill buffer with random bytes
    pub fn fill(&mut self, dest: &mut [u8]) {
        let mut offset = 0;
        
        while offset < dest.len() {
            // Refill buffer if needed
            if self.buffer_pos >= 64 {
                let counter = self.counter.fetch_add(1, Ordering::SeqCst);
                self.buffer = chacha20_block(&self.key, counter, &self.nonce);
                self.buffer_pos = 0;
            }
            
            // Copy from buffer
            let available = 64 - self.buffer_pos;
            let to_copy = core::cmp::min(available, dest.len() - offset);
            dest[offset..offset + to_copy].copy_from_slice(
                &self.buffer[self.buffer_pos..self.buffer_pos + to_copy]
            );
            
            self.buffer_pos += to_copy;
            offset += to_copy;
        }
    }

    /// Generate random u32
    pub fn next_u32(&mut self) -> u32 {
        let mut buf = [0u8; 4];
        self.fill(&mut buf);
        u32::from_le_bytes(buf)
    }

    /// Generate random u64
    pub fn next_u64(&mut self) -> u64 {
        let mut buf = [0u8; 8];
        self.fill(&mut buf);
        u64::from_le_bytes(buf)
    }

    /// Generate random bytes in a Vec
    pub fn random_vec(&mut self, len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len];
        self.fill(&mut buf);
        buf
    }

    /// Reseed from kernel entropy
    pub fn reseed(&mut self) -> Result<(), i32> {
        let mut seed = [0u8; 40];
        random_bytes(&mut seed)?;
        
        for i in 0..8 {
            self.key[i] = u32::from_le_bytes([
                seed[i * 4],
                seed[i * 4 + 1],
                seed[i * 4 + 2],
                seed[i * 4 + 3],
            ]);
        }
        
        self.nonce = [
            u32::from_le_bytes([seed[32], seed[33], seed[34], seed[35]]),
            u32::from_le_bytes([seed[36], seed[37], seed[38], seed[39]]),
        ];
        
        self.counter.store(0, Ordering::SeqCst);
        self.buffer_pos = 64;
        
        Ok(())
    }
}

impl Default for RngState {
    fn default() -> Self {
        Self::new().expect("Failed to initialize RNG")
    }
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// RAND_bytes - Fill buffer with random bytes
#[no_mangle]
pub extern "C" fn RAND_bytes(buf: *mut u8, num: i32) -> i32 {
    if buf.is_null() || num < 0 {
        return 0;
    }
    
    let slice = unsafe { core::slice::from_raw_parts_mut(buf, num as usize) };
    match random_bytes(slice) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// RAND_pseudo_bytes - Same as RAND_bytes (we don't have "pseudo" random)
#[no_mangle]
pub extern "C" fn RAND_pseudo_bytes(buf: *mut u8, num: i32) -> i32 {
    RAND_bytes(buf, num)
}

/// RAND_seed - Add seed to entropy pool (no-op, kernel handles this)
#[no_mangle]
pub extern "C" fn RAND_seed(_buf: *const u8, _num: i32) {
    // No-op: kernel manages entropy
}

/// RAND_add - Add entropy with quality estimate (no-op)
#[no_mangle]
pub extern "C" fn RAND_add(_buf: *const u8, _num: i32, _randomness: f64) {
    // No-op: kernel manages entropy
}

/// RAND_status - Check if PRNG is seeded
#[no_mangle]
pub extern "C" fn RAND_status() -> i32 {
    1 // Always seeded from kernel
}

/// RAND_poll - Gather entropy (no-op)
#[no_mangle]
pub extern "C" fn RAND_poll() -> i32 {
    1 // Success
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rng_deterministic() {
        let seed = [0u8; 40];
        let mut rng1 = RngState::with_seed(&seed);
        let mut rng2 = RngState::with_seed(&seed);
        
        let mut buf1 = [0u8; 100];
        let mut buf2 = [0u8; 100];
        
        rng1.fill(&mut buf1);
        rng2.fill(&mut buf2);
        
        assert_eq!(buf1, buf2);
    }

    #[test]
    fn test_chacha20_block() {
        let key = [0u32; 8];
        let nonce = [0u32; 2];
        let block = chacha20_block(&key, 0, &nonce);
        
        // Just verify it produces output
        assert!(block.iter().any(|&b| b != 0));
    }
}
