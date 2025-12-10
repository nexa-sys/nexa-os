//! ASN.1 Parsing and Encoding
//!
//! Provides basic ASN.1 DER parsing and encoding for X.509 and other cryptographic data.

use std::vec::Vec;

/// ASN.1 tag types
pub mod asn1_tag {
    pub const BOOLEAN: u8 = 0x01;
    pub const INTEGER: u8 = 0x02;
    pub const BIT_STRING: u8 = 0x03;
    pub const OCTET_STRING: u8 = 0x04;
    pub const NULL: u8 = 0x05;
    pub const OID: u8 = 0x06;
    pub const UTF8_STRING: u8 = 0x0C;
    pub const PRINTABLE_STRING: u8 = 0x13;
    pub const IA5_STRING: u8 = 0x16;
    pub const UTC_TIME: u8 = 0x17;
    pub const GENERALIZED_TIME: u8 = 0x18;
    pub const SEQUENCE: u8 = 0x30;
    pub const SET: u8 = 0x31;
    pub const CONTEXT_0: u8 = 0xA0;
    pub const CONTEXT_1: u8 = 0xA1;
    pub const CONTEXT_2: u8 = 0xA2;
    pub const CONTEXT_3: u8 = 0xA3;
}

/// ASN.1 Object Identifier
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Oid {
    /// DER-encoded OID bytes (without tag and length)
    pub bytes: Vec<u8>,
}

impl Oid {
    /// Create OID from bytes
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.to_vec(),
        }
    }

    /// Create OID from string (dotted notation like "1.2.840.113549.1.1.1")
    pub fn from_str(s: &str) -> Option<Self> {
        let parts: Vec<u32> = s.split('.').filter_map(|p| p.parse().ok()).collect();

        if parts.len() < 2 {
            return None;
        }

        let mut bytes = Vec::new();

        // First two components are encoded as (first * 40) + second
        bytes.push((parts[0] * 40 + parts[1]) as u8);

        // Remaining components use base-128 encoding
        for &comp in &parts[2..] {
            encode_base128(&mut bytes, comp);
        }

        Some(Self { bytes })
    }

    /// Convert to string (dotted notation)
    pub fn to_string(&self) -> String {
        if self.bytes.is_empty() {
            return String::new();
        }

        let mut result = Vec::new();

        // Decode first byte
        let first = self.bytes[0];
        result.push((first / 40) as u32);
        result.push((first % 40) as u32);

        // Decode remaining bytes
        let mut i = 1;
        while i < self.bytes.len() {
            let (val, consumed) = decode_base128(&self.bytes[i..]);
            result.push(val);
            i += consumed;
        }

        result
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(".")
    }
}

/// Encode value in base-128 (for OID)
fn encode_base128(out: &mut Vec<u8>, mut val: u32) {
    if val == 0 {
        out.push(0);
        return;
    }

    let mut bytes = Vec::new();
    while val > 0 {
        bytes.push((val & 0x7F) as u8);
        val >>= 7;
    }

    for (i, &b) in bytes.iter().rev().enumerate() {
        if i < bytes.len() - 1 {
            out.push(b | 0x80);
        } else {
            out.push(b);
        }
    }
}

/// Decode base-128 value
fn decode_base128(data: &[u8]) -> (u32, usize) {
    let mut val: u32 = 0;
    let mut i = 0;

    while i < data.len() {
        val = (val << 7) | (data[i] & 0x7F) as u32;
        i += 1;
        if data[i - 1] & 0x80 == 0 {
            break;
        }
    }

    (val, i)
}

/// Common OIDs
pub mod oids {
    use super::Oid;

    /// RSA encryption
    pub fn rsa_encryption() -> Oid {
        Oid::from_bytes(&[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01])
    }

    /// SHA256 with RSA
    pub fn sha256_with_rsa() -> Oid {
        Oid::from_bytes(&[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B])
    }

    /// SHA384 with RSA
    pub fn sha384_with_rsa() -> Oid {
        Oid::from_bytes(&[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0C])
    }

    /// SHA512 with RSA
    pub fn sha512_with_rsa() -> Oid {
        Oid::from_bytes(&[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0D])
    }

    /// ECDSA with SHA256
    pub fn ecdsa_with_sha256() -> Oid {
        Oid::from_bytes(&[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02])
    }

    /// ECDSA with SHA384
    pub fn ecdsa_with_sha384() -> Oid {
        Oid::from_bytes(&[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03])
    }

    /// Ed25519
    pub fn ed25519() -> Oid {
        Oid::from_bytes(&[0x2B, 0x65, 0x70])
    }

