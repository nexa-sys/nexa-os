//! X.509 Certificate Signature Verification
//!
//! Implements signature verification for X.509 certificates using
//! RSA-PKCS#1, RSA-PSS, and ECDSA-P256 algorithms.

use std::vec::Vec;

/// Signature algorithm OIDs
pub mod oid {
    // RSA PKCS#1 v1.5 signatures
    pub const SHA256_WITH_RSA: &[u8] = &[
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B,
    ];
    pub const SHA384_WITH_RSA: &[u8] = &[
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0C,
    ];
    pub const SHA512_WITH_RSA: &[u8] = &[
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0D,
    ];

    // RSA-PSS (generic OID, params in AlgorithmIdentifier)
    pub const RSA_PSS: &[u8] = &[
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0A,
    ];

    // ECDSA signatures
    pub const ECDSA_WITH_SHA256: &[u8] =
        &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02];
    pub const ECDSA_WITH_SHA384: &[u8] =
        &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03];
    pub const ECDSA_WITH_SHA512: &[u8] =
        &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x04];

    // Public key algorithm OIDs
    pub const RSA_ENCRYPTION: &[u8] = &[
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01,
    ];
    pub const EC_PUBLIC_KEY: &[u8] = &[0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];

    // EC curve OIDs
    pub const SECP256R1: &[u8] = &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
    pub const SECP384R1: &[u8] = &[0x06, 0x05, 0x2B, 0x81, 0x04, 0x00, 0x22];

    // Hash algorithm OIDs
    pub const SHA256: &[u8] = &[
        0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
    ];
    pub const SHA384: &[u8] = &[
        0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02,
    ];
    pub const SHA512: &[u8] = &[
        0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03,
    ];
}

/// Signature algorithm types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    /// RSA PKCS#1 v1.5 with SHA-256
    RsaPkcs1Sha256,
    /// RSA PKCS#1 v1.5 with SHA-384
    RsaPkcs1Sha384,
    /// RSA PKCS#1 v1.5 with SHA-512
    RsaPkcs1Sha512,
    /// RSA-PSS with SHA-256
    RsaPssSha256,
    /// RSA-PSS with SHA-384
    RsaPssSha384,
    /// RSA-PSS with SHA-512
    RsaPssSha512,
    /// ECDSA with P-256 and SHA-256
    EcdsaP256Sha256,
    /// ECDSA with P-384 and SHA-384
    EcdsaP384Sha384,
    /// ECDSA with P-521 and SHA-512
    EcdsaP521Sha512,
    /// Unknown algorithm
    Unknown,
}

impl SignatureAlgorithm {
    /// Parse signature algorithm from OID bytes
    pub fn from_oid(oid: &[u8]) -> Self {
        // Match known OID patterns (ignoring the length byte in the DER encoding)
        let oid_value = if oid.len() > 2 && oid[0] == 0x06 {
            &oid[2..]
        } else {
            oid
        };

        // RSA PKCS#1 v1.5 with SHA-256: 1.2.840.113549.1.1.11
        if oid_value == &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B] {
            return SignatureAlgorithm::RsaPkcs1Sha256;
        }
        // RSA PKCS#1 v1.5 with SHA-384: 1.2.840.113549.1.1.12
        if oid_value == &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0C] {
            return SignatureAlgorithm::RsaPkcs1Sha384;
        }
        // RSA PKCS#1 v1.5 with SHA-512: 1.2.840.113549.1.1.13
        if oid_value == &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0D] {
            return SignatureAlgorithm::RsaPkcs1Sha512;
        }
        // RSA-PSS: 1.2.840.113549.1.1.10 (hash specified in params)
        if oid_value == &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0A] {
            return SignatureAlgorithm::RsaPssSha256; // Default, params will override
        }
        // ECDSA with SHA-256: 1.2.840.10045.4.3.2
        if oid_value == &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02] {
            return SignatureAlgorithm::EcdsaP256Sha256;
        }
        // ECDSA with SHA-384: 1.2.840.10045.4.3.3
        if oid_value == &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03] {
            return SignatureAlgorithm::EcdsaP384Sha384;
        }
        // ECDSA with SHA-512: 1.2.840.10045.4.3.4
        if oid_value == &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x04] {
            return SignatureAlgorithm::EcdsaP521Sha512;
        }

        SignatureAlgorithm::Unknown
    }
}

