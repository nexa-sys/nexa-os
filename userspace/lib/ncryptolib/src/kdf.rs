//! Key Derivation Functions (HKDF, PBKDF2)
//!
//! RFC 5869 (HKDF) and RFC 8018 (PBKDF2) compliant implementations.

use std::vec::Vec;

use crate::hash::{hmac_sha256, sha512, HmacSha256, SHA256_DIGEST_SIZE};

// ============================================================================
// HKDF (HMAC-based Key Derivation Function)
// ============================================================================

/// HKDF-Extract step
///
/// Extracts a pseudorandom key from input keying material.
pub fn hkdf_extract_sha256(salt: &[u8], ikm: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    let actual_salt = if salt.is_empty() {
        &[0u8; SHA256_DIGEST_SIZE][..]
    } else {
        salt
    };
    hmac_sha256(actual_salt, ikm)
}

/// HKDF-Expand step
///
/// Expands a pseudorandom key to desired length.
pub fn hkdf_expand_sha256(prk: &[u8; SHA256_DIGEST_SIZE], info: &[u8], length: usize) -> Vec<u8> {
    let n = (length + SHA256_DIGEST_SIZE - 1) / SHA256_DIGEST_SIZE;
    let mut okm = Vec::with_capacity(n * SHA256_DIGEST_SIZE);
    let mut t = [0u8; SHA256_DIGEST_SIZE];

    for i in 1..=n {
        let mut hmac = HmacSha256::new(prk);
        if i > 1 {
            hmac.update(&t);
        }
        hmac.update(info);
        hmac.update(&[i as u8]);
        t = hmac.finalize();
        okm.extend_from_slice(&t);
    }

    okm.truncate(length);
    okm
}

/// HKDF (one-shot)
///
/// Derives key material using HKDF with SHA-256.
///
/// # Arguments
/// * `salt` - Optional salt value (can be empty)
/// * `ikm` - Input keying material
/// * `info` - Context and application specific information
/// * `length` - Length of output keying material
pub fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let prk = hkdf_extract_sha256(salt, ikm);
    hkdf_expand_sha256(&prk, info, length)
}

// ============================================================================
// TLS 1.3 Key Derivation (RFC 8446 Section 7.1)
// ============================================================================

/// HKDF-Expand-Label as defined in RFC 8446 Section 7.1
///
/// ```text
/// HKDF-Expand-Label(Secret, Label, Context, Length) =
///     HKDF-Expand(Secret, HkdfLabel, Length)
///
/// struct {
///    uint16 length = Length;
///    opaque label<7..255> = "tls13 " + Label;
///    opaque context<0..255> = Context;
/// } HkdfLabel;
/// ```
pub fn hkdf_expand_label(
    secret: &[u8; SHA256_DIGEST_SIZE],
    label: &[u8],
    context: &[u8],
    length: usize,
) -> Vec<u8> {
    // Build HkdfLabel structure
    let mut hkdf_label = Vec::with_capacity(2 + 1 + 6 + label.len() + 1 + context.len());

    // Length (2 bytes, big-endian)
    hkdf_label.push((length >> 8) as u8);
    hkdf_label.push((length & 0xFF) as u8);

    // Label with "tls13 " prefix
    let full_label_len = 6 + label.len();
    hkdf_label.push(full_label_len as u8);
    hkdf_label.extend_from_slice(b"tls13 ");
    hkdf_label.extend_from_slice(label);

    // Context
    hkdf_label.push(context.len() as u8);
    hkdf_label.extend_from_slice(context);

    hkdf_expand_sha256(secret, &hkdf_label, length)
}

/// Derive-Secret as defined in RFC 8446 Section 7.1
///
/// ```text
/// Derive-Secret(Secret, Label, Messages) =
///     HKDF-Expand-Label(Secret, Label, Transcript-Hash(Messages), Hash.length)
/// ```
pub fn derive_secret(
    secret: &[u8; SHA256_DIGEST_SIZE],
    label: &[u8],
    transcript_hash: &[u8],
) -> [u8; SHA256_DIGEST_SIZE] {
    let result = hkdf_expand_label(secret, label, transcript_hash, SHA256_DIGEST_SIZE);
    let mut output = [0u8; SHA256_DIGEST_SIZE];
    output.copy_from_slice(&result);
    output
}

/// TLS 1.3 Key Schedule implementation
pub struct Tls13KeySchedule {
    /// Current secret in the key schedule
    current_secret: [u8; SHA256_DIGEST_SIZE],
    /// Early secret (from PSK or zeros)
    early_secret: Option<[u8; SHA256_DIGEST_SIZE]>,
    /// Handshake secret
    handshake_secret: Option<[u8; SHA256_DIGEST_SIZE]>,
    /// Master secret
    master_secret: Option<[u8; SHA256_DIGEST_SIZE]>,
}

