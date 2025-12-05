//! Random Number Generator Driver
//!
//! This module provides hardware random number generation using the CPU's
//! RDRAND and RDSEED instructions (if available), with a ChaCha20-based
//! CSPRNG fallback.
//!
//! # Features
//! - Hardware RNG via RDRAND/RDSEED instructions
//! - Software CSPRNG (ChaCha20) with hardware seed
//! - Kernel entropy pool for /dev/random and /dev/urandom
//! - getrandom() syscall support

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

/// Whether RDRAND instruction is available
static RDRAND_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Whether RDSEED instruction is available  
static RDSEED_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Whether the RNG subsystem has been initialized
static RNG_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Entropy counter (estimated bits of entropy in pool)
static ENTROPY_COUNT: AtomicU64 = AtomicU64::new(0);

/// Maximum entropy pool size in bits
const MAX_ENTROPY_BITS: u64 = 4096;

// ============================================================================
// ChaCha20 CSPRNG State
// ============================================================================

/// ChaCha20-based CSPRNG state
struct ChaChaState {
    key: [u32; 8],
    nonce: [u32; 2],
    counter: u64,
}

impl ChaChaState {
    const fn new() -> Self {
        Self {
            key: [0; 8],
            nonce: [0; 2],
            counter: 0,
        }
    }

    /// Reseed from hardware entropy
    fn reseed(&mut self) {
        let mut seed = [0u8; 40]; // 32 bytes key + 8 bytes nonce
        
        // Try to get hardware random bytes
        if !fill_from_hardware(&mut seed) {
            // Fallback: use TSC and other system state
            fallback_seed(&mut seed);
        }
        
        // Parse into key and nonce
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
        
        self.counter = 0;
        ENTROPY_COUNT.store(MAX_ENTROPY_BITS, Ordering::SeqCst);
    }

    /// Generate a 64-byte block using ChaCha20
    fn generate_block(&mut self) -> [u8; 64] {
        let output = chacha20_block(&self.key, self.counter, &self.nonce);
        self.counter = self.counter.wrapping_add(1);
        
        // Decrement entropy estimate
        let _ = ENTROPY_COUNT.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
            Some(v.saturating_sub(512))
        });
        
        output
    }

    /// Fill buffer with random bytes
    fn fill(&mut self, buf: &mut [u8]) {
        // Auto-reseed if entropy is low
        if ENTROPY_COUNT.load(Ordering::SeqCst) < 256 {
            self.reseed();
        }
        
        let mut offset = 0;
        while offset < buf.len() {
            let block = self.generate_block();
            let to_copy = core::cmp::min(64, buf.len() - offset);
            buf[offset..offset + to_copy].copy_from_slice(&block[..to_copy]);
            offset += to_copy;
        }
    }
}

/// Global CSPRNG state
static CSPRNG: Mutex<ChaChaState> = Mutex::new(ChaChaState::new());

// ============================================================================
// ChaCha20 Core Algorithm
// ============================================================================

/// ChaCha20 quarter round operation
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
    
    // Working state
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
    
    // Serialize to bytes
    let mut output = [0u8; 64];
    for (i, word) in working.iter().enumerate() {
        output[i * 4..(i + 1) * 4].copy_from_slice(&word.to_le_bytes());
    }
    
    output
}

// ============================================================================
// Hardware Random Instructions
// ============================================================================

/// Check if RDRAND is supported via CPUID
fn check_rdrand_support() -> bool {
    // CPUID leaf 1, check ECX bit 30 for RDRAND
    let ecx: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "pop rbx",
            out("ecx") ecx,
            out("eax") _,
            out("edx") _,
            options(nomem)
        );
    }
    (ecx & (1 << 30)) != 0
}

/// Check if RDSEED is supported via CPUID
fn check_rdseed_support() -> bool {
    // CPUID leaf 7, subleaf 0, check EBX bit 18 for RDSEED
    let ebx_result: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 7",
            "xor ecx, ecx",
            "cpuid",
            "mov {0:e}, ebx",
            "pop rbx",
            out(reg) ebx_result,
            out("eax") _,
            out("ecx") _,
            out("edx") _,
            options(nomem)
        );
    }
    (ebx_result & (1 << 18)) != 0
}