/// Public key types
#[derive(Debug, Clone)]
pub enum PublicKey {
    /// RSA public key (n, e)
    Rsa { modulus: Vec<u8>, exponent: Vec<u8> },
    /// EC public key (P-256)
    EcP256 {
        /// Uncompressed point (65 bytes: 04 || x || y)
        point: Vec<u8>,
    },
    /// EC public key (P-384)
    EcP384 { point: Vec<u8> },
}

impl PublicKey {
    /// Parse public key from SubjectPublicKeyInfo DER
    pub fn from_spki(spki: &[u8]) -> Option<Self> {
        let mut parser = Asn1Parser::new(spki);

        // SEQUENCE
        let _seq = parser.read_sequence()?;

        // AlgorithmIdentifier SEQUENCE
        let alg_id = parser.read_sequence()?;
        let alg_oid = Asn1Parser::new(&alg_id).read_oid()?;

        // BIT STRING containing the public key
        let key_bits = parser.read_bit_string()?;

        // Check algorithm OID
        let alg_oid_value = if alg_oid.len() > 2 && alg_oid[0] == 0x06 {
            &alg_oid[2..]
        } else {
            &alg_oid
        };

        // RSA
        if alg_oid_value == &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01] {
            return Self::parse_rsa_public_key(&key_bits);
        }

        // EC
        if alg_oid_value == &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01] {
            // Get curve OID from AlgorithmIdentifier params
            let mut alg_parser = Asn1Parser::new(&alg_id);
            alg_parser.read_oid(); // Skip algorithm OID
            let curve_oid = alg_parser.read_oid()?;

            let curve_oid_value = if curve_oid.len() > 2 && curve_oid[0] == 0x06 {
                &curve_oid[2..]
            } else {
                &curve_oid
            };

            // P-256 (secp256r1)
            if curve_oid_value == &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07] {
                if key_bits.len() == 65 && key_bits[0] == 0x04 {
                    return Some(PublicKey::EcP256 {
                        point: key_bits.to_vec(),
                    });
                }
            }

            // P-384 (secp384r1)
            if curve_oid_value == &[0x2B, 0x81, 0x04, 0x00, 0x22] {
                if key_bits.len() == 97 && key_bits[0] == 0x04 {
                    return Some(PublicKey::EcP384 {
                        point: key_bits.to_vec(),
                    });
                }
            }
        }

        None
    }

    /// Parse RSA public key from BIT STRING content
    fn parse_rsa_public_key(key_bits: &[u8]) -> Option<Self> {
        let mut parser = Asn1Parser::new(key_bits);

        // RSAPublicKey ::= SEQUENCE { modulus INTEGER, publicExponent INTEGER }
        let _seq = parser.read_sequence()?;

        let mut inner = Asn1Parser::new(&key_bits[parser.pos - _seq.len()..]);
        inner.skip(2)?; // Skip SEQUENCE tag and length

        let modulus = inner.read_integer()?;
        let exponent = inner.read_integer()?;

        Some(PublicKey::Rsa { modulus, exponent })
    }
}

/// Certificate signature verifier
pub struct SignatureVerifier;

