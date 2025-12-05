//! EVP (Envelope) API - OpenSSL Compatibility Layer
//!
//! High-level cryptographic API compatible with OpenSSL EVP interface.

use std::vec::Vec;

use crate::hash::{Sha256, Sha384, Sha512};
use crate::sha3::{Sha3};
use crate::blake2::{Blake2b, Blake2s};
use crate::md5::Md5;
use crate::sha1::Sha1;
use crate::aes::{Aes128, Aes256, AesGcm, AesCtr, AesCbc, AES_128_KEY_SIZE, AES_256_KEY_SIZE};

// ============================================================================
// EVP Digest (Message Digest)
// ============================================================================

/// EVP_MD type identifiers
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvpMdType {
    // Legacy (file verification only)
    Md5 = 0,
    Sha1 = 1,
    // SHA-2 family
    Sha256 = 2,
    Sha384 = 3,
    Sha512 = 4,
    // SHA-3 family
    Sha3_256 = 5,
    Sha3_384 = 6,
    Sha3_512 = 7,
    // BLAKE2 family
    Blake2b512 = 8,
    Blake2s256 = 9,
}

/// EVP_MD - Message Digest algorithm descriptor
#[repr(C)]
pub struct EvpMd {
    /// Algorithm type
    pub md_type: EvpMdType,
    /// Output size in bytes
    pub md_size: usize,
    /// Block size in bytes
    pub block_size: usize,
}

/// MD5 algorithm (legacy - file verification only)
pub static EVP_MD5: EvpMd = EvpMd {
    md_type: EvpMdType::Md5,
    md_size: 16,
    block_size: 64,
};

/// SHA-1 algorithm (legacy - file verification only)
pub static EVP_SHA1: EvpMd = EvpMd {
    md_type: EvpMdType::Sha1,
    md_size: 20,
    block_size: 64,
};

/// SHA-256 algorithm
pub static EVP_SHA256: EvpMd = EvpMd {
    md_type: EvpMdType::Sha256,
    md_size: 32,
    block_size: 64,
};

/// SHA-384 algorithm
pub static EVP_SHA384: EvpMd = EvpMd {
    md_type: EvpMdType::Sha384,
    md_size: 48,
    block_size: 128,
};

/// SHA-512 algorithm
pub static EVP_SHA512: EvpMd = EvpMd {
    md_type: EvpMdType::Sha512,
    md_size: 64,
    block_size: 128,
};

/// SHA3-256 algorithm
pub static EVP_SHA3_256: EvpMd = EvpMd {
    md_type: EvpMdType::Sha3_256,
    md_size: 32,
    block_size: 136,
};

/// SHA3-384 algorithm
pub static EVP_SHA3_384: EvpMd = EvpMd {
    md_type: EvpMdType::Sha3_384,
    md_size: 48,
    block_size: 104,
};

/// SHA3-512 algorithm
pub static EVP_SHA3_512: EvpMd = EvpMd {
    md_type: EvpMdType::Sha3_512,
    md_size: 64,
    block_size: 72,
};

/// BLAKE2b-512 algorithm
pub static EVP_BLAKE2B512: EvpMd = EvpMd {
    md_type: EvpMdType::Blake2b512,
    md_size: 64,
    block_size: 128,
};

/// BLAKE2s-256 algorithm
pub static EVP_BLAKE2S256: EvpMd = EvpMd {
    md_type: EvpMdType::Blake2s256,
    md_size: 32,
    block_size: 64,
};

/// Internal digest state
enum DigestState {
    Md5(Md5),
    Sha1(Sha1),
    Sha256(Sha256),
    Sha384(Sha384),
    Sha512(Sha512),
    Sha3_256(Sha3),
    Sha3_384(Sha3),
    Sha3_512(Sha3),
    Blake2b512(Blake2b),
    Blake2s256(Blake2s),
}

/// EVP_MD_CTX - Message Digest Context
#[repr(C)]
pub struct EvpMdCtx {
    md: *const EvpMd,
    state: Option<DigestState>,
}

