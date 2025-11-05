#!/bin/bash
# Build ext2 root filesystem with full system

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_ROOT/build"
ROOTFS_DIR="$BUILD_DIR/rootfs"
ROOTFS_IMG="$BUILD_DIR/rootfs.ext2"
ROOTFS_SIZE_MB=50

echo "========================================"
echo "Building ext2 root filesystem"
echo "========================================"

# Create rootfs directory structure
echo "Creating rootfs directory structure..."
mkdir -p "$ROOTFS_DIR"/{bin,sbin,etc/ni,dev,proc,sys,tmp,var,home,root}

# Build userspace programs for rootfs
echo "Building userspace programs..."
mkdir -p "$BUILD_DIR/userspace-build"

cat > "$BUILD_DIR/userspace-build/Cargo.toml" << 'EOF'
[package]
name = "userspace"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "ni"
path = "../../userspace/init.rs"

[[bin]]
name = "sh"
path = "../../userspace/shell.rs"

[[bin]]
name = "getty"
path = "../../userspace/getty.rs"

[[bin]]
name = "login"
path = "../../userspace/login.rs"

[profile.release]
panic = "abort"
opt-level = 2
lto = true

[dependencies]
EOF

cd "$BUILD_DIR/userspace-build"
RUSTFLAGS="-C opt-level=2 -C panic=abort -C linker=rust-lld -C link-arg=--image-base=0x00400000" \
    cargo build -Z build-std=core --target "$PROJECT_ROOT/x86_64-nexaos.json" --release

# Copy binaries to rootfs
echo "Copying binaries to rootfs..."
cp "target/x86_64-nexaos/release/ni" "$ROOTFS_DIR/sbin/ni"
cp "target/x86_64-nexaos/release/getty" "$ROOTFS_DIR/sbin/getty"
cp "target/x86_64-nexaos/release/sh" "$ROOTFS_DIR/bin/sh"
cp "target/x86_64-nexaos/release/login" "$ROOTFS_DIR/bin/login"

# Strip symbols
strip --strip-all "$ROOTFS_DIR/sbin/ni" 2>/dev/null || true
strip --strip-all "$ROOTFS_DIR/sbin/getty" 2>/dev/null || true
strip --strip-all "$ROOTFS_DIR/bin/sh" 2>/dev/null || true
strip --strip-all "$ROOTFS_DIR/bin/login" 2>/dev/null || true

# Copy configuration files
echo "Copying configuration files..."
if [ -f "$PROJECT_ROOT/etc/ni/ni.conf" ]; then
    cp "$PROJECT_ROOT/etc/ni/ni.conf" "$ROOTFS_DIR/etc/ni/ni.conf"
fi

if [ -f "$PROJECT_ROOT/etc/inittab" ]; then
    cp "$PROJECT_ROOT/etc/inittab" "$ROOTFS_DIR/etc/inittab"
fi

# Create a welcome message
cat > "$ROOTFS_DIR/etc/motd" << 'EOF'
Welcome to NexaOS!

You are now running from the real root filesystem (ext2).
This system was mounted via pivot_root from initramfs.

EOF

# Create a simple init script as fallback
cat > "$ROOTFS_DIR/sbin/init" << 'EOF'
#!/bin/sh
# Simple init fallback
exec /sbin/ni
EOF
chmod +x "$ROOTFS_DIR/sbin/init"

echo "Rootfs directory contents:"
ls -lah "$ROOTFS_DIR"
find "$ROOTFS_DIR" -type f -ls

# Create ext2 filesystem image
echo "Creating ext2 filesystem image (${ROOTFS_SIZE_MB}MB)..."
dd if=/dev/zero of="$ROOTFS_IMG" bs=1M count=$ROOTFS_SIZE_MB status=progress

echo "Formatting as ext2..."
mkfs.ext2 -F -L "nexaos-root" "$ROOTFS_IMG"

# Mount and copy files
echo "Copying files to ext2 filesystem..."
MOUNT_POINT=$(mktemp -d)
sudo mount -o loop "$ROOTFS_IMG" "$MOUNT_POINT"

# Copy all files
sudo cp -a "$ROOTFS_DIR"/* "$MOUNT_POINT/"

# Create device nodes (basic ones)
sudo mknod "$MOUNT_POINT/dev/null" c 1 3 || true
sudo mknod "$MOUNT_POINT/dev/zero" c 1 5 || true
sudo mknod "$MOUNT_POINT/dev/console" c 5 1 || true

# Set permissions
sudo chmod 755 "$MOUNT_POINT"/{bin,sbin}
sudo chmod 755 "$MOUNT_POINT"/{bin,sbin}/*
sudo chmod 1777 "$MOUNT_POINT/tmp"

# Unmount
sudo umount "$MOUNT_POINT"
rmdir "$MOUNT_POINT"

echo "✓ Root filesystem created: $ROOTFS_IMG"
ls -lh "$ROOTFS_IMG"

# Verify ext2 filesystem
echo "Verifying ext2 filesystem..."
if file "$ROOTFS_IMG" | grep -q "ext2"; then
    echo "✓ Valid ext2 filesystem"
    dumpe2fs -h "$ROOTFS_IMG" 2>/dev/null | head -20
else
    echo "✗ Warning: May not be a valid ext2 filesystem"
fi

echo "========================================"
echo "Rootfs build complete!"
echo "Image: $ROOTFS_IMG"
echo "========================================"
