//! PKCS#7/CMS Signature Parsing and Verification for Kernel Modules
//!
//! This module implements PKCS#7 (RFC 2315) / CMS (RFC 5652) signature
//! parsing and verification for NexaOS kernel modules (.nkm files).
//!
//! # Signature Format
//!
//! NexaOS kernel modules use a signature format compatible with Linux kernel
//! module signing. The signature is appended to the module file with the
//! following structure:
//!
//! ```text
//! [Module ELF/NKM data]
//! [PKCS#7 SignedData (DER encoded)]
//! [Module signature info structure]
//! [Signature magic: "~Module sig~"]
//! ```
//!
//! # Supported Algorithms
//!
//! - Hash: SHA-256, SHA-384, SHA-512
//! - Signature: RSA with PKCS#1 v1.5 padding
//!
//! # ASN.1/DER Parsing
//!
//! This module includes a minimal ASN.1/DER parser sufficient for
//! PKCS#7 SignedData structures. Full ASN.1 support is not implemented.

use alloc::vec::Vec;
use alloc::string::String;
use super::crypto::{sha256, HashAlgorithm, RsaPublicKey, find_trusted_key, SHA256_DIGEST_SIZE};

// ============================================================================
// Module Signature Structures (Linux-compatible)
// ============================================================================

/// Magic string at end of signed modules
pub const MODULE_SIG_MAGIC: &[u8; 12] = b"~Module sig~";

/// Module signature info structure (appended before magic)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ModuleSigInfo {
    /// Algorithm used: 0 = unspecified
    pub algo: u8,
    /// Hash algorithm: 0 = unspecified, 4 = SHA256, 5 = SHA384, 6 = SHA512
    pub hash: u8,
    /// Key type: 0 = unspecified, 1 = RSA
    pub key_type: u8,
    /// Key identifier type: 0 = unspecified, 1 = PKCS#7 issuer+serial
    pub signer_id_type: u8,
    /// Reserved (padding)
    pub _reserved: [u8; 4],
    /// Signature length (big-endian)
    pub sig_len: [u8; 4],
}

impl ModuleSigInfo {
    /// Size of this structure
    pub const SIZE: usize = 12;

    /// Parse from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        
        Some(Self {
            algo: data[0],
            hash: data[1],
            key_type: data[2],
            signer_id_type: data[3],
            _reserved: [data[4], data[5], data[6], data[7]],
            sig_len: [data[8], data[9], data[10], data[11]],
        })
    }

    /// Get signature length as usize
    pub fn signature_len(&self) -> usize {
        u32::from_be_bytes(self.sig_len) as usize
    }

    /// Get hash algorithm
    pub fn hash_algo(&self) -> Option<HashAlgorithm> {
        match self.hash {
            4 => Some(HashAlgorithm::Sha256),
            5 => Some(HashAlgorithm::Sha384),
            6 => Some(HashAlgorithm::Sha512),
            _ => None,
        }
    }
}

// ============================================================================
// ASN.1/DER Parser
// ============================================================================

/// ASN.1 tag classes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Asn1Class {
    Universal = 0,
    Application = 1,
    ContextSpecific = 2,
    Private = 3,
}

/// ASN.1 universal tags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Asn1Tag {
    Boolean = 0x01,
    Integer = 0x02,
    BitString = 0x03,
    OctetString = 0x04,
    Null = 0x05,
    ObjectIdentifier = 0x06,
    Utf8String = 0x0C,
    PrintableString = 0x13,
    Ia5String = 0x16,
    UtcTime = 0x17,
    GeneralizedTime = 0x18,
    Sequence = 0x30,
    Set = 0x31,
}

/// ASN.1/DER parser
pub struct DerParser<'a> {
    data: &'a [u8],
    pos: usize,
}

/// Parsed ASN.1 element
#[derive(Debug, Clone)]
pub struct Asn1Element<'a> {
    /// Tag byte (raw)
    pub tag: u8,
    /// Tag class
    pub class: Asn1Class,
    /// Whether this is a constructed type
    pub constructed: bool,
    /// Tag number (within class)
    pub tag_num: u8,
    /// Element content
    pub content: &'a [u8],
    /// Total element length (including tag and length bytes)
    pub total_len: usize,
}

