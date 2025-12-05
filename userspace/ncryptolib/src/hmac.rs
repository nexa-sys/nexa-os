//! HMAC (Hash-based Message Authentication Code)
//!
//! RFC 2104 compliant HMAC implementation.
//! Supports multiple hash functions: SHA-256, SHA-384, SHA-512, SHA3-256.

use std::vec::Vec;

use crate::hash::{Sha384, Sha512, SHA384_BLOCK_SIZE, SHA512_BLOCK_SIZE};
use crate::sha3::Sha3;

// ============================================================================
// HMAC Trait
// ============================================================================

/// Generic HMAC interface
pub trait Hmac: Sized {
    /// Output size in bytes
    const OUTPUT_SIZE: usize;
    /// Block size in bytes
    const BLOCK_SIZE: usize;

    /// Create new HMAC instance with key
    fn new(key: &[u8]) -> Self;
    /// Update with data
    fn update(&mut self, data: &[u8]);
    /// Finalize and get MAC
    fn finalize(self) -> Vec<u8>;

    /// One-shot HMAC computation
    fn mac(key: &[u8], data: &[u8]) -> Vec<u8> {
        let mut hmac = Self::new(key);
        hmac.update(data);
        hmac.finalize()
    }
}

// ============================================================================
// HMAC-SHA384
// ============================================================================

/// HMAC-SHA384
pub struct HmacSha384 {
    inner: Sha384,
    outer_key: Vec<u8>,
}

impl Hmac for HmacSha384 {
    const OUTPUT_SIZE: usize = 48;
    const BLOCK_SIZE: usize = SHA384_BLOCK_SIZE;

    fn new(key: &[u8]) -> Self {
        let mut padded_key = vec![0u8; Self::BLOCK_SIZE];

        if key.len() > Self::BLOCK_SIZE {
            let hash = crate::hash::sha384(key);
            padded_key[..hash.len()].copy_from_slice(&hash);
        } else {
            padded_key[..key.len()].copy_from_slice(key);
        }

        // Inner key = key XOR ipad (0x36)
        let mut inner_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            inner_key[i] = padded_key[i] ^ 0x36;
        }

        // Outer key = key XOR opad (0x5c)
        let mut outer_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            outer_key[i] = padded_key[i] ^ 0x5c;
        }

        let mut inner = Sha512::new_384();
        inner.update(&inner_key);

        Self { inner, outer_key }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finalize(mut self) -> Vec<u8> {
        let inner_hash = self.inner.finalize();
        let mut outer = Sha512::new_384();
        outer.update(&self.outer_key);
        outer.update(&inner_hash);
        outer.finalize().to_vec()
    }
}

/// Compute HMAC-SHA384
pub fn hmac_sha384(key: &[u8], data: &[u8]) -> [u8; 48] {
    let result = HmacSha384::mac(key, data);
    let mut out = [0u8; 48];
    out.copy_from_slice(&result);
    out
}

// ============================================================================
// HMAC-SHA512
// ============================================================================

/// HMAC-SHA512
pub struct HmacSha512 {
    inner: Sha512,
    outer_key: Vec<u8>,
}

impl Hmac for HmacSha512 {
    const OUTPUT_SIZE: usize = 64;
    const BLOCK_SIZE: usize = SHA512_BLOCK_SIZE;

    fn new(key: &[u8]) -> Self {
        let mut padded_key = vec![0u8; Self::BLOCK_SIZE];

        if key.len() > Self::BLOCK_SIZE {
            let hash = crate::hash::sha512(key);
            padded_key[..hash.len()].copy_from_slice(&hash);
        } else {
            padded_key[..key.len()].copy_from_slice(key);
        }

        let mut inner_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            inner_key[i] = padded_key[i] ^ 0x36;
        }

        let mut outer_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            outer_key[i] = padded_key[i] ^ 0x5c;
        }

        let mut inner = Sha512::new();
        inner.update(&inner_key);

        Self { inner, outer_key }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finalize(mut self) -> Vec<u8> {
        let inner_hash = self.inner.finalize();
        let mut outer = Sha512::new();
        outer.update(&self.outer_key);
        outer.update(&inner_hash);
        outer.finalize().to_vec()
    }
}