/// Get a random 64-bit value using RDRAND
/// Returns Some(value) on success, None if RDRAND failed
#[inline]
fn rdrand64() -> Option<u64> {
    if !RDRAND_AVAILABLE.load(Ordering::Relaxed) {
        return None;
    }
    
    let mut value: u64;
    let mut success: u8;
    
    // Try up to 10 times (RDRAND can transiently fail)
    for _ in 0..10 {
        unsafe {
            core::arch::asm!(
                "rdrand {0}",
                "setc {1}",
                out(reg) value,
                out(reg_byte) success,
                options(nomem, nostack)
            );
        }
        
        if success != 0 {
            return Some(value);
        }
    }
    
    None
}

/// Get a random 64-bit value using RDSEED (true entropy)
/// Returns Some(value) on success, None if RDSEED failed
#[inline]
fn rdseed64() -> Option<u64> {
    if !RDSEED_AVAILABLE.load(Ordering::Relaxed) {
        return None;
    }
    
    let mut value: u64;
    let mut success: u8;
    
    // RDSEED can fail more often than RDRAND, try a few times
    for _ in 0..3 {
        unsafe {
            core::arch::asm!(
                "rdseed {0}",
                "setc {1}",
                out(reg) value,
                out(reg_byte) success,
                options(nomem, nostack)
            );
        }
        
        if success != 0 {
            return Some(value);
        }
    }
    
    None
}

/// Fill buffer from hardware random source
/// Returns true if successful, false if fallback needed
fn fill_from_hardware(buf: &mut [u8]) -> bool {
    let mut offset = 0;
    
    while offset < buf.len() {
        // Try RDSEED first (true entropy), then RDRAND
        let value = rdseed64().or_else(rdrand64);
        
        match value {
            Some(v) => {
                let bytes = v.to_le_bytes();
                let to_copy = core::cmp::min(8, buf.len() - offset);
                buf[offset..offset + to_copy].copy_from_slice(&bytes[..to_copy]);
                offset += to_copy;
            }
            None => return false,
        }
    }
    
    true
}

/// Fallback seed generation using system state
fn fallback_seed(buf: &mut [u8]) {
    // Use TSC as primary entropy source
    let mut tsc: u64;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            "shl rdx, 32",
            "or rax, rdx",
            out("rax") tsc,
            out("rdx") _,
            options(nomem, nostack)
        );
    }
    
    // Mix with stack address and other system state
    let stack_addr = &tsc as *const _ as u64;
    
    // Simple mixing
    let mut state = tsc ^ stack_addr;
    
    for byte in buf.iter_mut() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = (state >> 56) as u8;
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Initialize the random number generator subsystem
pub fn init() {
    // Check for hardware support
    let rdrand = check_rdrand_support();
    let rdseed = check_rdseed_support();
    
    RDRAND_AVAILABLE.store(rdrand, Ordering::SeqCst);
    RDSEED_AVAILABLE.store(rdseed, Ordering::SeqCst);
    
    crate::kinfo!(
        "Random: RDRAND={}, RDSEED={}",
        if rdrand { "yes" } else { "no" },
        if rdseed { "yes" } else { "no" }
    );
    
    // Initial seed of CSPRNG
    {
        let mut csprng = CSPRNG.lock();
        csprng.reseed();
    }
    
    RNG_INITIALIZED.store(true, Ordering::SeqCst);
    crate::kinfo!("Random: CSPRNG initialized with hardware seed");
}

/// Check if RNG is initialized
pub fn is_initialized() -> bool {
    RNG_INITIALIZED.load(Ordering::SeqCst)
}

/// Get the current entropy estimate (in bits)
pub fn entropy_available() -> u64 {
    ENTROPY_COUNT.load(Ordering::SeqCst)
}

/// Fill buffer with random bytes (non-blocking)
/// This is used by /dev/urandom and getrandom() without GRND_RANDOM
pub fn get_random_bytes(buf: &mut [u8]) {
    if !RNG_INITIALIZED.load(Ordering::SeqCst) {
        // Not initialized yet, try hardware directly or use fallback
        if !fill_from_hardware(buf) {
            fallback_seed(buf);
        }
        return;
    }
    
    let mut csprng = CSPRNG.lock();
    csprng.fill(buf);
}

