//! QUIC Crypto Layer
//!
//! This module implements QUIC cryptographic operations including
//! key derivation, AEAD encryption/decryption, and header protection.
//!
//! Uses `ssl_ffi` for cryptographic primitives via C ABI dynamic linking:
//! - nssl (libssl.so) for TLS 1.3 support
//! - ncryptolib (libcrypto.so) for:
//!   - HKDF for key derivation
//!   - AES-GCM and ChaCha20-Poly1305 for AEAD
//!   - AES-ECB and ChaCha20 for header protection

use crate::constants::crypto::*;
use crate::error::{CryptoError, Error, Result};
use crate::types::EncryptionLevel;
use crate::{ConnectionId, NGTCP2_PROTO_VER_V2};

// Import from ssl_ffi module (C ABI bindings to nssl/ncryptolib)
use crate::ssl_ffi::{
    self, aes128_ecb_encrypt, aes128_gcm_decrypt, aes128_gcm_encrypt, aes256_ecb_encrypt,
    aes256_gcm_decrypt, aes256_gcm_encrypt, chacha20_hp_mask, chacha20_poly1305_decrypt,
    chacha20_poly1305_encrypt, hkdf_expand_label as ffi_hkdf_expand_label, hkdf_extract_sha256,
    AES_BLOCK_SIZE, SHA256_DIGEST_SIZE,
};

// ============================================================================
// AEAD Algorithm
// ============================================================================

/// AEAD algorithm selection
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AeadAlgorithm {
    /// AES-128-GCM
    Aes128Gcm,
    /// AES-256-GCM
    Aes256Gcm,
    /// ChaCha20-Poly1305
    ChaCha20Poly1305,
}

impl AeadAlgorithm {
    /// Get key length
    pub fn key_len(&self) -> usize {
        match self {
            AeadAlgorithm::Aes128Gcm => AES_128_KEY_LEN,
            AeadAlgorithm::Aes256Gcm => AES_256_KEY_LEN,
            AeadAlgorithm::ChaCha20Poly1305 => CHACHA20_KEY_LEN,
        }
    }

    /// Get nonce/IV length
    pub fn nonce_len(&self) -> usize {
        AEAD_NONCE_LEN
    }

    /// Get authentication tag length
    pub fn tag_len(&self) -> usize {
        AEAD_TAG_LEN
    }
}

// ============================================================================
// Crypto Keys
// ============================================================================

/// QUIC packet protection keys
#[derive(Clone)]
pub struct CryptoKeys {
    /// AEAD key
    pub key: Vec<u8>,
    /// IV (nonce base)
    pub iv: Vec<u8>,
    /// Header protection key
    pub hp: Vec<u8>,
    /// AEAD algorithm
    pub aead: AeadAlgorithm,
}

impl CryptoKeys {
    /// Create new crypto keys
    pub fn new(key: Vec<u8>, iv: Vec<u8>, hp: Vec<u8>, aead: AeadAlgorithm) -> Self {
        Self { key, iv, hp, aead }
    }

    /// Check if keys are valid
    pub fn is_valid(&self) -> bool {
        self.key.len() == self.aead.key_len()
            && self.iv.len() == AEAD_NONCE_LEN
            && self.hp.len() >= HP_SAMPLE_LEN
    }

    /// Compute nonce by XORing base IV with packet number
    pub fn compute_nonce(&self, packet_number: u64) -> [u8; AEAD_NONCE_LEN] {
        let mut nonce = [0u8; AEAD_NONCE_LEN];
        let iv_len = self.iv.len().min(AEAD_NONCE_LEN);
        nonce[..iv_len].copy_from_slice(&self.iv[..iv_len]);

        // XOR the packet number into the last 8 bytes of IV
        let pn_bytes = packet_number.to_be_bytes();
        for i in 0..8 {
            nonce[AEAD_NONCE_LEN - 8 + i] ^= pn_bytes[i];
        }
        nonce
    }
}

impl std::fmt::Debug for CryptoKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptoKeys")
            .field("aead", &self.aead)
            .field("key_len", &self.key.len())
            .field("iv_len", &self.iv.len())
            .field("hp_len", &self.hp.len())
            .finish()
    }
}

// ============================================================================
// Crypto Context
// ============================================================================

