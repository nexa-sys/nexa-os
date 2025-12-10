//! EC (Elliptic Curve) OpenSSL Compatibility API
//!
//! Provides OpenSSL-compatible EC_KEY, EC_POINT, and EC_GROUP functions
//! for P-256 (secp256r1/prime256v1) curve operations.

use crate::p256::{P256KeyPair, P256Point, P256Signature};
use std::vec::Vec;

// ============================================================================
// EC_GROUP - Elliptic Curve Group
// ============================================================================

/// NID constants for curve identification
pub mod nid {
    /// P-256 (secp256r1, prime256v1)
    pub const NID_X9_62_prime256v1: i32 = 415;
    /// P-384 (secp384r1)
    pub const NID_secp384r1: i32 = 715;
    /// P-521 (secp521r1)
    pub const NID_secp521r1: i32 = 716;
    /// X25519
    pub const NID_X25519: i32 = 1034;
    /// Ed25519
    pub const NID_ED25519: i32 = 1087;
}

/// EC_GROUP - Represents an elliptic curve
pub struct EC_GROUP {
    /// Curve NID
    nid: i32,
    /// Curve name
    name: &'static str,
    /// Field size in bits
    field_bits: usize,
}

impl EC_GROUP {
    /// Get curve NID
    pub fn get_curve_name(&self) -> i32 {
        self.nid
    }

    /// Get field size in bits
    pub fn get_degree(&self) -> usize {
        self.field_bits
    }
}

/// P-256 group singleton
static P256_GROUP: EC_GROUP = EC_GROUP {
    nid: nid::NID_X9_62_prime256v1,
    name: "P-256",
    field_bits: 256,
};

// ============================================================================
// EC_POINT - Point on elliptic curve
// ============================================================================

/// EC_POINT - Represents a point on an elliptic curve
pub struct EC_POINT {
    /// X coordinate (32 bytes for P-256)
    x: Vec<u8>,
    /// Y coordinate (32 bytes for P-256)
    y: Vec<u8>,
    /// Is this the point at infinity?
    is_infinity: bool,
    /// Associated group
    group_nid: i32,
}

impl EC_POINT {
    /// Create new point from coordinates
    pub fn new(group: &EC_GROUP) -> Self {
        Self {
            x: vec![0u8; 32],
            y: vec![0u8; 32],
            is_infinity: true,
            group_nid: group.nid,
        }
    }

    /// Create from P256Point
    pub fn from_p256_point(point: &P256Point) -> Self {
        let uncompressed = point.to_uncompressed();
        if uncompressed.len() == 65 && uncompressed[0] == 0x04 {
            Self {
                x: uncompressed[1..33].to_vec(),
                y: uncompressed[33..65].to_vec(),
                is_infinity: false,
                group_nid: nid::NID_X9_62_prime256v1,
            }
        } else {
            Self {
                x: vec![0u8; 32],
                y: vec![0u8; 32],
                is_infinity: true,
                group_nid: nid::NID_X9_62_prime256v1,
            }
        }
    }

    /// Convert to P256Point
    pub fn to_p256_point(&self) -> Option<P256Point> {
        if self.is_infinity {
            return Some(P256Point::infinity());
        }

        // Create uncompressed format
        let mut uncompressed = [0u8; 65];
        uncompressed[0] = 0x04;
        uncompressed[1..33].copy_from_slice(&self.x);
        uncompressed[33..65].copy_from_slice(&self.y);

        P256Point::from_uncompressed(&uncompressed)
    }

    /// Set from uncompressed format (04 || x || y)
    pub fn set_from_uncompressed(&mut self, data: &[u8]) -> bool {
        if data.len() != 65 || data[0] != 0x04 {
            return false;
        }

        self.x = data[1..33].to_vec();
        self.y = data[33..65].to_vec();
        self.is_infinity = false;
        true
    }

    /// Get as uncompressed format
    pub fn to_uncompressed(&self) -> Vec<u8> {
        if self.is_infinity {
            return vec![0x00]; // Point at infinity
        }

        let mut result = Vec::with_capacity(65);
        result.push(0x04);
        result.extend_from_slice(&self.x);
        result.extend_from_slice(&self.y);
        result
    }
}

// ============================================================================
// EC_KEY - EC Key Pair
// ============================================================================

/// EC_KEY - Represents an EC key pair
pub struct EC_KEY {
    /// Private key (32 bytes for P-256)
    private_key: Option<Vec<u8>>,
    /// Public key point
    public_key: Option<EC_POINT>,
    /// Associated group
    group: *const EC_GROUP,
}

impl EC_KEY {
    /// Create new empty key
    pub fn new() -> Self {
        Self {
            private_key: None,
            public_key: None,
            group: core::ptr::null(),
        }
    }

