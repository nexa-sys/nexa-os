//! OpenSSL OBJ_* Compatibility Functions
//!
//! Object identifier (OID) management compatible with OpenSSL API.

use std::collections::HashMap;
use std::sync::Mutex;

/// ASN1_OBJECT structure for OpenSSL compatibility
#[repr(C)]
pub struct ASN1_OBJECT {
    /// Short name (e.g., "sha256")
    pub sn: *const i8,
    /// Long name (e.g., "sha256WithRSAEncryption")
    pub ln: *const i8,
    /// Numeric ID (NID)
    pub nid: i32,
    /// DER-encoded OID length
    pub length: i32,
    /// DER-encoded OID data
    pub data: *const u8,
    /// Flags
    pub flags: i32,
}

/// Known NIDs (Numeric IDs)
pub mod nid {
    pub const NID_undef: i32 = 0;
    
    // Digests
    pub const NID_md5: i32 = 4;
    pub const NID_sha1: i32 = 64;
    pub const NID_sha224: i32 = 675;
    pub const NID_sha256: i32 = 672;
    pub const NID_sha384: i32 = 673;
    pub const NID_sha512: i32 = 674;
    pub const NID_sha3_224: i32 = 1096;
    pub const NID_sha3_256: i32 = 1097;
    pub const NID_sha3_384: i32 = 1098;
    pub const NID_sha3_512: i32 = 1099;
    
    // Ciphers
    pub const NID_aes_128_cbc: i32 = 419;
    pub const NID_aes_192_cbc: i32 = 423;
    pub const NID_aes_256_cbc: i32 = 427;
    pub const NID_aes_128_gcm: i32 = 895;
    pub const NID_aes_192_gcm: i32 = 898;
    pub const NID_aes_256_gcm: i32 = 901;
    pub const NID_aes_128_ccm: i32 = 896;
    pub const NID_aes_256_ccm: i32 = 902;
    pub const NID_chacha20: i32 = 1019;
    pub const NID_chacha20_poly1305: i32 = 1018;
    
    // RSA/DSA/EC
    pub const NID_rsaEncryption: i32 = 6;
    pub const NID_sha256WithRSAEncryption: i32 = 668;
    pub const NID_sha384WithRSAEncryption: i32 = 669;
    pub const NID_sha512WithRSAEncryption: i32 = 670;
    pub const NID_dsa: i32 = 116;
    pub const NID_X9_62_id_ecPublicKey: i32 = 408;
    pub const NID_ED25519: i32 = 1087;
    pub const NID_ED448: i32 = 1088;
    pub const NID_X25519: i32 = 1034;
    pub const NID_X448: i32 = 1035;
    
    // EC curves
    pub const NID_X9_62_prime256v1: i32 = 415; // P-256
    pub const NID_secp384r1: i32 = 715;        // P-384
    pub const NID_secp521r1: i32 = 716;        // P-521
    pub const NID_secp256k1: i32 = 714;        // Bitcoin curve
    
    // X.509
    pub const NID_commonName: i32 = 13;
    pub const NID_countryName: i32 = 14;
    pub const NID_localityName: i32 = 15;
    pub const NID_stateOrProvinceName: i32 = 16;
    pub const NID_organizationName: i32 = 17;
    pub const NID_organizationalUnitName: i32 = 18;
    
    // Key usage
    pub const NID_basic_constraints: i32 = 87;
    pub const NID_key_usage: i32 = 83;
    pub const NID_ext_key_usage: i32 = 126;
    pub const NID_subject_alt_name: i32 = 85;
    pub const NID_subject_key_identifier: i32 = 82;
    pub const NID_authority_key_identifier: i32 = 90;
}