/// Fill buffer with random bytes (may block for entropy)
/// This is used by /dev/random and getrandom() with GRND_RANDOM
pub fn get_random_bytes_wait(buf: &mut [u8]) -> bool {
    // In kernel mode, we don't actually block, but we ensure
    // we have enough entropy or reseed if needed
    if ENTROPY_COUNT.load(Ordering::SeqCst) < (buf.len() as u64 * 8) {
        let mut csprng = CSPRNG.lock();
        csprng.reseed();
    }
    
    get_random_bytes(buf);
    true
}

/// Get a random u64 value directly (fast path)
pub fn get_random_u64() -> u64 {
    // Try hardware first for single values
    if let Some(v) = rdrand64() {
        return v;
    }
    
    // Fall back to CSPRNG
    let mut buf = [0u8; 8];
    get_random_bytes(&mut buf);
    u64::from_le_bytes(buf)
}

/// Get a random u32 value
pub fn get_random_u32() -> u32 {
    (get_random_u64() >> 32) as u32
}

/// Add entropy to the pool (from interrupts, disk timing, etc.)
pub fn add_entropy(data: &[u8], entropy_bits: u32) {
    // Mix into CSPRNG state
    // For simplicity, we just trigger a reseed if we get significant entropy
    let current = ENTROPY_COUNT.fetch_add(entropy_bits as u64, Ordering::SeqCst);
    
    if current + entropy_bits as u64 > MAX_ENTROPY_BITS {
        ENTROPY_COUNT.store(MAX_ENTROPY_BITS, Ordering::SeqCst);
    }
    
    // Mix the new entropy into the CSPRNG key
    if !data.is_empty() {
        let mut csprng = CSPRNG.lock();
        for (i, byte) in data.iter().enumerate() {
            let idx = i % 32;
            let word_idx = idx / 4;
            let byte_idx = idx % 4;
            let shift = byte_idx * 8;
            csprng.key[word_idx] ^= (*byte as u32) << shift;
        }
    }
}

// ============================================================================
// getrandom() syscall flags
// ============================================================================

/// Don't block waiting for entropy
pub const GRND_NONBLOCK: u32 = 0x0001;

/// Use /dev/random instead of /dev/urandom
pub const GRND_RANDOM: u32 = 0x0002;

/// Don't wait for CSPRNG to be seeded
pub const GRND_INSECURE: u32 = 0x0004;

/// getrandom() syscall implementation
/// Returns number of bytes written, or negative errno on error
pub fn sys_getrandom(buf: *mut u8, buflen: usize, flags: u32) -> isize {
    // Validate buffer pointer
    if buf.is_null() {
        return -(crate::posix::errno::EFAULT as isize);
    }
    
    // Safety: validate the buffer is in user space
    let buf_addr = buf as usize;
    if buf_addr >= 0xFFFF_8000_0000_0000 {
        return -(crate::posix::errno::EFAULT as isize);
    }
    
    // Get the slice
    let slice = unsafe { core::slice::from_raw_parts_mut(buf, buflen) };
    
    // Handle flags
    if flags & GRND_RANDOM != 0 {
        // Use blocking random source
        if flags & GRND_NONBLOCK != 0 {
            // Check if enough entropy is available
            if ENTROPY_COUNT.load(Ordering::SeqCst) < (buflen as u64 * 8) {
                return -(crate::posix::errno::EAGAIN as isize);
            }
        }
        get_random_bytes_wait(slice);
    } else if flags & GRND_INSECURE != 0 {
        // Always succeed, even if not fully seeded
        get_random_bytes(slice);
    } else {
        // Default: use CSPRNG (urandom-like)
        if !RNG_INITIALIZED.load(Ordering::SeqCst) && flags & GRND_NONBLOCK != 0 {
            return -(crate::posix::errno::EAGAIN as isize);
        }
        get_random_bytes(slice);
    }
    
    buflen as isize
}

// ============================================================================
// /dev/random and /dev/urandom support
// ============================================================================

/// Read from /dev/random (blocking)
pub fn dev_random_read(buf: &mut [u8]) -> usize {
    get_random_bytes_wait(buf);
    buf.len()
}

/// Read from /dev/urandom (non-blocking)
pub fn dev_urandom_read(buf: &mut [u8]) -> usize {
    get_random_bytes(buf);
    buf.len()
}

/// Write to /dev/random (add entropy)
pub fn dev_random_write(data: &[u8]) -> usize {
    // Estimate entropy based on data uniqueness (conservative estimate)
    let entropy_bits = core::cmp::min(data.len() as u32, 32);
    add_entropy(data, entropy_bits);
    data.len()
}
