#!/bin/bash
# NexaOS Build System - nrlib Builder
# Build the NexaOS runtime library (static and shared)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"

init_build_env

# ============================================================================
# Build nrlib
# ============================================================================

NRLIB_SRC="$PROJECT_ROOT/userspace/nrlib"
SYSROOT_DIR="$BUILD_DIR/userspace-build/sysroot"
SYSROOT_PIC_DIR="$BUILD_DIR/userspace-build/sysroot-pic"

build_nrlib_static() {
    log_step "Building nrlib staticlib (libc.a)..."
    
    ensure_dir "$SYSROOT_DIR/lib"
    ensure_dir "$SYSROOT_PIC_DIR/lib"
    
    cd "$NRLIB_SRC"
    
    # Build non-PIC version for static linking
    RUSTFLAGS="$(get_nrlib_rustflags)" \
        cargo build -Z build-std=core --target "$TARGET_USERSPACE" --release
    
    local staticlib="$NRLIB_SRC/target/x86_64-nexaos-userspace/release/libnrlib.a"
    
    if [ -f "$staticlib" ]; then
        cp "$staticlib" "$SYSROOT_DIR/lib/libc.a"
        # Create empty libunwind.a (std has its own unwind)
        ar crs "$SYSROOT_DIR/lib/libunwind.a"
        log_success "libc.a installed to sysroot ($(file_size "$staticlib"))"
    else
        log_error "Failed to build nrlib staticlib"
        return 1
    fi
    
    # Build PIC version for dynamic linking (PIE executables need PIC code)
    # This goes into a separate sysroot-pic directory so that -lc finds it
    log_step "Building nrlib staticlib with PIC for PIE executables..."
    
    RUSTFLAGS="-C opt-level=2 -C panic=abort -C relocation-model=pic" \
        cargo build -Z build-std=core --target "$TARGET_USERSPACE_PIC" --release
    
    local staticlib_pic="$NRLIB_SRC/target/x86_64-nexaos-userspace-pic/release/libnrlib.a"
    
    if [ -f "$staticlib_pic" ]; then
        # Install PIC version as libc.a in the PIC sysroot
        # This way PIE executables can use -L sysroot-pic/lib and -lc will find PIC version
        cp "$staticlib_pic" "$SYSROOT_PIC_DIR/lib/libc.a"
        # Also create empty libunwind.a and libgcc_s.a (not needed with panic=abort)
        ar crs "$SYSROOT_PIC_DIR/lib/libunwind.a"
        ar crs "$SYSROOT_PIC_DIR/lib/libgcc_s.a"
        # Keep a copy as libc_pic.a in main sysroot for reference
        cp "$staticlib_pic" "$SYSROOT_DIR/lib/libc_pic.a"
        log_success "PIC libc.a installed to sysroot-pic ($(file_size "$staticlib_pic"))"
    else
        log_error "Failed to build nrlib PIC staticlib"
        return 1
    fi
}

build_nrlib_shared() {
    local dest_dir="${1:-$SYSROOT_DIR/lib}"
    
    log_step "Building nrlib shared library (libnrlib.so)..."
    
    ensure_dir "$dest_dir"
    
    cd "$NRLIB_SRC"
    
    RUSTFLAGS="$(get_pic_rustflags)" \
        cargo build -Z build-std=core --target "$TARGET_USERSPACE_PIC" --release
    
    local sharedlib="$NRLIB_SRC/target/x86_64-nexaos-userspace-pic/release/libnrlib.so"
    
    if [ -f "$sharedlib" ]; then
        cp "$sharedlib" "$dest_dir/libnrlib.so"
        # Use --strip-unneeded instead of --strip-all to preserve _start and other 
        # symbols that might be needed for dynamic linking
        strip --strip-unneeded "$dest_dir/libnrlib.so" 2>/dev/null || true
        
        # Create compatibility symlinks
        ln -sf libnrlib.so "$dest_dir/libc.so"
        ln -sf libnrlib.so "$dest_dir/libc.so.6"
        ln -sf libnrlib.so "$dest_dir/libc.musl-x86_64.so.1"
        
        log_success "libnrlib.so installed ($(file_size "$dest_dir/libnrlib.so"))"
    else
        log_error "Failed to build nrlib shared library"
        return 1
    fi
}

build_dynamic_linker() {
    local dest_dir="${1:-$SYSROOT_DIR/lib}"
    local build_dir="$BUILD_DIR/ld-nrlib-build"
    
    log_step "Building dynamic linker (ld-nrlib-x86_64.so.1)..."
    
    ensure_dir "$dest_dir" "$build_dir"
    
    cat > "$build_dir/Cargo.toml" << 'EOF'
[package]
name = "ld-nrlib"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "ld-nrlib"
path = "../../userspace/ld-nrlib.rs"

[profile.release]
panic = "abort"
opt-level = "s"
lto = true
EOF

    cd "$build_dir"
    
    RUSTFLAGS="$(get_ld_rustflags)" \
        cargo build -Z build-std=core --target "$TARGET_LD" --release
    
    local ld_bin="$build_dir/target/x86_64-nexaos-ld/release/ld-nrlib"
    
    if [ -f "$ld_bin" ]; then
        cp "$ld_bin" "$dest_dir/ld-nrlib-x86_64.so.1"
        strip --strip-all "$dest_dir/ld-nrlib-x86_64.so.1" 2>/dev/null || true
        chmod +x "$dest_dir/ld-nrlib-x86_64.so.1"
        
        # Create compatibility symlinks
        ln -sf ld-nrlib-x86_64.so.1 "$dest_dir/ld-musl-x86_64.so.1"
        ln -sf ld-nrlib-x86_64.so.1 "$dest_dir/ld-nexaos.so.1"
        ln -sf ld-nrlib-x86_64.so.1 "$dest_dir/ld-linux-x86-64.so.2"
        
        log_success "ld-nrlib-x86_64.so.1 installed ($(file_size "$dest_dir/ld-nrlib-x86_64.so.1"))"
    else
        log_error "Failed to build dynamic linker"
        return 1
    fi
}

build_all_nrlib() {
    local dest_dir="${1:-}"
    
    log_section "Building nrlib Components"
    
    build_nrlib_static
    
    if [ -n "$dest_dir" ]; then
        build_nrlib_shared "$dest_dir"
        build_dynamic_linker "$dest_dir"
    else
        build_nrlib_shared
        build_dynamic_linker
    fi
    
    log_success "All nrlib components built"
}

# Main
if [ "${BASH_SOURCE[0]}" == "${0}" ]; then
    case "${1:-all}" in
        static)
            build_nrlib_static
            ;;
        shared)
            build_nrlib_shared "${2:-}"
            ;;
        ld|linker)
            build_dynamic_linker "${2:-}"
            ;;
        all)
            build_all_nrlib "${2:-}"
            ;;
        *)
            echo "Usage: $0 {static|shared|ld|all} [dest_dir]"
            exit 1
            ;;
    esac
fi