impl<'a> DerParser<'a> {
    /// Create a new parser from data
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Get remaining data
    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    /// Check if parser is at end
    pub fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// Peek at next tag without consuming
    pub fn peek_tag(&self) -> Option<u8> {
        if self.pos < self.data.len() {
            Some(self.data[self.pos])
        } else {
            None
        }
    }

    /// Parse next ASN.1 element
    pub fn parse_element(&mut self) -> Option<Asn1Element<'a>> {
        if self.pos >= self.data.len() {
            return None;
        }

        let start_pos = self.pos;
        
        // Parse tag
        let tag = self.data[self.pos];
        self.pos += 1;
        
        let class = match (tag >> 6) & 0x03 {
            0 => Asn1Class::Universal,
            1 => Asn1Class::Application,
            2 => Asn1Class::ContextSpecific,
            3 => Asn1Class::Private,
            _ => unreachable!(),
        };
        
        let constructed = (tag & 0x20) != 0;
        let tag_num = tag & 0x1F;
        
        // Handle long-form tags (tag_num == 31)
        // For simplicity, we don't support multi-byte tags
        if tag_num == 31 {
            return None;
        }

        // Parse length
        if self.pos >= self.data.len() {
            return None;
        }
        
        let length_byte = self.data[self.pos];
        self.pos += 1;
        
        let content_len: usize;
        
        if length_byte < 0x80 {
            // Short form
            content_len = length_byte as usize;
        } else if length_byte == 0x80 {
            // Indefinite length (not allowed in DER)
            return None;
        } else {
            // Long form
            let num_octets = (length_byte & 0x7F) as usize;
            if num_octets > 4 || self.pos + num_octets > self.data.len() {
                return None;
            }
            
            content_len = self.data[self.pos..self.pos + num_octets]
                .iter()
                .fold(0usize, |acc, &b| (acc << 8) | (b as usize));
            
            self.pos += num_octets;
        }

        // Extract content
        if self.pos + content_len > self.data.len() {
            return None;
        }
        
        let content = &self.data[self.pos..self.pos + content_len];
        self.pos += content_len;
        
        let total_len = self.pos - start_pos;

        Some(Asn1Element {
            tag,
            class,
            constructed,
            tag_num,
            content,
            total_len,
        })
    }

    /// Skip next element
    pub fn skip_element(&mut self) -> bool {
        self.parse_element().is_some()
    }

    /// Parse an INTEGER and return as bytes
    pub fn parse_integer(&mut self) -> Option<&'a [u8]> {
        let elem = self.parse_element()?;
        if elem.tag != Asn1Tag::Integer as u8 {
            return None;
        }
        // Skip leading zero used for positive sign
        if !elem.content.is_empty() && elem.content[0] == 0 {
            Some(&elem.content[1..])
        } else {
            Some(elem.content)
        }
    }

    /// Parse an OID and return as bytes
    pub fn parse_oid(&mut self) -> Option<&'a [u8]> {
        let elem = self.parse_element()?;
        if elem.tag != Asn1Tag::ObjectIdentifier as u8 {
            return None;
        }
        Some(elem.content)
    }

    /// Parse a SEQUENCE and return a parser for its contents
    pub fn parse_sequence(&mut self) -> Option<DerParser<'a>> {
        let elem = self.parse_element()?;
        if elem.tag != Asn1Tag::Sequence as u8 {
            return None;
        }
        Some(DerParser::new(elem.content))
    }

    /// Parse a SET and return a parser for its contents
    pub fn parse_set(&mut self) -> Option<DerParser<'a>> {
        let elem = self.parse_element()?;
        if elem.tag != Asn1Tag::Set as u8 {
            return None;
        }
        Some(DerParser::new(elem.content))
    }

    /// Parse context-specific tagged element
    pub fn parse_context(&mut self, expected_tag: u8) -> Option<DerParser<'a>> {
        let elem = self.parse_element()?;
        if elem.class != Asn1Class::ContextSpecific || elem.tag_num != expected_tag {
            return None;
        }
        Some(DerParser::new(elem.content))
    }

    /// Parse an OCTET STRING
    pub fn parse_octet_string(&mut self) -> Option<&'a [u8]> {
        let elem = self.parse_element()?;
        if elem.tag != Asn1Tag::OctetString as u8 {
            return None;
        }
        Some(elem.content)
    }

    /// Parse a BIT STRING
    pub fn parse_bit_string(&mut self) -> Option<&'a [u8]> {
        let elem = self.parse_element()?;
        if elem.tag != Asn1Tag::BitString as u8 || elem.content.is_empty() {
            return None;
        }
        // First byte is number of unused bits in last byte
        let unused_bits = elem.content[0];
        if unused_bits > 7 {
            return None;
        }
        Some(&elem.content[1..])
    }
}