impl SignatureVerifier {
    /// Verify certificate signature
    ///
    /// # Arguments
    /// * `tbs_certificate` - The TBSCertificate (To Be Signed Certificate) DER bytes
    /// * `signature_algorithm` - The signature algorithm used
    /// * `signature` - The signature bytes
    /// * `issuer_public_key` - The issuer's public key
    pub fn verify(
        tbs_certificate: &[u8],
        signature_algorithm: SignatureAlgorithm,
        signature: &[u8],
        issuer_public_key: &PublicKey,
    ) -> bool {
        match (signature_algorithm, issuer_public_key) {
            // RSA PKCS#1 v1.5
            (SignatureAlgorithm::RsaPkcs1Sha256, PublicKey::Rsa { modulus, exponent }) => {
                Self::verify_rsa_pkcs1_sha256(tbs_certificate, signature, modulus, exponent)
            }
            (SignatureAlgorithm::RsaPkcs1Sha384, PublicKey::Rsa { modulus, exponent }) => {
                Self::verify_rsa_pkcs1_sha384(tbs_certificate, signature, modulus, exponent)
            }
            (SignatureAlgorithm::RsaPkcs1Sha512, PublicKey::Rsa { modulus, exponent }) => {
                Self::verify_rsa_pkcs1_sha512(tbs_certificate, signature, modulus, exponent)
            }

            // RSA-PSS
            (SignatureAlgorithm::RsaPssSha256, PublicKey::Rsa { modulus, exponent }) => {
                Self::verify_rsa_pss_sha256(tbs_certificate, signature, modulus, exponent)
            }
            (SignatureAlgorithm::RsaPssSha384, PublicKey::Rsa { modulus, exponent }) => {
                Self::verify_rsa_pss_sha384(tbs_certificate, signature, modulus, exponent)
            }
            (SignatureAlgorithm::RsaPssSha512, PublicKey::Rsa { modulus, exponent }) => {
                Self::verify_rsa_pss_sha512(tbs_certificate, signature, modulus, exponent)
            }

            // ECDSA P-256
            (SignatureAlgorithm::EcdsaP256Sha256, PublicKey::EcP256 { point }) => {
                Self::verify_ecdsa_p256_sha256(tbs_certificate, signature, point)
            }

            // ECDSA P-384 (not fully implemented yet)
            (SignatureAlgorithm::EcdsaP384Sha384, PublicKey::EcP384 { .. }) => {
                // TODO: Implement P-384
                false
            }

            _ => false,
        }
    }

    /// Verify RSA PKCS#1 v1.5 with SHA-256
    fn verify_rsa_pkcs1_sha256(
        message: &[u8],
        signature: &[u8],
        modulus: &[u8],
        exponent: &[u8],
    ) -> bool {
        use crate::ncryptolib::bigint::BigInt;
        use crate::ncryptolib::rsa::RsaPublicKey;

        // Build RSA public key from components
        let n = BigInt::from_bytes_be(modulus);
        let e = BigInt::from_bytes_be(exponent);
        let pubkey = RsaPublicKey::new(n, e);

        // Verify signature (rsa_verify hashes internally, pass raw message)
        match crate::ncryptolib::rsa::rsa_verify(message, signature, &pubkey) {
            Ok(valid) => valid,
            Err(_) => false,
        }
    }

    /// Verify RSA PKCS#1 v1.5 with SHA-384
    fn verify_rsa_pkcs1_sha384(
        message: &[u8],
        signature: &[u8],
        modulus: &[u8],
        exponent: &[u8],
    ) -> bool {
        use crate::ncryptolib::bigint::BigInt;
        use crate::ncryptolib::rsa::RsaPublicKey;

        // Build RSA public key from components
        let n = BigInt::from_bytes_be(modulus);
        let e = BigInt::from_bytes_be(exponent);
        let pubkey = RsaPublicKey::new(n, e);

        // Note: Current rsa_verify only supports SHA-256
        // For SHA-384, we would need a separate implementation
        match crate::ncryptolib::rsa::rsa_verify(message, signature, &pubkey) {
            Ok(valid) => valid,
            Err(_) => false,
        }
    }

    /// Verify RSA PKCS#1 v1.5 with SHA-512
    fn verify_rsa_pkcs1_sha512(
        message: &[u8],
        signature: &[u8],
        modulus: &[u8],
        exponent: &[u8],
    ) -> bool {
        use crate::ncryptolib::bigint::BigInt;
        use crate::ncryptolib::rsa::RsaPublicKey;

        // Build RSA public key from components
        let n = BigInt::from_bytes_be(modulus);
        let e = BigInt::from_bytes_be(exponent);
        let pubkey = RsaPublicKey::new(n, e);

        // Note: Current rsa_verify only supports SHA-256
        // For SHA-512, we would need a separate implementation
        match crate::ncryptolib::rsa::rsa_verify(message, signature, &pubkey) {
            Ok(valid) => valid,
            Err(_) => false,
        }
    }

    /// Verify RSA-PSS with SHA-256
    fn verify_rsa_pss_sha256(
        message: &[u8],
        signature: &[u8],
        modulus: &[u8],
        exponent: &[u8],
    ) -> bool {
        use crate::ncryptolib::bigint::BigInt;
        use crate::ncryptolib::rsa::RsaPublicKey;

        // Build RSA public key from components
        let n = BigInt::from_bytes_be(modulus);
        let e = BigInt::from_bytes_be(exponent);
        let pubkey = RsaPublicKey::new(n, e);

        // Verify PSS signature (rsa_pss_verify hashes internally)
        match crate::ncryptolib::rsa::rsa_pss_verify(message, signature, &pubkey) {
            Ok(valid) => valid,
            Err(_) => false,
        }
    }