impl EvpMdCtx {
    /// Create new context
    pub fn new() -> Self {
        Self {
            md: core::ptr::null(),
            state: None,
        }
    }

    /// Initialize with algorithm
    pub fn init(&mut self, md: &EvpMd) -> Result<(), i32> {
        self.md = md;
        self.state = Some(match md.md_type {
            EvpMdType::Md5 => DigestState::Md5(Md5::new()),
            EvpMdType::Sha1 => DigestState::Sha1(Sha1::new()),
            EvpMdType::Sha256 => DigestState::Sha256(Sha256::new()),
            EvpMdType::Sha384 => DigestState::Sha384(Sha384::new()),
            EvpMdType::Sha512 => DigestState::Sha512(Sha512::new()),
            EvpMdType::Sha3_256 => DigestState::Sha3_256(Sha3::new_256()),
            EvpMdType::Sha3_384 => DigestState::Sha3_384(Sha3::new_384()),
            EvpMdType::Sha3_512 => DigestState::Sha3_512(Sha3::new_512()),
            EvpMdType::Blake2b512 => DigestState::Blake2b512(Blake2b::new(64)),
            EvpMdType::Blake2s256 => DigestState::Blake2s256(Blake2s::new(32)),
        });
        Ok(())
    }

    /// Update with data
    pub fn update(&mut self, data: &[u8]) -> Result<(), i32> {
        match &mut self.state {
            Some(DigestState::Md5(h)) => h.update(data),
            Some(DigestState::Sha1(h)) => h.update(data),
            Some(DigestState::Sha256(h)) => h.update(data),
            Some(DigestState::Sha384(h)) => h.update(data),
            Some(DigestState::Sha512(h)) => h.update(data),
            Some(DigestState::Sha3_256(h)) => h.update(data),
            Some(DigestState::Sha3_384(h)) => h.update(data),
            Some(DigestState::Sha3_512(h)) => h.update(data),
            Some(DigestState::Blake2b512(h)) => h.update(data),
            Some(DigestState::Blake2s256(h)) => h.update(data),
            None => return Err(-1),
        }
        Ok(())
    }

    /// Finalize and get digest
    pub fn finalize(&mut self, out: &mut [u8]) -> Result<usize, i32> {
        let size = match self.state.take() {
            Some(DigestState::Md5(mut h)) => {
                let hash = h.finalize();
                let size = 16.min(out.len());
                out[..size].copy_from_slice(&hash[..size]);
                size
            }
            Some(DigestState::Sha1(mut h)) => {
                let hash = h.finalize();
                let size = 20.min(out.len());
                out[..size].copy_from_slice(&hash[..size]);
                size
            }
            Some(DigestState::Sha256(mut h)) => {
                let hash = h.finalize();
                let size = 32.min(out.len());
                out[..size].copy_from_slice(&hash[..size]);
                size
            }
            Some(DigestState::Sha384(mut h)) => {
                let hash = h.finalize();
                let size = 48.min(out.len());
                out[..size].copy_from_slice(&hash[..size]);
                size
            }
            Some(DigestState::Sha512(mut h)) => {
                let hash = h.finalize();
                let size = 64.min(out.len());
                out[..size].copy_from_slice(&hash[..size]);
                size
            }
            Some(DigestState::Sha3_256(mut h)) => {
                let hash = h.finalize();
                let size = 32.min(out.len());
                out[..size].copy_from_slice(&hash[..size]);
                size
            }
            Some(DigestState::Sha3_384(mut h)) => {
                let hash = h.finalize();
                let size = 48.min(out.len());
                out[..size].copy_from_slice(&hash[..size]);
                size
            }
            Some(DigestState::Sha3_512(mut h)) => {
                let hash = h.finalize();
                let size = 64.min(out.len());
                out[..size].copy_from_slice(&hash[..size]);
                size
            }
            Some(DigestState::Blake2b512(mut h)) => {
                let hash = h.finalize();
                let size = 64.min(out.len());
                out[..size].copy_from_slice(&hash[..size]);
                size
            }
            Some(DigestState::Blake2s256(mut h)) => {
                let hash = h.finalize();
                let size = 32.min(out.len());
                out[..size].copy_from_slice(&hash[..size]);
                size
            }
            None => return Err(-1),
        };
        Ok(size)
    }

