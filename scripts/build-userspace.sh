#!/bin/bash
# Build user-space programs and create initramfs

set -e

USERSPACE_DIR="userspace"
BUILD_DIR="build/initramfs"
INITRAMFS_CPIO="build/initramfs.cpio"

echo "Building user-space programs..."

# Create build directory
mkdir -p "$BUILD_DIR/bin"

# Create temporary Cargo.toml for user-space programs
cat > "$BUILD_DIR/Cargo.toml" << 'EOF'
[package]
name = "userspace"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "sh"
path = "../../userspace/shell.rs"

[dependencies]
EOF

# Compile shell
echo "Compiling /bin/sh..."
cd "$BUILD_DIR"
RUSTFLAGS="-C opt-level=s -C panic=abort -C linker=rust-lld" cargo build -Z build-std=core --target /home/hanxi-cat/dev/nexa-os/x86_64-nexaos.json --release --bin sh
cd ../..

# Copy the binary
cp "$BUILD_DIR/target/x86_64-nexaos/release/sh" "$BUILD_DIR/bin/sh"

# Strip symbols
strip --strip-all "$BUILD_DIR/bin/sh"

echo "User-space programs built successfully"

# Create initramfs
echo "Creating initramfs..."
cd "$BUILD_DIR"
find bin/ -type f -print0 | cpio --null --create --format=newc > "../../$INITRAMFS_CPIO"
cd ../..

echo "Initramfs created: $INITRAMFS_CPIO"
ls -lh "$INITRAMFS_CPIO"

# Verify it's a valid CPIO archive
if file "$INITRAMFS_CPIO" | grep -q "cpio"; then
    echo "✓ Valid CPIO archive created"
else
    echo "✗ Warning: Generated file may not be a valid CPIO archive"
fi