/// Compute HMAC-SHA512
pub fn hmac_sha512(key: &[u8], data: &[u8]) -> [u8; 64] {
    let result = HmacSha512::mac(key, data);
    let mut out = [0u8; 64];
    out.copy_from_slice(&result);
    out
}

// ============================================================================
// HMAC-SHA3-256
// ============================================================================

/// SHA3-256 block size (rate)
const SHA3_256_BLOCK_SIZE: usize = 136;

/// HMAC-SHA3-256
pub struct HmacSha3_256 {
    inner: Sha3,
    outer_key: Vec<u8>,
}

impl Hmac for HmacSha3_256 {
    const OUTPUT_SIZE: usize = 32;
    const BLOCK_SIZE: usize = SHA3_256_BLOCK_SIZE;

    fn new(key: &[u8]) -> Self {
        let mut padded_key = vec![0u8; Self::BLOCK_SIZE];

        if key.len() > Self::BLOCK_SIZE {
            let hash = crate::sha3::sha3_256(key);
            padded_key[..hash.len()].copy_from_slice(&hash);
        } else {
            padded_key[..key.len()].copy_from_slice(key);
        }

        let mut inner_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            inner_key[i] = padded_key[i] ^ 0x36;
        }

        let mut outer_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            outer_key[i] = padded_key[i] ^ 0x5c;
        }

        let mut inner = Sha3::new_256();
        inner.update(&inner_key);

        Self { inner, outer_key }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finalize(mut self) -> Vec<u8> {
        let inner_hash = self.inner.finalize();
        let mut outer = Sha3::new_256();
        outer.update(&self.outer_key);
        outer.update(&inner_hash);
        outer.finalize().to_vec()
    }
}