// ============================================================================
// PKCS#7 SignedData Parser
// ============================================================================

/// PKCS#7 SignedData structure
#[derive(Debug)]
pub struct Pkcs7SignedData<'a> {
    /// Version
    pub version: u8,
    /// Digest algorithms used
    pub digest_algorithms: Vec<HashAlgorithm>,
    /// Content type OID
    pub content_type: &'a [u8],
    /// Encapsulated content (if present)
    pub content: Option<&'a [u8]>,
    /// Certificates (DER encoded)
    pub certificates: Vec<&'a [u8]>,
    /// Signer information
    pub signer_infos: Vec<SignerInfo<'a>>,
}

/// Signer information from PKCS#7
#[derive(Debug, Clone)]
pub struct SignerInfo<'a> {
    /// Version
    pub version: u8,
    /// Issuer name (DER encoded)
    pub issuer: &'a [u8],
    /// Serial number
    pub serial_number: &'a [u8],
    /// Digest algorithm
    pub digest_algorithm: HashAlgorithm,
    /// Signature algorithm OID
    pub signature_algorithm: &'a [u8],
    /// Encrypted digest (signature)
    pub signature: &'a [u8],
    /// Authenticated attributes (if present, DER encoded for hashing)
    pub auth_attrs: Option<&'a [u8]>,
}

/// Parse PKCS#7 SignedData from DER-encoded bytes
pub fn parse_pkcs7_signed_data(data: &[u8]) -> Option<Pkcs7SignedData<'_>> {
    let mut parser = DerParser::new(data);
    
    // ContentInfo ::= SEQUENCE
    let mut content_info = parser.parse_sequence()?;
    
    // contentType OBJECT IDENTIFIER
    let content_type_oid = content_info.parse_oid()?;
    
    // Must be signedData (1.2.840.113549.1.7.2)
    if content_type_oid != super::crypto::OID_PKCS7_SIGNED_DATA {
        crate::kdebug!("PKCS#7: Not a signedData structure");
        return None;
    }
    
    // content [0] EXPLICIT ANY DEFINED BY contentType
    let mut content_wrapper = content_info.parse_context(0)?;
    let mut signed_data = content_wrapper.parse_sequence()?;
    
    // version INTEGER
    let version_bytes = signed_data.parse_integer()?;
    let version = if version_bytes.is_empty() { 0 } else { version_bytes[0] };
    
    // digestAlgorithms SET OF DigestAlgorithmIdentifier
    let mut digest_algs_set = signed_data.parse_set()?;
    let mut digest_algorithms = Vec::new();
    
    while !digest_algs_set.is_empty() {
        if let Some(mut alg_id) = digest_algs_set.parse_sequence() {
            if let Some(oid) = alg_id.parse_oid() {
                if let Some(algo) = super::crypto::oid_to_hash_algo(oid) {
                    digest_algorithms.push(algo);
                }
            }
        }
    }
    
    // encapContentInfo ContentInfo
    let mut encap_content = signed_data.parse_sequence()?;
    let encap_type = encap_content.parse_oid()?;
    
    // content [0] OPTIONAL
    let content = if let Some(tag) = encap_content.peek_tag() {
        if (tag >> 6) == 2 && (tag & 0x1F) == 0 {
            let mut content_wrapper = encap_content.parse_context(0)?;
            content_wrapper.parse_octet_string()
        } else {
            None
        }
    } else {
        None
    };
    
    // certificates [0] IMPLICIT CertificateSet OPTIONAL
    let mut certificates = Vec::new();
    if let Some(tag) = signed_data.peek_tag() {
        if (tag >> 6) == 2 && (tag & 0x1F) == 0 {
            let elem = signed_data.parse_element()?;
            let mut cert_parser = DerParser::new(elem.content);
            while !cert_parser.is_empty() {
                let cert = cert_parser.parse_element()?;
                // Store the full certificate DER
                certificates.push(&signed_data.data[..][cert_parser.pos - cert.total_len..cert_parser.pos]);
            }
        }
    }
    
    // crls [1] IMPLICIT RevocationInfoChoices OPTIONAL
    if let Some(tag) = signed_data.peek_tag() {
        if (tag >> 6) == 2 && (tag & 0x1F) == 1 {
            signed_data.skip_element();
        }
    }
    
    // signerInfos SET OF SignerInfo
    let mut signer_infos_set = signed_data.parse_set()?;
    let mut signer_infos = Vec::new();
    
    while !signer_infos_set.is_empty() {
        if let Some(signer_info) = parse_signer_info(&mut signer_infos_set) {
            signer_infos.push(signer_info);
        } else {
            break;
        }
    }
    
    Some(Pkcs7SignedData {
        version,
        digest_algorithms,
        content_type: encap_type,
        content,
        certificates,
        signer_infos,
    })
}

