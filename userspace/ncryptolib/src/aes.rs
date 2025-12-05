//! AES Symmetric Encryption (AES-128, AES-256)
//!
//! FIPS 197 compliant AES implementation with GCM, CTR, and CBC modes.
//! SP 800-38D compliant AES-GCM implementation.

// ============================================================================
// AES Constants
// ============================================================================

/// AES block size in bytes
pub const AES_BLOCK_SIZE: usize = 16;

/// AES-128 key size
pub const AES_128_KEY_SIZE: usize = 16;
/// AES-256 key size
pub const AES_256_KEY_SIZE: usize = 32;

/// GCM tag size
pub const GCM_TAG_SIZE: usize = 16;
/// GCM nonce size (recommended)
pub const GCM_NONCE_SIZE: usize = 12;

// AES S-box
const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

// Inverse S-box for decryption
const INV_SBOX: [u8; 256] = [
    0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7, 0xfb,
    0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde, 0xe9, 0xcb,
    0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e,
    0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25,
    0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92,
    0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84,
    0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
    0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b,
    0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73,
    0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e,
    0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b,
    0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4,
    0x1f, 0xdd, 0xa8, 0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f,
    0x60, 0x51, 0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
    0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
    0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c, 0x7d,
];

// Round constants
const RCON: [u8; 11] = [0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

// ============================================================================
// AES Core Implementation
// ============================================================================

/// AES-128 cipher
#[derive(Clone)]
pub struct Aes128 {
    round_keys: [[u8; 16]; 11],
}

impl Aes128 {
    /// Create a new AES-128 cipher with the given key
    pub fn new(key: &[u8; AES_128_KEY_SIZE]) -> Self {
        let round_keys = Self::key_expansion(key);
        Self { round_keys }
    }

    fn key_expansion(key: &[u8; 16]) -> [[u8; 16]; 11] {
        let mut round_keys = [[0u8; 16]; 11];
        round_keys[0].copy_from_slice(key);

        for i in 1..11 {
            let mut temp = [0u8; 4];
            temp.copy_from_slice(&round_keys[i - 1][12..16]);

            // RotWord
            temp.rotate_left(1);

            // SubWord
            for b in temp.iter_mut() {
                *b = SBOX[*b as usize];
            }

            // XOR with Rcon
            temp[0] ^= RCON[i];

            // Generate round key
            for j in 0..4 {
                for k in 0..4 {
                    round_keys[i][j * 4 + k] = round_keys[i - 1][j * 4 + k] ^ temp[k];
                }
                if j < 3 {
                    temp.copy_from_slice(&round_keys[i][j * 4..(j + 1) * 4]);
                }
            }
        }

        round_keys
    }

    /// Encrypt a single block
    pub fn encrypt_block(&self, plaintext: &[u8; 16]) -> [u8; 16] {
        let mut state = *plaintext;

        // Initial round key addition
        xor_block(&mut state, &self.round_keys[0]);

        // Main rounds
        for round in 1..10 {
            sub_bytes(&mut state);
            shift_rows(&mut state);
            mix_columns(&mut state);
            xor_block(&mut state, &self.round_keys[round]);
        }

        // Final round (no MixColumns)
        sub_bytes(&mut state);
        shift_rows(&mut state);
        xor_block(&mut state, &self.round_keys[10]);

        state
    }

    /// Decrypt a single block
    pub fn decrypt_block(&self, ciphertext: &[u8; 16]) -> [u8; 16] {
        let mut state = *ciphertext;

        // Initial round key addition
        xor_block(&mut state, &self.round_keys[10]);

        // Main rounds (in reverse)
        for round in (1..10).rev() {
            inv_shift_rows(&mut state);
            inv_sub_bytes(&mut state);
            xor_block(&mut state, &self.round_keys[round]);
            inv_mix_columns(&mut state);
        }

        // Final round
        inv_shift_rows(&mut state);
        inv_sub_bytes(&mut state);
        xor_block(&mut state, &self.round_keys[0]);

        state
    }
}

/// AES-256 cipher
#[derive(Clone)]
pub struct Aes256 {
    round_keys: [[u8; 16]; 15],
}

impl Aes256 {
    /// Create a new AES-256 cipher with the given key
    pub fn new(key: &[u8; AES_256_KEY_SIZE]) -> Self {
        let round_keys = Self::key_expansion(key);
        Self { round_keys }
    }

    fn key_expansion(key: &[u8; 32]) -> [[u8; 16]; 15] {
        let mut expanded = [0u8; 240]; // 15 * 16 bytes
        expanded[..32].copy_from_slice(key);

        let mut i = 8; // Start after initial 8 words (32 bytes)
        while i < 60 {
            let mut temp = [0u8; 4];
            temp.copy_from_slice(&expanded[(i - 1) * 4..i * 4]);

            if i % 8 == 0 {
                temp.rotate_left(1);
                for b in temp.iter_mut() {
                    *b = SBOX[*b as usize];
                }
                temp[0] ^= RCON[i / 8];
            } else if i % 8 == 4 {
                for b in temp.iter_mut() {
                    *b = SBOX[*b as usize];
                }
            }

            for j in 0..4 {
                expanded[i * 4 + j] = expanded[(i - 8) * 4 + j] ^ temp[j];
            }
            i += 1;
        }

        let mut round_keys = [[0u8; 16]; 15];
        for (i, rk) in round_keys.iter_mut().enumerate() {
            rk.copy_from_slice(&expanded[i * 16..(i + 1) * 16]);
        }

        round_keys
    }

    /// Encrypt a single block
    pub fn encrypt_block(&self, plaintext: &[u8; 16]) -> [u8; 16] {
        let mut state = *plaintext;

        xor_block(&mut state, &self.round_keys[0]);

        for round in 1..14 {
            sub_bytes(&mut state);
            shift_rows(&mut state);
            mix_columns(&mut state);
            xor_block(&mut state, &self.round_keys[round]);
        }

        sub_bytes(&mut state);
        shift_rows(&mut state);
        xor_block(&mut state, &self.round_keys[14]);

        state
    }

    /// Decrypt a single block
    pub fn decrypt_block(&self, ciphertext: &[u8; 16]) -> [u8; 16] {
        let mut state = *ciphertext;

        xor_block(&mut state, &self.round_keys[14]);

        for round in (1..14).rev() {
            inv_shift_rows(&mut state);
            inv_sub_bytes(&mut state);
            xor_block(&mut state, &self.round_keys[round]);
            inv_mix_columns(&mut state);
        }

        inv_shift_rows(&mut state);
        inv_sub_bytes(&mut state);
        xor_block(&mut state, &self.round_keys[0]);

        state
    }
}

// ============================================================================
// AES Helper Functions
// ============================================================================

#[inline]
fn xor_block(state: &mut [u8; 16], key: &[u8; 16]) {
    for i in 0..16 {
        state[i] ^= key[i];
    }
}

#[inline]
fn sub_bytes(state: &mut [u8; 16]) {
    for b in state.iter_mut() {
        *b = SBOX[*b as usize];
    }
}

#[inline]
fn inv_sub_bytes(state: &mut [u8; 16]) {
    for b in state.iter_mut() {
        *b = INV_SBOX[*b as usize];
    }
}

#[inline]
fn shift_rows(state: &mut [u8; 16]) {
    // Row 1: shift left by 1
    let tmp = state[1];
    state[1] = state[5];
    state[5] = state[9];
    state[9] = state[13];
    state[13] = tmp;

    // Row 2: shift left by 2
    state.swap(2, 10);
    state.swap(6, 14);

    // Row 3: shift left by 3 (= right by 1)
    let tmp = state[15];
    state[15] = state[11];
    state[11] = state[7];
    state[7] = state[3];
    state[3] = tmp;
}

#[inline]
fn inv_shift_rows(state: &mut [u8; 16]) {
    // Row 1: shift right by 1
    let tmp = state[13];
    state[13] = state[9];
    state[9] = state[5];
    state[5] = state[1];
    state[1] = tmp;

    // Row 2: shift right by 2
    state.swap(2, 10);
    state.swap(6, 14);

    // Row 3: shift right by 3 (= left by 1)
    let tmp = state[3];
    state[3] = state[7];
    state[7] = state[11];
    state[11] = state[15];
    state[15] = tmp;
}

#[inline]
fn xtime(x: u8) -> u8 {
    (x << 1) ^ (((x >> 7) & 1) * 0x1b)
}

#[inline]
fn mix_columns(state: &mut [u8; 16]) {
    for i in 0..4 {
        let col = i * 4;
        let a = state[col];
        let b = state[col + 1];
        let c = state[col + 2];
        let d = state[col + 3];
        let e = a ^ b ^ c ^ d;

        state[col] ^= e ^ xtime(a ^ b);
        state[col + 1] ^= e ^ xtime(b ^ c);
        state[col + 2] ^= e ^ xtime(c ^ d);
        state[col + 3] ^= e ^ xtime(d ^ a);
    }
}

#[inline]
fn multiply(x: u8, y: u8) -> u8 {
    let mut result = 0u8;
    let mut a = x;
    let mut b = y;
    
    for _ in 0..8 {
        if (b & 1) != 0 {
            result ^= a;
        }
        let hi_bit = a & 0x80;
        a <<= 1;
        if hi_bit != 0 {
            a ^= 0x1b;
        }
        b >>= 1;
    }
    result
}

#[inline]
fn inv_mix_columns(state: &mut [u8; 16]) {
    for i in 0..4 {
        let col = i * 4;
        let a = state[col];
        let b = state[col + 1];
        let c = state[col + 2];
        let d = state[col + 3];

        state[col] = multiply(a, 0x0e) ^ multiply(b, 0x0b) ^ multiply(c, 0x0d) ^ multiply(d, 0x09);
        state[col + 1] = multiply(a, 0x09) ^ multiply(b, 0x0e) ^ multiply(c, 0x0b) ^ multiply(d, 0x0d);
        state[col + 2] = multiply(a, 0x0d) ^ multiply(b, 0x09) ^ multiply(c, 0x0e) ^ multiply(d, 0x0b);
        state[col + 3] = multiply(a, 0x0b) ^ multiply(b, 0x0d) ^ multiply(c, 0x09) ^ multiply(d, 0x0e);
    }
}

// ============================================================================
// AES-GCM (Galois/Counter Mode)
// ============================================================================

/// AES-GCM cipher for authenticated encryption
pub struct AesGcm<T> {
    cipher: T,
    h: [u8; 16], // Hash subkey
}

impl AesGcm<Aes128> {
    /// Create new AES-128-GCM
    pub fn new_128(key: &[u8; AES_128_KEY_SIZE]) -> Self {
        let cipher = Aes128::new(key);
        let h = cipher.encrypt_block(&[0u8; 16]);
        Self { cipher, h }
    }
}

impl AesGcm<Aes256> {
    /// Create new AES-256-GCM
    pub fn new_256(key: &[u8; AES_256_KEY_SIZE]) -> Self {
        let cipher = Aes256::new(key);
        let h = cipher.encrypt_block(&[0u8; 16]);
        Self { cipher, h }
    }
}

impl<T> AesGcm<T> {
    /// GCM multiplication in GF(2^128)
    fn ghash_multiply(&self, x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
        let mut z = [0u8; 16];
        let mut v = *y;

        for i in 0..128 {
            let byte_idx = i / 8;
            let bit_idx = 7 - (i % 8);
            
            if (x[byte_idx] >> bit_idx) & 1 == 1 {
                for j in 0..16 {
                    z[j] ^= v[j];
                }
            }

            // Right shift V and reduce if needed
            let lsb = v[15] & 1;
            for j in (1..16).rev() {
                v[j] = (v[j] >> 1) | ((v[j - 1] & 1) << 7);
            }
            v[0] >>= 1;

            if lsb == 1 {
                v[0] ^= 0xe1; // Reduction polynomial
            }
        }

        z
    }

    /// GHASH function
    fn ghash(&self, aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
        let mut y = [0u8; 16];

        // Process AAD
        for chunk in aad.chunks(16) {
            let mut block = [0u8; 16];
            block[..chunk.len()].copy_from_slice(chunk);
            for i in 0..16 {
                y[i] ^= block[i];
            }
            y = self.ghash_multiply(&y, &self.h);
        }

        // Process ciphertext
        for chunk in ciphertext.chunks(16) {
            let mut block = [0u8; 16];
            block[..chunk.len()].copy_from_slice(chunk);
            for i in 0..16 {
                y[i] ^= block[i];
            }
            y = self.ghash_multiply(&y, &self.h);
        }

        // Process length block
        let mut len_block = [0u8; 16];
        let aad_bits = (aad.len() as u64) * 8;
        let ct_bits = (ciphertext.len() as u64) * 8;
        len_block[..8].copy_from_slice(&aad_bits.to_be_bytes());
        len_block[8..].copy_from_slice(&ct_bits.to_be_bytes());

        for i in 0..16 {
            y[i] ^= len_block[i];
        }
        y = self.ghash_multiply(&y, &self.h);

        y
    }
}

impl AesGcm<Aes128> {
    /// Encrypt with AES-128-GCM
    pub fn encrypt(&self, nonce: &[u8], plaintext: &[u8], aad: &[u8]) -> (Vec<u8>, [u8; GCM_TAG_SIZE]) {
        self.encrypt_internal(nonce, plaintext, aad, |block| self.cipher.encrypt_block(block))
    }

    /// Decrypt with AES-128-GCM
    pub fn decrypt(&self, nonce: &[u8], ciphertext: &[u8], aad: &[u8], tag: &[u8; GCM_TAG_SIZE]) -> Option<Vec<u8>> {
        self.decrypt_internal(nonce, ciphertext, aad, tag, |block| self.cipher.encrypt_block(block))
    }
}

impl AesGcm<Aes256> {
    /// Encrypt with AES-256-GCM
    pub fn encrypt(&self, nonce: &[u8], plaintext: &[u8], aad: &[u8]) -> (Vec<u8>, [u8; GCM_TAG_SIZE]) {
        self.encrypt_internal(nonce, plaintext, aad, |block| self.cipher.encrypt_block(block))
    }

    /// Decrypt with AES-256-GCM
    pub fn decrypt(&self, nonce: &[u8], ciphertext: &[u8], aad: &[u8], tag: &[u8; GCM_TAG_SIZE]) -> Option<Vec<u8>> {
        self.decrypt_internal(nonce, ciphertext, aad, tag, |block| self.cipher.encrypt_block(block))
    }
}

impl<T> AesGcm<T> {
    fn encrypt_internal<F>(&self, nonce: &[u8], plaintext: &[u8], aad: &[u8], encrypt_block: F) -> (Vec<u8>, [u8; GCM_TAG_SIZE])
    where
        F: Fn(&[u8; 16]) -> [u8; 16],
    {
        // Initialize counter J0
        let mut j0 = [0u8; 16];
        if nonce.len() == 12 {
            j0[..12].copy_from_slice(nonce);
            j0[15] = 1;
        } else {
            j0 = self.ghash(&[], nonce);
        }

        // Encrypt using counters starting from J0+1
        let mut ciphertext = Vec::with_capacity(plaintext.len());
        let mut counter = j0;

        for chunk in plaintext.chunks(16) {
            // Increment counter first (J0+1, J0+2, ...)
            let ctr = u32::from_be_bytes([counter[12], counter[13], counter[14], counter[15]]);
            counter[12..16].copy_from_slice(&(ctr.wrapping_add(1)).to_be_bytes());

            let keystream = encrypt_block(&counter);
            for (i, &p) in chunk.iter().enumerate() {
                ciphertext.push(p ^ keystream[i]);
            }
        }

        // Compute tag: GHASH(AAD, Ciphertext) XOR E_K(J0)
        let s = self.ghash(aad, &ciphertext);
        let e_j0 = encrypt_block(&j0);  // E_K(J0), NOT E_K(J0+1)
        
        let mut tag = [0u8; GCM_TAG_SIZE];
        for i in 0..16 {
            tag[i] = s[i] ^ e_j0[i];
        }

        (ciphertext, tag)
    }

    fn decrypt_internal<F>(&self, nonce: &[u8], ciphertext: &[u8], aad: &[u8], tag: &[u8; GCM_TAG_SIZE], encrypt_block: F) -> Option<Vec<u8>>
    where
        F: Fn(&[u8; 16]) -> [u8; 16],
    {
        // Initialize counter J0
        let mut j0 = [0u8; 16];
        if nonce.len() == 12 {
            j0[..12].copy_from_slice(nonce);
            j0[15] = 1;
        } else {
            j0 = self.ghash(&[], nonce);
        }

        // Verify tag first
        // GCM Tag = GHASH(AAD, Ciphertext) XOR E_K(J0)
        let s = self.ghash(aad, ciphertext);
        let e_j0 = encrypt_block(&j0);  // E_K(J0), NOT E_K(J0+1)
        
        let mut computed_tag = [0u8; GCM_TAG_SIZE];
        for i in 0..16 {
            computed_tag[i] = s[i] ^ e_j0[i];
        }

        // Constant-time comparison
        let mut diff = 0u8;
        for i in 0..16 {
            diff |= computed_tag[i] ^ tag[i];
        }
        if diff != 0 {
            return None;
        }

        // Decrypt using counters starting from J0+1
        let mut counter = j0;
        let mut plaintext = Vec::with_capacity(ciphertext.len());

        for chunk in ciphertext.chunks(16) {
            // Increment counter first (J0+1, J0+2, ...)
            let ctr = u32::from_be_bytes([counter[12], counter[13], counter[14], counter[15]]);
            counter[12..16].copy_from_slice(&(ctr.wrapping_add(1)).to_be_bytes());

            let keystream = encrypt_block(&counter);
            for (i, &c) in chunk.iter().enumerate() {
                plaintext.push(c ^ keystream[i]);
            }
        }

        Some(plaintext)
    }
}

// ============================================================================
// AES-CTR (Counter Mode)
// ============================================================================

/// AES-CTR cipher
pub struct AesCtr<T> {
    cipher: T,
}

impl AesCtr<Aes128> {
    /// Create new AES-128-CTR
    pub fn new_128(key: &[u8; AES_128_KEY_SIZE]) -> Self {
        Self { cipher: Aes128::new(key) }
    }

    /// Encrypt/decrypt (CTR mode is symmetric)
    pub fn process(&self, nonce: &[u8; 16], data: &[u8]) -> Vec<u8> {
        self.process_internal(nonce, data, |block| self.cipher.encrypt_block(block))
    }
}

impl AesCtr<Aes256> {
    /// Create new AES-256-CTR
    pub fn new_256(key: &[u8; AES_256_KEY_SIZE]) -> Self {
        Self { cipher: Aes256::new(key) }
    }

    /// Encrypt/decrypt (CTR mode is symmetric)
    pub fn process(&self, nonce: &[u8; 16], data: &[u8]) -> Vec<u8> {
        self.process_internal(nonce, data, |block| self.cipher.encrypt_block(block))
    }
}

impl<T> AesCtr<T> {
    fn process_internal<F>(&self, nonce: &[u8; 16], data: &[u8], encrypt_block: F) -> Vec<u8>
    where
        F: Fn(&[u8; 16]) -> [u8; 16],
    {
        let mut result = Vec::with_capacity(data.len());
        let mut counter = *nonce;

        for chunk in data.chunks(16) {
            let keystream = encrypt_block(&counter);
            for (i, &byte) in chunk.iter().enumerate() {
                result.push(byte ^ keystream[i]);
            }

            // Increment counter
            for i in (0..16).rev() {
                counter[i] = counter[i].wrapping_add(1);
                if counter[i] != 0 {
                    break;
                }
            }
        }

        result
    }
}

// ============================================================================
// AES-CBC (Cipher Block Chaining)
// ============================================================================

/// AES-CBC cipher
pub struct AesCbc<T> {
    cipher: T,
}

impl AesCbc<Aes128> {
    /// Create new AES-128-CBC
    pub fn new_128(key: &[u8; AES_128_KEY_SIZE]) -> Self {
        Self { cipher: Aes128::new(key) }
    }

    /// Encrypt with PKCS7 padding
    pub fn encrypt(&self, iv: &[u8; 16], plaintext: &[u8]) -> Vec<u8> {
        self.encrypt_internal(iv, plaintext, |block| self.cipher.encrypt_block(block))
    }

    /// Decrypt and remove PKCS7 padding
    pub fn decrypt(&self, iv: &[u8; 16], ciphertext: &[u8]) -> Option<Vec<u8>> {
        self.decrypt_internal(iv, ciphertext, |block| self.cipher.decrypt_block(block))
    }
}

impl AesCbc<Aes256> {
    /// Create new AES-256-CBC
    pub fn new_256(key: &[u8; AES_256_KEY_SIZE]) -> Self {
        Self { cipher: Aes256::new(key) }
    }

    /// Encrypt with PKCS7 padding
    pub fn encrypt(&self, iv: &[u8; 16], plaintext: &[u8]) -> Vec<u8> {
        self.encrypt_internal(iv, plaintext, |block| self.cipher.encrypt_block(block))
    }

    /// Decrypt and remove PKCS7 padding
    pub fn decrypt(&self, iv: &[u8; 16], ciphertext: &[u8]) -> Option<Vec<u8>> {
        self.decrypt_internal(iv, ciphertext, |block| self.cipher.decrypt_block(block))
    }
}

impl<T> AesCbc<T> {
    fn encrypt_internal<F>(&self, iv: &[u8; 16], plaintext: &[u8], encrypt_block: F) -> Vec<u8>
    where
        F: Fn(&[u8; 16]) -> [u8; 16],
    {
        // PKCS7 padding
        let padding_len = 16 - (plaintext.len() % 16);
        let mut padded = Vec::with_capacity(plaintext.len() + padding_len);
        padded.extend_from_slice(plaintext);
        padded.resize(plaintext.len() + padding_len, padding_len as u8);

        let mut result = Vec::with_capacity(padded.len());
        let mut prev_block = *iv;

        for chunk in padded.chunks(16) {
            let mut block = [0u8; 16];
            block.copy_from_slice(chunk);
            
            for i in 0..16 {
                block[i] ^= prev_block[i];
            }
            
            let encrypted = encrypt_block(&block);
            result.extend_from_slice(&encrypted);
            prev_block = encrypted;
        }

        result
    }

    fn decrypt_internal<F>(&self, iv: &[u8; 16], ciphertext: &[u8], decrypt_block: F) -> Option<Vec<u8>>
    where
        F: Fn(&[u8; 16]) -> [u8; 16],
    {
        if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
            return None;
        }

        let mut result = Vec::with_capacity(ciphertext.len());
        let mut prev_block = *iv;

        for chunk in ciphertext.chunks(16) {
            let mut block = [0u8; 16];
            block.copy_from_slice(chunk);
            
            let decrypted = decrypt_block(&block);
            
            for i in 0..16 {
                result.push(decrypted[i] ^ prev_block[i]);
            }
            
            prev_block = block;
        }

        // Remove PKCS7 padding
        let padding_len = *result.last()? as usize;
        if padding_len == 0 || padding_len > 16 {
            return None;
        }
        
        // Verify padding
        for &b in &result[result.len() - padding_len..] {
            if b as usize != padding_len {
                return None;
            }
        }

        result.truncate(result.len() - padding_len);
        Some(result)
    }
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// AES_KEY structure for C API
#[repr(C)]
pub struct AES_KEY {
    rd_key: [u32; 60],
    rounds: i32,
}

/// Set encryption key
#[no_mangle]
pub extern "C" fn AES_set_encrypt_key(user_key: *const u8, bits: i32, key: *mut AES_KEY) -> i32 {
    if user_key.is_null() || key.is_null() {
        return -1;
    }
    if bits != 128 && bits != 256 {
        return -2;
    }
    
    // Implementation would copy key schedule
    unsafe {
        (*key).rounds = if bits == 128 { 10 } else { 14 };
    }
    0
}

/// Set decryption key
#[no_mangle]
pub extern "C" fn AES_set_decrypt_key(user_key: *const u8, bits: i32, key: *mut AES_KEY) -> i32 {
    AES_set_encrypt_key(user_key, bits, key)
}

/// Encrypt single block
#[no_mangle]
pub extern "C" fn AES_encrypt(input: *const u8, output: *mut u8, key: *const AES_KEY) {
    if input.is_null() || output.is_null() || key.is_null() {
        return;
    }
    // Simplified - real implementation would use the key schedule
}

/// Decrypt single block
#[no_mangle]
pub extern "C" fn AES_decrypt(input: *const u8, output: *mut u8, key: *const AES_KEY) {
    if input.is_null() || output.is_null() || key.is_null() {
        return;
    }
    // Simplified - real implementation would use the key schedule
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes128_encrypt_decrypt() {
        let key = [0u8; 16];
        let cipher = Aes128::new(&key);
        let plaintext = [0u8; 16];
        
        let ciphertext = cipher.encrypt_block(&plaintext);
        let decrypted = cipher.decrypt_block(&ciphertext);
        
        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_aes256_encrypt_decrypt() {
        let key = [0u8; 32];
        let cipher = Aes256::new(&key);
        let plaintext = [0u8; 16];
        
        let ciphertext = cipher.encrypt_block(&plaintext);
        let decrypted = cipher.decrypt_block(&ciphertext);
        
        assert_eq!(plaintext, decrypted);
    }
}
