#!/bin/bash
# Build complete NexaOS system with root filesystem

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "========================================"
echo "Building Complete NexaOS System"
echo "========================================"
echo ""

# Step 1: Build kernel
echo "Step 1/4: Building kernel..."
cd "$PROJECT_ROOT"
cargo build --release
echo "✓ Kernel built"
echo ""

# Step 2: Build minimal initramfs (early boot only)
echo "Step 2/4: Building minimal initramfs..."
bash "$SCRIPT_DIR/build-userspace.sh"
echo "✓ Minimal initramfs ready"
echo ""

# Step 3: Build ext2 root filesystem (full system)
echo "Step 3/4: Building ext2 root filesystem..."
bash "$SCRIPT_DIR/build-rootfs.sh"
echo "✓ Root filesystem ready"
echo ""

# Step 4: Build bootable ISO (release build)
echo "Step 4/4: Building bootable ISO..."
bash "$SCRIPT_DIR/build-iso.sh" release
echo "✓ ISO created"
echo ""

echo "========================================"
echo "Build Complete!"
echo "========================================"
echo ""
echo "System components:"
echo "  - Kernel: target/x86_64-nexaos/release/nexa-os"
echo "  - Initramfs: build/initramfs.cpio (minimal, $(stat -c%s "$PROJECT_ROOT/build/initramfs.cpio" | numfmt --to=iec-i)B)"
echo "  - Root FS: build/rootfs.ext2 (full system, $(stat -c%s "$PROJECT_ROOT/build/rootfs.ext2" | numfmt --to=iec-i)B)"
echo "  - ISO: dist/nexaos.iso"
echo ""
echo "To run in QEMU:"
echo "  ./scripts/run-qemu.sh"
echo ""
echo "Boot parameters (in GRUB):"
echo "  root=/dev/vda1 rootfstype=ext2 loglevel=info"
echo ""