    /// Verify RSA-PSS with SHA-384
    fn verify_rsa_pss_sha384(
        message: &[u8],
        signature: &[u8],
        modulus: &[u8],
        exponent: &[u8],
    ) -> bool {
        use crate::ncryptolib::bigint::BigInt;
        use crate::ncryptolib::rsa::RsaPublicKey;

        // Build RSA public key from components
        let n = BigInt::from_bytes_be(modulus);
        let e = BigInt::from_bytes_be(exponent);
        let pubkey = RsaPublicKey::new(n, e);

        // Note: Current rsa_pss_verify only supports SHA-256
        match crate::ncryptolib::rsa::rsa_pss_verify(message, signature, &pubkey) {
            Ok(valid) => valid,
            Err(_) => false,
        }
    }

    /// Verify RSA-PSS with SHA-512
    fn verify_rsa_pss_sha512(
        message: &[u8],
        signature: &[u8],
        modulus: &[u8],
        exponent: &[u8],
    ) -> bool {
        use crate::ncryptolib::bigint::BigInt;
        use crate::ncryptolib::rsa::RsaPublicKey;

        // Build RSA public key from components
        let n = BigInt::from_bytes_be(modulus);
        let e = BigInt::from_bytes_be(exponent);
        let pubkey = RsaPublicKey::new(n, e);

        // Note: Current rsa_pss_verify only supports SHA-256
        match crate::ncryptolib::rsa::rsa_pss_verify(message, signature, &pubkey) {
            Ok(valid) => valid,
            Err(_) => false,
        }
    }

    /// Verify ECDSA P-256 with SHA-256
    fn verify_ecdsa_p256_sha256(message: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        // Hash the message
        let hash = crate::ncryptolib::sha256(message);

        // Parse the public key point
        let point = match crate::ncryptolib::p256::P256Point::from_uncompressed(public_key) {
            Some(p) => p,
            None => return false,
        };

        // Parse ECDSA signature from DER
        let sig = match crate::ncryptolib::p256::P256Signature::from_der(signature) {
            Some(s) => s,
            None => return false,
        };

        // Verify signature
        sig.verify(&point, &hash)
    }
}

/// Parse ECDSA signature from DER format
///
/// ECDSA-Sig-Value ::= SEQUENCE {
///     r INTEGER,
///     s INTEGER
/// }
fn parse_ecdsa_signature_der(der: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut parser = Asn1Parser::new(der);

    // Read outer SEQUENCE
    let _seq = parser.read_sequence()?;

    // Re-parse from the sequence content
    let mut inner = Asn1Parser::new(&der[2..]);

    // Read r INTEGER
    let r = inner.read_integer()?;

    // Read s INTEGER
    let s = inner.read_integer()?;

    // Normalize to 32 bytes (remove leading zeros or pad)
    let r_normalized = normalize_integer(&r, 32);
    let s_normalized = normalize_integer(&s, 32);

    Some((r_normalized, s_normalized))
}

/// Normalize integer to fixed size (remove leading zeros or pad with zeros)
fn normalize_integer(data: &[u8], size: usize) -> Vec<u8> {
    // Skip leading zeros
    let mut start = 0;
    while start < data.len() && data[start] == 0 {
        start += 1;
    }

    let significant = &data[start..];

    if significant.len() >= size {
        // Take the last 'size' bytes
        significant[significant.len() - size..].to_vec()
    } else {
        // Pad with leading zeros
        let mut result = vec![0u8; size - significant.len()];
        result.extend_from_slice(significant);
        result
    }
}

