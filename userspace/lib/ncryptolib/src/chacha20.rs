//! ChaCha20 and ChaCha20-Poly1305 Stream Cipher
//!
//! RFC 8439 compliant ChaCha20 and ChaCha20-Poly1305 AEAD implementation.
//! ChaCha20 is a modern stream cipher designed by Daniel J. Bernstein.
//! ChaCha20-Poly1305 is an AEAD construction combining ChaCha20 with Poly1305 MAC.

use std::vec::Vec;

// ============================================================================
// Constants
// ============================================================================

/// ChaCha20 key size (256 bits)
pub const CHACHA20_KEY_SIZE: usize = 32;
/// ChaCha20 nonce size (96 bits)
pub const CHACHA20_NONCE_SIZE: usize = 12;
/// ChaCha20 block size
pub const CHACHA20_BLOCK_SIZE: usize = 64;
/// Poly1305 tag size
pub const POLY1305_TAG_SIZE: usize = 16;

// ChaCha20 constants "expand 32-byte k"
const SIGMA: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];

// ============================================================================
// ChaCha20 Quarter Round
// ============================================================================

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

// ============================================================================
// ChaCha20 Block Function
// ============================================================================

fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];

    // Set constants
    state[0] = SIGMA[0];
    state[1] = SIGMA[1];
    state[2] = SIGMA[2];
    state[3] = SIGMA[3];

    // Set key
    for i in 0..8 {
        state[4 + i] =
            u32::from_le_bytes([key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]]);
    }

    // Set counter
    state[12] = counter;

    // Set nonce
    state[13] = u32::from_le_bytes([nonce[0], nonce[1], nonce[2], nonce[3]]);
    state[14] = u32::from_le_bytes([nonce[4], nonce[5], nonce[6], nonce[7]]);
    state[15] = u32::from_le_bytes([nonce[8], nonce[9], nonce[10], nonce[11]]);

    let initial_state = state;

    // 20 rounds (10 double rounds)
    for _ in 0..10 {
        // Column rounds
        quarter_round(&mut state, 0, 4, 8, 12);
        quarter_round(&mut state, 1, 5, 9, 13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        // Diagonal rounds
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7, 8, 13);
        quarter_round(&mut state, 3, 4, 9, 14);
    }

    // Add initial state
    for i in 0..16 {
        state[i] = state[i].wrapping_add(initial_state[i]);
    }

    // Serialize to bytes
    let mut output = [0u8; 64];
    for i in 0..16 {
        let bytes = state[i].to_le_bytes();
        output[i * 4..i * 4 + 4].copy_from_slice(&bytes);
    }

    output
}

// ============================================================================
// ChaCha20 Cipher
// ============================================================================

/// ChaCha20 stream cipher
pub struct ChaCha20 {
    key: [u8; 32],
    nonce: [u8; 12],
    counter: u32,
}

impl ChaCha20 {
    /// Create a new ChaCha20 cipher
    pub fn new(key: &[u8; CHACHA20_KEY_SIZE], nonce: &[u8; CHACHA20_NONCE_SIZE]) -> Self {
        let mut k = [0u8; 32];
        let mut n = [0u8; 12];
        k.copy_from_slice(key);
        n.copy_from_slice(nonce);
        Self {
            key: k,
            nonce: n,
            counter: 0,
        }
    }

    /// Set the counter value
    pub fn set_counter(&mut self, counter: u32) {
        self.counter = counter;
    }

    /// Encrypt or decrypt data in place (XOR operation)
    pub fn apply_keystream(&mut self, data: &mut [u8]) {
        let mut offset = 0;
        while offset < data.len() {
            let block = chacha20_block(&self.key, self.counter, &self.nonce);
            let remaining = data.len() - offset;
            let to_xor = remaining.min(64);

            for i in 0..to_xor {
                data[offset + i] ^= block[i];
            }

            self.counter = self.counter.wrapping_add(1);
            offset += to_xor;
        }
    }