    /// Reset context
    pub fn reset(&mut self) -> Result<(), i32> {
        if self.md.is_null() {
            return Err(-1);
        }
        let md = unsafe { &*self.md };
        self.init(md)
    }
}

impl Default for EvpMdCtx {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// EVP Cipher
// ============================================================================

/// EVP cipher type identifiers
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvpCipherType {
    Aes128Gcm = 1,
    Aes256Gcm = 2,
    Aes128Ctr = 3,
    Aes256Ctr = 4,
    Aes128Cbc = 5,
    Aes256Cbc = 6,
}

/// EVP_CIPHER - Cipher algorithm descriptor
#[repr(C)]
pub struct EvpCipher {
    /// Algorithm type
    pub cipher_type: EvpCipherType,
    /// Key size in bytes
    pub key_len: usize,
    /// IV size in bytes
    pub iv_len: usize,
    /// Block size in bytes
    pub block_size: usize,
}

/// AES-128-GCM
pub static EVP_AES_128_GCM: EvpCipher = EvpCipher {
    cipher_type: EvpCipherType::Aes128Gcm,
    key_len: 16,
    iv_len: 12,
    block_size: 16,
};

/// AES-256-GCM
pub static EVP_AES_256_GCM: EvpCipher = EvpCipher {
    cipher_type: EvpCipherType::Aes256Gcm,
    key_len: 32,
    iv_len: 12,
    block_size: 16,
};

/// AES-128-CTR
pub static EVP_AES_128_CTR: EvpCipher = EvpCipher {
    cipher_type: EvpCipherType::Aes128Ctr,
    key_len: 16,
    iv_len: 16,
    block_size: 16,
};

/// AES-256-CTR
pub static EVP_AES_256_CTR: EvpCipher = EvpCipher {
    cipher_type: EvpCipherType::Aes256Ctr,
    key_len: 32,
    iv_len: 16,
    block_size: 16,
};

/// AES-128-CBC
pub static EVP_AES_128_CBC: EvpCipher = EvpCipher {
    cipher_type: EvpCipherType::Aes128Cbc,
    key_len: 16,
    iv_len: 16,
    block_size: 16,
};

/// AES-256-CBC
pub static EVP_AES_256_CBC: EvpCipher = EvpCipher {
    cipher_type: EvpCipherType::Aes256Cbc,
    key_len: 32,
    iv_len: 16,
    block_size: 16,
};

/// Internal cipher state - stores the cipher and accumulated data
enum CipherState {
    Aes128Gcm { gcm: AesGcm<Aes128>, nonce: [u8; 12] },
    Aes256Gcm { gcm: AesGcm<Aes256>, nonce: [u8; 12] },
    Aes128Ctr { ctr: AesCtr<Aes128>, nonce: [u8; 16] },
    Aes256Ctr { ctr: AesCtr<Aes256>, nonce: [u8; 16] },
    Aes128Cbc { cbc: AesCbc<Aes128>, iv: [u8; 16] },
    Aes256Cbc { cbc: AesCbc<Aes256>, iv: [u8; 16] },
}

/// EVP_CIPHER_CTX - Cipher Context
pub struct EvpCipherCtx {
    cipher: *const EvpCipher,
    state: Option<CipherState>,
    encrypting: bool,
    aad: Vec<u8>,
    buffer: Vec<u8>,  // Accumulated input data
    tag: [u8; 16],
}

impl EvpCipherCtx {
    /// Create new context
    pub fn new() -> Self {
        Self {
            cipher: core::ptr::null(),
            state: None,
            encrypting: true,
            aad: Vec::new(),
            buffer: Vec::new(),
            tag: [0u8; 16],
        }
    }

    /// Initialize for encryption
    pub fn encrypt_init(
        &mut self,
        cipher: &EvpCipher,
        key: &[u8],
        iv: &[u8],
    ) -> Result<(), i32> {
        self.cipher = cipher;
        self.encrypting = true;
        self.aad.clear();
        self.buffer.clear();
        self.init_cipher(cipher, key, iv)
    }

