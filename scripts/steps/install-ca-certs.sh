#!/bin/bash
# NexaOS Build System - CA Certificate Installer
# Downloads and installs CA certificates for TLS/SSL support

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"

init_build_env

# ============================================================================
# Configuration
# ============================================================================

# CA certificates bundle sources (in order of preference)
CA_BUNDLE_URLS=(
    "https://curl.se/ca/cacert.pem"
    "https://raw.githubusercontent.com/curl/curl/master/lib/mk-ca-bundle.pl"
)

# Local paths
SSL_DIR="$PROJECT_ROOT/etc/ssl"
CERTS_DIR="$SSL_DIR/certs"
CA_BUNDLE_FILE="$CERTS_DIR/ca-certificates.crt"

# Cache directory for downloaded certificates
CACHE_DIR="$BUILD_DIR/.cache/ssl"

# ============================================================================
# Download Functions
# ============================================================================

download_ca_bundle() {
    log_step "Downloading CA certificate bundle..."
    
    ensure_dir "$CACHE_DIR"
    local cache_file="$CACHE_DIR/cacert.pem"
    local max_age=$((7 * 24 * 3600))  # 7 days in seconds
    
    # Check if we have a cached version that's not too old
    if [ -f "$cache_file" ]; then
        local file_age=$(($(date +%s) - $(stat -c %Y "$cache_file" 2>/dev/null || echo 0)))
        if [ "$file_age" -lt "$max_age" ]; then
            log_info "Using cached CA bundle (age: ${file_age}s)"
            return 0
        fi
    fi
    
    # Download fresh copy
    local url="${CA_BUNDLE_URLS[0]}"
    log_info "Downloading from: $url"
    
    if command -v curl &>/dev/null; then
        if curl -fsSL -o "$cache_file.tmp" "$url"; then
            mv "$cache_file.tmp" "$cache_file"
            log_success "Downloaded CA bundle successfully"
            return 0
        fi
    elif command -v wget &>/dev/null; then
        if wget -q -O "$cache_file.tmp" "$url"; then
            mv "$cache_file.tmp" "$cache_file"
            log_success "Downloaded CA bundle successfully"
            return 0
        fi
    fi
    
    # Try to use system CA bundle as fallback
    log_warn "Could not download CA bundle, trying system fallback..."
    use_system_ca_bundle
}

use_system_ca_bundle() {
    local system_bundles=(
        "/etc/ssl/certs/ca-certificates.crt"
        "/etc/pki/tls/certs/ca-bundle.crt"
        "/etc/ssl/ca-bundle.pem"
        "/etc/ssl/cert.pem"
        "/usr/share/ca-certificates/mozilla"
    )
    
    for bundle in "${system_bundles[@]}"; do
        if [ -f "$bundle" ]; then
            log_info "Using system CA bundle: $bundle"
            cp "$bundle" "$CACHE_DIR/cacert.pem"
            return 0
        fi
    done
    
    log_error "No CA certificate bundle found!"
    return 1
}

# ============================================================================
# Installation Functions
# ============================================================================

install_ca_certs() {
    log_step "Installing CA certificates to source tree..."
    
    ensure_dir "$CERTS_DIR"
    
    local cache_file="$CACHE_DIR/cacert.pem"
    
    if [ ! -f "$cache_file" ]; then
        log_error "CA bundle not found in cache"
        return 1
    fi
    
    # Copy to source tree
    cp "$cache_file" "$CA_BUNDLE_FILE"
    
    # Create symlinks for compatibility
    ln -sf "ca-certificates.crt" "$CERTS_DIR/ca-bundle.crt"
    ln -sf "ca-certificates.crt" "$CERTS_DIR/cert.pem"
    
    log_success "CA certificates installed to: $CERTS_DIR"
    
    # Count certificates
    local cert_count=$(grep -c "BEGIN CERTIFICATE" "$CA_BUNDLE_FILE" 2>/dev/null || echo "0")
    log_info "Installed $cert_count CA certificates"
}

install_to_rootfs() {
    local rootfs_dir="${1:-$BUILD_DIR/rootfs}"
    
    log_step "Installing CA certificates to rootfs..."
    
    ensure_dir "$rootfs_dir/etc/ssl/certs"
    
    if [ -f "$CA_BUNDLE_FILE" ]; then
        cp "$CA_BUNDLE_FILE" "$rootfs_dir/etc/ssl/certs/ca-certificates.crt"
        ln -sf "ca-certificates.crt" "$rootfs_dir/etc/ssl/certs/ca-bundle.crt"
        ln -sf "ca-certificates.crt" "$rootfs_dir/etc/ssl/certs/cert.pem"
        log_success "CA certificates installed to rootfs"
    else
        log_warn "No CA bundle to install (run 'download' first)"
        return 1
    fi
}

install_to_initramfs() {
    local initramfs_dir="${1:-$BUILD_DIR/initramfs}"
    
    log_step "Installing CA certificates to initramfs..."
    
    ensure_dir "$initramfs_dir/etc/ssl/certs"
    
    if [ -f "$CA_BUNDLE_FILE" ]; then
        cp "$CA_BUNDLE_FILE" "$initramfs_dir/etc/ssl/certs/ca-certificates.crt"
        ln -sf "ca-certificates.crt" "$initramfs_dir/etc/ssl/certs/ca-bundle.crt"
        log_success "CA certificates installed to initramfs"
    else
        log_warn "No CA bundle to install (run 'download' first)"
        return 1
    fi
}

# ============================================================================
# Verification Functions
# ============================================================================

verify_ca_bundle() {
    log_step "Verifying CA bundle..."
    
    if [ ! -f "$CA_BUNDLE_FILE" ]; then
        log_error "CA bundle not found: $CA_BUNDLE_FILE"
        return 1
    fi
    
    # Basic validation
    if ! grep -q "BEGIN CERTIFICATE" "$CA_BUNDLE_FILE"; then
        log_error "Invalid CA bundle format"
        return 1
    fi
    
    local cert_count=$(grep -c "BEGIN CERTIFICATE" "$CA_BUNDLE_FILE")
    log_info "CA bundle contains $cert_count certificates"
    
    # Check for some well-known root CAs
    local known_cas=("DigiCert" "Let's Encrypt" "GlobalSign" "Comodo" "GeoTrust")
    for ca in "${known_cas[@]}"; do
        if grep -q "$ca" "$CA_BUNDLE_FILE"; then
            log_info "  ✓ Found: $ca"
        fi
    done
    
    log_success "CA bundle verification passed"
}

# ============================================================================
# Main Build Flow
# ============================================================================

build_ca_certs() {
    log_section "Installing CA Certificates"
    
    download_ca_bundle
    install_ca_certs
    verify_ca_bundle
    
    log_success "CA certificates ready"
}

# ============================================================================
# Main
# ============================================================================

if [ "${BASH_SOURCE[0]}" == "${0}" ]; then
    case "${1:-all}" in
        all)
            build_ca_certs
            ;;
        download)
            download_ca_bundle
            ;;
        install)
            install_ca_certs
            ;;
        rootfs)
            install_to_rootfs "$2"
            ;;
        initramfs)
            install_to_initramfs "$2"
            ;;
        verify)
            verify_ca_bundle
            ;;
        *)
            echo "Usage: $0 {all|download|install|rootfs|initramfs|verify}"
            exit 1
            ;;
    esac
fi
