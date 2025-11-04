#!/bin/bash
# Build user-space programs and create initramfs

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_ROOT/build/initramfs"
INITRAMFS_CPIO="$PROJECT_ROOT/build/initramfs.cpio"

echo "Building user-space programs..."

# Create build directories
mkdir -p "$BUILD_DIR/bin"
mkdir -p "$BUILD_DIR/sbin"
mkdir -p "$BUILD_DIR/etc/ni"

# Always regenerate Cargo.toml to ensure it has all binaries
cat > "$BUILD_DIR/Cargo.toml" << 'EOF'
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

# Build all binaries
echo "Compiling userspace programs..."
cd "$BUILD_DIR"
RUSTFLAGS="-C opt-level=2 -C panic=abort -C linker=rust-lld -C link-arg=--image-base=0x00400000" \
    cargo build -Z build-std=core --target "$PROJECT_ROOT/x86_64-nexaos.json" --release

# Copy binaries
echo "Copying binaries..."
cp "target/x86_64-nexaos/release/ni" "$BUILD_DIR/sbin/ni"
cp "target/x86_64-nexaos/release/getty" "$BUILD_DIR/sbin/getty"
cp "target/x86_64-nexaos/release/sh" "$BUILD_DIR/bin/sh"
cp "target/x86_64-nexaos/release/login" "$BUILD_DIR/bin/login"

# Strip symbols
echo "Stripping binaries..."
strip --strip-all "$BUILD_DIR/sbin/ni" 2>/dev/null || true
strip --strip-all "$BUILD_DIR/sbin/getty" 2>/dev/null || true
strip --strip-all "$BUILD_DIR/bin/sh" 2>/dev/null || true
strip --strip-all "$BUILD_DIR/bin/login" 2>/dev/null || true

echo "User-space programs built successfully:"
ls -lh "$BUILD_DIR/sbin/ni"
ls -lh "$BUILD_DIR/sbin/getty"
ls -lh "$BUILD_DIR/bin/sh"
ls -lh "$BUILD_DIR/bin/login"

# Copy configuration files
echo "Copying configuration files..."
if [ -f "$PROJECT_ROOT/etc/ni/ni.conf" ]; then
    cp "$PROJECT_ROOT/etc/ni/ni.conf" "$BUILD_DIR/etc/ni/ni.conf"
    echo "  - Copied /etc/ni/ni.conf"
else
    echo "  - Warning: /etc/ni/ni.conf not found"
fi

if [ -f "$PROJECT_ROOT/etc/inittab" ]; then
    mkdir -p "$BUILD_DIR/etc"
    cp "$PROJECT_ROOT/etc/inittab" "$BUILD_DIR/etc/inittab"
    echo "  - Copied /etc/inittab"
fi

# Create initramfs
echo "Creating initramfs..."
cd "$BUILD_DIR"
find sbin bin etc -type f -print0 2>/dev/null | cpio --null -o --format=newc > "$INITRAMFS_CPIO"
cd "$PROJECT_ROOT"

echo "Initramfs created: $INITRAMFS_CPIO"
ls -lh "$INITRAMFS_CPIO"

# Verify it's a valid CPIO archive
if file "$INITRAMFS_CPIO" | grep -q "cpio"; then
    echo "✓ Valid CPIO archive created"
    echo "✓ Contents:"
    cpio -itv < "$INITRAMFS_CPIO"
else
    echo "✗ Warning: Generated file may not be a valid CPIO archive"
fi

echo "Build complete!"