/// Compute HMAC-SHA3-256
pub fn hmac_sha3_256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let result = HmacSha3_256::mac(key, data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

// ============================================================================
// OpenSSL HMAC_CTX Compatible Interface
// ============================================================================

use std::boxed::Box;
use crate::hash::{Sha256, SHA256_BLOCK_SIZE, HmacSha256};
use crate::evp::{EvpMd, EvpMdType};

/// HMAC Context (OpenSSL compatible)
pub struct HMAC_CTX {
    /// Algorithm type
    md_type: EvpMdType,
    /// Key (padded)
    key: Vec<u8>,
    /// Inner hash state
    state: HmacState,
    /// Has been initialized
    initialized: bool,
}

enum HmacState {
    None,
    Sha256(HmacSha256),
    Sha384(HmacSha384),
    Sha512(HmacSha512),
    Sha3_256(HmacSha3_256),
}

impl HMAC_CTX {
    pub fn new() -> Self {
        Self {
            md_type: EvpMdType::Sha256,
            key: Vec::new(),
            state: HmacState::None,
            initialized: false,
        }
    }

    pub fn init(&mut self, key: &[u8], md: &EvpMd) -> bool {
        self.md_type = md.md_type;
        self.key = key.to_vec();
        self.state = match md.md_type {
            EvpMdType::Sha256 => HmacState::Sha256(HmacSha256::new(key)),
            EvpMdType::Sha384 => HmacState::Sha384(HmacSha384::new(key)),
            EvpMdType::Sha512 => HmacState::Sha512(HmacSha512::new(key)),
            EvpMdType::Sha3_256 => HmacState::Sha3_256(HmacSha3_256::new(key)),
            _ => return false,
        };
        self.initialized = true;
        true
    }

    pub fn update(&mut self, data: &[u8]) -> bool {
        if !self.initialized {
            return false;
        }
        match &mut self.state {
            HmacState::Sha256(h) => h.update(data),
            HmacState::Sha384(h) => h.update(data),
            HmacState::Sha512(h) => h.update(data),
            HmacState::Sha3_256(h) => h.update(data),
            HmacState::None => return false,
        }
        true
    }

    pub fn finalize(&mut self, out: &mut [u8]) -> usize {
        if !self.initialized {
            return 0;
        }
        let result: Vec<u8> = match core::mem::replace(&mut self.state, HmacState::None) {
            HmacState::Sha256(mut h) => h.finalize().to_vec(),
            HmacState::Sha384(h) => h.finalize(),
            HmacState::Sha512(h) => h.finalize(),
            HmacState::Sha3_256(h) => h.finalize(),
            HmacState::None => return 0,
        };
        let len = result.len().min(out.len());
        out[..len].copy_from_slice(&result[..len]);
        self.initialized = false;
        len
    }

    pub fn reset(&mut self) -> bool {
        if self.key.is_empty() {
            return false;
        }
        self.state = match self.md_type {
            EvpMdType::Sha256 => HmacState::Sha256(HmacSha256::new(&self.key)),
            EvpMdType::Sha384 => HmacState::Sha384(HmacSha384::new(&self.key)),
            EvpMdType::Sha512 => HmacState::Sha512(HmacSha512::new(&self.key)),
            EvpMdType::Sha3_256 => HmacState::Sha3_256(HmacSha3_256::new(&self.key)),
            _ => return false,
        };
        self.initialized = true;
        true
    }
}

impl Default for HMAC_CTX {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// C ABI Exports
// ============================================================================

use crate::{c_int, size_t};

/// HMAC_CTX_new - Create new HMAC context
#[no_mangle]
pub extern "C" fn HMAC_CTX_new() -> *mut HMAC_CTX {
    Box::into_raw(Box::new(HMAC_CTX::new()))
}

/// HMAC_CTX_free - Free HMAC context
#[no_mangle]
pub extern "C" fn HMAC_CTX_free(ctx: *mut HMAC_CTX) {
    if !ctx.is_null() {
        unsafe { drop(Box::from_raw(ctx)); }
    }
}

/// HMAC_CTX_reset - Reset HMAC context
#[no_mangle]
pub extern "C" fn HMAC_CTX_reset(ctx: *mut HMAC_CTX) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    if unsafe { (*ctx).reset() } { 1 } else { 0 }
}

/// HMAC_Init_ex - Initialize HMAC context
#[no_mangle]
pub extern "C" fn HMAC_Init_ex(
    ctx: *mut HMAC_CTX,
    key: *const u8,
    len: c_int,
    md: *const EvpMd,
    _engine: *mut core::ffi::c_void,
) -> c_int {
    if ctx.is_null() || md.is_null() {
        return 0;
    }

    unsafe {
        let ctx_ref = &mut *ctx;
        let md_ref = &*md;
        
        // Clone the key if using existing key
        let key_vec: Vec<u8> = if key.is_null() || len <= 0 {
            ctx_ref.key.clone()
        } else {
            core::slice::from_raw_parts(key, len as usize).to_vec()
        };

        if ctx_ref.init(&key_vec, md_ref) { 1 } else { 0 }
    }
}

/// HMAC_Update - Update HMAC with data
#[no_mangle]
pub extern "C" fn HMAC_Update(ctx: *mut HMAC_CTX, data: *const u8, len: usize) -> c_int {
    if ctx.is_null() || data.is_null() {
        return 0;
    }
    let data_slice = unsafe { core::slice::from_raw_parts(data, len) };
    if unsafe { (*ctx).update(data_slice) } { 1 } else { 0 }
}

/// HMAC_Final - Finalize HMAC
#[no_mangle]
pub extern "C" fn HMAC_Final(ctx: *mut HMAC_CTX, md: *mut u8, len: *mut u32) -> c_int {
    if ctx.is_null() || md.is_null() {
        return 0;
    }
    let mut buf = [0u8; 64]; // Max hash size
    let result_len = unsafe { (*ctx).finalize(&mut buf) };
    
    unsafe {
        core::ptr::copy_nonoverlapping(buf.as_ptr(), md, result_len);
        if !len.is_null() {
            *len = result_len as u32;
        }
    }
    1
}

/// HMAC - One-shot HMAC computation
#[no_mangle]
pub extern "C" fn HMAC(
    evp_md: *const EvpMd,
    key: *const u8,
    key_len: c_int,
    data: *const u8,
    data_len: usize,
    md: *mut u8,
    md_len: *mut u32,
) -> *mut u8 {
    if evp_md.is_null() || key.is_null() || data.is_null() || md.is_null() {
        return core::ptr::null_mut();
    }

    let evp_md = unsafe { &*evp_md };
    let key_slice = unsafe { core::slice::from_raw_parts(key, key_len as usize) };
    let data_slice = unsafe { core::slice::from_raw_parts(data, data_len) };

    let result: Vec<u8> = match evp_md.md_type {
        EvpMdType::Sha256 => {
            let mac = crate::hash::hmac_sha256(key_slice, data_slice);
            mac.to_vec()
        }
        EvpMdType::Sha384 => {
            hmac_sha384(key_slice, data_slice).to_vec()
        }
        EvpMdType::Sha512 => {
            hmac_sha512(key_slice, data_slice).to_vec()
        }
        EvpMdType::Sha3_256 => {
            hmac_sha3_256(key_slice, data_slice).to_vec()
        }
        _ => return core::ptr::null_mut(),
    };

    unsafe {
        core::ptr::copy_nonoverlapping(result.as_ptr(), md, result.len());
        if !md_len.is_null() {
            *md_len = result.len() as u32;
        }
    }

    md
}

/// HMAC-SHA384 (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_hmac_sha384(
    key: *const u8,
    key_len: size_t,
    data: *const u8,
    data_len: size_t,
    output: *mut u8,
) -> c_int {
    if key.is_null() || data.is_null() || output.is_null() {
        return -1;
    }

    let key_slice = core::slice::from_raw_parts(key, key_len);
    let data_slice = core::slice::from_raw_parts(data, data_len);

    let mac = hmac_sha384(key_slice, data_slice);
    core::ptr::copy_nonoverlapping(mac.as_ptr(), output, 48);

    0
}