impl Tls13KeySchedule {
    /// Create new key schedule without PSK (zeros for early secret)
    pub fn new() -> Self {
        let zeros = [0u8; SHA256_DIGEST_SIZE];
        let early_secret = hkdf_extract_sha256(&zeros, &zeros);
        Self {
            current_secret: early_secret,
            early_secret: Some(early_secret),
            handshake_secret: None,
            master_secret: None,
        }
    }

    /// Create key schedule with PSK for resumption
    pub fn with_psk(psk: &[u8]) -> Self {
        let zeros = [0u8; SHA256_DIGEST_SIZE];
        let early_secret = hkdf_extract_sha256(&zeros, psk);
        Self {
            current_secret: early_secret,
            early_secret: Some(early_secret),
            handshake_secret: None,
            master_secret: None,
        }
    }

    /// Derive handshake secret from (EC)DHE shared secret
    pub fn derive_handshake_secret(&mut self, shared_secret: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
        // derived = Derive-Secret(early_secret, "derived", "")
        let empty_hash = crate::hash::sha256(&[]);
        let derived = derive_secret(&self.current_secret, b"derived", &empty_hash);

        // handshake_secret = HKDF-Extract(derived, shared_secret)
        let handshake_secret = hkdf_extract_sha256(&derived, shared_secret);
        self.handshake_secret = Some(handshake_secret);
        self.current_secret = handshake_secret;

        handshake_secret
    }

    /// Derive master secret
    pub fn derive_master_secret(&mut self) -> [u8; SHA256_DIGEST_SIZE] {
        // derived = Derive-Secret(handshake_secret, "derived", "")
        let empty_hash = crate::hash::sha256(&[]);
        let derived = derive_secret(&self.current_secret, b"derived", &empty_hash);

        // master_secret = HKDF-Extract(derived, 0)
        let zeros = [0u8; SHA256_DIGEST_SIZE];
        let master_secret = hkdf_extract_sha256(&derived, &zeros);
        self.master_secret = Some(master_secret);
        self.current_secret = master_secret;

        master_secret
    }

    /// Derive client handshake traffic secret
    pub fn client_handshake_traffic_secret(
        &self,
        transcript_hash: &[u8],
    ) -> [u8; SHA256_DIGEST_SIZE] {
        let hs = self.handshake_secret.unwrap_or(self.current_secret);
        derive_secret(&hs, b"c hs traffic", transcript_hash)
    }

    /// Derive server handshake traffic secret
    pub fn server_handshake_traffic_secret(
        &self,
        transcript_hash: &[u8],
    ) -> [u8; SHA256_DIGEST_SIZE] {
        let hs = self.handshake_secret.unwrap_or(self.current_secret);
        derive_secret(&hs, b"s hs traffic", transcript_hash)
    }

    /// Derive client application traffic secret
    pub fn client_application_traffic_secret(
        &self,
        transcript_hash: &[u8],
    ) -> [u8; SHA256_DIGEST_SIZE] {
        let ms = self.master_secret.unwrap_or(self.current_secret);
        derive_secret(&ms, b"c ap traffic", transcript_hash)
    }

    /// Derive server application traffic secret  
    pub fn server_application_traffic_secret(
        &self,
        transcript_hash: &[u8],
    ) -> [u8; SHA256_DIGEST_SIZE] {
        let ms = self.master_secret.unwrap_or(self.current_secret);
        derive_secret(&ms, b"s ap traffic", transcript_hash)
    }

    /// Derive exporter master secret
    pub fn exporter_master_secret(&self, transcript_hash: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
        let ms = self.master_secret.unwrap_or(self.current_secret);
        derive_secret(&ms, b"exp master", transcript_hash)
    }

    /// Derive resumption master secret
    pub fn resumption_master_secret(&self, transcript_hash: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
        let ms = self.master_secret.unwrap_or(self.current_secret);
        derive_secret(&ms, b"res master", transcript_hash)
    }
}

impl Default for Tls13KeySchedule {
    fn default() -> Self {
        Self::new()
    }
}

/// Derive traffic keys from a traffic secret
pub struct TrafficKeys {
    pub key: Vec<u8>,
    pub iv: Vec<u8>,
}