    /// Initialize for decryption
    pub fn decrypt_init(
        &mut self,
        cipher: &EvpCipher,
        key: &[u8],
        iv: &[u8],
    ) -> Result<(), i32> {
        self.cipher = cipher;
        self.encrypting = false;
        self.aad.clear();
        self.buffer.clear();
        self.init_cipher(cipher, key, iv)
    }

    fn init_cipher(&mut self, cipher: &EvpCipher, key: &[u8], iv: &[u8]) -> Result<(), i32> {
        self.state = Some(match cipher.cipher_type {
            EvpCipherType::Aes128Gcm => {
                let mut key_arr = [0u8; AES_128_KEY_SIZE];
                key_arr.copy_from_slice(&key[..AES_128_KEY_SIZE]);
                let mut nonce = [0u8; 12];
                nonce.copy_from_slice(&iv[..12]);
                CipherState::Aes128Gcm {
                    gcm: AesGcm::new_128(&key_arr),
                    nonce,
                }
            }
            EvpCipherType::Aes256Gcm => {
                let mut key_arr = [0u8; AES_256_KEY_SIZE];
                key_arr.copy_from_slice(&key[..AES_256_KEY_SIZE]);
                let mut nonce = [0u8; 12];
                nonce.copy_from_slice(&iv[..12]);
                CipherState::Aes256Gcm {
                    gcm: AesGcm::new_256(&key_arr),
                    nonce,
                }
            }
            EvpCipherType::Aes128Ctr => {
                let mut key_arr = [0u8; AES_128_KEY_SIZE];
                key_arr.copy_from_slice(&key[..AES_128_KEY_SIZE]);
                let mut nonce = [0u8; 16];
                nonce.copy_from_slice(&iv[..16]);
                CipherState::Aes128Ctr {
                    ctr: AesCtr::new_128(&key_arr),
                    nonce,
                }
            }
            EvpCipherType::Aes256Ctr => {
                let mut key_arr = [0u8; AES_256_KEY_SIZE];
                key_arr.copy_from_slice(&key[..AES_256_KEY_SIZE]);
                let mut nonce = [0u8; 16];
                nonce.copy_from_slice(&iv[..16]);
                CipherState::Aes256Ctr {
                    ctr: AesCtr::new_256(&key_arr),
                    nonce,
                }
            }
            EvpCipherType::Aes128Cbc => {
                let mut key_arr = [0u8; AES_128_KEY_SIZE];
                key_arr.copy_from_slice(&key[..AES_128_KEY_SIZE]);
                let mut iv_arr = [0u8; 16];
                iv_arr.copy_from_slice(&iv[..16]);
                CipherState::Aes128Cbc {
                    cbc: AesCbc::new_128(&key_arr),
                    iv: iv_arr,
                }
            }
            EvpCipherType::Aes256Cbc => {
                let mut key_arr = [0u8; AES_256_KEY_SIZE];
                key_arr.copy_from_slice(&key[..AES_256_KEY_SIZE]);
                let mut iv_arr = [0u8; 16];
                iv_arr.copy_from_slice(&iv[..16]);
                CipherState::Aes256Cbc {
                    cbc: AesCbc::new_256(&key_arr),
                    iv: iv_arr,
                }
            }
        });
        Ok(())
    }

    /// Set AAD (Additional Authenticated Data) for GCM
    pub fn set_aad(&mut self, aad: &[u8]) -> Result<(), i32> {
        self.aad = aad.to_vec();
        Ok(())
    }

    /// Update with data - accumulates data for processing in finalize
    pub fn update(&mut self, input: &[u8], output: &mut [u8]) -> Result<usize, i32> {
        // For streaming modes (CTR), we can process immediately
        // For block modes (GCM, CBC), we accumulate
        match &self.state {
            Some(CipherState::Aes128Ctr { ctr, nonce }) => {
                let result = ctr.process(nonce, input);
                let len = result.len().min(output.len());
                output[..len].copy_from_slice(&result[..len]);
                Ok(len)
            }
            Some(CipherState::Aes256Ctr { ctr, nonce }) => {
                let result = ctr.process(nonce, input);
                let len = result.len().min(output.len());
                output[..len].copy_from_slice(&result[..len]);
                Ok(len)
            }
            _ => {
                // Accumulate for block modes
                self.buffer.extend_from_slice(input);
                Ok(0) // Output in finalize
            }
        }
    }