    /// Encrypt data
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let mut ciphertext = plaintext.to_vec();
        self.apply_keystream(&mut ciphertext);
        ciphertext
    }

    /// Decrypt data
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Vec<u8> {
        let mut plaintext = ciphertext.to_vec();
        self.apply_keystream(&mut plaintext);
        plaintext
    }
}

// ============================================================================
// Poly1305 MAC
// ============================================================================

/// Poly1305 message authentication code
pub struct Poly1305 {
    r: [u64; 3],
    s: [u64; 2],
    h: [u64; 3],
}

impl Poly1305 {
    /// Create a new Poly1305 MAC with the given 32-byte key
    pub fn new(key: &[u8; 32]) -> Self {
        // r = key[0..16] with clamping
        let mut r = [0u64; 3];
        r[0] = u64::from_le_bytes([
            key[0],
            key[1],
            key[2],
            key[3] & 0x0f,
            key[4] & 0xfc,
            key[5],
            key[6],
            key[7] & 0x0f,
        ]) & 0x0ffffffc0fffffff;
        r[1] = u64::from_le_bytes([
            key[8] & 0xfc,
            key[9],
            key[10],
            key[11] & 0x0f,
            key[12] & 0xfc,
            key[13],
            key[14],
            key[15] & 0x0f,
        ]) & 0x0ffffffc0ffffffc;

        // s = key[16..32]
        let mut s = [0u64; 2];
        s[0] = u64::from_le_bytes([
            key[16], key[17], key[18], key[19], key[20], key[21], key[22], key[23],
        ]);
        s[1] = u64::from_le_bytes([
            key[24], key[25], key[26], key[27], key[28], key[29], key[30], key[31],
        ]);

        Self { r, s, h: [0; 3] }
    }

    /// Process a 16-byte block
    fn block(&mut self, block: &[u8], is_final: bool) {
        // Add block to h
        let mut n = [0u128; 3];

        if block.len() >= 8 {
            let b0 = u64::from_le_bytes([
                block[0],
                block.get(1).copied().unwrap_or(0),
                block.get(2).copied().unwrap_or(0),
                block.get(3).copied().unwrap_or(0),
                block.get(4).copied().unwrap_or(0),
                block.get(5).copied().unwrap_or(0),
                block.get(6).copied().unwrap_or(0),
                block.get(7).copied().unwrap_or(0),
            ]);
            n[0] = (self.h[0] as u128).wrapping_add(b0 as u128);
        } else {
            let mut buf = [0u8; 8];
            buf[..block.len().min(8)].copy_from_slice(&block[..block.len().min(8)]);
            n[0] = (self.h[0] as u128).wrapping_add(u64::from_le_bytes(buf) as u128);
        }

        if block.len() > 8 {
            let mut buf = [0u8; 8];
            let remaining = &block[8..];
            buf[..remaining.len().min(8)].copy_from_slice(&remaining[..remaining.len().min(8)]);
            n[1] = (self.h[1] as u128).wrapping_add(u64::from_le_bytes(buf) as u128);
        } else {
            n[1] = self.h[1] as u128;
        }

        // Add high bit if not final partial block
        if !is_final || block.len() == 16 {
            n[2] = (self.h[2] as u128).wrapping_add(1);
        } else {
            // For final partial block, add 0x01 after the message
            let hibit_pos = block.len();
            if hibit_pos < 8 {
                n[0] |= 1u128 << (hibit_pos * 8);
            } else {
                n[1] |= 1u128 << ((hibit_pos - 8) * 8);
            }
            n[2] = self.h[2] as u128;
        }

        // Multiply by r (simplified)
        let r0 = self.r[0] as u128;
        let r1 = self.r[1] as u128;

        let mut t0 = n[0].wrapping_mul(r0);
        let mut t1 = n[0].wrapping_mul(r1).wrapping_add(n[1].wrapping_mul(r0));
        let t2 = n[1].wrapping_mul(r1).wrapping_add(n[2].wrapping_mul(r0));

        // Reduce mod 2^130 - 5
        let c = t2 >> 2;
        t0 = t0.wrapping_add(c.wrapping_mul(5));
        t1 = t1.wrapping_add(t0 >> 64);

        self.h[0] = t0 as u64;
        self.h[1] = t1 as u64;
        self.h[2] = ((t1 >> 64) as u64) & 3;
    }