    /// Generate new key pair
    pub fn generate(group: &EC_GROUP) -> Option<Self> {
        if group.nid != nid::NID_X9_62_prime256v1 {
            return None; // Only P-256 supported currently
        }

        let keypair = P256KeyPair::generate()?;

        Some(Self {
            private_key: Some(keypair.private_key.to_vec()),
            public_key: Some(EC_POINT::from_p256_point(&keypair.public_key)),
            group,
        })
    }

    /// Set private key
    pub fn set_private_key(&mut self, key: &[u8]) -> bool {
        if key.len() != 32 {
            return false;
        }

        let mut priv_arr = [0u8; 32];
        priv_arr.copy_from_slice(key);

        // Derive public key
        if let Some(keypair) = P256KeyPair::from_private_key(&priv_arr) {
            self.private_key = Some(key.to_vec());
            self.public_key = Some(EC_POINT::from_p256_point(&keypair.public_key));
            true
        } else {
            false
        }
    }

    /// Get private key
    pub fn get_private_key(&self) -> Option<&[u8]> {
        self.private_key.as_deref()
    }

    /// Get public key point
    pub fn get_public_key(&self) -> Option<&EC_POINT> {
        self.public_key.as_ref()
    }

    /// Sign data
    pub fn sign(&self, hash: &[u8]) -> Option<Vec<u8>> {
        let priv_key = self.private_key.as_ref()?;
        if priv_key.len() != 32 {
            return None;
        }

        let sig = P256Signature::sign(priv_key, hash)?;

        Some(sig.to_der())
    }

    /// Verify signature
    pub fn verify(&self, hash: &[u8], signature: &[u8]) -> bool {
        let public_point = match self.public_key.as_ref() {
            Some(p) => match p.to_p256_point() {
                Some(pt) => pt,
                None => return false,
            },
            None => return false,
        };

        // Parse DER signature
        let sig = match P256Signature::from_der(signature) {
            Some(s) => s,
            None => return false,
        };

        sig.verify(&public_point, hash)
    }
}

impl Default for EC_KEY {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// ECDSA Signature Operations
// ============================================================================

/// ECDSA_SIG - ECDSA signature
pub struct ECDSA_SIG {
    /// r component
    r: Vec<u8>,
    /// s component
    s: Vec<u8>,
}

impl ECDSA_SIG {
    /// Create from r and s components
    pub fn new(r: &[u8], s: &[u8]) -> Self {
        Self {
            r: r.to_vec(),
            s: s.to_vec(),
        }
    }

    /// Create from P256Signature
    pub fn from_p256_sig(sig: &P256Signature) -> Self {
        Self {
            r: sig.r.to_vec(),
            s: sig.s.to_vec(),
        }
    }

    /// Get r component
    pub fn get_r(&self) -> &[u8] {
        &self.r
    }

    /// Get s component
    pub fn get_s(&self) -> &[u8] {
        &self.s
    }

    /// Convert to DER format
    pub fn to_der(&self) -> Vec<u8> {
        // ECDSA-Sig-Value ::= SEQUENCE {
        //     r INTEGER,
        //     s INTEGER
        // }

        let r_der = integer_to_der(&self.r);
        let s_der = integer_to_der(&self.s);

        let inner_len = r_der.len() + s_der.len();
        let mut result = Vec::with_capacity(2 + inner_len);

        // SEQUENCE tag
        result.push(0x30);
        if inner_len < 128 {
            result.push(inner_len as u8);
        } else {
            result.push(0x81);
            result.push(inner_len as u8);
        }

        result.extend_from_slice(&r_der);
        result.extend_from_slice(&s_der);

        result
    }