/// OID data for common objects
mod oid_data {
    // SHA-256: 2.16.840.1.101.3.4.2.1
    pub static SHA256_OID: [u8; 9] = [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
    
    // SHA-384: 2.16.840.1.101.3.4.2.2
    pub static SHA384_OID: [u8; 9] = [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02];
    
    // SHA-512: 2.16.840.1.101.3.4.2.3
    pub static SHA512_OID: [u8; 9] = [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03];
    
    // RSA encryption: 1.2.840.113549.1.1.1
    pub static RSA_OID: [u8; 9] = [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
    
    // EC public key: 1.2.840.10045.2.1
    pub static EC_OID: [u8; 7] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
    
    // P-256: 1.2.840.10045.3.1.7
    pub static P256_OID: [u8; 8] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
    
    // P-384: 1.3.132.0.34
    pub static P384_OID: [u8; 5] = [0x2B, 0x81, 0x04, 0x00, 0x22];
    
    // P-521: 1.3.132.0.35
    pub static P521_OID: [u8; 5] = [0x2B, 0x81, 0x04, 0x00, 0x23];
    
    // Ed25519: 1.3.101.112
    pub static ED25519_OID: [u8; 3] = [0x2B, 0x65, 0x70];
    
    // X25519: 1.3.101.110
    pub static X25519_OID: [u8; 3] = [0x2B, 0x65, 0x6E];
    
    // Common Name: 2.5.4.3
    pub static CN_OID: [u8; 3] = [0x55, 0x04, 0x03];
    
    // Organization: 2.5.4.10
    pub static O_OID: [u8; 3] = [0x55, 0x04, 0x0A];
}

/// Static object storage
struct ObjEntry {
    sn: &'static str,
    ln: &'static str,
    nid: i32,
    oid: &'static [u8],
}

static OBJECTS: &[ObjEntry] = &[
    ObjEntry { sn: "SHA256", ln: "sha256", nid: nid::NID_sha256, oid: &oid_data::SHA256_OID },
    ObjEntry { sn: "SHA384", ln: "sha384", nid: nid::NID_sha384, oid: &oid_data::SHA384_OID },
    ObjEntry { sn: "SHA512", ln: "sha512", nid: nid::NID_sha512, oid: &oid_data::SHA512_OID },
    ObjEntry { sn: "rsaEncryption", ln: "rsaEncryption", nid: nid::NID_rsaEncryption, oid: &oid_data::RSA_OID },
    ObjEntry { sn: "id-ecPublicKey", ln: "id-ecPublicKey", nid: nid::NID_X9_62_id_ecPublicKey, oid: &oid_data::EC_OID },
    ObjEntry { sn: "prime256v1", ln: "prime256v1", nid: nid::NID_X9_62_prime256v1, oid: &oid_data::P256_OID },
    ObjEntry { sn: "secp384r1", ln: "secp384r1", nid: nid::NID_secp384r1, oid: &oid_data::P384_OID },
    ObjEntry { sn: "secp521r1", ln: "secp521r1", nid: nid::NID_secp521r1, oid: &oid_data::P521_OID },
    ObjEntry { sn: "ED25519", ln: "ED25519", nid: nid::NID_ED25519, oid: &oid_data::ED25519_OID },
    ObjEntry { sn: "X25519", ln: "X25519", nid: nid::NID_X25519, oid: &oid_data::X25519_OID },
    ObjEntry { sn: "CN", ln: "commonName", nid: nid::NID_commonName, oid: &oid_data::CN_OID },
    ObjEntry { sn: "O", ln: "organizationName", nid: nid::NID_organizationName, oid: &oid_data::O_OID },
];

// ============================================================================
// C ABI Exports
// ============================================================================

/// OBJ_nid2sn - Get short name from NID
#[no_mangle]
pub extern "C" fn OBJ_nid2sn(nid: i32) -> *const i8 {
    for obj in OBJECTS {
        if obj.nid == nid {
            return obj.sn.as_ptr() as *const i8;
        }
    }
    core::ptr::null()
}

/// OBJ_nid2ln - Get long name from NID
#[no_mangle]
pub extern "C" fn OBJ_nid2ln(nid: i32) -> *const i8 {
    for obj in OBJECTS {
        if obj.nid == nid {
            return obj.ln.as_ptr() as *const i8;
        }
    }
    core::ptr::null()
}

/// OBJ_nid2obj - Get ASN1_OBJECT from NID
#[no_mangle]
pub extern "C" fn OBJ_nid2obj(nid: i32) -> *mut ASN1_OBJECT {
    for obj in OBJECTS {
        if obj.nid == nid {
            let asn1_obj = Box::new(ASN1_OBJECT {
                sn: obj.sn.as_ptr() as *const i8,
                ln: obj.ln.as_ptr() as *const i8,
                nid: obj.nid,
                length: obj.oid.len() as i32,
                data: obj.oid.as_ptr(),
                flags: 0,
            });
            return Box::into_raw(asn1_obj);
        }
    }
    core::ptr::null_mut()
}

/// OBJ_obj2nid - Get NID from ASN1_OBJECT
#[no_mangle]
pub extern "C" fn OBJ_obj2nid(obj: *const ASN1_OBJECT) -> i32 {
    if obj.is_null() {
        return nid::NID_undef;
    }
    
    let obj = unsafe { &*obj };
    obj.nid
}

/// OBJ_txt2nid - Get NID from text name
#[no_mangle]
pub extern "C" fn OBJ_txt2nid(s: *const i8) -> i32 {
    if s.is_null() {
        return nid::NID_undef;
    }
    
    let c_str = unsafe { core::ffi::CStr::from_ptr(s) };
    let name = match c_str.to_str() {
        Ok(n) => n,
        Err(_) => return nid::NID_undef,
    };
    
    // Search by short name or long name
    for obj in OBJECTS {
        if obj.sn.eq_ignore_ascii_case(name) || obj.ln.eq_ignore_ascii_case(name) {
            return obj.nid;
        }
    }
    
    // Try specific aliases
    match name.to_lowercase().as_str() {
        "sha-256" | "sha_256" => nid::NID_sha256,
        "sha-384" | "sha_384" => nid::NID_sha384,
        "sha-512" | "sha_512" => nid::NID_sha512,
        "p-256" | "p256" => nid::NID_X9_62_prime256v1,
        "p-384" | "p384" => nid::NID_secp384r1,
        "p-521" | "p521" => nid::NID_secp521r1,
        _ => nid::NID_undef,
    }
}

/// OBJ_txt2obj - Get ASN1_OBJECT from text
#[no_mangle]
pub extern "C" fn OBJ_txt2obj(s: *const i8, no_name: i32) -> *mut ASN1_OBJECT {
    let nid = OBJ_txt2nid(s);
    if nid != nid::NID_undef || no_name == 0 {
        return OBJ_nid2obj(nid);
    }
    
    // TODO: Parse dotted OID string like "1.2.3.4"
    core::ptr::null_mut()
}

/// OBJ_obj2txt - Convert ASN1_OBJECT to text
#[no_mangle]
pub extern "C" fn OBJ_obj2txt(
    buf: *mut i8,
    buf_len: i32,
    obj: *const ASN1_OBJECT,
    no_name: i32,
) -> i32 {
    if obj.is_null() {
        return 0;
    }
    
    let obj = unsafe { &*obj };
    
    // Get name if allowed
    let name = if no_name == 0 && !obj.ln.is_null() {
        unsafe { core::ffi::CStr::from_ptr(obj.ln) }
            .to_str()
            .ok()
            .map(|s| s.to_string())
    } else {
        None
    };
    
    let text = name.unwrap_or_else(|| {
        // Convert OID to dotted notation
        // Simplified: just return unknown
        "unknown".to_string()
    });
    
    let text_len = text.len() as i32;
    
    if !buf.is_null() && buf_len > 0 {
        let copy_len = text_len.min(buf_len - 1) as usize;
        unsafe {
            core::ptr::copy_nonoverlapping(text.as_ptr(), buf as *mut u8, copy_len);
            *(buf as *mut u8).add(copy_len) = 0;
        }
    }
    
    text_len
}

/// OBJ_sn2nid - Get NID from short name
#[no_mangle]
pub extern "C" fn OBJ_sn2nid(s: *const i8) -> i32 {
    OBJ_txt2nid(s)
}

/// OBJ_ln2nid - Get NID from long name
#[no_mangle]
pub extern "C" fn OBJ_ln2nid(s: *const i8) -> i32 {
    OBJ_txt2nid(s)
}

/// OBJ_cmp - Compare two ASN1_OBJECTs
#[no_mangle]
pub extern "C" fn OBJ_cmp(a: *const ASN1_OBJECT, b: *const ASN1_OBJECT) -> i32 {
    if a.is_null() && b.is_null() {
        return 0;
    }
    if a.is_null() {
        return -1;
    }
    if b.is_null() {
        return 1;
    }
    
    let a = unsafe { &*a };
    let b = unsafe { &*b };
    
    if a.nid == b.nid {
        0
    } else if a.nid < b.nid {
        -1
    } else {
        1
    }
}

/// OBJ_dup - Duplicate ASN1_OBJECT
#[no_mangle]
pub extern "C" fn OBJ_dup(obj: *const ASN1_OBJECT) -> *mut ASN1_OBJECT {
    if obj.is_null() {
        return core::ptr::null_mut();
    }
    
    let obj = unsafe { &*obj };
    let new_obj = Box::new(ASN1_OBJECT {
        sn: obj.sn,
        ln: obj.ln,
        nid: obj.nid,
        length: obj.length,
        data: obj.data,
        flags: obj.flags,
    });
    
    Box::into_raw(new_obj)
}

/// ASN1_OBJECT_free - Free ASN1_OBJECT
#[no_mangle]
pub extern "C" fn ASN1_OBJECT_free(obj: *mut ASN1_OBJECT) {
    if !obj.is_null() {
        unsafe { let _ = Box::from_raw(obj); }
    }
}

/// OBJ_create - Create custom OID
#[no_mangle]
pub extern "C" fn OBJ_create(
    _oid: *const i8,
    _sn: *const i8,
    _ln: *const i8,
) -> i32 {
    // Custom OID creation not supported
    nid::NID_undef
}

/// OBJ_cleanup - Cleanup OID table
#[no_mangle]
pub extern "C" fn OBJ_cleanup() {
    // No-op: static data
}

/// OBJ_NAME_* functions for algorithm lookup

/// OBJ_NAME_get - Get object name
#[no_mangle]
pub extern "C" fn OBJ_NAME_get(
    name: *const i8,
    type_: i32,
) -> *const i8 {
    if name.is_null() {
        return core::ptr::null();
    }
    // Just return the input name
    name
}

/// OBJ_NAME_add - Add object name
#[no_mangle]
pub extern "C" fn OBJ_NAME_add(
    _name: *const i8,
    _type_: i32,
    _data: *const i8,
) -> i32 {
    1 // Success (no-op)
}

/// OBJ_NAME_remove - Remove object name
#[no_mangle]
pub extern "C" fn OBJ_NAME_remove(
    _name: *const i8,
    _type_: i32,
) -> i32 {
    1 // Success (no-op)
}

/// OBJ_NAME_cleanup - Cleanup name table
#[no_mangle]
pub extern "C" fn OBJ_NAME_cleanup(_type_: i32) {
    // No-op
}

/// OBJ_NAME types
pub mod name_type {
    pub const OBJ_NAME_TYPE_UNDEF: i32 = 0x00;
    pub const OBJ_NAME_TYPE_MD_METH: i32 = 0x01;
    pub const OBJ_NAME_TYPE_CIPHER_METH: i32 = 0x02;
    pub const OBJ_NAME_TYPE_PKEY_METH: i32 = 0x03;
    pub const OBJ_NAME_TYPE_COMP_METH: i32 = 0x04;
    pub const OBJ_NAME_TYPE_NUM: i32 = 0x05;
}

/// OBJ_find_sigid_algs - Find signature algorithm components
#[no_mangle]
pub extern "C" fn OBJ_find_sigid_algs(
    sig_nid: i32,
    pdig_nid: *mut i32,
    ppkey_nid: *mut i32,
) -> i32 {
    match sig_nid {
        nid::NID_sha256WithRSAEncryption => {
            if !pdig_nid.is_null() {
                unsafe { *pdig_nid = nid::NID_sha256; }
            }
            if !ppkey_nid.is_null() {
                unsafe { *ppkey_nid = nid::NID_rsaEncryption; }
            }
            1
        }
        nid::NID_sha384WithRSAEncryption => {
            if !pdig_nid.is_null() {
                unsafe { *pdig_nid = nid::NID_sha384; }
            }
            if !ppkey_nid.is_null() {
                unsafe { *ppkey_nid = nid::NID_rsaEncryption; }
            }
            1
        }
        nid::NID_sha512WithRSAEncryption => {
            if !pdig_nid.is_null() {
                unsafe { *pdig_nid = nid::NID_sha512; }
            }
            if !ppkey_nid.is_null() {
                unsafe { *ppkey_nid = nid::NID_rsaEncryption; }
            }
            1
        }
        _ => 0,
    }
}

/// OBJ_find_sigid_by_algs - Find signature NID from components
#[no_mangle]
pub extern "C" fn OBJ_find_sigid_by_algs(
    psig_nid: *mut i32,
    dig_nid: i32,
    pkey_nid: i32,
) -> i32 {
    let sig_nid = match (dig_nid, pkey_nid) {
        (nid::NID_sha256, nid::NID_rsaEncryption) => nid::NID_sha256WithRSAEncryption,
        (nid::NID_sha384, nid::NID_rsaEncryption) => nid::NID_sha384WithRSAEncryption,
        (nid::NID_sha512, nid::NID_rsaEncryption) => nid::NID_sha512WithRSAEncryption,
        _ => return 0,
    };
    
    if !psig_nid.is_null() {
        unsafe { *psig_nid = sig_nid; }
    }
    1
}