    /// Finalize operation
    pub fn finalize(&mut self, output: &mut [u8]) -> Result<usize, i32> {
        match self.state.take() {
            Some(CipherState::Aes128Gcm { gcm, nonce }) => {
                if self.encrypting {
                    let (ciphertext, tag) = gcm.encrypt(&nonce, &self.buffer, &self.aad);
                    let len = ciphertext.len().min(output.len());
                    output[..len].copy_from_slice(&ciphertext[..len]);
                    self.tag = tag;
                    Ok(len)
                } else {
                    match gcm.decrypt(&nonce, &self.buffer, &self.aad, &self.tag) {
                        Some(plaintext) => {
                            let len = plaintext.len().min(output.len());
                            output[..len].copy_from_slice(&plaintext[..len]);
                            Ok(len)
                        }
                        None => Err(-1), // Tag verification failed
                    }
                }
            }
            Some(CipherState::Aes256Gcm { gcm, nonce }) => {
                if self.encrypting {
                    let (ciphertext, tag) = gcm.encrypt(&nonce, &self.buffer, &self.aad);
                    let len = ciphertext.len().min(output.len());
                    output[..len].copy_from_slice(&ciphertext[..len]);
                    self.tag = tag;
                    Ok(len)
                } else {
                    match gcm.decrypt(&nonce, &self.buffer, &self.aad, &self.tag) {
                        Some(plaintext) => {
                            let len = plaintext.len().min(output.len());
                            output[..len].copy_from_slice(&plaintext[..len]);
                            Ok(len)
                        }
                        None => Err(-1),
                    }
                }
            }
            Some(CipherState::Aes128Cbc { cbc, iv }) => {
                if self.encrypting {
                    let ciphertext = cbc.encrypt(&iv, &self.buffer);
                    let len = ciphertext.len().min(output.len());
                    output[..len].copy_from_slice(&ciphertext[..len]);
                    Ok(len)
                } else {
                    match cbc.decrypt(&iv, &self.buffer) {
                        Some(plaintext) => {
                            let len = plaintext.len().min(output.len());
                            output[..len].copy_from_slice(&plaintext[..len]);
                            Ok(len)
                        }
                        None => Err(-1),
                    }
                }
            }
            Some(CipherState::Aes256Cbc { cbc, iv }) => {
                if self.encrypting {
                    let ciphertext = cbc.encrypt(&iv, &self.buffer);
                    let len = ciphertext.len().min(output.len());
                    output[..len].copy_from_slice(&ciphertext[..len]);
                    Ok(len)
                } else {
                    match cbc.decrypt(&iv, &self.buffer) {
                        Some(plaintext) => {
                            let len = plaintext.len().min(output.len());
                            output[..len].copy_from_slice(&plaintext[..len]);
                            Ok(len)
                        }
                        None => Err(-1),
                    }
                }
            }
            _ => Ok(0),
        }
    }

    /// Get GCM tag (after encryption)
    pub fn get_tag(&self, tag: &mut [u8]) -> Result<(), i32> {
        let len = tag.len().min(16);
        tag[..len].copy_from_slice(&self.tag[..len]);
        Ok(())
    }

