#!/bin/bash
# NexaOS Build System - Initramfs Builder
# Build minimal initramfs for early boot

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"

init_build_env

# ============================================================================
# Configuration
# ============================================================================

INITRAMFS_DIR="$BUILD_DIR/initramfs"
INITRAMFS_BUILD="$BUILD_DIR/initramfs-build"
INITRAMFS_STAGING="$INITRAMFS_DIR/staging"

# ============================================================================
# Setup Functions
# ============================================================================

setup_initramfs_cargo() {
    log_step "Setting up initramfs build environment..."
    
    ensure_dir "$INITRAMFS_BUILD"
    
    cat > "$INITRAMFS_BUILD/Cargo.toml" << 'EOF'
[package]
name = "initramfs-tools"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "sh"
path = "../../userspace/shell.rs"

[profile.release]
panic = "abort"
opt-level = 2
lto = false
EOF
}

create_init_script() {
    log_step "Creating init script..."
    
    cat > "$INITRAMFS_DIR/init" << 'INIT_SCRIPT'
#!/bin/sh
# Minimal initramfs init script
# Purpose: Mount proc/sys, detect root device, mount it, and pivot to real root

echo "[initramfs] Starting early userspace init..."

# Mount essential filesystems
mount -t proc none /proc 2>/dev/null || echo "[initramfs] proc already mounted"
mount -t sysfs none /sys 2>/dev/null || echo "[initramfs] sys already mounted"

echo "[initramfs] Early init complete, kernel will handle root mounting"
echo "[initramfs] If you see this, something went wrong - dropping to emergency shell"

# Drop to emergency shell if we get here
exec /bin/sh
INIT_SCRIPT

    chmod +x "$INITRAMFS_DIR/init"
}

create_readme() {
    cat > "$INITRAMFS_DIR/README.txt" << 'EOF'
NexaOS Initramfs
================

This is a minimal initial RAM filesystem designed for early boot.

Purpose:
- Provide emergency recovery shell
- Mount /proc and /sys  
- Load necessary drivers (future)
- Detect and prepare root device (future)
- Bridge to real root filesystem

Contents:
- /init - Early init script executed by kernel
- /bin/sh - Emergency shell for recovery
- /dev, /proc, /sys - Mount points for virtual filesystems
- /sysroot - Mount point for real root filesystem
- /lib/modules - Loadable kernel modules (.nkm)

Note: The actual root mounting is currently handled by the kernel's
boot_stages module. This initramfs serves as a safety net and
provides the foundation for future driver loading capabilities.
EOF
}

# ============================================================================
# Build Functions
# ============================================================================

build_initramfs_shell() {
    log_step "Building emergency shell for initramfs..."
    
    setup_initramfs_cargo
    
    # Build nrlib staticlib for shell
    ensure_dir "$INITRAMFS_BUILD/sysroot/lib"
    
    cd "$PROJECT_ROOT/userspace/nrlib"
    RUSTFLAGS="$(get_nrlib_rustflags)" \
        cargo build -Z build-std=core --target "$TARGET_USERSPACE" --release
    
    cp "$PROJECT_ROOT/userspace/nrlib/target/x86_64-nexaos-userspace/release/libnrlib.a" \
       "$INITRAMFS_BUILD/sysroot/lib/libc.a"
    ar crs "$INITRAMFS_BUILD/sysroot/lib/libunwind.a"
    
    # Build shell with std
    cd "$INITRAMFS_BUILD"
    local rustflags
    rustflags="$(get_std_rustflags "$INITRAMFS_BUILD/sysroot/lib")"
    
    RUSTFLAGS="$rustflags" \
        cargo build -Z build-std=std,panic_abort \
            --target "$TARGET_USERSPACE" \
            --release
    
    ensure_dir "$INITRAMFS_DIR/bin"
    cp "$INITRAMFS_BUILD/target/x86_64-nexaos-userspace/release/sh" "$INITRAMFS_DIR/bin/sh"
    strip --strip-all "$INITRAMFS_DIR/bin/sh" 2>/dev/null || true
    
    log_success "Emergency shell built: $(file_size "$INITRAMFS_DIR/bin/sh")"
}

build_initramfs_libs() {
    log_step "Building libraries for initramfs..."
    
    ensure_dir "$INITRAMFS_DIR/lib64"
    
    # Build shared library
    cd "$PROJECT_ROOT/userspace/nrlib"
    RUSTFLAGS="$(get_pic_rustflags)" \
        cargo build -Z build-std=core --target "$TARGET_USERSPACE_PIC" --release
    
    cp "$PROJECT_ROOT/userspace/nrlib/target/x86_64-nexaos-userspace-pic/release/libnrlib.so" \
       "$INITRAMFS_DIR/lib64/"
    strip --strip-all "$INITRAMFS_DIR/lib64/libnrlib.so" 2>/dev/null || true
    
    # Symlinks
    ln -sf libnrlib.so "$INITRAMFS_DIR/lib64/libc.so"
    ln -sf libnrlib.so "$INITRAMFS_DIR/lib64/libc.so.6"
    ln -sf libnrlib.so "$INITRAMFS_DIR/lib64/libc.musl-x86_64.so.1"
    
    log_success "libnrlib.so installed"
    
    # Build dynamic linker
    bash "$SCRIPT_DIR/build-nrlib.sh" ld "$INITRAMFS_DIR/lib64"
}