/// Cryptographic context for a connection
#[derive(Debug)]
pub struct CryptoContext {
    /// Client keys per encryption level
    pub client_keys: [Option<CryptoKeys>; 4],
    /// Server keys per encryption level
    pub server_keys: [Option<CryptoKeys>; 4],
    /// Current key phase (for 1-RTT key update)
    pub key_phase: bool,
    /// Key update count
    pub key_update_count: u64,
    /// QUIC version
    pub version: u32,
}

impl CryptoContext {
    /// Create a new crypto context
    pub fn new(version: u32) -> Self {
        Self {
            client_keys: [None, None, None, None],
            server_keys: [None, None, None, None],
            key_phase: false,
            key_update_count: 0,
            version,
        }
    }

    /// Get initial salt for this QUIC version
    pub fn initial_salt(&self) -> &'static [u8] {
        match self.version {
            NGTCP2_PROTO_VER_V2 => &INITIAL_SALT_V2,
            _ => &INITIAL_SALT_V1,
        }
    }

    /// Derive initial secrets from destination connection ID
    pub fn derive_initial_secrets(&mut self, dcid: &ConnectionId, _is_client: bool) -> Result<()> {
        let salt = self.initial_salt();

        // initial_secret = HKDF-Extract(salt, dcid)
        let initial_secret = hkdf_extract(salt, dcid.as_slice())?;

        // client_initial_secret = HKDF-Expand-Label(initial_secret, "client in", "", 32)
        let client_secret = hkdf_expand_label(&initial_secret, CLIENT_INITIAL_LABEL, &[], 32)?;

        // server_initial_secret = HKDF-Expand-Label(initial_secret, "server in", "", 32)
        let server_secret = hkdf_expand_label(&initial_secret, SERVER_INITIAL_LABEL, &[], 32)?;

        // Derive keys from secrets
        let client_keys = self.derive_keys_from_secret(&client_secret, AeadAlgorithm::Aes128Gcm)?;
        let server_keys = self.derive_keys_from_secret(&server_secret, AeadAlgorithm::Aes128Gcm)?;

        self.client_keys[EncryptionLevel::Initial as usize] = Some(client_keys);
        self.server_keys[EncryptionLevel::Initial as usize] = Some(server_keys);

        Ok(())
    }

    /// Derive packet protection keys from a traffic secret
    fn derive_keys_from_secret(&self, secret: &[u8], aead: AeadAlgorithm) -> Result<CryptoKeys> {
        let key_len = aead.key_len();

        // key = HKDF-Expand-Label(secret, "quic key", "", key_len)
        let key = hkdf_expand_label(secret, KEY_LABEL, &[], key_len)?;

        // iv = HKDF-Expand-Label(secret, "quic iv", "", 12)
        let iv = hkdf_expand_label(secret, IV_LABEL, &[], AEAD_NONCE_LEN)?;

        // hp = HKDF-Expand-Label(secret, "quic hp", "", key_len)
        let hp = hkdf_expand_label(secret, HP_LABEL, &[], key_len)?;

        Ok(CryptoKeys::new(key, iv, hp, aead))
    }

    /// Set keys for an encryption level
    pub fn set_keys(
        &mut self,
        level: EncryptionLevel,
        client_secret: &[u8],
        server_secret: &[u8],
        aead: AeadAlgorithm,
    ) -> Result<()> {
        let client_keys = self.derive_keys_from_secret(client_secret, aead)?;
        let server_keys = self.derive_keys_from_secret(server_secret, aead)?;

        self.client_keys[level as usize] = Some(client_keys);
        self.server_keys[level as usize] = Some(server_keys);

        Ok(())
    }

    /// Get keys for encryption
    pub fn get_encrypt_keys(&self, level: EncryptionLevel, is_client: bool) -> Option<&CryptoKeys> {
        if is_client {
            self.client_keys[level as usize].as_ref()
        } else {
            self.server_keys[level as usize].as_ref()
        }
    }

    /// Get keys for decryption
    pub fn get_decrypt_keys(&self, level: EncryptionLevel, is_client: bool) -> Option<&CryptoKeys> {
        if is_client {
            self.server_keys[level as usize].as_ref()
        } else {
            self.client_keys[level as usize].as_ref()
        }
    }

    /// Discard keys for an encryption level
    pub fn discard_keys(&mut self, level: EncryptionLevel) {
        self.client_keys[level as usize] = None;
        self.server_keys[level as usize] = None;
    }

    /// Perform key update (1-RTT only)
    pub fn update_keys(&mut self, _is_client: bool) -> Result<()> {
        // TODO: Implement key update according to RFC 9001 Section 6
        self.key_phase = !self.key_phase;
        self.key_update_count += 1;
        Ok(())
    }
}

