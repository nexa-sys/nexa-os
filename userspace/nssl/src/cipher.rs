//! SSL Cipher
//!
//! Cipher suite definitions and management.

use std::vec::Vec;
use crate::c_char;

/// SSL Cipher descriptor
#[repr(C)]
#[derive(Clone)]
pub struct SslCipher {
    /// Cipher suite ID
    pub id: u16,
    /// Cipher name (null-terminated)
    name: &'static [u8],
    /// Key exchange algorithm
    pub kex: KeyExchange,
    /// Authentication algorithm
    pub auth: Authentication,
    /// Encryption algorithm
    pub enc: Encryption,
    /// MAC algorithm
    pub mac: Mac,
    /// Minimum TLS version
    pub min_version: u16,
    /// Key size in bits
    pub key_bits: u16,
}

/// Key exchange algorithms
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyExchange {
    None = 0,
    Ecdhe = 1,
    Dhe = 2,
    Psk = 3,
    EcdhePsk = 4,
}

/// Authentication algorithms
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authentication {
    None = 0,
    Rsa = 1,
    Ecdsa = 2,
    Psk = 3,
}

/// Encryption algorithms
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encryption {
    None = 0,
    Aes128Gcm = 1,
    Aes256Gcm = 2,
    ChaCha20Poly1305 = 3,
    Aes128Ccm = 4,
    Aes256Ccm = 5,
}

/// MAC algorithms
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mac {
    None = 0,      // AEAD modes
    Sha256 = 1,
    Sha384 = 2,
}

impl SslCipher {
    /// Get cipher name
    pub fn get_name(&self) -> *const c_char {
        self.name.as_ptr() as *const c_char
    }

    /// Get cipher name as string
    pub fn name_str(&self) -> &'static str {
        // Safe because our names are ASCII and null-terminated
        unsafe {
            let len = self.name.len() - 1; // Exclude null terminator
            core::str::from_utf8_unchecked(&self.name[..len])
        }
    }

    /// Get cipher ID
    pub fn get_id(&self) -> u16 {
        self.id
    }

    /// Get key size in bits
    pub fn get_bits(&self) -> u16 {
        self.key_bits
    }

    /// Check if cipher is AEAD
    pub fn is_aead(&self) -> bool {
        matches!(self.mac, Mac::None)
    }

    /// Get cipher from ID
    pub fn from_id(id: u16) -> Option<Self> {
        // TLS 1.3 suites
        match id {
            0x1301 => Some(TLS13_AES_128_GCM_SHA256.clone()),
            0x1302 => Some(TLS13_AES_256_GCM_SHA384.clone()),
            0x1303 => Some(TLS13_CHACHA20_POLY1305_SHA256.clone()),
            // TLS 1.2 suites
            0xC02F => Some(TLS12_ECDHE_RSA_WITH_AES_128_GCM_SHA256.clone()),
            0xC030 => Some(TLS12_ECDHE_RSA_WITH_AES_256_GCM_SHA384.clone()),
            0xCCA8 => Some(TLS12_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256.clone()),
            0xC02B => Some(TLS12_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256.clone()),
            0xC02C => Some(TLS12_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384.clone()),
            0xCCA9 => Some(TLS12_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256.clone()),
            _ => None,
        }
    }
}

// ============================================================================
// TLS 1.3 Cipher Suites
// ============================================================================

pub static TLS13_AES_128_GCM_SHA256: SslCipher = SslCipher {
    id: 0x1301,
    name: b"TLS_AES_128_GCM_SHA256\0",
    kex: KeyExchange::None, // TLS 1.3 uses key_share extension
    auth: Authentication::None,
    enc: Encryption::Aes128Gcm,
    mac: Mac::None, // AEAD
    min_version: crate::TLS1_3_VERSION,
    key_bits: 128,
};

pub static TLS13_AES_256_GCM_SHA384: SslCipher = SslCipher {
    id: 0x1302,
    name: b"TLS_AES_256_GCM_SHA384\0",
    kex: KeyExchange::None,
    auth: Authentication::None,
    enc: Encryption::Aes256Gcm,
    mac: Mac::None,
    min_version: crate::TLS1_3_VERSION,
    key_bits: 256,
};

pub static TLS13_CHACHA20_POLY1305_SHA256: SslCipher = SslCipher {
    id: 0x1303,
    name: b"TLS_CHACHA20_POLY1305_SHA256\0",
    kex: KeyExchange::None,
    auth: Authentication::None,
    enc: Encryption::ChaCha20Poly1305,
    mac: Mac::None,
    min_version: crate::TLS1_3_VERSION,
    key_bits: 256,
};

// ============================================================================
// TLS 1.2 Cipher Suites (ECDHE only - no static RSA)
// ============================================================================

pub static TLS12_ECDHE_RSA_WITH_AES_128_GCM_SHA256: SslCipher = SslCipher {
    id: 0xC02F,
    name: b"ECDHE-RSA-AES128-GCM-SHA256\0",
    kex: KeyExchange::Ecdhe,
    auth: Authentication::Rsa,
    enc: Encryption::Aes128Gcm,
    mac: Mac::None,
    min_version: crate::TLS1_2_VERSION,
    key_bits: 128,
};

