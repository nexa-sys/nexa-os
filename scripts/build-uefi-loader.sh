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

resolve_llvm_objcopy() {
    local candidate=""

    if command -v llvm-objcopy >/dev/null 2>&1; then
        candidate="$(command -v llvm-objcopy)"
    fi

    if [ -n "$candidate" ]; then
        printf '%s' "$candidate"
        return 0
    fi

    if ! command -v rustup >/dev/null 2>&1; then
        return 0
    fi

    local host_triple=""
    if command -v rustc >/dev/null 2>&1; then
        host_triple="$(rustc -vV 2>/dev/null | awk '/^host: / {print $2; exit}')"
    fi

    local toolchain_list=()
    if [ -n "${RUSTUP_TOOLCHAIN:-}" ]; then
        toolchain_list+=("$RUSTUP_TOOLCHAIN")
    fi
    local active_toolchain="$(rustup show active-toolchain 2>/dev/null | awk '{print $1}')"
    if [ -n "$active_toolchain" ]; then
        toolchain_list+=("$active_toolchain")
    fi
    if [ -n "$host_triple" ]; then
        toolchain_list+=("nightly-$host_triple")
    fi
    toolchain_list+=(nightly stable)

    local seen=""
    for tc in "${toolchain_list[@]}"; do
        if [ -z "$tc" ]; then
            continue
        fi
        case "$seen" in
            *"|$tc|"*) continue ;;
        esac
        seen+="|$tc|"

        candidate="$(rustup which --toolchain "$tc" llvm-objcopy 2>/dev/null || true)"
        if [ -n "$candidate" ] && [ -x "$candidate" ]; then
            printf '%s' "$candidate"
            return 0
        fi

        if [ -n "$host_triple" ]; then
            local rustc_path="$(rustup which --toolchain "$tc" rustc 2>/dev/null || true)"
            if [ -n "$rustc_path" ]; then
                local toolchain_bin="$(dirname "$rustc_path")"
                local alt_dir="$toolchain_bin/../lib/rustlib/$host_triple/bin"
                if [ -d "$alt_dir" ]; then
                    local alt_path="$alt_dir/llvm-objcopy"
                    if [ -x "$alt_path" ]; then
                        printf '%s' "$alt_path"
                        return 0
                    fi
                fi
            fi
        fi
    done

    printf '%s' ""
}

LLVM_OBJCOPY="$(resolve_llvm_objcopy)"

if [ -z "$LLVM_OBJCOPY" ]; then
    echo "Error: llvm-objcopy not found. Install binutils or run 'rustup component add llvm-tools-preview'." >&2
    exit 1
fi

BUILD_STD_ARGS=(-Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem)
cargo build "${BUILD_STD_ARGS[@]}" --manifest-path "$ROOT_DIR/boot/uefi-loader/Cargo.toml" --target "$TARGET" --$PROFILE

INPUT_DIR="$ROOT_DIR/boot/uefi-loader/target/$TARGET/$PROFILE"
INPUT="$INPUT_DIR/nexa-uefi-loader.efi"
if [ ! -f "$INPUT" ]; then
    INPUT="$INPUT_DIR/nexa-uefi-loader"
fi
if [ ! -f "$INPUT" ]; then
    INPUT="$INPUT_DIR/nexa-uefi-loader.so"
fi
if [ ! -f "$INPUT" ]; then
    INPUT="$INPUT_DIR/nexa-uefi-loader.dll"
fi
if [ ! -f "$INPUT" ]; then
    INPUT="$INPUT_DIR/nexa-uefi-loader.dylib"
fi

if [ ! -f "$INPUT" ]; then
    echo "Error: compiled UEFI loader artifact not found (expected .efi or shared library)." >&2
    exit 1
fi

mkdir -p "$OUTPUT_DIR"

if [[ "$INPUT" == *.efi ]]; then
    cp "$INPUT" "$EFI_OUTPUT"
else
    "$LLVM_OBJCOPY" --strip-debug --output-target=efi-app-x86_64 "$INPUT" "$EFI_OUTPUT"
fi

echo "UEFI loader built at $EFI_OUTPUT"
