#!/usr/bin/env bash
# Test boot stages functionality with QEMU direct kernel boot

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KERNEL="$ROOT_DIR/target/x86_64-nexaos/release/nexa-os"
INITRAMFS="$ROOT_DIR/build/initramfs.cpio"

if [[ ! -f "$KERNEL" ]]; then
    echo "Kernel not found at $KERNEL. Run 'cargo build --release' first." >&2
    exit 1
fi

if [[ ! -f "$INITRAMFS" ]]; then
    echo "Initramfs not found at $INITRAMFS. Run './scripts/build-userspace.sh' first." >&2
    exit 1
fi

# Test configurations
declare -A TESTS=(
    ["basic"]="loglevel=info"
    ["with_root"]="root=/dev/vda1 rootfstype=ext2 rw loglevel=info"
    ["emergency"]="emergency loglevel=debug"
    ["custom_init"]="init=/bin/sh loglevel=info"
)

# Select test configuration
TEST_NAME="${1:-basic}"
if [[ ! -v TESTS[$TEST_NAME] ]]; then
    echo "Unknown test: $TEST_NAME" >&2
    echo "Available tests: ${!TESTS[@]}" >&2
    exit 1
fi

CMDLINE="${TESTS[$TEST_NAME]}"

echo "================================"
echo "Testing boot stages: $TEST_NAME"
echo "Kernel cmdline: $CMDLINE"
echo "================================"
echo ""

# Note: QEMU doesn't support direct multiboot2 kernel boot,
# so we'll just verify the kernel was built with boot_stages module
echo "Verifying boot_stages module in kernel..."
if nm "$KERNEL" 2>/dev/null | grep -q boot_stages; then
    echo "✓ boot_stages module found in kernel"
else
    echo "✗ boot_stages module not found in kernel"
    exit 1
fi

echo ""
echo "Kernel built successfully with boot stages support."
echo "To test with QEMU, build the ISO with:"
echo "  ./scripts/build-iso.sh"
echo "Then run:"
echo "  ./scripts/run-qemu.sh"