    /// X25519
    pub fn x25519() -> Oid {
        Oid::from_bytes(&[0x2B, 0x65, 0x6E])
    }

    /// P-256 curve
    pub fn secp256r1() -> Oid {
        Oid::from_bytes(&[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07])
    }

    /// P-384 curve
    pub fn secp384r1() -> Oid {
        Oid::from_bytes(&[0x2B, 0x81, 0x04, 0x00, 0x22])
    }

    /// Common Name (CN)
    pub fn common_name() -> Oid {
        Oid::from_bytes(&[0x55, 0x04, 0x03])
    }

    /// Organization (O)
    pub fn organization() -> Oid {
        Oid::from_bytes(&[0x55, 0x04, 0x0A])
    }

    /// Country (C)
    pub fn country() -> Oid {
        Oid::from_bytes(&[0x55, 0x04, 0x06])
    }
}

/// Parse DER length
pub fn parse_length(data: &[u8]) -> Option<(usize, usize)> {
    if data.is_empty() {
        return None;
    }

    let first = data[0];

    if first < 0x80 {
        // Short form
        return Some((first as usize, 1));
    }

    if first == 0x80 {
        // Indefinite length (not supported in DER)
        return None;
    }

    // Long form
    let num_bytes = (first & 0x7F) as usize;
    if num_bytes > 4 || data.len() < 1 + num_bytes {
        return None;
    }

    let mut len: usize = 0;
    for i in 0..num_bytes {
        len = (len << 8) | data[1 + i] as usize;
    }

    Some((len, 1 + num_bytes))
}

/// Encode DER length
pub fn encode_length(len: usize) -> Vec<u8> {
    if len < 0x80 {
        return vec![len as u8];
    }

    let mut bytes = Vec::new();
    let mut val = len;
    while val > 0 {
        bytes.push((val & 0xFF) as u8);
        val >>= 8;
    }
    bytes.reverse();

    let mut result = vec![0x80 | bytes.len() as u8];
    result.extend(bytes);
    result
}

/// Parse DER TLV (Tag-Length-Value)
pub fn parse_tlv(data: &[u8]) -> Option<(u8, &[u8], usize)> {
    if data.is_empty() {
        return None;
    }

    let tag = data[0];
    let (len, len_bytes) = parse_length(&data[1..])?;

    let total = 1 + len_bytes + len;
    if data.len() < total {
        return None;
    }

    let value = &data[1 + len_bytes..1 + len_bytes + len];
    Some((tag, value, total))
}

/// Encode DER TLV
pub fn encode_tlv(tag: u8, value: &[u8]) -> Vec<u8> {
    let mut result = vec![tag];
    result.extend(encode_length(value.len()));
    result.extend(value);
    result
}

/// Parse INTEGER from DER
pub fn parse_integer(data: &[u8]) -> Option<Vec<u8>> {
    let (tag, value, _) = parse_tlv(data)?;
    if tag != asn1_tag::INTEGER {
        return None;
    }

    // Remove leading zero if present (sign byte)
    if !value.is_empty() && value[0] == 0 {
        Some(value[1..].to_vec())
    } else {
        Some(value.to_vec())
    }
}

/// Encode INTEGER to DER
pub fn encode_integer(value: &[u8]) -> Vec<u8> {
    // Skip leading zeros
    let mut start = 0;
    while start < value.len() && value[start] == 0 {
        start += 1;
    }

    let significant = if start == value.len() {
        &[0u8][..]
    } else {
        &value[start..]
    };

    // Add leading zero if high bit is set
    let needs_zero = !significant.is_empty() && significant[0] & 0x80 != 0;

    let mut data = Vec::new();
    if needs_zero {
        data.push(0);
    }
    data.extend(significant);

    encode_tlv(asn1_tag::INTEGER, &data)
}

/// Parse BIT STRING from DER
pub fn parse_bit_string(data: &[u8]) -> Option<(u8, Vec<u8>)> {
    let (tag, value, _) = parse_tlv(data)?;
    if tag != asn1_tag::BIT_STRING || value.is_empty() {
        return None;
    }

    let unused_bits = value[0];
    Some((unused_bits, value[1..].to_vec()))
}

/// Encode BIT STRING to DER
pub fn encode_bit_string(unused_bits: u8, value: &[u8]) -> Vec<u8> {
    let mut data = vec![unused_bits];
    data.extend(value);
    encode_tlv(asn1_tag::BIT_STRING, &data)
}

/// Parse OCTET STRING from DER
pub fn parse_octet_string(data: &[u8]) -> Option<Vec<u8>> {
    let (tag, value, _) = parse_tlv(data)?;
    if tag != asn1_tag::OCTET_STRING {
        return None;
    }
    Some(value.to_vec())
}

