#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Support both debug and release builds
BUILD_TYPE="${1:-release}"
TARGET_DIR="$ROOT_DIR/target/x86_64-nexaos/debug"
ISO_DIR="$ROOT_DIR/target/iso"
DIST_DIR="$ROOT_DIR/dist"
KERNEL_BIN="$TARGET_DIR/nexa-os"
# Boot with root device on virtio disk
GRUB_CMDLINE="root=/dev/vda1 rootfstype=ext2 loglevel=debug"

echo "Building ISO with $BUILD_TYPE kernel..."

for tool in grub-mkrescue xorriso; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Error: required tool '$tool' not found. Please install it via your package manager." >&2
        exit 1
    fi
done

cargo build

# Build minimal initramfs (for early boot only)
echo "Building minimal initramfs..."
bash "$ROOT_DIR/scripts/build-userspace.sh"

# Note: To create the full root filesystem on ext2 disk, run:
#   scripts/build-rootfs.sh
# This creates build/rootfs.ext2 which QEMU will attach as /dev/vda

rm -rf "$ISO_DIR" "$DIST_DIR"
mkdir -p "$ISO_DIR/boot/grub" "$DIST_DIR"

cp "$KERNEL_BIN" "$ISO_DIR/boot/kernel.elf"

# Copy default GRUB font if present (required for gfxterm on EFI systems)
GRUB_FONT_SOURCE=""
for candidate in /usr/share/grub/unicode.pf2 /usr/share/grub2/unicode.pf2; do
    if [ -z "$GRUB_FONT_SOURCE" ] && [ -f "$candidate" ]; then
        GRUB_FONT_SOURCE="$candidate"
    fi
done

if [ -n "$GRUB_FONT_SOURCE" ]; then
    mkdir -p "$ISO_DIR/boot/grub/fonts"
    cp "$GRUB_FONT_SOURCE" "$ISO_DIR/boot/grub/fonts/unicode.pf2"
fi

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

if [ "\$grub_platform" = "efi" ]; then
    if loadfont /boot/grub/fonts/unicode.pf2; then
        set gfxmode=auto
        insmod efi_gop
        insmod efi_uga
        insmod gfxterm
        terminal_output gfxterm
    else
        terminal_output console
    fi
else
    terminal_output console
fi

set gfxpayload=keep
insmod video_bochs
insmod video_cirrus

menuentry "NexaOS" {
    multiboot2 /boot/kernel.elf $GRUB_CMDLINE
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
