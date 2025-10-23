#!/bin/bash
# Build user-space programs and create initramfs

set -e

USERSPACE_DIR="userspace"
BUILD_DIR="build/initramfs"
INITRAMFS_CPIO="build/initramfs.cpio"

echo "Building user-space programs..."

# Create build directory
mkdir -p "$BUILD_DIR/bin"

# Compile shell
echo "Compiling /bin/sh..."
rustc --target x86_64-unknown-linux-gnu \
    -C opt-level=s \
    -C panic=abort \
    -C linker=rust-lld \
    -o "$BUILD_DIR/bin/sh" "$USERSPACE_DIR/shell.rs"

# Strip symbols
strip --strip-all "$BUILD_DIR/bin/sh"

echo "User-space programs built successfully"

# Create initramfs
echo "Creating initramfs..."
cd "$BUILD_DIR"
find . -print0 | cpio --null --create --format=newc > "../../$INITRAMFS_CPIO"
cd ../..

echo "Initramfs created: $INITRAMFS_CPIO"
ls -lh "$INITRAMFS_CPIO"

# Verify it's a valid CPIO archive
if file "$INITRAMFS_CPIO" | grep -q "cpio"; then
    echo "✓ Valid CPIO archive created"
else
    echo "✗ Warning: Generated file may not be a valid CPIO archive"
fi
