#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ISO_PATH="$ROOT_DIR/dist/nexaos.iso"
ROOTFS_IMG="$ROOT_DIR/build/rootfs.ext2"

# Check for ISO
if [[ ! -f "$ISO_PATH" ]]; then
    echo "ISO image not found at $ISO_PATH. Run scripts/build-iso.sh first." >&2
    exit 1
fi

echo "Starting NexaOS in QEMU..."
echo "  Kernel: via ISO"
echo "  Root device: ${ROOTFS_IMG}"

# Prepare QEMU command
QEMU_CMD=(
    qemu-system-x86_64
    -m 512M
    -serial stdio
    -cdrom "$ISO_PATH"
    -d guest_errors
    -monitor none
)

# Add root filesystem disk if it exists
if [[ -f "$ROOTFS_IMG" ]]; then
    echo "  Found root filesystem: $ROOTFS_IMG"
    echo "  Boot will use: root=/dev/vda1 rootfstype=ext2"
    QEMU_CMD+=(
        -drive file="$ROOTFS_IMG",format=raw,if=virtio
    )
    # Note: GRUB config should include: root=/dev/vda1 rootfstype=ext2
else
    echo "  Warning: Root filesystem not found at $ROOTFS_IMG"
    echo "  System will boot from initramfs only"
    echo "  Run 'scripts/build-rootfs.sh' to create root filesystem"
fi

# Run QEMU
exec "${QEMU_CMD[@]}"  