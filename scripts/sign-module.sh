#!/bin/bash
# Sign NexaOS kernel modules (.nkm) with PKCS#7 signatures
#
# This script generates and applies PKCS#7/CMS signatures to kernel modules,
# compatible with the Linux kernel module signing format.
#
# Usage:
#   ./sign-module.sh <module.nkm> [key.pem] [cert.pem]
#
# If key.pem and cert.pem are not provided, uses default paths:
#   - signing_key.pem (private key)
#   - signing_key.x509 (X.509 certificate)
#
# Requirements:
#   - OpenSSL 1.1+ or 3.x
#   - Existing signing key pair
#
# To generate a new signing key:
#   ./sign-module.sh --generate-key
#
# Output: <module.nkm>.signed (or replaces original with -i flag)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Default key paths
DEFAULT_KEY="$PROJECT_ROOT/certs/signing_key.pem"
DEFAULT_CERT="$PROJECT_ROOT/certs/signing_key.x509"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

print_usage() {
    echo "Usage: $0 [OPTIONS] <module.nkm> [key.pem] [cert.pem]"
    echo ""
    echo "Options:"
    echo "  -h, --help         Show this help message"
    echo "  -i, --in-place     Replace original module with signed version"
    echo "  -v, --verify       Verify an existing signed module"
    echo "  --generate-key     Generate a new signing key pair"
    echo "  --key-dir DIR      Directory for key storage (default: certs/)"
    echo ""
    echo "Examples:"
    echo "  $0 build/modules/ext2.nkm                    # Sign with default key"
    echo "  $0 -i build/modules/ext2.nkm                 # Sign in place"
    echo "  $0 --verify build/modules/ext2.nkm           # Verify signature"
    echo "  $0 --generate-key                            # Generate new key pair"
}

generate_signing_key() {
    local key_dir="${1:-$PROJECT_ROOT/certs}"
    
    echo -e "${YELLOW}Generating NexaOS kernel module signing key...${NC}"
    
    mkdir -p "$key_dir"
    
    # Configuration file for the certificate
    local config_file="$key_dir/x509.genkey"
    cat > "$config_file" << 'EOF'
[ req ]
default_bits = 4096
distinguished_name = req_distinguished_name
prompt = no
string_mask = utf8only
x509_extensions = myexts

[ req_distinguished_name ]
O = NexaOS
CN = NexaOS Kernel Module Signing Key
emailAddress = official@nexa-os.top

[ myexts ]
basicConstraints=critical,CA:FALSE
keyUsage=digitalSignature
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid
EOF

    # Generate private key
    echo "Generating 4096-bit RSA private key..."
    openssl genpkey -algorithm RSA -out "$key_dir/signing_key.pem" \
        -pkeyopt rsa_keygen_bits:4096 2>/dev/null
    
    # Generate self-signed certificate
    echo "Generating X.509 certificate..."
    openssl req -new -x509 -sha256 \
        -key "$key_dir/signing_key.pem" \
        -out "$key_dir/signing_key.x509" \
        -days 36500 \
        -config "$config_file" 2>/dev/null
    
    # Also generate DER format certificate
    openssl x509 -in "$key_dir/signing_key.x509" -outform DER \
        -out "$key_dir/signing_key.der" 2>/dev/null
    
    # Extract public key components for embedding in kernel
    echo "Extracting public key components..."
    openssl rsa -in "$key_dir/signing_key.pem" -pubout \
        -out "$key_dir/signing_key.pub.pem" 2>/dev/null
    
    # Generate Rust code for embedding the key
    generate_embedded_key "$key_dir"
    
    # Clean up config file
    rm -f "$config_file"
    
    echo -e "${GREEN}Signing key generated successfully:${NC}"
    echo "  Private key: $key_dir/signing_key.pem"
    echo "  Certificate: $key_dir/signing_key.x509"
    echo "  DER cert:    $key_dir/signing_key.der"
    echo "  Public key:  $key_dir/signing_key.pub.pem"
    echo "  Rust embed:  $key_dir/embedded_key.rs"
    
    # Print certificate info
    echo ""
    echo "Certificate details:"
    openssl x509 -in "$key_dir/signing_key.x509" -noout -subject -dates
}