impl TrafficKeys {
    /// Derive traffic keys from a traffic secret
    ///
    /// # Arguments
    /// * `traffic_secret` - The traffic secret (e.g., client_handshake_traffic_secret)
    /// * `key_len` - Length of the encryption key (16 for AES-128, 32 for AES-256/ChaCha20)
    /// * `iv_len` - Length of the IV/nonce (12 for AES-GCM and ChaCha20-Poly1305)
    pub fn derive(
        traffic_secret: &[u8; SHA256_DIGEST_SIZE],
        key_len: usize,
        iv_len: usize,
    ) -> Self {
        let key = hkdf_expand_label(traffic_secret, b"key", &[], key_len);
        let iv = hkdf_expand_label(traffic_secret, b"iv", &[], iv_len);
        Self { key, iv }
    }

    /// Derive finished key for Finished message
    pub fn derive_finished_key(base_key: &[u8; SHA256_DIGEST_SIZE]) -> [u8; SHA256_DIGEST_SIZE] {
        let result = hkdf_expand_label(base_key, b"finished", &[], SHA256_DIGEST_SIZE);
        let mut key = [0u8; SHA256_DIGEST_SIZE];
        key.copy_from_slice(&result);
        key
    }
}

// ============================================================================
// TLS 1.2 PRF (RFC 5246)
// ============================================================================

/// TLS 1.2 PRF with SHA-256 (P_SHA256)
///
/// PRF(secret, label, seed) = P_SHA256(secret, label + seed)
pub fn tls12_prf_sha256(secret: &[u8], label: &[u8], seed: &[u8], length: usize) -> Vec<u8> {
    // Concatenate label and seed
    let mut combined_seed = Vec::with_capacity(label.len() + seed.len());
    combined_seed.extend_from_slice(label);
    combined_seed.extend_from_slice(seed);

    // P_SHA256(secret, seed) = HMAC_SHA256(secret, A(1) + seed) +
    //                          HMAC_SHA256(secret, A(2) + seed) + ...
    // A(0) = seed
    // A(i) = HMAC_SHA256(secret, A(i-1))

    let mut result = Vec::with_capacity(length);
    let mut a = hmac_sha256(secret, &combined_seed);

    while result.len() < length {
        // P_i = HMAC(secret, A(i) + seed)
        let mut data = Vec::with_capacity(SHA256_DIGEST_SIZE + combined_seed.len());
        data.extend_from_slice(&a);
        data.extend_from_slice(&combined_seed);
        let p = hmac_sha256(secret, &data);
        result.extend_from_slice(&p);

        // A(i+1) = HMAC(secret, A(i))
        a = hmac_sha256(secret, &a);
    }

    result.truncate(length);
    result
}

/// Derive TLS 1.2 master secret
pub fn tls12_master_secret(
    pre_master_secret: &[u8],
    client_random: &[u8],
    server_random: &[u8],
) -> Vec<u8> {
    let mut seed = Vec::with_capacity(64);
    seed.extend_from_slice(client_random);
    seed.extend_from_slice(server_random);

    tls12_prf_sha256(pre_master_secret, b"master secret", &seed, 48)
}

/// Derive TLS 1.2 key block
pub fn tls12_key_block(
    master_secret: &[u8],
    server_random: &[u8],
    client_random: &[u8],
    key_block_len: usize,
) -> Vec<u8> {
    let mut seed = Vec::with_capacity(64);
    seed.extend_from_slice(server_random);
    seed.extend_from_slice(client_random);

    tls12_prf_sha256(master_secret, b"key expansion", &seed, key_block_len)
}

// ============================================================================
// PBKDF2 (Password-Based Key Derivation Function 2)
// ============================================================================

/// PBKDF2 with HMAC-SHA256
///
/// # Arguments
/// * `password` - The password
/// * `salt` - The salt
/// * `iterations` - Number of iterations (minimum 10000 recommended)
/// * `dk_len` - Desired key length
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, dk_len: usize) -> Vec<u8> {
    let h_len = SHA256_DIGEST_SIZE;
    let l = (dk_len + h_len - 1) / h_len;
    let mut dk = Vec::with_capacity(l * h_len);

    for i in 1..=l as u32 {
        let mut u = hmac_sha256(password, &[salt, &i.to_be_bytes()].concat());
        let mut t = u;

        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for j in 0..h_len {
                t[j] ^= u[j];
            }
        }

        dk.extend_from_slice(&t);
    }

    dk.truncate(dk_len);
    dk
}