/// HMAC-SHA512 (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_hmac_sha512(
    key: *const u8,
    key_len: size_t,
    data: *const u8,
    data_len: size_t,
    output: *mut u8,
) -> c_int {
    if key.is_null() || data.is_null() || output.is_null() {
        return -1;
    }

    let key_slice = core::slice::from_raw_parts(key, key_len);
    let data_slice = core::slice::from_raw_parts(data, data_len);

    let mac = hmac_sha512(key_slice, data_slice);
    core::ptr::copy_nonoverlapping(mac.as_ptr(), output, 64);

    0
}

/// HMAC-SHA3-256 (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_hmac_sha3_256(
    key: *const u8,
    key_len: size_t,
    data: *const u8,
    data_len: size_t,
    output: *mut u8,
) -> c_int {
    if key.is_null() || data.is_null() || output.is_null() {
        return -1;
    }

    let key_slice = core::slice::from_raw_parts(key, key_len);
    let data_slice = core::slice::from_raw_parts(data, data_len);

    let mac = hmac_sha3_256(key_slice, data_slice);
    core::ptr::copy_nonoverlapping(mac.as_ptr(), output, 32);

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_sha384() {
        let key = b"key";
        let data = b"The quick brown fox jumps over the lazy dog";
        let mac = hmac_sha384(key, data);
        assert_eq!(mac.len(), 48);
    }

    #[test]
    fn test_hmac_sha512() {
        let key = b"key";
        let data = b"The quick brown fox jumps over the lazy dog";
        let mac = hmac_sha512(key, data);
        assert_eq!(mac.len(), 64);
    }

    #[test]
    fn test_hmac_sha3_256() {
        let key = b"key";
        let data = b"The quick brown fox jumps over the lazy dog";
        let mac = hmac_sha3_256(key, data);
        assert_eq!(mac.len(), 32);
    }

    #[test]
    fn test_hmac_deterministic() {
        let key = b"secret_key";
        let data = b"test data";

        let mac1 = hmac_sha512(key, data);
        let mac2 = hmac_sha512(key, data);
        assert_eq!(mac1, mac2);
    }
}
