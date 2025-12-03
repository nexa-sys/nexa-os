#!/bin/bash
# NexaOS Build System - ISO Builder
# Build bootable ISO image

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"

init_build_env

# ============================================================================
# Configuration
# ============================================================================

ISO_WORK_DIR="$TARGET_DIR/iso"
GRUB_CMDLINE="root=/dev/vda1 rootfstype=ext2 loglevel=${LOG_LEVEL:-debug}"

# ============================================================================
# Functions
# ============================================================================

check_dependencies() {
    require_cmds grub-mkrescue xorriso
}

setup_iso_structure() {
    log_step "Setting up ISO structure..."
    
    rm -rf "$ISO_WORK_DIR"
    ensure_dir "$ISO_WORK_DIR/boot/grub" "$DIST_DIR"
    
    # Copy kernel
    cp "$KERNEL_BIN" "$ISO_WORK_DIR/boot/kernel.elf"
    log_info "Kernel: $(file_size "$KERNEL_BIN")"
}

copy_uefi_loader() {
    if [ -f "$BUILD_DIR/BootX64.EFI" ]; then
        log_step "Adding UEFI loader..."
        
        ensure_dir "$ISO_WORK_DIR/EFI/BOOT"
        cp "$BUILD_DIR/BootX64.EFI" "$ISO_WORK_DIR/EFI/BOOT/BOOTX64.EFI"
        cp "$KERNEL_BIN" "$ISO_WORK_DIR/EFI/BOOT/KERNEL.ELF"
        cp "$KERNEL_BIN" "$ISO_WORK_DIR/boot/KERNEL.ELF"
        
        log_success "UEFI loader included"
        return 0
    fi
    return 1
}

copy_grub_font() {
    log_step "Copying GRUB font..."
    
    local font_source=""
    for candidate in /usr/share/grub/unicode.pf2 /usr/share/grub2/unicode.pf2; do
        if [ -f "$candidate" ]; then
            font_source="$candidate"
            break
        fi
    done
    
    if [ -n "$font_source" ]; then
        ensure_dir "$ISO_WORK_DIR/boot/grub/fonts"
        cp "$font_source" "$ISO_WORK_DIR/boot/grub/fonts/unicode.pf2"
        log_success "GRUB font installed"
    else
        log_warn "GRUB font not found"
    fi
}

copy_initramfs() {
    local has_initramfs=0
    
    if [ -f "$INITRAMFS_CPIO" ]; then
        log_step "Adding initramfs..."
        
        cp "$INITRAMFS_CPIO" "$ISO_WORK_DIR/boot/initramfs.cpio"
        
        # Also copy to EFI directory if UEFI loader exists
        if [ -f "$BUILD_DIR/BootX64.EFI" ]; then
            cp "$INITRAMFS_CPIO" "$ISO_WORK_DIR/EFI/BOOT/INITRAMFS.CPIO"
            cp "$INITRAMFS_CPIO" "$ISO_WORK_DIR/boot/INITRAMFS.CPIO"
        fi
        
        log_success "Initramfs included ($(file_size "$INITRAMFS_CPIO"))"
        has_initramfs=1
    fi
    
    echo $has_initramfs
}

generate_grub_config() {
    local has_initramfs="$1"
    local has_uefi="$2"
    
    log_step "Generating GRUB configuration..."
    
    {
        cat <<GRUBCFG
set timeout=3
set default=0

# Detect UEFI environment
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
GRUBCFG

        if [ "$has_uefi" = "1" ]; then
            cat <<'GRUBCFG'
    
    # UEFI boot entry
    menuentry "NexaOS (UEFI)" {
        insmod part_msdos
        insmod ext2
        echo 'Loading NexaOS UEFI Loader...'
        chainloader /EFI/BOOT/BOOTX64.EFI
    }
GRUBCFG
        fi

        cat <<'GRUBCFG'
else
    terminal_output console
fi

set gfxpayload=keep
insmod video_bochs
insmod video_cirrus

GRUBCFG

        cat <<GRUBCFG
# Legacy BIOS boot entry
menuentry "NexaOS (Legacy)" {
    multiboot2 /boot/kernel.elf $GRUB_CMDLINE
GRUBCFG

        if [ "$has_initramfs" = "1" ]; then
            echo "    module2 /boot/initramfs.cpio"
        fi

        cat <<'GRUBCFG'
    boot
}
GRUBCFG
    } > "$ISO_WORK_DIR/boot/grub/grub.cfg"
    
    log_success "GRUB config generated"
}