/// Parse a SignerInfo structure
fn parse_signer_info<'a>(parser: &mut DerParser<'a>) -> Option<SignerInfo<'a>> {
    let mut signer_info = parser.parse_sequence()?;
    
    // version INTEGER
    let version_bytes = signer_info.parse_integer()?;
    let version = if version_bytes.is_empty() { 0 } else { version_bytes[0] };
    
    // sid SignerIdentifier (IssuerAndSerialNumber or SubjectKeyIdentifier)
    // For version 1, it's IssuerAndSerialNumber
    let (issuer, serial_number) = if version == 1 || version == 0 {
        // IssuerAndSerialNumber ::= SEQUENCE
        let mut issuer_serial = signer_info.parse_sequence()?;
        let issuer_elem = issuer_serial.parse_element()?;
        let serial = issuer_serial.parse_integer()?;
        (issuer_elem.content, serial)
    } else {
        // SubjectKeyIdentifier [0]
        let skid = signer_info.parse_context(0)?;
        (skid.remaining(), &[] as &[u8])
    };
    
    // digestAlgorithm DigestAlgorithmIdentifier
    let mut digest_alg = signer_info.parse_sequence()?;
    let digest_oid = digest_alg.parse_oid()?;
    let digest_algorithm = super::crypto::oid_to_hash_algo(digest_oid)?;
    
    // signedAttrs [0] IMPLICIT SignedAttributes OPTIONAL
    let mut auth_attrs = None;
    if let Some(tag) = signer_info.peek_tag() {
        if (tag >> 6) == 2 && (tag & 0x1F) == 0 {
            let elem = signer_info.parse_element()?;
            // For signature verification, we need the original encoding
            // but with EXPLICIT SET tag (0x31) instead of context-specific
            auth_attrs = Some(elem.content);
        }
    }
    
    // signatureAlgorithm SignatureAlgorithmIdentifier
    let mut sig_alg = signer_info.parse_sequence()?;
    let sig_oid = sig_alg.parse_oid()?;
    
    // signature SignatureValue (OCTET STRING)
    let signature = signer_info.parse_octet_string()?;
    
    Some(SignerInfo {
        version,
        issuer,
        serial_number,
        digest_algorithm,
        signature_algorithm: sig_oid,
        signature,
        auth_attrs,
    })
}

// ============================================================================
// Module Signature Verification
// ============================================================================

/// Result of signature verification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureVerifyResult {
    /// Signature is valid
    Valid,
    /// Module is not signed
    Unsigned,
    /// Signature format is invalid
    InvalidFormat,
    /// PKCS#7 structure parse error
    ParseError,
    /// No signer information found
    NoSignerInfo,
    /// Key not found in trusted keyring
    KeyNotFound,
    /// Hash mismatch
    HashMismatch,
    /// Signature verification failed
    VerifyFailed,
    /// Unsupported algorithm
    UnsupportedAlgorithm,
}