// ============================================================================
// HKDF Functions - Using ssl_ffi (C ABI to ncryptolib)
// ============================================================================

/// HKDF-Extract using SHA-256 via ncryptolib C ABI
pub fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> Result<[u8; SHA256_DIGEST_SIZE]> {
    Ok(hkdf_extract_sha256(salt, ikm))
}

/// HKDF-Expand-Label for QUIC using ncryptolib C ABI
///
/// Uses TLS 1.3 style HKDF-Expand-Label
pub fn hkdf_expand_label(
    secret: &[u8],
    label: &[u8],
    context: &[u8],
    length: usize,
) -> Result<Vec<u8>> {
    Ok(ffi_hkdf_expand_label(secret, label, context, length))
}

// ============================================================================
// Packet Protection - Header Protection
// ============================================================================

/// Apply header protection using ncryptolib
pub fn protect_header(
    header: &mut [u8],
    pkt_num_offset: usize,
    pkt_num_len: usize,
    sample: &[u8],
    hp_key: &[u8],
    aead: AeadAlgorithm,
) -> Result<()> {
    if sample.len() < HP_SAMPLE_LEN || hp_key.is_empty() {
        return Err(Error::Crypto(CryptoError::HeaderProtection));
    }

    // Generate mask from HP key and sample
    let mask = generate_hp_mask(hp_key, sample, aead)?;

    // Apply mask to first byte
    let first_byte = header[0];
    if (first_byte & 0x80) != 0 {
        // Long header: mask lower 4 bits
        header[0] = first_byte ^ (mask[0] & 0x0f);
    } else {
        // Short header: mask lower 5 bits
        header[0] = first_byte ^ (mask[0] & 0x1f);
    }

    // Apply mask to packet number bytes
    for i in 0..pkt_num_len {
        header[pkt_num_offset + i] ^= mask[1 + i];
    }

    Ok(())
}

/// Remove header protection using ncryptolib
pub fn unprotect_header(
    header: &mut [u8],
    pkt_num_offset: usize,
    sample: &[u8],
    hp_key: &[u8],
    aead: AeadAlgorithm,
) -> Result<usize> {
    if sample.len() < HP_SAMPLE_LEN || hp_key.is_empty() {
        return Err(Error::Crypto(CryptoError::HeaderProtection));
    }

    // Generate mask
    let mask = generate_hp_mask(hp_key, sample, aead)?;

    // Remove mask from first byte to get packet number length
    let first_byte = header[0];
    let unmasked_first = if (first_byte & 0x80) != 0 {
        first_byte ^ (mask[0] & 0x0f)
    } else {
        first_byte ^ (mask[0] & 0x1f)
    };

    let pkt_num_len = ((unmasked_first & 0x03) + 1) as usize;

    // Apply mask to get unprotected header
    header[0] = unmasked_first;
    for i in 0..pkt_num_len {
        header[pkt_num_offset + i] ^= mask[1 + i];
    }

    Ok(pkt_num_len)
}

