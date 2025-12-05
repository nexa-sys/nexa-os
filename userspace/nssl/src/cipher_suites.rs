//! Cipher Suite Definitions
//!
//! Constants and utilities for TLS cipher suites.

use crate::cipher::SslCipher;

/// TLS 1.3 Cipher Suite IDs
pub mod tls13 {
    pub const TLS_AES_128_GCM_SHA256: u16 = 0x1301;
    pub const TLS_AES_256_GCM_SHA384: u16 = 0x1302;
    pub const TLS_CHACHA20_POLY1305_SHA256: u16 = 0x1303;
    pub const TLS_AES_128_CCM_SHA256: u16 = 0x1304;
    pub const TLS_AES_128_CCM_8_SHA256: u16 = 0x1305;
}

/// TLS 1.2 Cipher Suite IDs (secure subset only)
pub mod tls12 {
    // ECDHE + RSA
    pub const TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256: u16 = 0xC02F;
    pub const TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384: u16 = 0xC030;
    pub const TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256: u16 = 0xCCA8;
    
    // ECDHE + ECDSA
    pub const TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256: u16 = 0xC02B;
    pub const TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384: u16 = 0xC02C;
    pub const TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256: u16 = 0xCCA9;
    
    // DHE + RSA (less preferred)
    pub const TLS_DHE_RSA_WITH_AES_128_GCM_SHA256: u16 = 0x009E;
    pub const TLS_DHE_RSA_WITH_AES_256_GCM_SHA384: u16 = 0x009F;
    pub const TLS_DHE_RSA_WITH_CHACHA20_POLY1305_SHA256: u16 = 0xCCAA;
}

/// Default TLS 1.3 cipher suites (preference order)
pub const TLS13_CIPHER_SUITES: &[u16] = &[
    tls13::TLS_AES_256_GCM_SHA384,
    tls13::TLS_AES_128_GCM_SHA256,
    tls13::TLS_CHACHA20_POLY1305_SHA256,
];

/// Default TLS 1.2 cipher suites (preference order)
pub const TLS12_CIPHER_SUITES: &[u16] = &[
    // Prefer ECDSA authentication
    tls12::TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
    tls12::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
    tls12::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
    // Then RSA authentication
    tls12::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
    tls12::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
    tls12::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
    // DHE as fallback
    tls12::TLS_DHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
    tls12::TLS_DHE_RSA_WITH_AES_256_GCM_SHA384,
    tls12::TLS_DHE_RSA_WITH_AES_128_GCM_SHA256,
];

/// Cipher suite information
#[derive(Clone, Copy, Debug)]
pub struct CipherSuite {
    /// Cipher suite ID
    pub id: u16,
    /// Human-readable name
    pub name: &'static str,
    /// OpenSSL-style name
    pub openssl_name: &'static str,
    /// Key size in bits
    pub key_bits: u16,
    /// Whether this is a TLS 1.3 suite
    pub is_tls13: bool,
}

impl CipherSuite {
    /// Get cipher suite by ID
    pub fn from_id(id: u16) -> Option<Self> {
        ALL_CIPHER_SUITES.iter().find(|c| c.id == id).copied()
    }

    /// Get cipher suite by name
    pub fn from_name(name: &str) -> Option<Self> {
        ALL_CIPHER_SUITES.iter()
            .find(|c| c.name == name || c.openssl_name == name)
            .copied()
    }

    /// Check if cipher is AEAD
    pub fn is_aead(&self) -> bool {
        true // All our supported ciphers are AEAD
    }

    /// Check if uses perfect forward secrecy
    pub fn is_pfs(&self) -> bool {
        true // All our supported ciphers use ephemeral key exchange
    }
}

/// All supported cipher suites
pub static ALL_CIPHER_SUITES: &[CipherSuite] = &[
    // TLS 1.3
    CipherSuite {
        id: tls13::TLS_AES_256_GCM_SHA384,
        name: "TLS_AES_256_GCM_SHA384",
        openssl_name: "TLS_AES_256_GCM_SHA384",
        key_bits: 256,
        is_tls13: true,
    },
    CipherSuite {
        id: tls13::TLS_AES_128_GCM_SHA256,
        name: "TLS_AES_128_GCM_SHA256",
        openssl_name: "TLS_AES_128_GCM_SHA256",
        key_bits: 128,
        is_tls13: true,
    },
    CipherSuite {
        id: tls13::TLS_CHACHA20_POLY1305_SHA256,
        name: "TLS_CHACHA20_POLY1305_SHA256",
        openssl_name: "TLS_CHACHA20_POLY1305_SHA256",
        key_bits: 256,
        is_tls13: true,
    },
    // TLS 1.2 ECDHE + ECDSA
    CipherSuite {
        id: tls12::TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
        name: "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256",
        openssl_name: "ECDHE-ECDSA-CHACHA20-POLY1305",
        key_bits: 256,
        is_tls13: false,
    },
    CipherSuite {
        id: tls12::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
        name: "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
        openssl_name: "ECDHE-ECDSA-AES256-GCM-SHA384",
        key_bits: 256,
        is_tls13: false,
    },
    CipherSuite {
        id: tls12::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
        name: "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256",
        openssl_name: "ECDHE-ECDSA-AES128-GCM-SHA256",
        key_bits: 128,
        is_tls13: false,
    },
    // TLS 1.2 ECDHE + RSA
    CipherSuite {
        id: tls12::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
        name: "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
        openssl_name: "ECDHE-RSA-CHACHA20-POLY1305",
        key_bits: 256,
        is_tls13: false,
    },
    CipherSuite {
        id: tls12::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
        name: "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
        openssl_name: "ECDHE-RSA-AES256-GCM-SHA384",
        key_bits: 256,
        is_tls13: false,
    },
    CipherSuite {
        id: tls12::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
        name: "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256",
        openssl_name: "ECDHE-RSA-AES128-GCM-SHA256",
        key_bits: 128,
        is_tls13: false,
    },
    // TLS 1.2 DHE + RSA
    CipherSuite {
        id: tls12::TLS_DHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
        name: "TLS_DHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
        openssl_name: "DHE-RSA-CHACHA20-POLY1305",
        key_bits: 256,
        is_tls13: false,
    },
    CipherSuite {
        id: tls12::TLS_DHE_RSA_WITH_AES_256_GCM_SHA384,
        name: "TLS_DHE_RSA_WITH_AES_256_GCM_SHA384",
        openssl_name: "DHE-RSA-AES256-GCM-SHA384",
        key_bits: 256,
        is_tls13: false,
    },
    CipherSuite {
        id: tls12::TLS_DHE_RSA_WITH_AES_128_GCM_SHA256,
        name: "TLS_DHE_RSA_WITH_AES_128_GCM_SHA256",
        openssl_name: "DHE-RSA-AES128-GCM-SHA256",
        key_bits: 128,
        is_tls13: false,
    },
];

/// Get cipher suite name from ID
pub fn cipher_suite_name(id: u16) -> Option<&'static str> {
    CipherSuite::from_id(id).map(|c| c.name)
}

/// Check if a cipher suite ID is valid
pub fn is_valid_cipher_suite(id: u16) -> bool {
    CipherSuite::from_id(id).is_some()
}