    /// Parse from DER format
    pub fn from_der(der: &[u8]) -> Option<Self> {
        if der.len() < 6 || der[0] != 0x30 {
            return None;
        }

        let (len, offset) = if der[1] < 0x80 {
            (der[1] as usize, 2)
        } else if der[1] == 0x81 {
            (der[2] as usize, 3)
        } else {
            return None;
        };

        if der.len() < offset + len {
            return None;
        }

        let mut pos = offset;

        // Parse r
        if der[pos] != 0x02 {
            return None;
        }
        pos += 1;
        let r_len = der[pos] as usize;
        pos += 1;
        let r = der[pos..pos + r_len].to_vec();
        pos += r_len;

        // Parse s
        if der[pos] != 0x02 {
            return None;
        }
        pos += 1;
        let s_len = der[pos] as usize;
        pos += 1;
        let s = der[pos..pos + s_len].to_vec();

        Some(Self { r, s })
    }
}

/// Convert integer bytes to DER INTEGER encoding
fn integer_to_der(data: &[u8]) -> Vec<u8> {
    // Skip leading zeros
    let mut start = 0;
    while start < data.len() && data[start] == 0 {
        start += 1;
    }

    let significant = if start == data.len() {
        &[0u8][..]
    } else {
        &data[start..]
    };

    // Add leading zero if high bit is set (to keep positive)
    let needs_zero = significant[0] & 0x80 != 0;

    let len = significant.len() + if needs_zero { 1 } else { 0 };
    let mut result = Vec::with_capacity(2 + len);

    result.push(0x02); // INTEGER tag
    result.push(len as u8);

    if needs_zero {
        result.push(0x00);
    }
    result.extend_from_slice(significant);

    result
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// EC_GROUP_new_by_curve_name
#[no_mangle]
pub extern "C" fn EC_GROUP_new_by_curve_name(nid: i32) -> *mut EC_GROUP {
    match nid {
        nid::NID_X9_62_prime256v1 => {
            let group = Box::new(EC_GROUP {
                nid,
                name: "P-256",
                field_bits: 256,
            });
            Box::into_raw(group)
        }
        _ => core::ptr::null_mut(),
    }
}

/// EC_GROUP_free
#[no_mangle]
pub extern "C" fn EC_GROUP_free(group: *mut EC_GROUP) {
    if !group.is_null() {
        unsafe {
            drop(Box::from_raw(group));
        }
    }
}

/// EC_GROUP_get_curve_name
#[no_mangle]
pub extern "C" fn EC_GROUP_get_curve_name(group: *const EC_GROUP) -> i32 {
    if group.is_null() {
        return 0;
    }
    unsafe { (*group).nid }
}

/// EC_GROUP_get_degree
#[no_mangle]
pub extern "C" fn EC_GROUP_get_degree(group: *const EC_GROUP) -> i32 {
    if group.is_null() {
        return 0;
    }
    unsafe { (*group).field_bits as i32 }
}

/// EC_KEY_new
#[no_mangle]
pub extern "C" fn EC_KEY_new() -> *mut EC_KEY {
    let key = Box::new(EC_KEY::new());
    Box::into_raw(key)
}

/// EC_KEY_free
#[no_mangle]
pub extern "C" fn EC_KEY_free(key: *mut EC_KEY) {
    if !key.is_null() {
        unsafe {
            drop(Box::from_raw(key));
        }
    }
}

/// EC_KEY_new_by_curve_name
#[no_mangle]
pub extern "C" fn EC_KEY_new_by_curve_name(nid: i32) -> *mut EC_KEY {
    if nid != nid::NID_X9_62_prime256v1 {
        return core::ptr::null_mut();
    }

    let mut key = Box::new(EC_KEY::new());
    key.group = &P256_GROUP;
    Box::into_raw(key)
}

/// EC_KEY_generate_key
#[no_mangle]
pub extern "C" fn EC_KEY_generate_key(key: *mut EC_KEY) -> i32 {
    if key.is_null() {
        return 0;
    }

    let keypair = match P256KeyPair::generate() {
        Some(kp) => kp,
        None => return 0,
    };

    unsafe {
        (*key).private_key = Some(keypair.private_key.to_vec());
        (*key).public_key = Some(EC_POINT::from_p256_point(&keypair.public_key));
    }

    1
}

/// EC_KEY_get0_group
#[no_mangle]
pub extern "C" fn EC_KEY_get0_group(key: *const EC_KEY) -> *const EC_GROUP {
    if key.is_null() {
        return core::ptr::null();
    }
    unsafe { (*key).group }
}

/// EC_KEY_set_group
#[no_mangle]
pub extern "C" fn EC_KEY_set_group(key: *mut EC_KEY, group: *const EC_GROUP) -> i32 {
    if key.is_null() || group.is_null() {
        return 0;
    }
    unsafe {
        (*key).group = group;
    }
    1
}

/// EC_KEY_get0_public_key
#[no_mangle]
pub extern "C" fn EC_KEY_get0_public_key(key: *const EC_KEY) -> *const EC_POINT {
    if key.is_null() {
        return core::ptr::null();
    }
    unsafe {
        match &(*key).public_key {
            Some(pk) => pk,
            None => core::ptr::null(),
        }
    }
}

/// EC_POINT_new
#[no_mangle]
pub extern "C" fn EC_POINT_new(group: *const EC_GROUP) -> *mut EC_POINT {
    if group.is_null() {
        return core::ptr::null_mut();
    }
    let group = unsafe { &*group };
    let point = Box::new(EC_POINT::new(group));
    Box::into_raw(point)
}

/// EC_POINT_free
#[no_mangle]
pub extern "C" fn EC_POINT_free(point: *mut EC_POINT) {
    if !point.is_null() {
        unsafe {
            drop(Box::from_raw(point));
        }
    }
}

/// EC_POINT_point2oct - Convert point to octet string
#[no_mangle]
pub extern "C" fn EC_POINT_point2oct(
    _group: *const EC_GROUP,
    point: *const EC_POINT,
    form: i32, // POINT_CONVERSION_FORM
    buf: *mut u8,
    len: usize,
    _ctx: *mut core::ffi::c_void,
) -> usize {
    if point.is_null() {
        return 0;
    }

    let point = unsafe { &*point };

    // form: 4 = uncompressed
    if form != 4 {
        return 0; // Only uncompressed supported
    }

    let data = point.to_uncompressed();

    if buf.is_null() {
        return data.len();
    }

    if len < data.len() {
        return 0;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len());
    }

    data.len()
}

/// EC_POINT_oct2point - Convert octet string to point
#[no_mangle]
pub extern "C" fn EC_POINT_oct2point(
    _group: *const EC_GROUP,
    point: *mut EC_POINT,
    buf: *const u8,
    len: usize,
    _ctx: *mut core::ffi::c_void,
) -> i32 {
    if point.is_null() || buf.is_null() || len < 1 {
        return 0;
    }

    let data = unsafe { core::slice::from_raw_parts(buf, len) };
    let point = unsafe { &mut *point };

    if point.set_from_uncompressed(data) {
        1
    } else {
        0
    }
}

// Note: ECDSA_sign, ECDSA_verify, and ECDSA_size are defined in ecdsa.rs
// The EC_KEY-based sign/verify methods above can be used internally

/// ecdsa_size_p256 - Return maximum signature size for P-256 (internal helper)
pub fn ecdsa_size_p256(_eckey: *const EC_KEY) -> i32 {
    // P-256 signature: 2 + 2 + 33 + 2 + 33 = 72 bytes max
    72
}

/// ECDSA_SIG_new
#[no_mangle]
pub extern "C" fn ECDSA_SIG_new() -> *mut ECDSA_SIG {
    let sig = Box::new(ECDSA_SIG {
        r: Vec::new(),
        s: Vec::new(),
    });
    Box::into_raw(sig)
}

/// ECDSA_SIG_free
#[no_mangle]
pub extern "C" fn ECDSA_SIG_free(sig: *mut ECDSA_SIG) {
    if !sig.is_null() {
        unsafe {
            drop(Box::from_raw(sig));
        }
    }
}

/// d2i_ECDSA_SIG - Parse DER signature
#[no_mangle]
pub extern "C" fn d2i_ECDSA_SIG(
    sig: *mut *mut ECDSA_SIG,
    pp: *mut *const u8,
    len: i64,
) -> *mut ECDSA_SIG {
    if pp.is_null() {
        return core::ptr::null_mut();
    }

    let data = unsafe { core::slice::from_raw_parts(*pp, len as usize) };

    match ECDSA_SIG::from_der(data) {
        Some(parsed) => {
            let boxed = Box::new(parsed);
            let ptr = Box::into_raw(boxed);

            if !sig.is_null() {
                unsafe {
                    *sig = ptr;
                }
            }

            ptr
        }
        None => core::ptr::null_mut(),
    }
}

/// i2d_ECDSA_SIG - Encode signature to DER
#[no_mangle]
pub extern "C" fn i2d_ECDSA_SIG(sig: *const ECDSA_SIG, pp: *mut *mut u8) -> i32 {
    if sig.is_null() {
        return -1;
    }

    let sig = unsafe { &*sig };
    let der = sig.to_der();
    let len = der.len();

    if !pp.is_null() && !unsafe { (*pp).is_null() } {
        unsafe {
            core::ptr::copy_nonoverlapping(der.as_ptr(), *pp, len);
            *pp = (*pp).add(len);
        }
    }

    len as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ec_key_generate() {
        let key = EC_KEY::generate(&P256_GROUP).unwrap();
        assert!(key.private_key.is_some());
        assert!(key.public_key.is_some());
    }

    #[test]
    fn test_ecdsa_sign_verify() {
        let key = EC_KEY::generate(&P256_GROUP).unwrap();
        let hash = [0x12u8; 32];

        let signature = key.sign(&hash).unwrap();
        assert!(key.verify(&hash, &signature));

        // Verify with wrong hash fails
        let wrong_hash = [0x34u8; 32];
        assert!(!key.verify(&wrong_hash, &signature));
    }

    #[test]
    fn test_ecdsa_sig_der() {
        let sig = ECDSA_SIG::new(&[0x12; 32], &[0x34; 32]);
        let der = sig.to_der();

        let parsed = ECDSA_SIG::from_der(&der).unwrap();
        assert_eq!(parsed.r.len(), 32);
        assert_eq!(parsed.s.len(), 32);
    }
}