    /// Set GCM tag (before decryption)
    pub fn set_tag(&mut self, tag: &[u8]) -> Result<(), i32> {
        let len = tag.len().min(16);
        self.tag[..len].copy_from_slice(&tag[..len]);
        Ok(())
    }
}

impl Default for EvpCipherCtx {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// EVP PKEY (Public Key)
// ============================================================================

/// EVP key type
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvpPkeyType {
    Ed25519 = 1,
    X25519 = 2,
    EcdsaP256 = 3,
    EcdsaP384 = 4,
}

/// EVP_PKEY - Key container
pub struct EvpPkey {
    key_type: EvpPkeyType,
    private_key: Vec<u8>,
    public_key: Vec<u8>,
}

impl EvpPkey {
    /// Create Ed25519 key from seed
    pub fn ed25519_from_seed(seed: &[u8; 32]) -> Self {
        use crate::ed25519::Ed25519KeyPair;
        let keypair = Ed25519KeyPair::from_seed(seed);
        Self {
            key_type: EvpPkeyType::Ed25519,
            private_key: seed.to_vec(),
            public_key: keypair.public_key().to_vec(),
        }
    }

    /// Create X25519 key from private key
    pub fn x25519_from_private(private: &[u8; 32]) -> Self {
        use crate::x25519::X25519PrivateKey;
        let priv_key = X25519PrivateKey::from_bytes(private);
        let pub_key = priv_key.public_key();
        Self {
            key_type: EvpPkeyType::X25519,
            private_key: private.to_vec(),
            public_key: pub_key.to_bytes().to_vec(),
        }
    }

    /// Get key type
    pub fn key_type(&self) -> EvpPkeyType {
        self.key_type
    }

