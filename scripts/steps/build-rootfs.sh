#!/bin/bash
# NexaOS Build System - Rootfs Builder
# Build ext2 root filesystem

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"

init_build_env

# ============================================================================
# Configuration
# ============================================================================

ROOTFS_DIR="$BUILD_DIR/rootfs"
ROOTFS_SIZE_MB="${ROOTFS_SIZE_MB:-50}"

# ============================================================================
# Setup Functions
# ============================================================================

setup_rootfs_dirs() {
    log_step "Creating rootfs directory structure..."
    
    ensure_dir "$ROOTFS_DIR"/{bin,sbin,etc/ni,dev,proc,sys,tmp,var,home,root,lib64}
}

install_configs() {
    log_step "Installing configuration files..."
    
    # Copy ni config
    if [ -f "$PROJECT_ROOT/etc/ni/ni.conf" ]; then
        cp "$PROJECT_ROOT/etc/ni/ni.conf" "$ROOTFS_DIR/etc/ni/ni.conf"
    fi
    
    # Copy inittab
    if [ -f "$PROJECT_ROOT/etc/inittab" ]; then
        cp "$PROJECT_ROOT/etc/inittab" "$ROOTFS_DIR/etc/inittab"
    fi
    
    # Create motd
    cat > "$ROOTFS_DIR/etc/motd" << 'EOF'
Welcome to NexaOS!

You are now running from the real root filesystem (ext2).
This system was mounted via pivot_root from initramfs.

EOF
    
    # Create fallback init
    cat > "$ROOTFS_DIR/sbin/init" << 'EOF'
#!/bin/sh
# Simple init fallback
exec /sbin/ni
EOF
    chmod +x "$ROOTFS_DIR/sbin/init"
    
    log_success "Configuration files installed"
}

install_libs() {
    log_step "Installing libraries to rootfs..."
    
    # Build and install nrlib (libc)
    bash "$SCRIPT_DIR/build-nrlib.sh" all "$ROOTFS_DIR/lib64"
    
    # Build and install ncryptolib (libcrypto)
    bash "$SCRIPT_DIR/build-ncryptolib.sh" shared "$ROOTFS_DIR/lib64"
}

# ============================================================================
# Build Functions
# ============================================================================

build_rootfs_programs() {
    log_step "Building userspace programs for rootfs..."
    
    bash "$SCRIPT_DIR/build-userspace-programs.sh" all "$ROOTFS_DIR"
}

create_ext2_image() {
    log_step "Creating ext2 filesystem image (${ROOTFS_SIZE_MB}MB)..."
    
    require_cmds dd mkfs.ext2
    
    # Show rootfs contents
    log_info "Rootfs directory contents:"
    ls -lah "$ROOTFS_DIR"
    find "$ROOTFS_DIR" -type f -ls
    
    # Create image file
    dd if=/dev/zero of="$ROOTFS_IMG" bs=1M count="$ROOTFS_SIZE_MB" status=progress
    
    # Format as ext2
    log_info "Formatting as ext2..."
    mkfs.ext2 -F -L "nexaos-root" "$ROOTFS_IMG"
    
    # Mount and copy files
    log_info "Copying files to ext2 filesystem..."
    local mount_point
    mount_point=$(mktemp -d)
    
    sudo mount -o loop "$ROOTFS_IMG" "$mount_point"
    
    # Copy all files
    sudo cp -a "$ROOTFS_DIR"/* "$mount_point/"
    
    # Create device nodes
    sudo mknod "$mount_point/dev/null" c 1 3 || true
    sudo mknod "$mount_point/dev/zero" c 1 5 || true
    sudo mknod "$mount_point/dev/console" c 5 1 || true
    
    # Set permissions
    sudo chmod 755 "$mount_point"/{bin,sbin}
    sudo chmod 755 "$mount_point"/{bin,sbin}/* 2>/dev/null || true
    sudo chmod 1777 "$mount_point/tmp"
    
    # Unmount
    sudo umount "$mount_point"
    rmdir "$mount_point"
    
    log_success "Root filesystem created: $ROOTFS_IMG ($(file_size "$ROOTFS_IMG"))"
    
    # Verify
    if file "$ROOTFS_IMG" | grep -q "ext2"; then
        log_success "Valid ext2 filesystem"
        dumpe2fs -h "$ROOTFS_IMG" 2>/dev/null | head -20
    else
        log_warn "May not be a valid ext2 filesystem"
    fi
}

build_rootfs() {
    log_section "Building ext2 Root Filesystem"
    
    setup_rootfs_dirs
    install_libs
    build_rootfs_programs
    install_configs
    create_ext2_image
    
    log_success "Rootfs build complete"
}

# ============================================================================
# Main
# ============================================================================

if [ "${BASH_SOURCE[0]}" == "${0}" ]; then
    case "${1:-all}" in
        all)
            build_rootfs
            ;;
        dirs)
            setup_rootfs_dirs
            ;;
        libs)
            install_libs
            ;;
        programs)
            build_rootfs_programs
            ;;
        configs)
            install_configs
            ;;
        image)
            create_ext2_image
            ;;
        *)
            echo "Usage: $0 {all|dirs|libs|programs|configs|image}"
            exit 1
            ;;
    esac
fi
