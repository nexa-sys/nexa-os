# NexaOS CA Certificates

This document describes how CA (Certificate Authority) certificates are managed in NexaOS.

## Overview

NexaOS automatically downloads and installs Mozilla's trusted CA certificate bundle during the build process. This enables HTTPS/TLS connections to validate server certificates out of the box.

## Certificate Bundle Source

The CA certificates are sourced from:
- **Primary**: https://curl.se/ca/cacert.pem (Mozilla's CA bundle, maintained by curl project)
- **Fallback**: System CA certificates from the build host

## Installation Paths

CA certificates are installed to the following standard locations in the rootfs:

```
/etc/ssl/certs/ca-certificates.crt   # Main certificate bundle
/etc/ssl/certs/ca-bundle.crt         # Symlink for compatibility
/etc/ssl/certs/cert.pem              # Symlink for compatibility
```

## Build Integration

### Automatic Installation

CA certificates are automatically installed during the rootfs build:

```bash
./scripts/build.sh all        # Full build includes CA certs
./scripts/build.sh rootfs     # Rootfs build includes CA certs
```

### Manual Installation

You can also manage CA certificates manually:

```bash
# Download and install CA certificates
./scripts/steps/install-ca-certs.sh all

# Download only (to cache)
./scripts/steps/install-ca-certs.sh download

# Install to source tree
./scripts/steps/install-ca-certs.sh install

# Install to specific rootfs directory
./scripts/steps/install-ca-certs.sh rootfs /path/to/rootfs

# Verify installed certificates
./scripts/steps/install-ca-certs.sh verify
```

## Using CA Certificates in Applications

### nssl Library

The `nssl` library automatically loads CA certificates from standard paths:

```rust
use nssl::context::SslContext;
use nssl::ssl::SslMethod;

// Create SSL context
let mut ctx = SslContext::new(&SslMethod::tls_client())?;

// Load default CA certificates
ctx.set_default_verify_paths();

// Or load from specific file
ctx.load_verify_locations(Some("/etc/ssl/certs/ca-certificates.crt"), None);
```

### nurl (curl-like tool)

The `nurl` utility uses the CA certificates automatically for HTTPS connections:

```bash
# HTTPS request (uses system CA certs)
nurl https://example.com/

# Skip certificate verification (insecure)
nurl -k https://example.com/
```

## Certificate Updates

The CA bundle is cached for 7 days. To force a refresh:

```bash
rm -rf build/.cache/ssl
./scripts/steps/install-ca-certs.sh download
```

## Troubleshooting

### Certificate Verification Failures

1. Ensure CA certificates are installed:
   ```bash
   ls -la /etc/ssl/certs/
   ```

2. Check if the certificate bundle contains expected CAs:
   ```bash
   grep -c "BEGIN CERTIFICATE" /etc/ssl/certs/ca-certificates.crt
   ```

3. Verify a specific CA is present:
   ```bash
   grep "DigiCert" /etc/ssl/certs/ca-certificates.crt
   ```

### Download Failures

If the download fails during build:

1. Check network connectivity
2. Try using system CA certificates as fallback (automatically attempted)
3. Manually download and place in `etc/ssl/certs/ca-certificates.crt`

## Security Considerations

- The CA bundle is downloaded over HTTPS from curl.se
- Certificates are from Mozilla's trusted root program
- The bundle is verified to contain expected well-known CAs
- Consider pinning specific certificates for high-security applications
