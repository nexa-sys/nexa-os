#!/bin/bash
# Build kernel modules (.nkm files) for NexaOS
# 
# This script builds loadable kernel modules that can be loaded at runtime.
# For ELF-based modules, it compiles Rust code as a staticlib, then extracts
# the object file which contains relocatable code.
#
# Output: build/modules/*.nkm
# Installation: build/initramfs/lib/modules/*.nkm

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_ROOT/build"
MODULES_DIR="$BUILD_DIR/modules"
INITRAMFS_MODULES="$BUILD_DIR/initramfs/lib/modules"
TARGET_JSON="$PROJECT_ROOT/targets/x86_64-nexaos-module.json"
KERNEL_TARGET_JSON="$PROJECT_ROOT/targets/x86_64-nexaos.json"

echo "========================================"
echo "Building NexaOS Kernel Modules (.nkm)"
echo "========================================"

mkdir -p "$MODULES_DIR"
mkdir -p "$INITRAMFS_MODULES"

# Function to create a simple NKM metadata file
# Arguments: name, type (1=fs, 2=blk, 3=chr, 4=net), version, description, output_file
create_simple_nkm() {
    local name="$1"
    local type="$2"
    local version="$3"
    local description="$4"
    local output="$5"

    python3 << EOF
import struct

# NKM Header format (80 bytes total):
# 0-3:   magic "NKM\x01"
# 4:     version (u8)
# 5:     module_type (u8)
# 6:     dep_count (u8)
# 7:     flags (u8)
# 8-11:  code_offset (u32)
# 12-15: code_size (u32)
# 16-19: init_offset (u32)
# 20-23: init_size (u32)
# 24-31: reserved (8 bytes)
# 32-35: string_table_offset (u32)
# 36-39: string_table_size (u32)
# 40-71: name (32 bytes, null-terminated)
# 72-79: padding to 80 bytes

name = b"$name"
version_str = b"$version"
description_str = b"$description"
module_type = $type

string_table = version_str + b'\x00' + description_str + b'\x00'
header_size = 80
string_table_offset = header_size
string_table_size = len(string_table)

with open("$output", "wb") as f:
    f.write(b'NKM\x01')
    f.write(struct.pack('B', 1))  # format version
    f.write(struct.pack('B', module_type))
    f.write(struct.pack('B', 0))  # dep_count
    f.write(struct.pack('B', 0))  # flags
    f.write(struct.pack('<I', header_size))  # code_offset
    f.write(struct.pack('<I', string_table_size))  # code_size
    f.write(struct.pack('<I', header_size))  # init_offset
    f.write(struct.pack('<I', string_table_size))  # init_size
    f.write(b'\x00' * 8)  # reserved
    f.write(struct.pack('<I', string_table_offset))
    f.write(struct.pack('<I', string_table_size))
    name_padded = name[:31] + b'\x00' * (32 - min(len(name), 31))
    f.write(name_padded)
    f.write(b'\x00' * 8)  # padding
    f.write(string_table)

print(f"  Created simple NKM: $output ({header_size + len(string_table)} bytes)")
EOF
}

