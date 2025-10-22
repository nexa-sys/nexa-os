#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ISO_PATH="$ROOT_DIR/dist/nexaos.iso"

if [[ ! -f "$ISO_PATH" ]]; then
    echo "ISO image not found at $ISO_PATH. Run scripts/build-iso.sh first." >&2
    exit 1
fi

qemu-system-x86_64 \
    -serial stdio \
    -cdrom "$ISO_PATH" \
    -d guest_errors \
    -no-reboot \
    -monitor none