    /// Get public key bytes
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Get private key bytes  
    pub fn private_key(&self) -> &[u8] {
        &self.private_key
    }
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// EVP_MD_CTX_new
#[no_mangle]
pub extern "C" fn EVP_MD_CTX_new() -> *mut EvpMdCtx {
    let ctx = Box::new(EvpMdCtx::new());
    Box::into_raw(ctx)
}

/// EVP_MD_CTX_free
#[no_mangle]
pub extern "C" fn EVP_MD_CTX_free(ctx: *mut EvpMdCtx) {
    if !ctx.is_null() {
        unsafe {
            drop(Box::from_raw(ctx));
        }
    }
}

/// EVP_DigestInit
#[no_mangle]
pub extern "C" fn EVP_DigestInit(ctx: *mut EvpMdCtx, md: *const EvpMd) -> i32 {
    if ctx.is_null() || md.is_null() {
        return 0;
    }
    let ctx = unsafe { &mut *ctx };
    let md = unsafe { &*md };
    match ctx.init(md) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// EVP_DigestUpdate
#[no_mangle]
pub extern "C" fn EVP_DigestUpdate(ctx: *mut EvpMdCtx, data: *const u8, len: usize) -> i32 {
    if ctx.is_null() || data.is_null() {
        return 0;
    }
    let ctx = unsafe { &mut *ctx };
    let data = unsafe { core::slice::from_raw_parts(data, len) };
    match ctx.update(data) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// EVP_DigestFinal
#[no_mangle]
pub extern "C" fn EVP_DigestFinal(
    ctx: *mut EvpMdCtx,
    md: *mut u8,
    md_len: *mut u32,
) -> i32 {
    if ctx.is_null() || md.is_null() {
        return 0;
    }
    let ctx = unsafe { &mut *ctx };
    let mut buffer = [0u8; 64];
    match ctx.finalize(&mut buffer) {
        Ok(len) => {
            unsafe {
                core::ptr::copy_nonoverlapping(buffer.as_ptr(), md, len);
                if !md_len.is_null() {
                    *md_len = len as u32;
                }
            }
            1
        }
        Err(_) => 0,
    }
}

/// EVP_sha256
#[no_mangle]
pub extern "C" fn EVP_sha256() -> *const EvpMd {
    &EVP_SHA256
}

/// EVP_sha384
#[no_mangle]
pub extern "C" fn EVP_sha384() -> *const EvpMd {
    &EVP_SHA384
}

/// EVP_sha512
#[no_mangle]
pub extern "C" fn EVP_sha512() -> *const EvpMd {
    &EVP_SHA512
}

/// EVP_CIPHER_CTX_new
#[no_mangle]
pub extern "C" fn EVP_CIPHER_CTX_new() -> *mut EvpCipherCtx {
    let ctx = Box::new(EvpCipherCtx::new());
    Box::into_raw(ctx)
}

/// EVP_CIPHER_CTX_free
#[no_mangle]
pub extern "C" fn EVP_CIPHER_CTX_free(ctx: *mut EvpCipherCtx) {
    if !ctx.is_null() {
        unsafe {
            drop(Box::from_raw(ctx));
        }
    }
}

/// EVP_EncryptInit_ex
#[no_mangle]
pub extern "C" fn EVP_EncryptInit_ex(
    ctx: *mut EvpCipherCtx,
    cipher: *const EvpCipher,
    _engine: *mut core::ffi::c_void,
    key: *const u8,
    iv: *const u8,
) -> i32 {
    if ctx.is_null() || cipher.is_null() {
        return 0;
    }
    let ctx = unsafe { &mut *ctx };
    let cipher = unsafe { &*cipher };
    
    let key_slice = if key.is_null() {
        return 0;
    } else {
        unsafe { core::slice::from_raw_parts(key, cipher.key_len) }
    };
    
    let iv_slice = if iv.is_null() {
        return 0;
    } else {
        unsafe { core::slice::from_raw_parts(iv, cipher.iv_len) }
    };

    match ctx.encrypt_init(cipher, key_slice, iv_slice) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// EVP_DecryptInit_ex
#[no_mangle]
pub extern "C" fn EVP_DecryptInit_ex(
    ctx: *mut EvpCipherCtx,
    cipher: *const EvpCipher,
    _engine: *mut core::ffi::c_void,
    key: *const u8,
    iv: *const u8,
) -> i32 {
    if ctx.is_null() || cipher.is_null() {
        return 0;
    }
    let ctx = unsafe { &mut *ctx };
    let cipher = unsafe { &*cipher };
    
    let key_slice = if key.is_null() {
        return 0;
    } else {
        unsafe { core::slice::from_raw_parts(key, cipher.key_len) }
    };
    
    let iv_slice = if iv.is_null() {
        return 0;
    } else {
        unsafe { core::slice::from_raw_parts(iv, cipher.iv_len) }
    };

    match ctx.decrypt_init(cipher, key_slice, iv_slice) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// EVP_EncryptUpdate
#[no_mangle]
pub extern "C" fn EVP_EncryptUpdate(
    ctx: *mut EvpCipherCtx,
    out: *mut u8,
    out_len: *mut i32,
    input: *const u8,
    input_len: i32,
) -> i32 {
    if ctx.is_null() || out.is_null() || input.is_null() {
        return 0;
    }
    let ctx = unsafe { &mut *ctx };
    let input_slice = unsafe { core::slice::from_raw_parts(input, input_len as usize) };
    let output_slice = unsafe { core::slice::from_raw_parts_mut(out, input_len as usize + 16) };

    match ctx.update(input_slice, output_slice) {
        Ok(len) => {
            if !out_len.is_null() {
                unsafe { *out_len = len as i32; }
            }
            1
        }
        Err(_) => 0,
    }
}

/// EVP_DecryptUpdate
#[no_mangle]
pub extern "C" fn EVP_DecryptUpdate(
    ctx: *mut EvpCipherCtx,
    out: *mut u8,
    out_len: *mut i32,
    input: *const u8,
    input_len: i32,
) -> i32 {
    EVP_EncryptUpdate(ctx, out, out_len, input, input_len)
}

/// EVP_EncryptFinal_ex
#[no_mangle]
pub extern "C" fn EVP_EncryptFinal_ex(
    ctx: *mut EvpCipherCtx,
    out: *mut u8,
    out_len: *mut i32,
) -> i32 {
    if ctx.is_null() {
        return 0;
    }
    let ctx = unsafe { &mut *ctx };
    let output_slice = unsafe { core::slice::from_raw_parts_mut(out, 1024) };

    match ctx.finalize(output_slice) {
        Ok(len) => {
            if !out_len.is_null() {
                unsafe { *out_len = len as i32; }
            }
            1
        }
        Err(_) => 0,
    }
}

/// EVP_DecryptFinal_ex
#[no_mangle]
pub extern "C" fn EVP_DecryptFinal_ex(
    ctx: *mut EvpCipherCtx,
    out: *mut u8,
    out_len: *mut i32,
) -> i32 {
    EVP_EncryptFinal_ex(ctx, out, out_len)
}

/// EVP_aes_128_gcm
#[no_mangle]
pub extern "C" fn EVP_aes_128_gcm() -> *const EvpCipher {
    &EVP_AES_128_GCM
}

/// EVP_aes_256_gcm
#[no_mangle]
pub extern "C" fn EVP_aes_256_gcm() -> *const EvpCipher {
    &EVP_AES_256_GCM
}

/// EVP_aes_128_ctr
#[no_mangle]
pub extern "C" fn EVP_aes_128_ctr() -> *const EvpCipher {
    &EVP_AES_128_CTR
}

/// EVP_aes_256_ctr
#[no_mangle]
pub extern "C" fn EVP_aes_256_ctr() -> *const EvpCipher {
    &EVP_AES_256_CTR
}

/// EVP_aes_128_cbc
#[no_mangle]
pub extern "C" fn EVP_aes_128_cbc() -> *const EvpCipher {
    &EVP_AES_128_CBC
}

/// EVP_aes_256_cbc
#[no_mangle]
pub extern "C" fn EVP_aes_256_cbc() -> *const EvpCipher {
    &EVP_AES_256_CBC
}

/// EVP_CIPHER_CTX_ctrl (for GCM tag operations)
#[no_mangle]
pub extern "C" fn EVP_CIPHER_CTX_ctrl(
    ctx: *mut EvpCipherCtx,
    cmd: i32,
    arg: i32,
    ptr: *mut core::ffi::c_void,
) -> i32 {
    const EVP_CTRL_GCM_SET_TAG: i32 = 0x11;
    const EVP_CTRL_GCM_GET_TAG: i32 = 0x10;

    if ctx.is_null() {
        return 0;
    }
    let ctx = unsafe { &mut *ctx };

    match cmd {
        EVP_CTRL_GCM_SET_TAG => {
            if ptr.is_null() || arg <= 0 {
                return 0;
            }
            let tag = unsafe { core::slice::from_raw_parts(ptr as *const u8, arg as usize) };
            match ctx.set_tag(tag) {
                Ok(_) => 1,
                Err(_) => 0,
            }
        }
        EVP_CTRL_GCM_GET_TAG => {
            if ptr.is_null() || arg <= 0 {
                return 0;
            }
            let tag = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, arg as usize) };
            match ctx.get_tag(tag) {
                Ok(_) => 1,
                Err(_) => 0,
            }
        }
        _ => 0,
    }
}

/// EVP_MD_size - Get digest size
#[no_mangle]
pub extern "C" fn EVP_MD_size(md: *const EvpMd) -> i32 {
    if md.is_null() {
        return -1;
    }
    let md = unsafe { &*md };
    md.md_size as i32
}

/// EVP_MD_block_size - Get block size
#[no_mangle]
pub extern "C" fn EVP_MD_block_size(md: *const EvpMd) -> i32 {
    if md.is_null() {
        return -1;
    }
    let md = unsafe { &*md };
    md.block_size as i32
}

/// EVP_CIPHER_key_length
#[no_mangle]
pub extern "C" fn EVP_CIPHER_key_length(cipher: *const EvpCipher) -> i32 {
    if cipher.is_null() {
        return -1;
    }
    let cipher = unsafe { &*cipher };
    cipher.key_len as i32
}

/// EVP_CIPHER_iv_length
#[no_mangle]
pub extern "C" fn EVP_CIPHER_iv_length(cipher: *const EvpCipher) -> i32 {
    if cipher.is_null() {
        return -1;
    }
    let cipher = unsafe { &*cipher };
    cipher.iv_len as i32
}

/// EVP_CIPHER_block_size
#[no_mangle]
pub extern "C" fn EVP_CIPHER_block_size(cipher: *const EvpCipher) -> i32 {
    if cipher.is_null() {
        return -1;
    }
    let cipher = unsafe { &*cipher };
    cipher.block_size as i32
}
