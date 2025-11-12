#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="x86_64-unknown-uefi"
PROFILE="release"
OUTPUT_DIR="$ROOT_DIR/build"
EFI_OUTPUT="$OUTPUT_DIR/BootX64.EFI"

if ! command -v rustup >/dev/null 2>&1; then
    echo "Error: rustup is required to build the UEFI loader." >&2
    exit 1
fi

rustup component add llvm-tools-preview >/dev/null 2>&1 || true

LLVM_OBJCOPY=""
if command -v llvm-objcopy >/dev/null 2>&1; then
    LLVM_OBJCOPY="$(command -v llvm-objcopy)"
else
    LLVM_OBJCOPY="$(rustup which llvm-objcopy 2>/dev/null || true)"
    if [ -z "$LLVM_OBJCOPY" ]; then
        LLVM_OBJCOPY="$(rustup which --toolchain nightly llvm-objcopy 2>/dev/null || true)"
    fi
fi

if [ -z "$LLVM_OBJCOPY" ]; then
    echo "Error: llvm-objcopy not found. Install binutils or run 'rustup component add llvm-tools-preview'." >&2
    exit 1
fi

cargo build --manifest-path "$ROOT_DIR/boot/uefi-loader/Cargo.toml" --target "$TARGET" --$PROFILE

INPUT="$ROOT_DIR/boot/uefi-loader/target/$TARGET/$PROFILE/nexa-uefi-loader.so"
if [ ! -f "$INPUT" ]; then
    INPUT="$ROOT_DIR/boot/uefi-loader/target/$TARGET/$PROFILE/nexa-uefi-loader.dll"
fi
if [ ! -f "$INPUT" ]; then
    INPUT="$ROOT_DIR/boot/uefi-loader/target/$TARGET/$PROFILE/nexa-uefi-loader.dylib"
fi

if [ ! -f "$INPUT" ]; then
    echo "Error: compiled UEFI loader artifact not found (expected .so/.dll/.dylib)." >&2
    exit 1
fi

mkdir -p "$OUTPUT_DIR"

"$LLVM_OBJCOPY" --strip-debug --target=efi-app-x86_64 "$INPUT" "$EFI_OUTPUT"

echo "UEFI loader built at $EFI_OUTPUT"