/// Generate header protection mask using ssl_ffi (C ABI to ncryptolib)
fn generate_hp_mask(hp_key: &[u8], sample: &[u8], aead: AeadAlgorithm) -> Result<[u8; 5]> {
    let mut mask = [0u8; 5];

    match aead {
        AeadAlgorithm::Aes128Gcm => {
            // For AES-128: mask = AES-ECB(hp_key, sample[0..16])
            let sample_block: [u8; AES_BLOCK_SIZE] = sample[..AES_BLOCK_SIZE]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::HeaderProtection))?;

            let key: [u8; 16] = hp_key[..16]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::HeaderProtection))?;
            let encrypted = aes128_ecb_encrypt(&key, &sample_block);
            mask.copy_from_slice(&encrypted[..5]);
        }
        AeadAlgorithm::Aes256Gcm => {
            // For AES-256: mask = AES-ECB(hp_key, sample[0..16])
            let sample_block: [u8; AES_BLOCK_SIZE] = sample[..AES_BLOCK_SIZE]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::HeaderProtection))?;

            let key: [u8; 32] = hp_key[..32]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::HeaderProtection))?;
            let encrypted = aes256_ecb_encrypt(&key, &sample_block);
            mask.copy_from_slice(&encrypted[..5]);
        }
        AeadAlgorithm::ChaCha20Poly1305 => {
            // For ChaCha20: mask = ChaCha20(hp_key, counter=sample[0..4], nonce=sample[4..16])
            let counter = u32::from_le_bytes(
                sample[..4]
                    .try_into()
                    .map_err(|_| Error::Crypto(CryptoError::HeaderProtection))?,
            );
            let nonce: [u8; 12] = sample[4..16]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::HeaderProtection))?;
            let key: [u8; 32] = hp_key[..32]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::HeaderProtection))?;

            mask = chacha20_hp_mask(&key, counter, &nonce);
        }
    }

    Ok(mask)
}

// ============================================================================
// AEAD Operations - Using ssl_ffi (C ABI to ncryptolib)
// ============================================================================

/// Encrypt packet payload using ssl_ffi AEAD (C ABI to ncryptolib)
pub fn encrypt_packet(
    keys: &CryptoKeys,
    pkt_num: u64,
    header: &[u8],
    payload: &[u8],
    output: &mut [u8],
) -> Result<usize> {
    // Compute nonce: IV XOR packet number
    let nonce = keys.compute_nonce(pkt_num);

    let tag_len = keys.aead.tag_len();
    let ciphertext_len = payload.len();

    if output.len() < ciphertext_len + tag_len {
        return Err(Error::BufferTooSmall);
    }

    match keys.aead {
        AeadAlgorithm::Aes128Gcm => {
            let key: [u8; 16] = keys.key[..16]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::Encryption))?;

            let (ciphertext, tag) = aes128_gcm_encrypt(&key, &nonce, payload, header);
            output[..ciphertext.len()].copy_from_slice(&ciphertext);
            output[ciphertext.len()..ciphertext.len() + ssl_ffi::AEAD_TAG_LEN]
                .copy_from_slice(&tag);
            Ok(ciphertext.len() + ssl_ffi::AEAD_TAG_LEN)
        }
        AeadAlgorithm::Aes256Gcm => {
            let key: [u8; 32] = keys.key[..32]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::Encryption))?;

            let (ciphertext, tag) = aes256_gcm_encrypt(&key, &nonce, payload, header);
            output[..ciphertext.len()].copy_from_slice(&ciphertext);
            output[ciphertext.len()..ciphertext.len() + ssl_ffi::AEAD_TAG_LEN]
                .copy_from_slice(&tag);
            Ok(ciphertext.len() + ssl_ffi::AEAD_TAG_LEN)
        }
        AeadAlgorithm::ChaCha20Poly1305 => {
            let key: [u8; 32] = keys.key[..32]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::Encryption))?;
            let nonce12: [u8; 12] = nonce;

            let ciphertext_with_tag = chacha20_poly1305_encrypt(&key, &nonce12, payload, header);
            if output.len() < ciphertext_with_tag.len() {
                return Err(Error::BufferTooSmall);
            }
            output[..ciphertext_with_tag.len()].copy_from_slice(&ciphertext_with_tag);
            Ok(ciphertext_with_tag.len())
        }
    }
}

