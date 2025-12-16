//! Crypto tests (from src/kmod/crypto.rs)

use crate::kmod::crypto::{sha256, Sha256};

#[test]
fn test_sha256_empty() {
    let hash = sha256(b"");
    let expected: [u8; 32] = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
        0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
        0x78, 0x52, 0xb8, 0x55,
    ];
    assert_eq!(hash, expected);
}

#[test]
fn test_sha256_hello() {
    let hash = sha256(b"hello");
    let expected: [u8; 32] = [
        0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e, 0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9,
        0xe2, 0x9e, 0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e, 0x73, 0x04, 0x33, 0x62,
        0x93, 0x8b, 0x98, 0x24,
    ];
    assert_eq!(hash, expected);
}

#[test]
fn test_sha256_incremental() {
    let mut hasher1 = Sha256::new();
    hasher1.update(b"hello world");
    let digest1 = hasher1.finalize();

    let mut hasher2 = Sha256::new();
    hasher2.update(b"hello ");
    hasher2.update(b"world");
    let digest2 = hasher2.finalize();

    assert_eq!(digest1, digest2);
}

#[test]
fn test_sha256_reset() {
    let mut hasher = Sha256::new();
    hasher.update(b"garbage");
    hasher.reset();
    let digest = hasher.finalize();

    // Should be same as empty
    let expected = sha256(b"");
    assert_eq!(digest, expected);
}