generate_embedded_key() {
    local key_dir="$1"
    local output_file="$key_dir/embedded_key.rs"
    
    # Extract modulus in hex (uppercase)
    local modulus=$(openssl rsa -in "$key_dir/signing_key.pem" -noout -modulus 2>/dev/null | cut -d= -f2)
    
    # Extract public exponent - parse from text output  
    # Format is: "publicExponent: 65537 (0x10001)"
    local pubexp_line=$(openssl rsa -in "$key_dir/signing_key.pem" -noout -text 2>/dev/null | grep "publicExponent")
    local pubexp_hex=$(echo "$pubexp_line" | sed -n 's/.*0x\([0-9a-fA-F]*\).*/\1/p' | tr '[:lower:]' '[:upper:]')
    
    # Default to 65537 (0x10001) if extraction fails
    if [ -z "$pubexp_hex" ]; then
        pubexp_hex="010001"
    fi
    
    # Ensure even number of hex digits for byte conversion
    if [ $((${#pubexp_hex} % 2)) -eq 1 ]; then
        pubexp_hex="0$pubexp_hex"
    fi
    
    # Get certificate fingerprint for key ID
    local fingerprint=$(openssl x509 -in "$key_dir/signing_key.x509" -noout -fingerprint -sha256 2>/dev/null | \
        cut -d= -f2 | tr -d ':' | tr '[:upper:]' '[:lower:]')
    
    cat > "$output_file" << 'RUST_EOF'
// Embedded Module Signing Key for NexaOS
//
// This file is auto-generated by scripts/sign-module.sh
// Do not edit manually.
//
// To regenerate: ./scripts/sign-module.sh --generate-key

RUST_EOF

    # Append key data
    cat >> "$output_file" << EOF
/// Key fingerprint (SHA-256 of certificate)
pub const KEY_FINGERPRINT: &[u8] = &[
$(echo "$fingerprint" | sed 's/../    0x&,\n/g')
];

/// RSA modulus (n) in big-endian bytes
pub const RSA_MODULUS: &[u8] = &[
$(echo "$modulus" | sed 's/../    0x&,\n/g')
];

/// RSA public exponent (e) in big-endian bytes  
pub const RSA_EXPONENT: &[u8] = &[
$(echo "$pubexp_hex" | sed 's/../    0x&,\n/g')
];
EOF

    echo "Generated: $output_file"
}

sign_module() {
    local module="$1"
    local key="${2:-$DEFAULT_KEY}"
    local cert="${3:-$DEFAULT_CERT}"
    local in_place="${4:-false}"
    
    if [ ! -f "$module" ]; then
        echo -e "${RED}Error: Module file not found: $module${NC}"
        exit 1
    fi
    
    if [ ! -f "$key" ]; then
        echo -e "${RED}Error: Private key not found: $key${NC}"
        echo "Run '$0 --generate-key' to create a signing key."
        exit 1
    fi
    
    if [ ! -f "$cert" ]; then
        echo -e "${RED}Error: Certificate not found: $cert${NC}"
        echo "Run '$0 --generate-key' to create a signing key."
        exit 1
    fi
    
    local output_file
    if [ "$in_place" = "true" ]; then
        output_file="$module"
    else
        output_file="${module}.signed"
    fi
    
    local temp_dir=$(mktemp -d)
    local temp_sig="$temp_dir/sig.pkcs7"
    local temp_output="$temp_dir/module.signed"
    
    trap "rm -rf $temp_dir" EXIT
    
    echo -e "${YELLOW}Signing module: $module${NC}"
    
    # Get module size for hash calculation
    local module_size=$(stat -f%z "$module" 2>/dev/null || stat -c%s "$module")
    
    # Create PKCS#7 detached signature using SHA-256
    # Note: Using smime instead of cms due to OpenSSL 3.x compatibility issues
    echo "Creating PKCS#7 signature..."
    openssl smime -sign -binary -noattr -in "$module" \
        -signer "$cert" -inkey "$key" \
        -outform DER -out "$temp_sig" \
        -md sha256 2>/dev/null
    
    if [ ! -f "$temp_sig" ]; then
        echo -e "${RED}Error: Failed to create signature${NC}"
        exit 1
    fi
    
    local sig_size=$(stat -f%z "$temp_sig" 2>/dev/null || stat -c%s "$temp_sig")
    
    echo "Signature size: $sig_size bytes"
    
    # Build the signed module
    # Format: [module data] [pkcs7 sig] [sig_info] [magic]
    
    # Create signature info structure (12 bytes)
    # algo: 0 (unspecified)
    # hash: 4 (SHA-256)
    # key_type: 1 (RSA)
    # signer_id_type: 1 (issuer+serial)
    # reserved: 0 0 0 0
    # sig_len: big-endian 4 bytes
    
    local sig_info="$temp_dir/siginfo"
    printf '\x00\x04\x01\x01\x00\x00\x00\x00' > "$sig_info"
    # Append signature length in big-endian
    printf "$(printf '%08x' $sig_size | sed 's/../\\x&/g')" >> "$sig_info"
    
    # Combine: module + signature + sig_info + magic
    cat "$module" "$temp_sig" "$sig_info" > "$temp_output"
    echo -n "~Module sig~" >> "$temp_output"
    
    # Copy to output
    cp "$temp_output" "$output_file"
    
    local final_size=$(stat -f%z "$output_file" 2>/dev/null || stat -c%s "$output_file")
    
    echo -e "${GREEN}Module signed successfully:${NC}"
    echo "  Input:  $module ($module_size bytes)"
    echo "  Output: $output_file ($final_size bytes)"
    echo "  Signature: $sig_size bytes (SHA-256 + RSA)"
}

verify_module() {
    local module="$1"
    local cert="${2:-$DEFAULT_CERT}"
    
    if [ ! -f "$module" ]; then
        echo -e "${RED}Error: Module file not found: $module${NC}"
        exit 1
    fi
    
    echo -e "${YELLOW}Verifying module signature: $module${NC}"
    
    # Check for magic at end using tail and od
    local magic=$(tail -c 12 "$module" | od -A n -t x1 | tr -d ' \n')
    local expected_magic="7e4d6f64756c65207369677e"  # ~Module sig~
    
    if [ "$magic" != "$expected_magic" ]; then
        echo -e "${RED}Module is not signed (no signature magic found)${NC}"
        exit 1
    fi
    
    local file_size=$(stat -c%s "$module" 2>/dev/null || stat -f%z "$module")
    
    # Read sig_info (12 bytes before magic)
    local sig_info_offset=$((file_size - 24))  # 12 bytes magic + 12 bytes info
    local sig_len_hex=$(dd if="$module" bs=1 skip=$((sig_info_offset + 8)) count=4 2>/dev/null | od -A n -t x1 | tr -d ' \n')
    local sig_len=$((16#$sig_len_hex))
    
    echo "Signature length: $sig_len bytes"
    
    # Extract components
    local temp_dir=$(mktemp -d)
    trap "rm -rf $temp_dir" EXIT
    
    local content_len=$((file_size - sig_len - 24))
    local sig_offset=$content_len
    
    # Extract module content
    dd if="$module" bs=1 count=$content_len of="$temp_dir/content" 2>/dev/null
    
    # Extract signature
    dd if="$module" bs=1 skip=$sig_offset count=$sig_len of="$temp_dir/sig.pkcs7" 2>/dev/null
    
    # Verify using OpenSSL CMS
    if openssl cms -verify -binary -content "$temp_dir/content" \
        -in "$temp_dir/sig.pkcs7" -inform DER \
        -CAfile "$cert" -purpose any \
        -out /dev/null 2>/dev/null; then
        echo -e "${GREEN}Signature verification: PASSED${NC}"
        
        # Show signer info
        echo ""
        echo "Signer information:"
        openssl cms -in "$temp_dir/sig.pkcs7" -inform DER -cmsout -print 2>/dev/null | \
            grep -A5 "signerInfos:" | head -10
    else
        echo -e "${RED}Signature verification: FAILED${NC}"
        exit 1
    fi
}

# Parse arguments
IN_PLACE=false
VERIFY=false
GENERATE_KEY=false
KEY_DIR=""

while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            print_usage
            exit 0
            ;;
        -i|--in-place)
            IN_PLACE=true
            shift
            ;;
        -v|--verify)
            VERIFY=true
            shift
            ;;
        --generate-key)
            GENERATE_KEY=true
            shift
            ;;
        --key-dir)
            KEY_DIR="$2"
            shift 2
            ;;
        *)
            break
            ;;
    esac
done

if [ "$GENERATE_KEY" = "true" ]; then
    generate_signing_key "${KEY_DIR:-$PROJECT_ROOT/certs}"
    exit 0
fi

if [ -z "$1" ]; then
    print_usage
    exit 1
fi

if [ "$VERIFY" = "true" ]; then
    verify_module "$1" "$2"
else
    sign_module "$1" "$2" "$3" "$IN_PLACE"
fi
