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
mkdir -p "$ROOTFS_DIR"/{bin,sbin,etc/ni,dev,proc,sys,tmp,var,home,root,lib64}

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
required-features = []

[[bin]]
name = "sh"
path = "../../userspace/shell.rs"

[[bin]]
name = "getty"
path = "../../userspace/getty.rs"
required-features = []

[[bin]]
name = "login"
path = "../../userspace/login.rs"

[[bin]]
name = "nslookup"
path = "../../userspace/nslookup.rs"

[[bin]]
name = "uefi-compatd"
path = "../../userspace/uefi_compatd.rs"

[profile.release]
panic = "abort"
opt-level = 2

# Disable LTO for now to avoid PIC/LTO issues with user-space buffers (see docs/bugfixes)
lto = false

[dependencies]
nrlib = { path = "../../userspace/nrlib", optional = true, default-features = false }
nexa_boot_info = { path = "../../boot/boot-info" }

[features]
default = ["use-nrlib"]
use-nrlib = ["nrlib", "nrlib/panic-handler"]
use-nrlib-std = ["nrlib", "nrlib/std"]
use-std = ["nrlib"]

EOF

cd "$BUILD_DIR/userspace-build"

# Build init (ni) with std
echo "Building ni (init) with std..."

# First, build nrlib as staticlib to provide libc compatibility
mkdir -p "$BUILD_DIR/userspace-build/sysroot/lib"
echo "Building nrlib staticlib for libc compatibility..."
cd "$PROJECT_ROOT/userspace/nrlib"
RUSTFLAGS="-C opt-level=2 -C panic=abort" \
    cargo build -Z build-std=core --target "$PROJECT_ROOT/x86_64-nexaos-userspace.json" --release
    
# Copy nrlib staticlib as libc.a and libunwind.a
cp "$PROJECT_ROOT/userspace/nrlib/target/x86_64-nexaos-userspace/release/libnrlib.a" \
   "$BUILD_DIR/userspace-build/sysroot/lib/libc.a"

# Create an empty libunwind.a (std has its own unwind implementation)
ar crs "$BUILD_DIR/userspace-build/sysroot/lib/libunwind.a"

# Now build ni with std, linking against our nrlib-based libc
cd "$BUILD_DIR/userspace-build"
STD_RUSTFLAGS="-C opt-level=2 -C panic=abort -C linker=rust-lld -C link-arg=--image-base=0x00400000 -C link-arg=--entry=_start -L $BUILD_DIR/userspace-build/sysroot/lib -C link-arg=-upthread_mutexattr_settype -C link-arg=-upthread_mutexattr_init -C link-arg=-upthread_mutexattr_destroy -C link-arg=-upthread_mutex_init -C link-arg=-upthread_mutex_lock -C link-arg=-upthread_mutex_unlock -C link-arg=-upthread_mutex_destroy -C link-arg=-upthread_once -C link-arg=-u__libc_single_threaded"
RUSTFLAGS="$STD_RUSTFLAGS" \
    cargo build -Z build-std=std,panic_abort --target "$PROJECT_ROOT/x86_64-nexaos-userspace.json" --release \
    --bin ni --no-default-features

# Build getty with std to leverage std::io printing
echo "Building getty with std..."
RUSTFLAGS="$STD_RUSTFLAGS" \
    cargo build -Z build-std=std,panic_abort --target "$PROJECT_ROOT/x86_64-nexaos-userspace.json" --release \
    --bin getty --no-default-features

# Build shell with std
echo "Building sh (shell) with std..."
RUSTFLAGS="$STD_RUSTFLAGS" \
    cargo build -Z build-std=std,panic_abort --target "$PROJECT_ROOT/x86_64-nexaos-userspace.json" --release \
    --bin sh

# Build login with std
echo "Building login with std..."
RUSTFLAGS="$STD_RUSTFLAGS" \
    cargo build -Z build-std=std,panic_abort --target "$PROJECT_ROOT/x86_64-nexaos-userspace.json" --release \
    --bin login

# Build nslookup with std
echo "Building nslookup with std..."
RUSTFLAGS="$STD_RUSTFLAGS" \
    cargo build -Z build-std=std,panic_abort --target "$PROJECT_ROOT/x86_64-nexaos-userspace.json" --release \
    --bin nslookup --no-default-features

# Build uefi-compatd
echo "Building uefi-compatd with std..."
RUSTFLAGS="$STD_RUSTFLAGS" \
    cargo build -Z build-std=std,panic_abort --target "$PROJECT_ROOT/x86_64-nexaos-userspace.json" --release \
    --bin uefi-compatd --no-default-features --features use-nrlib-std

# Copy binaries to rootfs
echo "Copying binaries to rootfs..."
cp "target/x86_64-nexaos-userspace/release/ni" "$ROOTFS_DIR/sbin/ni"
cp "target/x86_64-nexaos-userspace/release/getty" "$ROOTFS_DIR/sbin/getty"
cp "target/x86_64-nexaos-userspace/release/sh" "$ROOTFS_DIR/bin/sh"
cp "target/x86_64-nexaos-userspace/release/login" "$ROOTFS_DIR/bin/login"
cp "target/x86_64-nexaos-userspace/release/nslookup" "$ROOTFS_DIR/bin/nslookup"
cp "target/x86_64-nexaos-userspace/release/uefi-compatd" "$ROOTFS_DIR/sbin/uefi-compatd"

# Strip symbols
strip --strip-all "$ROOTFS_DIR/sbin/ni" 2>/dev/null || true
strip --strip-all "$ROOTFS_DIR/sbin/getty" 2>/dev/null || true
strip --strip-all "$ROOTFS_DIR/bin/sh" 2>/dev/null || true
strip --strip-all "$ROOTFS_DIR/bin/login" 2>/dev/null || true
strip --strip-all "$ROOTFS_DIR/bin/nslookup" 2>/dev/null || true
strip --strip-all "$ROOTFS_DIR/sbin/uefi-compatd" 2>/dev/null || true

# Copy dynamic linker for dynamically linked programs
echo "Copying dynamic linker..."
if [ -f "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2" ]; then
    cp "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2" "$ROOTFS_DIR/lib64/"
    echo "✓ Added dynamic linker ld-linux-x86-64.so.2"
else
    echo "⚠ Warning: System dynamic linker not found, dynamically linked programs won't work"
fi

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
