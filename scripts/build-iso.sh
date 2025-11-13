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

echo "Building UEFI isolation loader..."
bash "$ROOT_DIR/scripts/build-uefi-loader.sh"

# Build minimal initramfs (for early boot only)
echo "Building minimal initramfs..."
bash "$ROOT_DIR/scripts/build-userspace.sh"

# Note: To create the full root filesystem on ext2 disk, run:
#   scripts/build-rootfs.sh
# This creates build/rootfs.ext2 which QEMU will attach as /dev/vda

rm -rf "$ISO_DIR" "$DIST_DIR"
mkdir -p "$ISO_DIR/boot/grub" "$DIST_DIR"

cp "$KERNEL_BIN" "$ISO_DIR/boot/kernel.elf"

# Copy UEFI loader and payload files (if available)
if [ -f "$ROOT_DIR/build/BootX64.EFI" ]; then
    mkdir -p "$ISO_DIR/EFI/BOOT"
    cp "$ROOT_DIR/build/BootX64.EFI" "$ISO_DIR/EFI/BOOT/BOOTX64.EFI"
    # Copy kernel to both EFI/BOOT and boot/ for maximum compatibility
    cp "$KERNEL_BIN" "$ISO_DIR/EFI/BOOT/KERNEL.ELF"
    cp "$KERNEL_BIN" "$ISO_DIR/boot/KERNEL.ELF"
    echo "Including UEFI loader (EFI/BOOT/BOOTX64.EFI)"
fi

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
    # Also copy to EFI/BOOT and root if UEFI loader exists
    if [ -f "$ROOT_DIR/build/BootX64.EFI" ]; then
        cp "$ROOT_DIR/build/initramfs.cpio" "$ISO_DIR/EFI/BOOT/INITRAMFS.CPIO"
        cp "$ROOT_DIR/build/initramfs.cpio" "$ISO_DIR/boot/INITRAMFS.CPIO"
    fi
    echo "Including initramfs in ISO"
    HAS_INITRAMFS=1
fi

{
    cat <<GRUBCFG
set timeout=3
set default=0

# 检测是否是UEFI环境
if [ "\$grub_platform" = "efi" ]; then
    # 设置UEFI显示模式
    if loadfont /boot/grub/fonts/unicode.pf2; then
        set gfxmode=auto
        insmod efi_gop
        insmod efi_uga
        insmod gfxterm
        terminal_output gfxterm
    else
        terminal_output console
    fi
    
    # 添加UEFI启动项
    menuentry "NexaOS (UEFI)" {
        insmod part_msdos
        insmod ext2
        echo 'Loading NexaOS UEFI Loader...'
        chainloader /EFI/BOOT/BOOTX64.EFI
    }
else
    terminal_output console
fi

set gfxpayload=keep
insmod video_bochs
insmod video_cirrus

# Legacy BIOS启动项
menuentry "NexaOS (Legacy)" {
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

# Post-process: Inject kernel and initramfs into ESP
if [ -f "$ROOT_DIR/build/BootX64.EFI" ]; then
    echo "Post-processing: Injecting files into ESP..."
    
    # Extract ISO to temporary directory
    ISO_TEMP="$ROOT_DIR/build/iso_extract"
    rm -rf "$ISO_TEMP"
    mkdir -p "$ISO_TEMP"
    
    # Extract using 7z or similar
    echo "  Extracting ISO..."
    if command -v 7z >/dev/null 2>&1; then
        7z x -o"$ISO_TEMP" "$DIST_DIR/nexaos.iso" -bsp0 -bso0 2>&1 | head -10
        echo "  Extraction complete"
    else
        echo "Error: 7z not found, cannot modify ESP" >&2
        exit 1
    fi
    
    # Find efi.img
    EFI_IMG=$(find "$ISO_TEMP" -name "efi.img" 2>/dev/null | head -1)
    echo "  Looking for efi.img: $EFI_IMG"
    
    if [ -n "$EFI_IMG" ] && [ -f "$EFI_IMG" ]; then
        echo "  Found original ESP: $EFI_IMG"
        
        # Create fresh 20MB ESP (bootloader 1MB + kernel 15MB + initramfs 400KB + overhead)
        NEW_ESP="$ROOT_DIR/build/new_efi.img"
        echo "  Creating 20MB ESP..."
        dd if=/dev/zero of="$NEW_ESP" bs=1M count=20 status=none
        mkfs.vfat -F 12 -n "UEFI" "$NEW_ESP" >/dev/null 2>&1
        
        # Create directory structure
        mmd -i "$NEW_ESP" ::/EFI 2>/dev/null || true
        mmd -i "$NEW_ESP" ::/EFI/BOOT 2>/dev/null || true
        
        # Copy bootloader
        echo "  Copying BOOTX64.EFI..."
        mcopy -i "$NEW_ESP" "$ROOT_DIR/build/BootX64.EFI" "::/EFI/BOOT/BOOTX64.EFI"
        
        # Add kernel
        echo "  Adding KERNEL.ELF..."
        mcopy -i "$NEW_ESP" "$KERNEL_BIN" "::/EFI/BOOT/KERNEL.ELF"
        
        if [ -f "$ROOT_DIR/build/initramfs.cpio" ]; then
            echo "  Adding INITRAMFS.CPIO..."
            mcopy -i "$NEW_ESP" "$ROOT_DIR/build/initramfs.cpio" "::/EFI/BOOT/INITRAMFS.CPIO"
        fi
        
        # Make old ESP writable and replace with new
        chmod u+w "$EFI_IMG"
        mv -f "$NEW_ESP" "$EFI_IMG"
        
        # Verify
        echo "  Final ESP contents:"
        mdir -i "$EFI_IMG" ::/EFI/BOOT/
        
        # Rebuild ISO
        echo "  Rebuilding ISO..."
        rm -f "$DIST_DIR/nexaos.iso"
        grub-mkrescue -o "$DIST_DIR/nexaos.iso" "$ISO_TEMP" 2>&1 | grep -v "xorriso.*WARNING" || true
        
        echo "  ✓ ESP modified successfully"
    else
        echo "Warning: Could not find efi.img in extracted ISO"
    fi
    
    # Cleanup
    rm -rf "$ISO_TEMP"
fi

echo "ISO image created at $DIST_DIR/nexaos.iso"