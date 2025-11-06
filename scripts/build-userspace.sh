#!/bin/bash
# Build minimal initramfs for early boot
# This initramfs only contains what's needed to mount the real root filesystem

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_ROOT/build/initramfs"
INITRAMFS_CPIO="$PROJECT_ROOT/build/initramfs.cpio"

echo "========================================"
echo "Building minimal initramfs"
echo "========================================"

# Create minimal directory structure for initramfs
mkdir -p "$BUILD_DIR"/{bin,sbin,dev,proc,sys,sysroot}

# Build only essential tools for initramfs
echo "Building emergency shell (for recovery)..."
mkdir -p "$PROJECT_ROOT/build/initramfs-build"

cat > "$PROJECT_ROOT/build/initramfs-build/Cargo.toml" << 'EOF'
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

[dependencies]
nrlib = { path = "../../userspace/nrlib" }
EOF

cd "$PROJECT_ROOT/build/initramfs-build"
RUSTFLAGS="-C opt-level=2 -C panic=abort -C linker=rust-lld -C link-arg=--image-base=0x00400000" \
    cargo build -Z build-std=core --target "$PROJECT_ROOT/x86_64-nexaos.json" --release

# Copy emergency shell to initramfs
cp "target/x86_64-nexaos/release/sh" "$BUILD_DIR/bin/sh"
strip --strip-all "$BUILD_DIR/bin/sh" 2>/dev/null || true

echo "✓ Emergency shell built: $(stat -c%s "$BUILD_DIR/bin/sh") bytes"

# Create init script for initramfs
# This script is executed by the kernel early in the boot process
cat > "$BUILD_DIR/init" << 'INIT_SCRIPT'
#!/bin/sh
# Minimal initramfs init script
# Purpose: Mount proc/sys, detect root device, mount it, and pivot to real root

echo "[initramfs] Starting early userspace init..."

# Mount essential filesystems
mount -t proc none /proc 2>/dev/null || echo "[initramfs] proc already mounted"
mount -t sysfs none /sys 2>/dev/null || echo "[initramfs] sys already mounted"

# Note: In a real implementation, this script would:
# 1. Load necessary kernel modules (storage drivers, filesystem drivers)
#    Example: modprobe virtio_blk
#    Example: modprobe ext4
# 
# 2. Wait for devices to appear
#    Example: udevadm trigger && udevadm settle
#
# 3. Handle complex storage (LVM, RAID, encryption)
#    Example: vgchange -ay
#    Example: cryptsetup open /dev/vda1 root
#
# 4. Run fsck if needed
#    Example: fsck -y /dev/vda1
#
# 5. Mount the real root filesystem
#    Example: mount -t ext2 -o ro /dev/vda1 /sysroot
#
# 6. Switch to real root
#    Example: exec switch_root /sysroot /sbin/init
#
# For now, the kernel handles root mounting via boot_stages module

echo "[initramfs] Early init complete, kernel will handle root mounting"
echo "[initramfs] If you see this, something went wrong - dropping to emergency shell"

# Drop to emergency shell if we get here
exec /bin/sh
INIT_SCRIPT

chmod +x "$BUILD_DIR/init"

# Add a README explaining initramfs purpose
cat > "$BUILD_DIR/README.txt" << 'EOF'
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

Note: The actual root mounting is currently handled by the kernel's
boot_stages module. This initramfs serves as a safety net and
provides the foundation for future driver loading capabilities.
EOF

# Create initramfs CPIO archive
echo "Creating initramfs CPIO archive..."

# Create a clean staging directory to avoid including build artifacts
STAGING_DIR="$BUILD_DIR/staging"
rm -rf "$STAGING_DIR"
mkdir -p "$STAGING_DIR"/{bin,dev,proc,sys,sysroot}

# Copy only the essential files
cp "$BUILD_DIR/init" "$STAGING_DIR/"
cp "$BUILD_DIR/README.txt" "$STAGING_DIR/"
cp "$BUILD_DIR/bin/sh" "$STAGING_DIR/bin/"

# Include rootfs.ext2 if it exists (for testing until real block devices work)
# Note: This embeds the entire root filesystem in initramfs for convenience.
# In production with real hardware drivers, rootfs.ext2 would be on a separate disk.
if [ -f "$PROJECT_ROOT/build/rootfs.ext2" ]; then
    echo "Including rootfs.ext2 in initramfs for testing..."
    cp "$PROJECT_ROOT/build/rootfs.ext2" "$STAGING_DIR/rootfs.ext2"
    ROOTFS_SIZE=$(stat -c%s "$STAGING_DIR/rootfs.ext2")
    echo "✓ Added rootfs.ext2 ($ROOTFS_SIZE bytes, $(numfmt --to=iec-i $ROOTFS_SIZE))"
fi

# Create CPIO archive from staging directory
cd "$STAGING_DIR"
find . -print0 | cpio --null -o --format=newc > "$INITRAMFS_CPIO" 2>/dev/null
cd "$PROJECT_ROOT"

# Clean up staging directory
rm -rf "$STAGING_DIR"

echo "✓ Initramfs created: $INITRAMFS_CPIO"
ls -lh "$INITRAMFS_CPIO"

# Verify it's a valid CPIO archive
if file "$INITRAMFS_CPIO" | grep -q "cpio"; then
    echo "✓ Valid CPIO archive"
    echo ""
    echo "Contents:"
    cpio -itv < "$INITRAMFS_CPIO" 2>/dev/null | head -20
else
    echo "✗ Warning: Generated file may not be a valid CPIO archive"
fi

echo ""
echo "========================================"
echo "Minimal initramfs build complete!"
echo "========================================"