build_initramfs_modules() {
    log_step "Building kernel modules for initramfs..."
    
    bash "$SCRIPT_DIR/build-modules.sh" all
}

create_cpio_archive() {
    log_step "Creating CPIO archive..."
    
    # Clean staging directory
    rm -rf "$INITRAMFS_STAGING"
    ensure_dir "$INITRAMFS_STAGING"/{bin,dev,proc,sys,sysroot,lib64,lib/modules}
    
    # Copy essential files
    cp "$INITRAMFS_DIR/init" "$INITRAMFS_STAGING/"
    cp "$INITRAMFS_DIR/README.txt" "$INITRAMFS_STAGING/"
    cp "$INITRAMFS_DIR/bin/sh" "$INITRAMFS_STAGING/bin/"
    
    # Copy libraries
    if [ -f "$INITRAMFS_DIR/lib64/ld-nrlib-x86_64.so.1" ]; then
        cp "$INITRAMFS_DIR/lib64/ld-nrlib-x86_64.so.1" "$INITRAMFS_STAGING/lib64/"
        ln -sf ld-nrlib-x86_64.so.1 "$INITRAMFS_STAGING/lib64/ld-linux-x86-64.so.2"
        ln -sf ld-nrlib-x86_64.so.1 "$INITRAMFS_STAGING/lib64/ld-musl-x86_64.so.1"
        ln -sf ld-nrlib-x86_64.so.1 "$INITRAMFS_STAGING/lib64/ld-nexaos.so.1"
    fi
    
    if [ -f "$INITRAMFS_DIR/lib64/libnrlib.so" ]; then
        cp "$INITRAMFS_DIR/lib64/libnrlib.so" "$INITRAMFS_STAGING/lib64/"
        ln -sf libnrlib.so "$INITRAMFS_STAGING/lib64/libc.so"
        ln -sf libnrlib.so "$INITRAMFS_STAGING/lib64/libc.so.6"
        ln -sf libnrlib.so "$INITRAMFS_STAGING/lib64/libc.musl-x86_64.so.1"
    fi
    
    # Copy kernel modules
    if [ -d "$BUILD_DIR/modules" ]; then
        for nkm in "$BUILD_DIR/modules"/*.nkm; do
            if [ -f "$nkm" ]; then
                cp "$nkm" "$INITRAMFS_STAGING/lib/modules/"
                log_info "Added module: $(basename "$nkm")"
            fi
        done
    fi
    
    # Copy kernel symbols if available
    if [ -f "$BUILD_DIR/kernel.syms" ]; then
        cp "$BUILD_DIR/kernel.syms" "$INITRAMFS_STAGING/"
    fi
    
    # Create CPIO archive
    cd "$INITRAMFS_STAGING"
    find . -print0 | cpio --null -o --format=newc > "$INITRAMFS_CPIO" 2>/dev/null
    cd "$PROJECT_ROOT"
    
    # Cleanup
    rm -rf "$INITRAMFS_STAGING"
    
    log_success "Initramfs created: $INITRAMFS_CPIO ($(file_size "$INITRAMFS_CPIO"))"
    
    # Verify
    if file "$INITRAMFS_CPIO" | grep -q "cpio"; then
        log_success "Valid CPIO archive"
        echo ""
        log_info "Contents:"
        cpio -itv < "$INITRAMFS_CPIO" 2>/dev/null | head -20
    else
        log_warn "May not be a valid CPIO archive"
    fi
}

build_initramfs() {
    log_section "Building Minimal Initramfs"
    
    ensure_dir "$INITRAMFS_DIR"/{bin,dev,proc,sys,sysroot,lib64,lib/modules}
    
    build_initramfs_shell
    build_initramfs_libs
    build_initramfs_modules
    
    create_init_script
    create_readme
    create_cpio_archive
    
    log_success "Initramfs build complete"
}

# ============================================================================
# Main
# ============================================================================

if [ "${BASH_SOURCE[0]}" == "${0}" ]; then
    case "${1:-all}" in
        all)
            build_initramfs
            ;;
        shell)
            build_initramfs_shell
            ;;
        libs)
            build_initramfs_libs
            ;;
        modules)
            build_initramfs_modules
            ;;
        cpio)
            create_cpio_archive
            ;;
        *)
            echo "Usage: $0 {all|shell|libs|modules|cpio}"
            exit 1
            ;;
    esac
fi