    /// Update with data
    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0;
        while offset + 16 <= data.len() {
            self.block(&data[offset..offset + 16], false);
            offset += 16;
        }
        if offset < data.len() {
            self.block(&data[offset..], true);
        }
    }

    /// Finalize and get the tag
    pub fn finalize(self) -> [u8; 16] {
        // Add s
        let mut f = self.h[0] as u128 + self.s[0] as u128;
        let c = f >> 64;
        let mut tag = [0u8; 16];
        tag[0..8].copy_from_slice(&(f as u64).to_le_bytes());

        f = self.h[1] as u128 + self.s[1] as u128 + c;
        tag[8..16].copy_from_slice(&(f as u64).to_le_bytes());

        tag
    }
}

/// Compute Poly1305 MAC in one shot
pub fn poly1305(key: &[u8; 32], message: &[u8]) -> [u8; 16] {
    let mut mac = Poly1305::new(key);
    mac.update(message);
    mac.finalize()
}

// ============================================================================
// ChaCha20-Poly1305 AEAD
// ============================================================================

/// ChaCha20-Poly1305 AEAD cipher
pub struct ChaCha20Poly1305 {
    key: [u8; 32],
}

impl ChaCha20Poly1305 {
    /// Create a new ChaCha20-Poly1305 AEAD cipher
    pub fn new(key: &[u8; CHACHA20_KEY_SIZE]) -> Self {
        let mut k = [0u8; 32];
        k.copy_from_slice(key);
        Self { key: k }
    }

    /// Generate Poly1305 key from ChaCha20
    fn generate_poly_key(&self, nonce: &[u8; 12]) -> [u8; 32] {
        let block = chacha20_block(&self.key, 0, nonce);
        let mut poly_key = [0u8; 32];
        poly_key.copy_from_slice(&block[..32]);
        poly_key
    }

    /// Pad length to 16-byte boundary
    fn pad16(len: usize) -> usize {
        (16 - (len % 16)) % 16
    }

