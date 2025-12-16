//! PKCS7 tests (from src/kmod/pkcs7.rs)

use crate::kmod::pkcs7::{ModuleSigInfo, verify_module_signature, SignatureVerifyResult};
use crate::kmod::crypto::HashAlgorithm;

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