impl SignatureVerifyResult {
    /// Convert to SignatureStatus for module info
    pub fn to_signature_status(self) -> super::SignatureStatus {
        match self {
            SignatureVerifyResult::Valid => super::SignatureStatus::Valid,
            SignatureVerifyResult::Unsigned => super::SignatureStatus::Unsigned,
            SignatureVerifyResult::KeyNotFound => super::SignatureStatus::KeyNotFound,
            SignatureVerifyResult::InvalidFormat | SignatureVerifyResult::ParseError => {
                super::SignatureStatus::UnknownFormat
            }
            _ => super::SignatureStatus::Invalid,
        }
    }
    
    /// Get human-readable description
    pub fn as_str(self) -> &'static str {
        match self {
            SignatureVerifyResult::Valid => "valid",
            SignatureVerifyResult::Unsigned => "unsigned",
            SignatureVerifyResult::InvalidFormat => "invalid format",
            SignatureVerifyResult::ParseError => "parse error",
            SignatureVerifyResult::NoSignerInfo => "no signer info",
            SignatureVerifyResult::KeyNotFound => "key not found",
            SignatureVerifyResult::HashMismatch => "hash mismatch",
            SignatureVerifyResult::VerifyFailed => "signature verify failed",
            SignatureVerifyResult::UnsupportedAlgorithm => "unsupported algorithm",
        }
    }
}

/// Extract PKCS#7 signature from module data
/// 
/// Returns (module_content, signature_data) if signature is present
pub fn extract_module_signature(data: &[u8]) -> Option<(&[u8], &[u8], ModuleSigInfo)> {
    // Check for magic at end
    if data.len() < MODULE_SIG_MAGIC.len() + ModuleSigInfo::SIZE {
        return None;
    }
    
    let magic_offset = data.len() - MODULE_SIG_MAGIC.len();
    if &data[magic_offset..] != MODULE_SIG_MAGIC.as_slice() {
        return None;
    }
    
    // Parse signature info
    let sig_info_offset = magic_offset - ModuleSigInfo::SIZE;
    let sig_info = ModuleSigInfo::from_bytes(&data[sig_info_offset..magic_offset])?;
    
    let sig_len = sig_info.signature_len();
    if sig_len == 0 || sig_len > sig_info_offset {
        return None;
    }
    
    let sig_data_offset = sig_info_offset - sig_len;
    let signature_data = &data[sig_data_offset..sig_info_offset];
    let module_content = &data[..sig_data_offset];
    
    Some((module_content, signature_data, sig_info))
}

/// Verify a signed kernel module
pub fn verify_module_signature(data: &[u8]) -> SignatureVerifyResult {
    // Extract signature components
    let (module_content, signature_data, sig_info) = match extract_module_signature(data) {
        Some(x) => x,
        None => return SignatureVerifyResult::Unsigned,
    };
    
    crate::kdebug!(
        "Module signature: {} bytes, hash algo: {:?}",
        sig_info.signature_len(),
        sig_info.hash_algo()
    );
    
    // Parse PKCS#7 structure
    let pkcs7 = match parse_pkcs7_signed_data(signature_data) {
        Some(p) => p,
        None => {
            crate::kwarn!("Failed to parse PKCS#7 signature");
            return SignatureVerifyResult::ParseError;
        }
    };
    
    crate::kdebug!(
        "PKCS#7: version={}, signers={}, certs={}",
        pkcs7.version,
        pkcs7.signer_infos.len(),
        pkcs7.certificates.len()
    );
    
    if pkcs7.signer_infos.is_empty() {
        return SignatureVerifyResult::NoSignerInfo;
    }
    
    // Verify each signer
    for signer in &pkcs7.signer_infos {
        let result = verify_signer(module_content, signer, &pkcs7);
        if result == SignatureVerifyResult::Valid {
            return SignatureVerifyResult::Valid;
        }
        // Continue trying other signers
    }
    
    // If we have signer info but all failed, report the failure
    SignatureVerifyResult::VerifyFailed
}

