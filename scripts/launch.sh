#!/bin/bash
# NexaOS Quick Launch Script with UEFI/Legacy BIOS options

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
ISO_PATH="$PROJECT_ROOT/dist/nexaos.iso"

show_help() {
    echo "NexaOS Quick Launch Script"
    echo ""
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  -u, --uefi        Boot in UEFI mode (requires OVMF firmware)"
    echo "  -l, --legacy      Boot in Legacy BIOS mode (default)"
    echo "  -b, --build       Rebuild kernel and ISO before launching"
    echo "  -s, --serial      Show serial output in terminal"
    echo "  -g, --graphics    Use SDL graphics (default)"
    echo "  -n, --nogfx       No graphics, serial only"
    echo "  -h, --help        Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0                      # Quick launch (Legacy BIOS)"
    echo "  $0 -u                   # UEFI mode"
    echo "  $0 -b -u                # Rebuild and boot UEFI"
    echo "  $0 -l -n                # Legacy mode, serial console only"
    echo ""
}

# Default options
BOOT_MODE="legacy"
DO_BUILD=false
DISPLAY_MODE="graphics"

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -u|--uefi)
            BOOT_MODE="uefi"
            shift
            ;;
        -l|--legacy)
            BOOT_MODE="legacy"
            shift
            ;;
        -b|--build)
            DO_BUILD=true
            shift
            ;;
        -s|--serial)
            DISPLAY_MODE="serial"
            shift
            ;;
        -g|--graphics)
            DISPLAY_MODE="graphics"
            shift
            ;;
        -n|--nogfx)
            DISPLAY_MODE="none"
            shift
            ;;
        -h|--help)
            show_help
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
done

# Build if requested
if [ "$DO_BUILD" = true ]; then
    echo "==> Building NexaOS..."
    cd "$PROJECT_ROOT"
    ./scripts/build-iso.sh || {
        echo "Build failed!"
        exit 1
    }
fi

# Check if ISO exists
if [ ! -f "$ISO_PATH" ]; then
    echo "Error: ISO not found at $ISO_PATH"
    echo "Run with -b flag to build, or run ./scripts/build-iso.sh manually"
    exit 1
fi

echo "==> Launching NexaOS"
echo "    Boot Mode: $BOOT_MODE"
echo "    Display: $DISPLAY_MODE"
echo "    ISO: $ISO_PATH"
echo ""

# Construct QEMU command
QEMU_CMD="qemu-system-x86_64"
QEMU_ARGS=()

# Boot mode specific settings
if [ "$BOOT_MODE" = "uefi" ]; then
    OVMF_CODE="/usr/share/OVMF/OVMF_CODE_4M.fd"
    OVMF_VARS="$HOME/OVMF_VARS.fd"
    
    if [ ! -f "$OVMF_CODE" ]; then
        echo "Error: OVMF firmware not found at $OVMF_CODE"
        echo "Install OVMF package: sudo apt install ovmf"
        exit 1
    fi
    
    # Create OVMF_VARS if it doesn't exist
    if [ ! -f "$OVMF_VARS" ]; then
        cp "/usr/share/OVMF/OVMF_VARS_4M.fd" "$OVMF_VARS" 2>/dev/null || \
        cp "/usr/share/OVMF/OVMF_VARS.fd" "$OVMF_VARS" 2>/dev/null || {
            echo "Warning: Could not create OVMF_VARS.fd"
        }
    fi
    
    QEMU_ARGS+=(-bios "$OVMF_CODE")
    QEMU_ARGS+=(-drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE")
    if [ -f "$OVMF_VARS" ]; then
        QEMU_ARGS+=(-drive "if=pflash,format=raw,file=$OVMF_VARS")
    fi
fi

# Display mode settings
case $DISPLAY_MODE in
    graphics)
        QEMU_ARGS+=(-vga std)
        QEMU_ARGS+=(-display sdl,gl=off)
        QEMU_ARGS+=(-serial stdio)
        ;;
    serial)
        QEMU_ARGS+=(-nographic)
        QEMU_ARGS+=(-serial mon:stdio)
        ;;
    none)
        QEMU_ARGS+=(-nographic)
        QEMU_ARGS+=(-serial stdio)
        ;;
esac

# Common settings
QEMU_ARGS+=(-cdrom "$ISO_PATH")
QEMU_ARGS+=(-m 512M)
QEMU_ARGS+=(-smp 1)

# Launch QEMU
echo "==> Starting QEMU..."
echo "    Command: $QEMU_CMD ${QEMU_ARGS[*]}"
echo ""
echo "    Press Ctrl-A X to exit QEMU"
echo "    Press Ctrl-C to interrupt (shell)"
echo ""

exec "$QEMU_CMD" "${QEMU_ARGS[@]}"