/// Encode OCTET STRING to DER
pub fn encode_octet_string(value: &[u8]) -> Vec<u8> {
    encode_tlv(asn1_tag::OCTET_STRING, value)
}

/// Parse OID from DER
pub fn parse_oid(data: &[u8]) -> Option<Oid> {
    let (tag, value, _) = parse_tlv(data)?;
    if tag != asn1_tag::OID {
        return None;
    }
    Some(Oid::from_bytes(value))
}

/// Encode OID to DER
pub fn encode_oid(oid: &Oid) -> Vec<u8> {
    encode_tlv(asn1_tag::OID, &oid.bytes)
}

/// Parse SEQUENCE from DER
pub fn parse_sequence(data: &[u8]) -> Option<Vec<u8>> {
    let (tag, value, _) = parse_tlv(data)?;
    if tag != asn1_tag::SEQUENCE {
        return None;
    }
    Some(value.to_vec())
}

/// Encode SEQUENCE to DER
pub fn encode_sequence(contents: &[u8]) -> Vec<u8> {
    encode_tlv(asn1_tag::SEQUENCE, contents)
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// ASN1_TIME type
pub struct ASN1_TIME {
    data: Vec<u8>,
    time_type: u8,
}

impl ASN1_TIME {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            time_type: asn1_tag::UTC_TIME,
        }
    }
}

impl Default for ASN1_TIME {
    fn default() -> Self {
        Self::new()
    }
}

/// ASN1_INTEGER type
pub struct ASN1_INTEGER {
    data: Vec<u8>,
}

impl ASN1_INTEGER {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }
}

impl Default for ASN1_INTEGER {
    fn default() -> Self {
        Self::new()
    }
}

/// ASN1_STRING type
pub struct ASN1_STRING {
    data: Vec<u8>,
    string_type: u8,
}

impl ASN1_STRING {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            string_type: asn1_tag::UTF8_STRING,
        }
    }
}

impl Default for ASN1_STRING {
    fn default() -> Self {
        Self::new()
    }
}

/// ASN1_TIME_new
#[no_mangle]
pub extern "C" fn ASN1_TIME_new() -> *mut ASN1_TIME {
    Box::into_raw(Box::new(ASN1_TIME::new()))
}

/// ASN1_TIME_free
#[no_mangle]
pub extern "C" fn ASN1_TIME_free(t: *mut ASN1_TIME) {
    if !t.is_null() {
        unsafe {
            drop(Box::from_raw(t));
        }
    }
}

/// ASN1_INTEGER_new
#[no_mangle]
pub extern "C" fn ASN1_INTEGER_new() -> *mut ASN1_INTEGER {
    Box::into_raw(Box::new(ASN1_INTEGER::new()))
}

/// ASN1_INTEGER_free
#[no_mangle]
pub extern "C" fn ASN1_INTEGER_free(i: *mut ASN1_INTEGER) {
    if !i.is_null() {
        unsafe {
            drop(Box::from_raw(i));
        }
    }
}

/// ASN1_STRING_new
#[no_mangle]
pub extern "C" fn ASN1_STRING_new() -> *mut ASN1_STRING {
    Box::into_raw(Box::new(ASN1_STRING::new()))
}

/// ASN1_STRING_free
#[no_mangle]
pub extern "C" fn ASN1_STRING_free(s: *mut ASN1_STRING) {
    if !s.is_null() {
        unsafe {
            drop(Box::from_raw(s));
        }
    }
}

/// ASN1_STRING_length
#[no_mangle]
pub extern "C" fn ASN1_STRING_length(s: *const ASN1_STRING) -> i32 {
    if s.is_null() {
        return 0;
    }
    unsafe { (*s).data.len() as i32 }
}

/// ASN1_STRING_get0_data
#[no_mangle]
pub extern "C" fn ASN1_STRING_get0_data(s: *const ASN1_STRING) -> *const u8 {
    if s.is_null() {
        return core::ptr::null();
    }
    unsafe { (*s).data.as_ptr() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oid_roundtrip() {
        let oid = Oid::from_str("1.2.840.113549.1.1.1").unwrap();
        let s = oid.to_string();
        assert_eq!(s, "1.2.840.113549.1.1.1");
    }

    #[test]
    fn test_integer_encode_decode() {
        let value = vec![0x01, 0x23, 0x45];
        let encoded = encode_integer(&value);
        let decoded = parse_integer(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_sequence() {
        let inner = encode_integer(&[1]);
        let seq = encode_sequence(&inner);
        let parsed = parse_sequence(&seq).unwrap();
        assert_eq!(parsed, inner);
    }
}