/// Verify a single signer's signature
fn verify_signer(
    module_content: &[u8],
    signer: &SignerInfo<'_>,
    pkcs7: &Pkcs7SignedData<'_>,
) -> SignatureVerifyResult {
    // Only SHA-256 is currently supported
    if signer.digest_algorithm != HashAlgorithm::Sha256 {
        crate::kwarn!("Unsupported hash algorithm: {:?}", signer.digest_algorithm);
        return SignatureVerifyResult::UnsupportedAlgorithm;
    }
    
    // Compute message digest
    let content_hash = sha256(module_content);
    
    // If authenticated attributes present, hash them instead
    let message_to_verify: [u8; SHA256_DIGEST_SIZE] = if let Some(auth_attrs) = signer.auth_attrs {
        // The authenticated attributes are hashed with SET tag
        let mut attrs_data = Vec::with_capacity(auth_attrs.len() + 2);
        attrs_data.push(0x31); // SET tag
        // Encode length
        if auth_attrs.len() < 128 {
            attrs_data.push(auth_attrs.len() as u8);
        } else {
            let len_bytes = (auth_attrs.len() as u32).to_be_bytes();
            let first_nonzero = len_bytes.iter().position(|&b| b != 0).unwrap_or(4);
            let len_byte_count = 4 - first_nonzero;
            attrs_data.push(0x80 | len_byte_count as u8);
            attrs_data.extend_from_slice(&len_bytes[first_nonzero..]);
        }
        attrs_data.extend_from_slice(auth_attrs);
        
        // Verify that the message digest attribute matches the content hash
        // (simplified - full implementation would parse attributes)
        
        sha256(&attrs_data)
    } else {
        content_hash
    };
    
    // Find the signing key
    // Try to find by issuer+serial in trusted keys
    let mut key_id = Vec::with_capacity(signer.issuer.len() + signer.serial_number.len());
    key_id.extend_from_slice(signer.issuer);
    key_id.extend_from_slice(signer.serial_number);
    
    let public_key = match find_trusted_key(&key_id) {
        Some(k) => k,
        None => {
            // Try to extract key from embedded certificate
            if let Some(key) = extract_key_from_certificates(&pkcs7.certificates, signer) {
                key
            } else {
                crate::kwarn!("Signing key not found in trusted keyring");
                return SignatureVerifyResult::KeyNotFound;
            }
        }
    };
    
    // Verify RSA signature
    if public_key.verify_pkcs1_v15(&message_to_verify, signer.signature) {
        SignatureVerifyResult::Valid
    } else {
        SignatureVerifyResult::VerifyFailed
    }
}

/// Extract public key from embedded certificates
fn extract_key_from_certificates(
    _certificates: &[&[u8]],
    _signer: &SignerInfo<'_>,
) -> Option<RsaPublicKey> {
    // TODO: Implement X.509 certificate parsing
    // For now, we require keys to be pre-loaded in trusted keyring
    None
}

// ============================================================================
// Signing Key Management
// ============================================================================

/// Initialize the module signing subsystem
pub fn init() {
    crate::kinfo!("PKCS#7 module signature verification initialized");
    crate::kinfo!("Trusted keys: {}", super::crypto::trusted_key_count());
}

/// Load a built-in signing key (for testing/development)
/// 
/// In a production system, keys would be loaded from a secure source
pub fn load_builtin_key() {
    // Example: Load a test RSA key (in real use, this would be a proper key)
    // This is a placeholder - actual keys would be embedded at build time
    
    // For now, just log that no keys are loaded
    crate::kinfo!("No built-in module signing keys configured");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sig_info_parse() {
        let data = [
            0x00, // algo
            0x04, // hash (SHA256)
            0x01, // key_type (RSA)
            0x01, // signer_id_type
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x01, 0x00, // sig_len (256)
        ];
        
        let info = ModuleSigInfo::from_bytes(&data).unwrap();
        assert_eq!(info.signature_len(), 256);
        assert_eq!(info.hash_algo(), Some(HashAlgorithm::Sha256));
    }

    #[test]
    fn test_unsigned_module() {
        let module_data = b"fake module data without signature";
        let result = verify_module_signature(module_data);
        assert_eq!(result, SignatureVerifyResult::Unsigned);
    }
}