/// Simple ASN.1 DER parser
pub struct Asn1Parser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Asn1Parser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Skip n bytes
    pub fn skip(&mut self, n: usize) -> Option<()> {
        if self.pos + n > self.data.len() {
            return None;
        }
        self.pos += n;
        Some(())
    }

    /// Read length field
    fn read_length(&mut self) -> Option<usize> {
        if self.pos >= self.data.len() {
            return None;
        }

        let first = self.data[self.pos];
        self.pos += 1;

        if first < 0x80 {
            Some(first as usize)
        } else if first == 0x81 {
            if self.pos >= self.data.len() {
                return None;
            }
            let len = self.data[self.pos] as usize;
            self.pos += 1;
            Some(len)
        } else if first == 0x82 {
            if self.pos + 1 >= self.data.len() {
                return None;
            }
            let len = ((self.data[self.pos] as usize) << 8) | (self.data[self.pos + 1] as usize);
            self.pos += 2;
            Some(len)
        } else if first == 0x83 {
            if self.pos + 2 >= self.data.len() {
                return None;
            }
            let len = ((self.data[self.pos] as usize) << 16)
                | ((self.data[self.pos + 1] as usize) << 8)
                | (self.data[self.pos + 2] as usize);
            self.pos += 3;
            Some(len)
        } else {
            None
        }
    }

    /// Read SEQUENCE and return contents
    pub fn read_sequence(&mut self) -> Option<Vec<u8>> {
        if self.pos >= self.data.len() || self.data[self.pos] != 0x30 {
            return None;
        }
        self.pos += 1;

        let len = self.read_length()?;
        if self.pos + len > self.data.len() {
            return None;
        }

        let content = self.data[self.pos..self.pos + len].to_vec();
        self.pos += len;
        Some(content)
    }

    /// Read OID
    pub fn read_oid(&mut self) -> Option<Vec<u8>> {
        if self.pos >= self.data.len() || self.data[self.pos] != 0x06 {
            return None;
        }

        let start = self.pos;
        self.pos += 1;

        let len = self.read_length()?;
        if self.pos + len > self.data.len() {
            return None;
        }

        let end = self.pos + len;
        self.pos = end;

        Some(self.data[start..end].to_vec())
    }

    /// Read INTEGER
    pub fn read_integer(&mut self) -> Option<Vec<u8>> {
        if self.pos >= self.data.len() || self.data[self.pos] != 0x02 {
            return None;
        }
        self.pos += 1;

        let len = self.read_length()?;
        if self.pos + len > self.data.len() {
            return None;
        }

        let content = self.data[self.pos..self.pos + len].to_vec();
        self.pos += len;
        Some(content)
    }

    /// Read BIT STRING
    pub fn read_bit_string(&mut self) -> Option<Vec<u8>> {
        if self.pos >= self.data.len() || self.data[self.pos] != 0x03 {
            return None;
        }
        self.pos += 1;

        let len = self.read_length()?;
        if self.pos + len > self.data.len() || len < 1 {
            return None;
        }

        // First byte is number of unused bits (usually 0)
        let _unused_bits = self.data[self.pos];
        self.pos += 1;

        let content = self.data[self.pos..self.pos + len - 1].to_vec();
        self.pos += len - 1;
        Some(content)
    }

    /// Read OCTET STRING
    pub fn read_octet_string(&mut self) -> Option<Vec<u8>> {
        if self.pos >= self.data.len() || self.data[self.pos] != 0x04 {
            return None;
        }
        self.pos += 1;

        let len = self.read_length()?;
        if self.pos + len > self.data.len() {
            return None;
        }

        let content = self.data[self.pos..self.pos + len].to_vec();
        self.pos += len;
        Some(content)
    }

    /// Read context-specific tag [n]
    pub fn read_context(&mut self, tag: u8) -> Option<Vec<u8>> {
        let expected_tag = 0xA0 | tag;
        if self.pos >= self.data.len() || self.data[self.pos] != expected_tag {
            return None;
        }
        self.pos += 1;

        let len = self.read_length()?;
        if self.pos + len > self.data.len() {
            return None;
        }

        let content = self.data[self.pos..self.pos + len].to_vec();
        self.pos += len;
        Some(content)
    }

    /// Peek at next tag
    pub fn peek_tag(&self) -> Option<u8> {
        if self.pos < self.data.len() {
            Some(self.data[self.pos])
        } else {
            None
        }
    }

    /// Get remaining data
    pub fn remaining(&self) -> &[u8] {
        &self.data[self.pos..]
    }

    /// Check if at end
    pub fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }
}

