# NexaOS SSL/TLS Library (nssl)

A modern, libssl.so ABI-compatible TLS library for NexaOS.

## Features

### Supported Protocols
- **TLS 1.3** (RFC 8446) - Recommended, default
- **TLS 1.2** (RFC 5246) - For legacy compatibility

### Cipher Suites (TLS 1.3)
- TLS_AES_256_GCM_SHA384 (0x1302)
- TLS_AES_128_GCM_SHA256 (0x1301)
- TLS_CHACHA20_POLY1305_SHA256 (0x1303)

### Cipher Suites (TLS 1.2)
- TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384
- TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
- TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256
- TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384
- TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
- TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256

### Key Exchange
- X25519 (preferred)
- P-256 (secp256r1)
- P-384 (secp384r1)

### Signature Algorithms
- Ed25519
- ECDSA with P-256/P-384
- RSA-PSS
- RSA PKCS#1 v1.5 (TLS 1.2 only)

### Extensions
- ALPN (Application-Layer Protocol Negotiation)
- SNI (Server Name Indication)
- Session Resumption (PSK)
- 0-RTT Early Data (TLS 1.3)
- OCSP Stapling
- Certificate Transparency

## Security Design

**Removed Legacy Protocols:**
- ❌ SSLv2, SSLv3
- ❌ TLS 1.0, TLS 1.1

**Removed Weak Ciphers:**
- ❌ RC4, DES, 3DES
- ❌ MD5-based MACs
- ❌ Export ciphers
- ❌ Static RSA key exchange

**Security Features:**
- ✅ ECDHE-only key exchange (Perfect Forward Secrecy)
- ✅ AEAD ciphers only (AES-GCM, ChaCha20-Poly1305)
- ✅ Constant-time operations (side-channel resistant)

## Building

```bash
cd userspace/nssl
cargo build --release
```

Output files:
- `libnssl.so` - Shared library
- `libnssl.a` - Static library

## Usage

### C API (OpenSSL Compatible)

```c
#include <openssl/ssl.h>

int main() {
    // Initialize
    SSL_library_init();
    
    // Create context
    SSL_CTX *ctx = SSL_CTX_new(TLS_client_method());
    SSL_CTX_set_verify(ctx, SSL_VERIFY_PEER, NULL);
    SSL_CTX_load_verify_locations(ctx, "ca-cert.pem", NULL);
    
    // Create connection
    SSL *ssl = SSL_new(ctx);
    SSL_set_fd(ssl, socket_fd);
    SSL_set_tlsext_host_name(ssl, "example.com");
    
    // Handshake
    if (SSL_connect(ssl) == 1) {
        // Send/receive data
        SSL_write(ssl, "GET / HTTP/1.1\r\n\r\n", 18);
        
        char buf[4096];
        int n = SSL_read(ssl, buf, sizeof(buf));
    }
    
    // Cleanup
    SSL_shutdown(ssl);
    SSL_free(ssl);
    SSL_CTX_free(ctx);
    
    return 0;
}
```

### Rust API

```rust
use nssl::{SslContext, SslMethod};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create context
    let ctx = SslContext::new(SslMethod::tls_client())?;
    ctx.set_verify(SSL_VERIFY_PEER);
    ctx.load_verify_locations("ca-cert.pem", None)?;
    
    // Create connection
    let ssl = ctx.new_ssl()?;
    ssl.set_fd(socket_fd);
    ssl.set_hostname("example.com");
    
    // Handshake and communicate
    ssl.connect()?;
    ssl.write(b"GET / HTTP/1.1\r\n\r\n")?;
    
    let mut buf = [0u8; 4096];
    let n = ssl.read(&mut buf)?;
    
    ssl.shutdown()?;
    Ok(())
}
```

## API Reference

### Context Functions
- `SSL_CTX_new()` - Create new SSL context
- `SSL_CTX_free()` - Free SSL context
- `SSL_CTX_set_verify()` - Set verification mode
- `SSL_CTX_load_verify_locations()` - Load CA certificates
- `SSL_CTX_use_certificate_file()` - Load certificate
- `SSL_CTX_use_PrivateKey_file()` - Load private key
- `SSL_CTX_set_cipher_list()` - Set TLS 1.2 ciphers
- `SSL_CTX_set_ciphersuites()` - Set TLS 1.3 ciphers
- `SSL_CTX_set_alpn_protos()` - Set ALPN protocols
- `SSL_CTX_set_min_proto_version()` - Set minimum version
- `SSL_CTX_set_max_proto_version()` - Set maximum version

### Connection Functions
- `SSL_new()` - Create SSL connection
- `SSL_free()` - Free SSL connection
- `SSL_set_fd()` - Set socket file descriptor
- `SSL_set_bio()` - Set BIO pair
- `SSL_connect()` - Perform client handshake
- `SSL_accept()` - Perform server handshake
- `SSL_read()` - Read decrypted data
- `SSL_write()` - Write data for encryption
- `SSL_shutdown()` - Shutdown connection
- `SSL_get_error()` - Get error code
- `SSL_set_tlsext_host_name()` - Set SNI hostname

### Certificate Functions
- `SSL_get_peer_certificate()` - Get peer certificate
- `SSL_get_peer_cert_chain()` - Get certificate chain
- `SSL_get_verify_result()` - Get verification result

### Error Functions
- `ERR_get_error()` - Get and remove error
- `ERR_peek_error()` - Peek at error
- `ERR_clear_error()` - Clear error queue
- `ERR_error_string()` - Get error string

### BIO Functions
- `BIO_new()` - Create BIO
- `BIO_free()` - Free BIO
- `BIO_new_socket()` - Create socket BIO
- `BIO_new_file()` - Create file BIO
- `BIO_new_mem_buf()` - Create memory BIO
- `BIO_read()` - Read from BIO
- `BIO_write()` - Write to BIO

## Module Structure

```
nssl/
├── src/
│   ├── lib.rs           # Main library, C ABI exports
│   ├── ssl.rs           # SSL method types
│   ├── context.rs       # SSL_CTX implementation
│   ├── connection.rs    # SSL connection handling
│   ├── tls.rs           # TLS protocol constants
│   ├── record.rs        # TLS record layer
│   ├── handshake.rs     # TLS handshake protocol
│   ├── alert.rs         # TLS alerts
│   ├── cipher.rs        # Cipher definitions
│   ├── cipher_suites.rs # Cipher suite constants
│   ├── kex.rs           # Key exchange (X25519, ECDH)
│   ├── x509.rs          # Certificate handling
│   ├── cert_verify.rs   # Certificate verification
│   ├── session.rs       # Session management
│   ├── extensions.rs    # TLS extensions
│   ├── bio.rs           # BIO abstraction
│   ├── error.rs         # Error handling
│   └── compat.rs        # OpenSSL compatibility
├── build.rs             # Build script
├── Cargo.toml           # Package manifest
└── README.md            # This file
```

## License

Same as NexaOS kernel.

## Dependencies

- `ncryptolib` - NexaOS cryptographic library