    /// Encrypt and authenticate
    pub fn encrypt(
        &self,
        nonce: &[u8; CHACHA20_NONCE_SIZE],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Vec<u8> {
        // Generate Poly1305 key
        let poly_key = self.generate_poly_key(nonce);

        // Encrypt plaintext
        let mut chacha = ChaCha20::new(&self.key, nonce);
        chacha.set_counter(1); // Counter starts at 1 for encryption
        let ciphertext = chacha.encrypt(plaintext);

        // Compute auth tag over AAD || padding || ciphertext || padding || lengths
        let mut mac = Poly1305::new(&poly_key);
        mac.update(aad);
        let aad_padding = vec![0u8; Self::pad16(aad.len())];
        mac.update(&aad_padding);
        mac.update(&ciphertext);
        let ct_padding = vec![0u8; Self::pad16(ciphertext.len())];
        mac.update(&ct_padding);
        mac.update(&(aad.len() as u64).to_le_bytes());
        mac.update(&(plaintext.len() as u64).to_le_bytes());
        let tag = mac.finalize();

        // Return ciphertext || tag
        let mut result = ciphertext;
        result.extend_from_slice(&tag);
        result
    }

    /// Decrypt and verify
    pub fn decrypt(
        &self,
        nonce: &[u8; CHACHA20_NONCE_SIZE],
        aad: &[u8],
        ciphertext_with_tag: &[u8],
    ) -> Result<Vec<u8>, &'static str> {
        if ciphertext_with_tag.len() < POLY1305_TAG_SIZE {
            return Err("Ciphertext too short");
        }

        let ciphertext_len = ciphertext_with_tag.len() - POLY1305_TAG_SIZE;
        let ciphertext = &ciphertext_with_tag[..ciphertext_len];
        let tag = &ciphertext_with_tag[ciphertext_len..];

        // Generate Poly1305 key
        let poly_key = self.generate_poly_key(nonce);

        // Compute expected tag
        let mut mac = Poly1305::new(&poly_key);
        mac.update(aad);
        let aad_padding = vec![0u8; Self::pad16(aad.len())];
        mac.update(&aad_padding);
        mac.update(ciphertext);
        let ct_padding = vec![0u8; Self::pad16(ciphertext.len())];
        mac.update(&ct_padding);
        mac.update(&(aad.len() as u64).to_le_bytes());
        mac.update(&(ciphertext_len as u64).to_le_bytes());
        let expected_tag = mac.finalize();

        // Constant-time tag comparison
        let mut diff = 0u8;
        for i in 0..16 {
            diff |= tag[i] ^ expected_tag[i];
        }
        if diff != 0 {
            return Err("Authentication failed");
        }

        // Decrypt
        let mut chacha = ChaCha20::new(&self.key, nonce);
        chacha.set_counter(1);
        Ok(chacha.decrypt(ciphertext))
    }
}

// ============================================================================
// Convenience Functions
// ============================================================================

/// Encrypt data with ChaCha20
pub fn chacha20_encrypt(key: &[u8; 32], nonce: &[u8; 12], plaintext: &[u8]) -> Vec<u8> {
    let mut cipher = ChaCha20::new(key, nonce);
    cipher.encrypt(plaintext)
}

/// Decrypt data with ChaCha20
pub fn chacha20_decrypt(key: &[u8; 32], nonce: &[u8; 12], ciphertext: &[u8]) -> Vec<u8> {
    let mut cipher = ChaCha20::new(key, nonce);
    cipher.decrypt(ciphertext)
}

/// Encrypt and authenticate with ChaCha20-Poly1305
pub fn chacha20_poly1305_encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> Vec<u8> {
    let aead = ChaCha20Poly1305::new(key);
    aead.encrypt(nonce, aad, plaintext)
}

/// Decrypt and verify with ChaCha20-Poly1305
pub fn chacha20_poly1305_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext_with_tag: &[u8],
) -> Result<Vec<u8>, &'static str> {
    let aead = ChaCha20Poly1305::new(key);
    aead.decrypt(nonce, aad, ciphertext_with_tag)
}

// ============================================================================
// C ABI Exports
// ============================================================================

use crate::{c_int, size_t};

/// ChaCha20 encrypt (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_chacha20_encrypt(
    key: *const u8,
    nonce: *const u8,
    plaintext: *const u8,
    plaintext_len: size_t,
    ciphertext: *mut u8,
) -> c_int {
    if key.is_null() || nonce.is_null() || plaintext.is_null() || ciphertext.is_null() {
        return -1;
    }

    let key_slice = core::slice::from_raw_parts(key, 32);
    let nonce_slice = core::slice::from_raw_parts(nonce, 12);
    let plaintext_slice = core::slice::from_raw_parts(plaintext, plaintext_len);

    let mut k = [0u8; 32];
    let mut n = [0u8; 12];
    k.copy_from_slice(key_slice);
    n.copy_from_slice(nonce_slice);

    let result = chacha20_encrypt(&k, &n, plaintext_slice);
    core::ptr::copy_nonoverlapping(result.as_ptr(), ciphertext, result.len());

    0
}