create_iso() {
    log_step "Creating ISO image..."
    
    grub-mkrescue -o "$ISO_FILE" "$ISO_WORK_DIR"
    
    if [ -f "$ISO_FILE" ]; then
        log_success "ISO created: $ISO_FILE ($(file_size "$ISO_FILE"))"
    else
        log_error "ISO creation failed"
        return 1
    fi
}

postprocess_esp() {
    # Only for UEFI builds
    if [ ! -f "$BUILD_DIR/BootX64.EFI" ]; then
        return 0
    fi
    
    require_cmd 7z "p7zip-full"
    
    log_step "Post-processing ESP..."
    
    local iso_temp="$BUILD_DIR/iso_extract_$$"
    rm -rf "$iso_temp"
    ensure_dir "$iso_temp"
    
    # Extract ISO
    log_info "Extracting ISO..."
    7z x -o"$iso_temp" "$ISO_FILE" -bsp0 -bso0 2>&1 | head -10 || true
    
    # Find efi.img
    local efi_img
    efi_img=$(find "$iso_temp" -name "efi.img" 2>/dev/null | head -1)
    
    if [ -n "$efi_img" ] && [ -f "$efi_img" ]; then
        log_info "Found ESP: $efi_img"
        
        # Calculate required ESP size
        local kernel_size bootloader_size initramfs_size total_bytes esp_size_mb
        kernel_size=$(stat -c%s "$KERNEL_BIN")
        bootloader_size=$(stat -c%s "$BUILD_DIR/BootX64.EFI")
        initramfs_size=0
        [ -f "$INITRAMFS_CPIO" ] && initramfs_size=$(stat -c%s "$INITRAMFS_CPIO")
        
        total_bytes=$((kernel_size + bootloader_size + initramfs_size))
        esp_size_mb=$(( (total_bytes * 120 / 100 + 1048575) / 1048576 ))
        [ "$esp_size_mb" -lt 16 ] && esp_size_mb=16
        
        log_info "Creating ${esp_size_mb}MB ESP..."
        
        local new_esp="$BUILD_DIR/new_efi.img"
        dd if=/dev/zero of="$new_esp" bs=1M count="$esp_size_mb" status=none
        mkfs.vfat -F 12 -n "UEFI" "$new_esp" >/dev/null 2>&1
        
        # Create directories and copy files
        mmd -i "$new_esp" ::/EFI 2>/dev/null || true
        mmd -i "$new_esp" ::/EFI/BOOT 2>/dev/null || true
        
        mcopy -i "$new_esp" "$BUILD_DIR/BootX64.EFI" "::/EFI/BOOT/BOOTX64.EFI"
        mcopy -i "$new_esp" "$KERNEL_BIN" "::/EFI/BOOT/KERNEL.ELF"
        
        if [ -f "$INITRAMFS_CPIO" ]; then
            mcopy -i "$new_esp" "$INITRAMFS_CPIO" "::/EFI/BOOT/INITRAMFS.CPIO"
        fi
        
        # Replace ESP
        chmod u+w "$efi_img"
        mv -f "$new_esp" "$efi_img"
        
        # Verify
        log_info "ESP contents:"
        mdir -i "$efi_img" ::/EFI/BOOT/
        
        # Rebuild ISO
        log_info "Rebuilding ISO..."
        rm -f "$ISO_FILE"
        grub-mkrescue -o "$ISO_FILE" "$iso_temp" 2>&1 | grep -v "xorriso.*WARNING" || true
        
        log_success "ESP modified successfully"
    else
        log_warn "Could not find efi.img"
    fi
    
    rm -rf "$iso_temp"
}

build_iso() {
    # Default to debug mode - must match kernel build mode
    local mode="${1:-debug}"
    
    log_section "Building Bootable ISO ($mode)"
    
    # Update kernel path based on mode
    KERNEL_BIN="$TARGET_DIR/x86_64-nexaos/$mode/nexa-os"
    
    if [ ! -f "$KERNEL_BIN" ]; then
        log_error "Kernel not found: $KERNEL_BIN"
        log_info "Run build-kernel.sh first"
        return 1
    fi
    
    check_dependencies
    setup_iso_structure
    
    local has_uefi=0
    copy_uefi_loader && has_uefi=1
    
    copy_grub_font
    
    local has_initramfs
    has_initramfs=$(copy_initramfs)
    
    generate_grub_config "$has_initramfs" "$has_uefi"
    create_iso
    
    [ "$has_uefi" = "1" ] && postprocess_esp
    
    log_success "ISO build complete: $ISO_FILE"
}

# ============================================================================
# Main
# ============================================================================

if [ "${BASH_SOURCE[0]}" == "${0}" ]; then
    # Default to debug mode to match kernel build
    build_iso "${1:-debug}"
fi
