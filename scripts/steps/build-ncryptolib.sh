#!/bin/bash
# NexaOS Build System - ncryptolib Builder
# Build the NexaOS cryptographic library (libcrypto.so compatible)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"

init_build_env

# ============================================================================
# Build ncryptolib
# ============================================================================

NCRYPTOLIB_SRC="$PROJECT_ROOT/userspace/ncryptolib"
SYSROOT_DIR="$BUILD_DIR/userspace-build/sysroot"
SYSROOT_PIC_DIR="$BUILD_DIR/userspace-build/sysroot-pic"
TARGET_LIB="$PROJECT_ROOT/targets/x86_64-nexaos-userspace-lib.json"

build_ncryptolib_static() {
    log_step "Building ncryptolib staticlib (libcrypto.a)..."
    
    ensure_dir "$SYSROOT_DIR/lib"
    
    cd "$NCRYPTOLIB_SRC"
    
    # Build static library with std support
    RUSTFLAGS="-C opt-level=2 -C panic=abort" \
        cargo build -Z build-std=std,core,alloc --target "$TARGET_LIB" --release
    
    local staticlib="$PROJECT_ROOT/userspace/target/x86_64-nexaos-userspace-lib/release/libncryptolib.a"
    
    if [ -f "$staticlib" ]; then
        cp "$staticlib" "$SYSROOT_DIR/lib/libcrypto.a"
        log_success "libcrypto.a installed to sysroot ($(file_size "$staticlib"))"
    else
        log_error "Failed to build ncryptolib staticlib"
        return 1
    fi
}

build_ncryptolib_shared() {
    local dest_dir="${1:-$SYSROOT_DIR/lib}"
    
    log_step "Building ncryptolib shared library (libcrypto.so)..."
    
    ensure_dir "$dest_dir"
    
    cd "$NCRYPTOLIB_SRC"
    
    # Build shared library with PIC and std support
    RUSTFLAGS="-C opt-level=2 -C panic=abort -C relocation-model=pic" \
        cargo build -Z build-std=std,core,alloc --target "$TARGET_LIB" --release
    
    local sharedlib="$PROJECT_ROOT/userspace/target/x86_64-nexaos-userspace-lib/release/libncryptolib.so"
    
    if [ -f "$sharedlib" ]; then
        cp "$sharedlib" "$dest_dir/libcrypto.so"
        strip --strip-unneeded "$dest_dir/libcrypto.so" 2>/dev/null || true
        
        # Create version symlinks
        ln -sf libcrypto.so "$dest_dir/libcrypto.so.3"
        ln -sf libcrypto.so "$dest_dir/libcrypto.so.3.0.0"
        
        log_success "libcrypto.so installed ($(file_size "$dest_dir/libcrypto.so"))"
    else
        log_error "Failed to build ncryptolib shared library"
        return 1
    fi
}

# ============================================================================
# Main
# ============================================================================

case "${1:-all}" in
    static)
        build_ncryptolib_static
        ;;
    shared)
        build_ncryptolib_shared "$2"
        ;;
    all)
        build_ncryptolib_static
        build_ncryptolib_shared
        ;;
    *)
        echo "Usage: $0 [static|shared|all] [dest_dir]"
        exit 1
        ;;
esac