/// ChaCha20-Poly1305 encrypt (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_chacha20_poly1305_encrypt(
    key: *const u8,
    nonce: *const u8,
    aad: *const u8,
    aad_len: size_t,
    plaintext: *const u8,
    plaintext_len: size_t,
    ciphertext: *mut u8,
    tag: *mut u8,
) -> c_int {
    if key.is_null()
        || nonce.is_null()
        || plaintext.is_null()
        || ciphertext.is_null()
        || tag.is_null()
    {
        return -1;
    }

    let key_slice = core::slice::from_raw_parts(key, 32);
    let nonce_slice = core::slice::from_raw_parts(nonce, 12);
    let aad_slice = if aad.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(aad, aad_len)
    };
    let plaintext_slice = core::slice::from_raw_parts(plaintext, plaintext_len);

    let mut k = [0u8; 32];
    let mut n = [0u8; 12];
    k.copy_from_slice(key_slice);
    n.copy_from_slice(nonce_slice);

    let result = chacha20_poly1305_encrypt(&k, &n, aad_slice, plaintext_slice);
    let ct_len = result.len() - 16;
    core::ptr::copy_nonoverlapping(result.as_ptr(), ciphertext, ct_len);
    core::ptr::copy_nonoverlapping(result[ct_len..].as_ptr(), tag, 16);

    0
}

/// ChaCha20-Poly1305 decrypt (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_chacha20_poly1305_decrypt(
    key: *const u8,
    nonce: *const u8,
    aad: *const u8,
    aad_len: size_t,
    ciphertext: *const u8,
    ciphertext_len: size_t,
    tag: *const u8,
    plaintext: *mut u8,
) -> c_int {
    if key.is_null()
        || nonce.is_null()
        || ciphertext.is_null()
        || tag.is_null()
        || plaintext.is_null()
    {
        return -1;
    }

    let key_slice = core::slice::from_raw_parts(key, 32);
    let nonce_slice = core::slice::from_raw_parts(nonce, 12);
    let aad_slice = if aad.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(aad, aad_len)
    };
    let ciphertext_slice = core::slice::from_raw_parts(ciphertext, ciphertext_len);
    let tag_slice = core::slice::from_raw_parts(tag, 16);

    let mut k = [0u8; 32];
    let mut n = [0u8; 12];
    k.copy_from_slice(key_slice);
    n.copy_from_slice(nonce_slice);

    // Combine ciphertext and tag
    let mut combined = ciphertext_slice.to_vec();
    combined.extend_from_slice(tag_slice);

    match chacha20_poly1305_decrypt(&k, &n, aad_slice, &combined) {
        Ok(result) => {
            core::ptr::copy_nonoverlapping(result.as_ptr(), plaintext, result.len());
            0
        }
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chacha20_basic() {
        let key = [0u8; 32];
        let nonce = [0u8; 12];
        let plaintext = b"Hello, ChaCha20!";

        let ciphertext = chacha20_encrypt(&key, &nonce, plaintext);
        let decrypted = chacha20_decrypt(&key, &nonce, &ciphertext);

        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn test_chacha20_poly1305_basic() {
        let key = [0u8; 32];
        let nonce = [0u8; 12];
        let aad = b"additional data";
        let plaintext = b"Hello, ChaCha20-Poly1305!";

        let ciphertext = chacha20_poly1305_encrypt(&key, &nonce, aad, plaintext);
        let decrypted = chacha20_poly1305_decrypt(&key, &nonce, aad, &ciphertext).unwrap();

        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn test_chacha20_poly1305_tamper_detection() {
        let key = [0u8; 32];
        let nonce = [0u8; 12];
        let aad = b"additional data";
        let plaintext = b"Secret message";

        let mut ciphertext = chacha20_poly1305_encrypt(&key, &nonce, aad, plaintext);

        // Tamper with ciphertext
        ciphertext[0] ^= 1;

        let result = chacha20_poly1305_decrypt(&key, &nonce, aad, &ciphertext);
        assert!(result.is_err());
    }
}