# Build ext2 filesystem module
build_ext2_module() {
    echo ""
    echo "Building ext2 filesystem module..."
    
    local MODULE_SRC="$PROJECT_ROOT/modules/ext2"
    
    if [ ! -d "$MODULE_SRC" ]; then
        echo "Error: ext2 module source not found at $MODULE_SRC"
        return 1
    fi
    
    cd "$MODULE_SRC"
    
    # Clean previous builds
    cargo clean 2>/dev/null || true
    
    # Build as staticlib using the kernel target
    # This will produce a .a file with relocatable object files
    # Use static relocation model to avoid GOT/PLT which requires special handling
    echo "  Compiling ext2 module as staticlib..."
    
    if RUSTFLAGS="-C relocation-model=static -C code-model=kernel -C panic=abort" \
        cargo +nightly build \
            -Z build-std=core,alloc,compiler_builtins \
            -Z build-std-features=compiler-builtins-mem \
            --target "$KERNEL_TARGET_JSON" \
            --release 2>&1; then
        
        echo "  Cargo build succeeded, looking for artifacts..."
        
        # Find the built staticlib
        local STATICLIB=$(find "$MODULE_SRC/target" -name "libext2_module.a" 2>/dev/null | head -1)
        
        if [ -n "$STATICLIB" ] && [ -f "$STATICLIB" ]; then
            echo "  Found staticlib: $STATICLIB"
            
            # Create a relocatable ELF by partially linking all objects from the staticlib
            # This includes compiler_builtins and core symbols the module needs
            cd "$MODULES_DIR"
            rm -f *.o ext2_combined.o 2>/dev/null || true
            
            # Extract all object files
            ar x "$STATICLIB" 2>/dev/null || true
            
            # Link all .o files into a single relocatable object with garbage collection
            # -r = relocatable, --gc-sections = remove unused sections
            local OBJ_FILES=$(ls -1 *.o 2>/dev/null | tr '\n' ' ')
            
            if [ -n "$OBJ_FILES" ]; then
                echo "  Linking $(echo $OBJ_FILES | wc -w) object files into relocatable module..."
                
                # Use ld to create a single relocatable object with gc-sections to strip unused
                ld.lld -r --gc-sections -o ext2.nkm $OBJ_FILES 2>/dev/null || \
                ld -r --gc-sections -o ext2.nkm $OBJ_FILES 2>/dev/null || \
                ld.lld -r -o ext2.nkm $OBJ_FILES 2>/dev/null || \
                ld -r -o ext2.nkm $OBJ_FILES 2>/dev/null || {
                    # Fallback: just use the main module object
                    local MAIN_OBJ=$(ls -1 *.o 2>/dev/null | grep -i 'ext2_module' | head -1)
                    if [ -n "$MAIN_OBJ" ]; then
                        mv "$MAIN_OBJ" "ext2.nkm"
                    fi
                }
                
                rm -f *.o 2>/dev/null || true  # Clean up extracted files
                
                if [ -f "ext2.nkm" ]; then
                    # Strip debug info to reduce size
                    strip --strip-debug ext2.nkm 2>/dev/null || true
                    echo "  ✓ Built ELF module: $MODULES_DIR/ext2.nkm ($(stat -c%s "$MODULES_DIR/ext2.nkm") bytes)"
                else
                    echo "  Error: Failed to create ext2.nkm"
                    return 1
                fi
            else
                echo "  Warning: No object files found in staticlib"
                return 1
            fi
        else
            echo "  Warning: No staticlib found, creating metadata-only module"
            create_simple_nkm "ext2" 1 "1.0.0" "ext2 filesystem driver (built-in)" "$MODULES_DIR/ext2.nkm"
        fi
    else
        echo "  Warning: Cargo build failed, creating metadata-only module"
        create_simple_nkm "ext2" 1 "1.0.0" "ext2 filesystem driver (built-in)" "$MODULES_DIR/ext2.nkm"
    fi
    
    # Install to initramfs
    cp "$MODULES_DIR/ext2.nkm" "$INITRAMFS_MODULES/ext2.nkm"
    echo "  ✓ Installed to initramfs: /lib/modules/ext2.nkm"
}

# Sign modules if signing key is available
sign_modules() {
    local SIGN_SCRIPT="$SCRIPT_DIR/sign-module.sh"
    local KEY_FILE="$PROJECT_ROOT/certs/signing_key.pem"
    
    if [ ! -x "$SIGN_SCRIPT" ]; then
        echo "  Note: Module signing script not found"
        return 0
    fi
    
    if [ ! -f "$KEY_FILE" ]; then
        echo "  Note: No signing key found at $KEY_FILE"
        echo "  Modules will be loaded unsigned (kernel will be tainted)"
        echo "  To generate a key: ./scripts/sign-module.sh --generate-key"
        return 0
    fi
    
    echo ""
    echo "Signing kernel modules..."
    
    for nkm in "$MODULES_DIR"/*.nkm; do
        if [ -f "$nkm" ]; then
            local name=$(basename "$nkm")
            echo "  Signing: $name"
            if "$SIGN_SCRIPT" -i "$nkm" 2>/dev/null; then
                echo "    ✓ Signed successfully"
                # Also update initramfs copy
                cp "$nkm" "$INITRAMFS_MODULES/$name"
            else
                echo "    ⚠ Signing failed (module will load unsigned)"
            fi
        fi
    done
}

# Build all modules
build_ext2_module

# Sign modules if key is available
sign_modules

echo ""
echo "========================================"
echo "Kernel Modules Build Complete"
echo "========================================"
echo ""
echo "Built modules:"
ls -lh "$MODULES_DIR"/*.nkm 2>/dev/null || echo "  (no modules)"
echo ""
echo "Initramfs modules:"
ls -lh "$INITRAMFS_MODULES"/*.nkm 2>/dev/null || echo "  (no modules installed)"
echo ""
echo "Note: Modules will be loaded automatically during kernel boot"
echo "      from /lib/modules/*.nkm in the initramfs."
echo ""
if [ -f "$PROJECT_ROOT/certs/signing_key.pem" ]; then
    echo "Module signing: ENABLED (key found)"
else
    echo "Module signing: DISABLED (no key - run ./scripts/sign-module.sh --generate-key)"
fi
