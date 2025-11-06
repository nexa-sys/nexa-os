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
# Use OVMF (EDK2) for UEFI boot. Ensure a writable vars file in build/
CAND_DIRS=(/usr/share/OVMF /usr/share/ovmf /usr/share/edk2/ovmf)

UEFI_CODE=""
UEFI_VARS_TEMPLATE=""

# Search for code firmware (matches OVMF_CODE*.fd, including OVMF_CODE_4M.fd etc.)
for d in "${CAND_DIRS[@]}"; do
    for f in "$d"/OVMF_CODE*.fd; do
        if [[ -f "$f" ]]; then
            UEFI_CODE="$f"
            break 2
        fi
    done
done

# Search for vars template (matches OVMF_VARS*.fd, including OVMF_VARS_4M.fd etc.)
for d in "${CAND_DIRS[@]}"; do
    for f in "$d"/OVMF_VARS*.fd; do
        if [[ -f "$f" ]]; then
            UEFI_VARS_TEMPLATE="$f"
            break 2
        fi
    done
done

if [[ -z "$UEFI_CODE" || -z "$UEFI_VARS_TEMPLATE" ]]; then
    echo "OVMF firmware not found. Install edk2-ovmf (package name may vary) and retry." >&2
    exit 1
fi

UEFI_VARS_COPY="$ROOT_DIR/build/OVMF_VARS.fd"
mkdir -p "$ROOT_DIR/build"
if [[ ! -f "$UEFI_VARS_COPY" ]]; then
    cp "$UEFI_VARS_TEMPLATE" "$UEFI_VARS_COPY"
fi

QEMU_CMD=(
    qemu-system-x86_64
    -m 512M
    -serial stdio
    # UEFI firmware: code (readonly) and writable vars copy
    -drive if=pflash,format=raw,readonly=on,file="$UEFI_CODE"
    -drive if=pflash,format=raw,file="$UEFI_VARS_COPY"
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