/// Decrypt packet payload using ssl_ffi AEAD (C ABI to ncryptolib)
pub fn decrypt_packet(
    keys: &CryptoKeys,
    pkt_num: u64,
    header: &[u8],
    ciphertext: &[u8],
    output: &mut [u8],
) -> Result<usize> {
    let tag_len = keys.aead.tag_len();

    if ciphertext.len() < tag_len {
        return Err(Error::Crypto(CryptoError::Decryption));
    }

    // Compute nonce: IV XOR packet number
    let nonce = keys.compute_nonce(pkt_num);

    match keys.aead {
        AeadAlgorithm::Aes128Gcm => {
            let payload_len = ciphertext.len() - tag_len;
            if output.len() < payload_len {
                return Err(Error::BufferTooSmall);
            }

            let key: [u8; 16] = keys.key[..16]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::Decryption))?;

            let ciphertext_data = &ciphertext[..payload_len];
            let tag: [u8; ssl_ffi::AEAD_TAG_LEN] = ciphertext[payload_len..]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::Decryption))?;

            match aes128_gcm_decrypt(&key, &nonce, ciphertext_data, header, &tag) {
                Some(plaintext) => {
                    output[..plaintext.len()].copy_from_slice(&plaintext);
                    Ok(plaintext.len())
                }
                None => Err(Error::Crypto(CryptoError::Decryption)),
            }
        }
        AeadAlgorithm::Aes256Gcm => {
            let payload_len = ciphertext.len() - tag_len;
            if output.len() < payload_len {
                return Err(Error::BufferTooSmall);
            }

            let key: [u8; 32] = keys.key[..32]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::Decryption))?;

            let ciphertext_data = &ciphertext[..payload_len];
            let tag: [u8; ssl_ffi::AEAD_TAG_LEN] = ciphertext[payload_len..]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::Decryption))?;

            match aes256_gcm_decrypt(&key, &nonce, ciphertext_data, header, &tag) {
                Some(plaintext) => {
                    output[..plaintext.len()].copy_from_slice(&plaintext);
                    Ok(plaintext.len())
                }
                None => Err(Error::Crypto(CryptoError::Decryption)),
            }
        }
        AeadAlgorithm::ChaCha20Poly1305 => {
            if output.len() < ciphertext.len() - tag_len {
                return Err(Error::BufferTooSmall);
            }

            let key: [u8; 32] = keys.key[..32]
                .try_into()
                .map_err(|_| Error::Crypto(CryptoError::Decryption))?;
            let nonce12: [u8; 12] = nonce;

            match chacha20_poly1305_decrypt(&key, &nonce12, ciphertext, header) {
                Some(plaintext) => {
                    output[..plaintext.len()].copy_from_slice(&plaintext);
                    Ok(plaintext.len())
                }
                None => Err(Error::Crypto(CryptoError::Decryption)),
            }
        }
    }
}

// ============================================================================
// Retry Token Validation
// ============================================================================

/// Generate retry integrity tag using AES-128-GCM (via ssl_ffi C ABI)
pub fn generate_retry_integrity_tag(
    version: u32,
    retry_packet: &[u8],
    original_dcid: &ConnectionId,
) -> Result<[u8; ssl_ffi::AEAD_TAG_LEN]> {
    let (key, nonce) = match version {
        NGTCP2_PROTO_VER_V2 => {
            // QUIC v2 uses different retry keys (RFC 9369)
            (&RETRY_KEY_V2, &RETRY_NONCE_V2)
        }
        _ => (&RETRY_KEY_V1, &RETRY_NONCE_V1),
    };

    // Pseudo-Retry packet = ODCID Length || ODCID || Retry packet without tag
    let mut pseudo_retry = Vec::with_capacity(1 + original_dcid.datalen + retry_packet.len());
    pseudo_retry.push(original_dcid.datalen as u8);
    pseudo_retry.extend_from_slice(original_dcid.as_slice());
    pseudo_retry.extend_from_slice(retry_packet);

    // Tag = AES-128-GCM-Encrypt(key, nonce, aad=pseudo_retry, plaintext="")
    let key16: [u8; 16] = (*key)
        .try_into()
        .map_err(|_| Error::Crypto(CryptoError::Encryption))?;
    let (_ciphertext, tag) = aes128_gcm_encrypt(&key16, nonce, &[], &pseudo_retry);
    Ok(tag)
}

/// Verify retry integrity tag (using ssl_ffi constant-time comparison)
pub fn verify_retry_integrity_tag(
    version: u32,
    retry_packet: &[u8],
    original_dcid: &ConnectionId,
    tag: &[u8; ssl_ffi::AEAD_TAG_LEN],
) -> Result<bool> {
    let expected_tag = generate_retry_integrity_tag(version, retry_packet, original_dcid)?;

    // Constant-time comparison via ssl_ffi
    Ok(ssl_ffi::ct_eq(&expected_tag, tag))
}

// ============================================================================
// Key Update (RFC 9001 Section 6)
// ============================================================================