/// Parsed X.509 TBSCertificate structure
pub struct TbsCertificate {
    /// Version (0 = v1, 1 = v2, 2 = v3)
    pub version: u8,
    /// Serial number
    pub serial_number: Vec<u8>,
    /// Signature algorithm OID
    pub signature_algorithm: Vec<u8>,
    /// Issuer name
    pub issuer: Vec<u8>,
    /// Validity period
    pub validity: CertValidity,
    /// Subject name
    pub subject: Vec<u8>,
    /// Subject public key info
    pub subject_public_key_info: Vec<u8>,
    /// Extensions (v3)
    pub extensions: Vec<Extension>,
}

/// Certificate validity period
pub struct CertValidity {
    /// Not Before (Unix timestamp)
    pub not_before: u64,
    /// Not After (Unix timestamp)
    pub not_after: u64,
}

/// X.509 Extension
pub struct Extension {
    /// OID
    pub oid: Vec<u8>,
    /// Critical flag
    pub critical: bool,
    /// Value
    pub value: Vec<u8>,
}

impl TbsCertificate {
    /// Parse TBSCertificate from DER
    pub fn from_der(der: &[u8]) -> Option<Self> {
        let mut parser = Asn1Parser::new(der);

        // TBSCertificate SEQUENCE
        let tbs_content = parser.read_sequence()?;
        let mut inner = Asn1Parser::new(&tbs_content);

        // Version [0] EXPLICIT INTEGER DEFAULT v1
        let version = if inner.peek_tag() == Some(0xA0) {
            let version_data = inner.read_context(0)?;
            let mut v_parser = Asn1Parser::new(&version_data);
            let v_int = v_parser.read_integer()?;
            v_int.first().copied().unwrap_or(0)
        } else {
            0 // Default v1
        };

        // Serial number
        let serial_number = inner.read_integer()?;

        // Signature algorithm
        let sig_alg_seq = inner.read_sequence()?;
        let mut sig_parser = Asn1Parser::new(&sig_alg_seq);
        let signature_algorithm = sig_parser.read_oid()?;

        // Issuer
        let issuer = inner.read_sequence()?;

        // Validity
        let validity_seq = inner.read_sequence()?;
        let validity = Self::parse_validity(&validity_seq)?;

        // Subject
        let subject = inner.read_sequence()?;

        // SubjectPublicKeyInfo
        let spki_start = inner.pos;
        let _spki = inner.read_sequence()?;
        let subject_public_key_info = tbs_content[spki_start..inner.pos].to_vec();

        // Extensions (optional, v3 only)
        let mut extensions = Vec::new();
        if version >= 2 && inner.peek_tag() == Some(0xA3) {
            let ext_data = inner.read_context(3)?;
            extensions = Self::parse_extensions(&ext_data)?;
        }

        Some(TbsCertificate {
            version,
            serial_number,
            signature_algorithm,
            issuer,
            validity,
            subject,
            subject_public_key_info,
            extensions,
        })
    }

    /// Parse validity period
    fn parse_validity(data: &[u8]) -> Option<CertValidity> {
        let mut parser = Asn1Parser::new(data);

        // notBefore
        let not_before = Self::parse_time(&mut parser)?;

        // notAfter
        let not_after = Self::parse_time(&mut parser)?;

        Some(CertValidity {
            not_before,
            not_after,
        })
    }

    /// Parse UTCTime or GeneralizedTime
    fn parse_time(parser: &mut Asn1Parser) -> Option<u64> {
        if parser.pos >= parser.data.len() {
            return None;
        }

        let tag = parser.data[parser.pos];
        parser.pos += 1;

        let len = parser.read_length()?;
        if parser.pos + len > parser.data.len() {
            return None;
        }

        let time_str = std::str::from_utf8(&parser.data[parser.pos..parser.pos + len]).ok()?;
        parser.pos += len;

        match tag {
            0x17 => Self::parse_utc_time(time_str),
            0x18 => Self::parse_generalized_time(time_str),
            _ => None,
        }
    }