pub static TLS12_ECDHE_RSA_WITH_AES_256_GCM_SHA384: SslCipher = SslCipher {
    id: 0xC030,
    name: b"ECDHE-RSA-AES256-GCM-SHA384\0",
    kex: KeyExchange::Ecdhe,
    auth: Authentication::Rsa,
    enc: Encryption::Aes256Gcm,
    mac: Mac::None,
    min_version: crate::TLS1_2_VERSION,
    key_bits: 256,
};

pub static TLS12_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256: SslCipher = SslCipher {
    id: 0xCCA8,
    name: b"ECDHE-RSA-CHACHA20-POLY1305\0",
    kex: KeyExchange::Ecdhe,
    auth: Authentication::Rsa,
    enc: Encryption::ChaCha20Poly1305,
    mac: Mac::None,
    min_version: crate::TLS1_2_VERSION,
    key_bits: 256,
};

pub static TLS12_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256: SslCipher = SslCipher {
    id: 0xC02B,
    name: b"ECDHE-ECDSA-AES128-GCM-SHA256\0",
    kex: KeyExchange::Ecdhe,
    auth: Authentication::Ecdsa,
    enc: Encryption::Aes128Gcm,
    mac: Mac::None,
    min_version: crate::TLS1_2_VERSION,
    key_bits: 128,
};

pub static TLS12_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384: SslCipher = SslCipher {
    id: 0xC02C,
    name: b"ECDHE-ECDSA-AES256-GCM-SHA384\0",
    kex: KeyExchange::Ecdhe,
    auth: Authentication::Ecdsa,
    enc: Encryption::Aes256Gcm,
    mac: Mac::None,
    min_version: crate::TLS1_2_VERSION,
    key_bits: 256,
};

pub static TLS12_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256: SslCipher = SslCipher {
    id: 0xCCA9,
    name: b"ECDHE-ECDSA-CHACHA20-POLY1305\0",
    kex: KeyExchange::Ecdhe,
    auth: Authentication::Ecdsa,
    enc: Encryption::ChaCha20Poly1305,
    mac: Mac::None,
    min_version: crate::TLS1_2_VERSION,
    key_bits: 256,
};

// ============================================================================
// Cipher List
// ============================================================================

/// Cipher list for TLS 1.2
pub struct CipherList {
    /// Ordered list of cipher suite IDs
    suites: Vec<u16>,
}

impl CipherList {
    /// Create empty cipher list
    pub fn new() -> Self {
        Self { suites: Vec::new() }
    }

    /// Create default secure cipher list
    pub fn default_secure() -> Self {
        Self {
            suites: vec![
                // Prefer ECDHE + ECDSA
                0xCCA9, // ECDHE-ECDSA-CHACHA20-POLY1305
                0xC02C, // ECDHE-ECDSA-AES256-GCM-SHA384
                0xC02B, // ECDHE-ECDSA-AES128-GCM-SHA256
                // Then ECDHE + RSA
                0xCCA8, // ECDHE-RSA-CHACHA20-POLY1305
                0xC030, // ECDHE-RSA-AES256-GCM-SHA384
                0xC02F, // ECDHE-RSA-AES128-GCM-SHA256
            ],
        }
    }

    /// Parse cipher list from OpenSSL-style string
    pub fn from_string(s: &str) -> Option<Self> {
        let mut suites = Vec::new();
        
        for part in s.split(':') {
            let name = part.trim();
            if name.is_empty() || name.starts_with('!') || name.starts_with('-') {
                continue;
            }
            
            // Handle special keywords
            match name {
                "HIGH" | "DEFAULT" => {
                    suites.extend_from_slice(&[0xCCA9, 0xC02C, 0xC02B, 0xCCA8, 0xC030, 0xC02F]);
                }
                "ECDHE" => {
                    suites.extend_from_slice(&[0xCCA9, 0xC02C, 0xC02B, 0xCCA8, 0xC030, 0xC02F]);
                }
                "AESGCM" => {
                    suites.extend_from_slice(&[0xC02C, 0xC02B, 0xC030, 0xC02F]);
                }
                "CHACHA20" => {
                    suites.extend_from_slice(&[0xCCA9, 0xCCA8]);
                }
                // Individual cipher names
                "ECDHE-RSA-AES128-GCM-SHA256" => suites.push(0xC02F),
                "ECDHE-RSA-AES256-GCM-SHA384" => suites.push(0xC030),
                "ECDHE-RSA-CHACHA20-POLY1305" => suites.push(0xCCA8),
                "ECDHE-ECDSA-AES128-GCM-SHA256" => suites.push(0xC02B),
                "ECDHE-ECDSA-AES256-GCM-SHA384" => suites.push(0xC02C),
                "ECDHE-ECDSA-CHACHA20-POLY1305" => suites.push(0xCCA9),
                _ => {}
            }
        }
        
        // Remove duplicates while preserving order
        let mut seen = std::collections::HashSet::new();
        suites.retain(|x| seen.insert(*x));
        
        if suites.is_empty() {
            None
        } else {
            Some(Self { suites })
        }
    }

    /// Get cipher suites
    pub fn get_suites(&self) -> &[u16] {
        &self.suites
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.suites.is_empty()
    }
}

impl Default for CipherList {
    fn default() -> Self {
        Self::default_secure()
    }
}
