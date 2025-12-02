# NexaOS Kernel Module Signing

## Overview

NexaOS supports PKCS#7/CMS cryptographic signatures for kernel modules (.nkm files), 
providing security guarantees similar to the Linux kernel's module signing infrastructure.

## Signature Format

NexaOS uses a signature format compatible with Linux kernel modules:

```
[Module ELF/NKM data]
[PKCS#7 SignedData (DER encoded)]
[12-byte signature info structure]
[12-byte magic: "~Module sig~"]
```

### Supported Algorithms

| Algorithm | Support |
|-----------|---------|
| SHA-256 | ✓ Full |
| SHA-384 | ✓ Parsing only |
| SHA-512 | ✓ Parsing only |
| RSA-2048 | ✓ Full |
| RSA-4096 | ✓ Full |
| ECDSA | ✗ Planned |

## Quick Start

### 1. Generate Signing Key

```bash
./scripts/sign-module.sh --generate-key
```

This creates:
- `certs/signing_key.pem` - Private key (keep secure!)
- `certs/signing_key.x509` - X.509 certificate
- `certs/signing_key.der` - DER-encoded certificate
- `certs/embedded_key.rs` - Rust code for kernel embedding

### 2. Sign a Module

```bash
# Sign and create .signed file
./scripts/sign-module.sh build/modules/ext2.nkm

# Sign in place
./scripts/sign-module.sh -i build/modules/ext2.nkm
```

### 3. Verify a Signed Module

```bash
./scripts/sign-module.sh --verify build/modules/ext2.nkm
```

## Integration with Build System

Add to `scripts/build-modules.sh`:

```bash
# Sign all modules after building
for nkm in "$MODULES_DIR"/*.nkm; do
    if [ -f "$PROJECT_ROOT/certs/signing_key.pem" ]; then
        "$SCRIPT_DIR/sign-module.sh" -i "$nkm"
    fi
done
```

## Kernel Configuration

### Embedding Trusted Keys

To embed the signing key in the kernel:

1. Generate the key: `./scripts/sign-module.sh --generate-key`
2. Include the generated Rust file in your kernel build:

```rust
// In src/kmod/embedded_keys.rs
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/certs/embedded_key.rs"));
```

3. Call during kernel init:
```rust
// In kmod::init()
embedded_keys::load_embedded_key();
```

### Runtime Key Loading

Keys can also be loaded at runtime via:

```rust
use crate::kmod::crypto;

// Load a key from raw bytes
crypto::add_trusted_key(
    key_id,      // Key identifier (e.g., fingerprint)
    modulus,     // RSA modulus (big-endian bytes)
    exponent,    // RSA exponent (big-endian bytes)
);
```

## Security Levels

### Strict Mode (Recommended for Production)

Only signed modules from trusted keyring are loaded:

```rust
// In ModuleLoadOptions
pub struct ModuleLoadOptions {
    pub require_signature: bool,  // Set to true
    pub allow_unsigned: false,
    // ...
}
```

### Permissive Mode (Development)

Unsigned modules can be loaded but taint the kernel:

```rust
// Default behavior - unsigned modules set TaintFlag::UnsignedModule
```

## Signature Info Structure

```c
struct module_sig_info {
    uint8_t  algo;           // 0 = unspecified
    uint8_t  hash;           // 4 = SHA-256, 5 = SHA-384, 6 = SHA-512
    uint8_t  key_type;       // 1 = RSA
    uint8_t  signer_id_type; // 1 = PKCS#7 issuer+serial
    uint8_t  reserved[4];
    uint32_t sig_len;        // Big-endian signature length
};
```

## PKCS#7 Structure

The signature uses PKCS#7 SignedData (RFC 2315) / CMS (RFC 5652):

```asn1
SignedData ::= SEQUENCE {
    version          INTEGER,
    digestAlgorithms SET OF DigestAlgorithmIdentifier,
    encapContentInfo ContentInfo,
    certificates     [0] IMPLICIT CertificateSet OPTIONAL,
    crls             [1] IMPLICIT RevocationInfoChoices OPTIONAL,
    signerInfos      SET OF SignerInfo
}
```

## API Reference

### Verification Functions

```rust
// Verify module signature
pub fn verify_module_signature(data: &[u8]) -> SignatureVerifyResult;

// Extract signature components
pub fn extract_module_signature(data: &[u8]) 
    -> Option<(&[u8], &[u8], ModuleSigInfo)>;

// Parse PKCS#7 structure
pub fn parse_pkcs7_signed_data(data: &[u8]) 
    -> Option<Pkcs7SignedData<'_>>;
```

### Key Management

```rust
// Add trusted key
pub fn add_trusted_key(id: &[u8], n: &[u8], e: &[u8]) -> bool;

// Find trusted key
pub fn find_trusted_key(id: &[u8]) -> Option<RsaPublicKey>;

// Get key count
pub fn trusted_key_count() -> usize;

// Clear all keys
pub fn clear_trusted_keys();
```

### Signature Status

```rust
pub enum SignatureVerifyResult {
    Valid,              // Signature verified successfully
    Unsigned,           // Module has no signature
    InvalidFormat,      // Signature format error
    ParseError,         // PKCS#7 parsing failed
    NoSignerInfo,       // No signer information
    KeyNotFound,        // Signing key not in trusted keyring
    HashMismatch,       // Content hash doesn't match
    VerifyFailed,       // RSA verification failed
    UnsupportedAlgorithm,
}
```

## Troubleshooting

### Module fails to load: "key not found"

The signing certificate is not in the trusted keyring. Either:
1. Embed the key at build time
2. Load the key before loading modules

### Module loads but taints kernel

Check `dmesg` output:
- `E` flag: Unsigned module loaded
- `P` flag: Proprietary license

### Signature verification fails

1. Ensure module wasn't modified after signing
2. Verify certificate hasn't expired
3. Check algorithm compatibility

## Security Considerations

1. **Protect private keys**: Store signing keys securely, never commit to version control
2. **Use strong keys**: Minimum RSA-2048, recommended RSA-4096
3. **Certificate validity**: Consider certificate expiration for long-lived deployments
4. **Key revocation**: Plan for key compromise scenarios

## File Locations

| File | Description |
|------|-------------|
| `src/kmod/crypto.rs` | SHA-256, RSA implementation |
| `src/kmod/pkcs7.rs` | PKCS#7/CMS parsing and verification |
| `src/kmod/mod.rs` | Module loading with signature checks |
| `scripts/sign-module.sh` | Module signing tool |
| `certs/` | Key storage directory |