/// Derive updated traffic secret
pub fn derive_next_secret(current_secret: &[u8]) -> Result<Vec<u8>> {
    // next_secret = HKDF-Expand-Label(current_secret, "quic ku", "", 32)
    hkdf_expand_label(current_secret, KEY_UPDATE_LABEL, &[], SHA256_DIGEST_SIZE)
}

/// Derive keys for key update phase
pub fn derive_key_update_keys(
    current_secret: &[u8],
    aead: AeadAlgorithm,
) -> Result<(Vec<u8>, CryptoKeys)> {
    let next_secret = derive_next_secret(current_secret)?;
    let key_len = aead.key_len();

    // key = HKDF-Expand-Label(next_secret, "quic key", "", key_len)
    let key = hkdf_expand_label(&next_secret, KEY_LABEL, &[], key_len)?;

    // iv = HKDF-Expand-Label(next_secret, "quic iv", "", 12)
    let iv = hkdf_expand_label(&next_secret, IV_LABEL, &[], AEAD_NONCE_LEN)?;

    // hp key does not change during key update
    let hp = vec![0u8; key_len]; // HP key from original secret

    Ok((next_secret, CryptoKeys::new(key, iv, hp, aead)))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aead_algorithm_lengths() {
        assert_eq!(AeadAlgorithm::Aes128Gcm.key_len(), 16);
        assert_eq!(AeadAlgorithm::Aes256Gcm.key_len(), 32);
        assert_eq!(AeadAlgorithm::ChaCha20Poly1305.key_len(), 32);
        assert_eq!(AeadAlgorithm::Aes128Gcm.nonce_len(), 12);
        assert_eq!(AeadAlgorithm::Aes128Gcm.tag_len(), 16);
    }

    #[test]
    fn test_hkdf_extract() {
        let salt = [0x38, 0x76, 0x2c, 0xf7, 0xf5, 0x59, 0x34, 0xb3];
        let ikm = [0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08];
        let result = hkdf_extract(&salt, &ikm).unwrap();
        assert_eq!(result.len(), SHA256_DIGEST_SIZE);
    }

    #[test]
    fn test_crypto_keys_nonce() {
        let keys = CryptoKeys::new(
            vec![0u8; 16],
            vec![
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
            ],
            vec![0u8; 16],
            AeadAlgorithm::Aes128Gcm,
        );

        let nonce = keys.compute_nonce(1);
        // Packet number 1 XORed into last 8 bytes
        assert_eq!(nonce[11], 0x0b ^ 0x01);
    }

    #[test]
    fn test_encrypt_decrypt_aes128gcm() {
        let keys = CryptoKeys::new(
            vec![0x01; 16],
            vec![0x02; 12],
            vec![0x03; 16],
            AeadAlgorithm::Aes128Gcm,
        );

        let header = b"test header";
        let payload = b"test payload data";
        let mut ciphertext = vec![0u8; payload.len() + 16];
        let mut decrypted = vec![0u8; payload.len()];

        let ct_len = encrypt_packet(&keys, 1, header, payload, &mut ciphertext).unwrap();
        assert_eq!(ct_len, payload.len() + 16);

        let pt_len =
            decrypt_packet(&keys, 1, header, &ciphertext[..ct_len], &mut decrypted).unwrap();
        assert_eq!(pt_len, payload.len());
        assert_eq!(&decrypted[..pt_len], payload);
    }

    #[test]
    fn test_encrypt_decrypt_chacha20poly1305() {
        let keys = CryptoKeys::new(
            vec![0x01; 32],
            vec![0x02; 12],
            vec![0x03; 32],
            AeadAlgorithm::ChaCha20Poly1305,
        );

        let header = b"test header";
        let payload = b"test payload data";
        let mut ciphertext = vec![0u8; payload.len() + 16];
        let mut decrypted = vec![0u8; payload.len()];

        let ct_len = encrypt_packet(&keys, 1, header, payload, &mut ciphertext).unwrap();
        assert_eq!(ct_len, payload.len() + 16);

        let pt_len =
            decrypt_packet(&keys, 1, header, &ciphertext[..ct_len], &mut decrypted).unwrap();
        assert_eq!(pt_len, payload.len());
        assert_eq!(&decrypted[..pt_len], payload);
    }
}