    /// Parse UTCTime (YYMMDDHHMMSSZ)
    fn parse_utc_time(s: &str) -> Option<u64> {
        if s.len() < 12 {
            return None;
        }

        let year: u32 = s[0..2].parse().ok()?;
        let year = if year >= 50 { 1900 + year } else { 2000 + year };
        let month: u32 = s[2..4].parse().ok()?;
        let day: u32 = s[4..6].parse().ok()?;
        let hour: u32 = s[6..8].parse().ok()?;
        let minute: u32 = s[8..10].parse().ok()?;
        let second: u32 = s[10..12].parse().ok()?;

        // Simplified: calculate Unix timestamp
        // Days from epoch to year start
        let mut days: u64 = 0;
        for y in 1970..year {
            days += if is_leap_year(y) { 366 } else { 365 };
        }

        // Days in months
        let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for m in 0..(month as usize - 1) {
            days += month_days[m] as u64;
            if m == 1 && is_leap_year(year) {
                days += 1;
            }
        }

        days += (day - 1) as u64;

        let timestamp = days * 86400 + (hour as u64) * 3600 + (minute as u64) * 60 + second as u64;

        Some(timestamp)
    }

    /// Parse GeneralizedTime (YYYYMMDDHHMMSSZ)
    fn parse_generalized_time(s: &str) -> Option<u64> {
        if s.len() < 14 {
            return None;
        }

        let year: u32 = s[0..4].parse().ok()?;
        let month: u32 = s[4..6].parse().ok()?;
        let day: u32 = s[6..8].parse().ok()?;
        let hour: u32 = s[8..10].parse().ok()?;
        let minute: u32 = s[10..12].parse().ok()?;
        let second: u32 = s[12..14].parse().ok()?;

        let mut days: u64 = 0;
        for y in 1970..year {
            days += if is_leap_year(y) { 366 } else { 365 };
        }

        let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for m in 0..(month as usize - 1) {
            days += month_days[m] as u64;
            if m == 1 && is_leap_year(year) {
                days += 1;
            }
        }

        days += (day - 1) as u64;

        let timestamp = days * 86400 + (hour as u64) * 3600 + (minute as u64) * 60 + second as u64;

        Some(timestamp)
    }

    /// Parse extensions
    fn parse_extensions(data: &[u8]) -> Option<Vec<Extension>> {
        let mut parser = Asn1Parser::new(data);

        // Extensions SEQUENCE
        let ext_seq = parser.read_sequence()?;
        let mut ext_parser = Asn1Parser::new(&ext_seq);

        let mut extensions = Vec::new();

        while !ext_parser.is_empty() {
            if let Some(ext) = Self::parse_extension(&mut ext_parser) {
                extensions.push(ext);
            } else {
                break;
            }
        }

        Some(extensions)
    }

    /// Parse single extension
    fn parse_extension(parser: &mut Asn1Parser) -> Option<Extension> {
        let ext_seq = parser.read_sequence()?;
        let mut inner = Asn1Parser::new(&ext_seq);

        let oid = inner.read_oid()?;

        // Critical flag (optional BOOLEAN, defaults to FALSE)
        let critical = if inner.peek_tag() == Some(0x01) {
            inner.pos += 1;
            let len = inner.read_length()?;
            if len == 1 && inner.pos < inner.data.len() {
                let val = inner.data[inner.pos] != 0;
                inner.pos += 1;
                val
            } else {
                false
            }
        } else {
            false
        };

        // Extension value (OCTET STRING)
        let value = inner.read_octet_string()?;

        Some(Extension {
            oid,
            critical,
            value,
        })
    }
}

/// Check if year is leap year
fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_algorithm_from_oid() {
        // SHA-256 with RSA
        let oid = &[
            0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B,
        ];
        assert_eq!(
            SignatureAlgorithm::from_oid(oid),
            SignatureAlgorithm::RsaPkcs1Sha256
        );

        // ECDSA with SHA-256
        let oid = &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02];
        assert_eq!(
            SignatureAlgorithm::from_oid(oid),
            SignatureAlgorithm::EcdsaP256Sha256
        );
    }

    #[test]
    fn test_parse_utc_time() {
        // 2024-01-01 00:00:00 UTC
        let timestamp = TbsCertificate::parse_utc_time("240101000000Z");
        assert!(timestamp.is_some());
    }

    #[test]
    fn test_normalize_integer() {
        // Remove leading zeros
        let data = vec![0x00, 0x00, 0x12, 0x34];
        let normalized = normalize_integer(&data, 2);
        assert_eq!(normalized, vec![0x12, 0x34]);

        // Pad with zeros
        let data = vec![0x12];
        let normalized = normalize_integer(&data, 4);
        assert_eq!(normalized, vec![0x00, 0x00, 0x00, 0x12]);
    }
}