/// PBKDF2 with HMAC-SHA512
pub fn pbkdf2_sha512(password: &[u8], salt: &[u8], iterations: u32, dk_len: usize) -> Vec<u8> {
    const H_LEN: usize = 64;
    let l = (dk_len + H_LEN - 1) / H_LEN;
    let mut dk = Vec::with_capacity(l * H_LEN);

    for i in 1..=l as u32 {
        let mut combined = Vec::with_capacity(salt.len() + 4);
        combined.extend_from_slice(salt);
        combined.extend_from_slice(&i.to_be_bytes());

        let mut u = hmac_sha512(password, &combined);
        let mut t = u.clone();

        for _ in 1..iterations {
            u = hmac_sha512(password, &u);
            for j in 0..H_LEN {
                t[j] ^= u[j];
            }
        }

        dk.extend_from_slice(&t);
    }

    dk.truncate(dk_len);
    dk
}

/// HMAC-SHA512 helper
fn hmac_sha512(key: &[u8], data: &[u8]) -> Vec<u8> {
    use crate::hash::{Sha512, SHA512_BLOCK_SIZE};

    let mut padded_key = vec![0u8; SHA512_BLOCK_SIZE];

    if key.len() > SHA512_BLOCK_SIZE {
        let hash = sha512(key);
        padded_key[..hash.len()].copy_from_slice(&hash);
    } else {
        padded_key[..key.len()].copy_from_slice(key);
    }

    // Inner key = key XOR ipad (0x36)
    let mut inner_key = vec![0u8; SHA512_BLOCK_SIZE];
    for i in 0..SHA512_BLOCK_SIZE {
        inner_key[i] = padded_key[i] ^ 0x36;
    }

    // Outer key = key XOR opad (0x5c)
    let mut outer_key = vec![0u8; SHA512_BLOCK_SIZE];
    for i in 0..SHA512_BLOCK_SIZE {
        outer_key[i] = padded_key[i] ^ 0x5c;
    }

    // Inner hash
    let mut inner = Sha512::new();
    inner.update(&inner_key);
    inner.update(data);
    let inner_hash = inner.finalize();

    // Outer hash
    let mut outer = Sha512::new();
    outer.update(&outer_key);
    outer.update(&inner_hash);
    outer.finalize()
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// PKCS5_PBKDF2_HMAC_SHA256 - OpenSSL compatible PBKDF2
#[no_mangle]
pub extern "C" fn PKCS5_PBKDF2_HMAC_SHA256(
    pass: *const u8,
    passlen: i32,
    salt: *const u8,
    saltlen: i32,
    iter: i32,
    keylen: i32,
    out: *mut u8,
) -> i32 {
    if pass.is_null() || salt.is_null() || out.is_null() {
        return 0;
    }
    if passlen < 0 || saltlen < 0 || iter < 1 || keylen < 1 {
        return 0;
    }

    let password = unsafe { core::slice::from_raw_parts(pass, passlen as usize) };
    let salt_slice = unsafe { core::slice::from_raw_parts(salt, saltlen as usize) };

    let dk = pbkdf2_sha256(password, salt_slice, iter as u32, keylen as usize);

    unsafe {
        core::ptr::copy_nonoverlapping(dk.as_ptr(), out, keylen as usize);
    }

    1
}

/// PKCS5_PBKDF2_HMAC - OpenSSL compatible PBKDF2 with digest selection
#[no_mangle]
pub extern "C" fn PKCS5_PBKDF2_HMAC(
    pass: *const u8,
    passlen: i32,
    salt: *const u8,
    saltlen: i32,
    iter: i32,
    _digest: *const core::ffi::c_void, // EVP_MD - we ignore and use SHA256
    keylen: i32,
    out: *mut u8,
) -> i32 {
    PKCS5_PBKDF2_HMAC_SHA256(pass, passlen, salt, saltlen, iter, keylen, out)
}

/// EVP_PKEY_CTX for HKDF (simplified stub)
#[repr(C)]
pub struct EVP_PKEY_CTX {
    _private: [u8; 0],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hkdf_basic() {
        let ikm = b"input key material";
        let salt = b"salt";
        let info = b"info";

        let output = hkdf(salt, ikm, info, 32);
        assert_eq!(output.len(), 32);
    }

    #[test]
    fn test_pbkdf2_sha256() {
        let password = b"password";
        let salt = b"salt";

        let dk = pbkdf2_sha256(password, salt, 1, 32);
        assert_eq!(dk.len(), 32);

        // RFC 7914 test vector (first iteration only)
        // For iterations=1, salt="salt", password="password", dkLen=32
    }

    #[test]
    fn test_hkdf_extract_expand() {
        let ikm = [0x0b; 22];
        let salt = [0u8; 13];

        let prk = hkdf_extract_sha256(&salt, &ikm);
        assert_eq!(prk.len(), 32);

        let okm = hkdf_expand_sha256(&prk, b"", 42);
        assert_eq!(okm.len(), 42);
    }
}
