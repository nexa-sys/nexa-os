#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/target/x86_64-nexaos/debug"
ISO_DIR="$ROOT_DIR/target/iso"
DIST_DIR="$ROOT_DIR/dist"
KERNEL_BIN="$TARGET_DIR/nexa-os"
GRUB_CMDLINE="log=debug"

for tool in grub-mkrescue xorriso; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Error: required tool '$tool' not found. Please install it via your package manager." >&2
        exit 1
    fi
done

cargo build

# Build userspace programs and initramfs
echo "Building user-space programs..."
bash "$ROOT_DIR/scripts/build-userspace.sh"

rm -rf "$ISO_DIR" "$DIST_DIR"
mkdir -p "$ISO_DIR/boot/grub" "$DIST_DIR"

cp "$KERNEL_BIN" "$ISO_DIR/boot/kernel.elf"

# Copy initramfs if it exists
HAS_INITRAMFS=0
if [ -f "$ROOT_DIR/build/initramfs.cpio" ]; then
    cp "$ROOT_DIR/build/initramfs.cpio" "$ISO_DIR/boot/initramfs.cpio"
    echo "Including initramfs in ISO"
    HAS_INITRAMFS=1
fi

{
    cat <<GRUBCFG
set timeout=3
set default=0

menuentry "NexaOS" {
    multiboot2 /boot/kernel.elf ${GRUB_CMDLINE}
GRUBCFG

    if [ "$HAS_INITRAMFS" -eq 1 ]; then
        cat <<'GRUBCFG_MODULE'
    module2 /boot/initramfs.cpio
GRUBCFG_MODULE
    fi

    cat <<'GRUBCFG_END'
    boot
}
GRUBCFG_END
} > "$ISO_DIR/boot/grub/grub.cfg"

grub-mkrescue -o "$DIST_DIR/nexaos.iso" "$ISO_DIR"

echo "ISO image created at $DIST_DIR/nexaos.iso"
