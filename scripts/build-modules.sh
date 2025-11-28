#!/bin/bash
# Build kernel modules (.nkm files) for NexaOS
# This script generates .nkm module files that can be loaded by the kernel

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_ROOT/build"
MODULES_DIR="$BUILD_DIR/modules"

echo "========================================"
echo "Building NexaOS kernel modules (.nkm)"
echo "========================================"

mkdir -p "$MODULES_DIR"

# NKM file format constants
NKM_MAGIC="NKM"
NKM_VERSION=1

# Function to create an NKM file
# Arguments: name, type (1=fs, 2=blk, 3=chr, 4=net, 255=other), version, description, output_file
create_nkm() {
    local name="$1"
    local type="$2"
    local version="$3"
    local description="$4"
    local output="$5"

    echo "Creating module: $name (type=$type, version=$version)"

    # Create a temporary file for the NKM
    local tmpfile=$(mktemp)

    # Write NKM header using Python for precise binary control
    python3 << EOF
import struct

# Header format (80 bytes total):
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

# Calculate string table
string_table = version_str + b'\x00' + description_str + b'\x00'
header_size = 80  # Total header is 80 bytes
string_table_offset = header_size
string_table_size = len(string_table)

with open("$tmpfile", "wb") as f:
    # Magic
    f.write(b'NKM\x01')
    # Version (format version, not module version)
    f.write(struct.pack('B', 1))
    # Module type
    f.write(struct.pack('B', module_type))
    # Dep count
    f.write(struct.pack('B', 0))
    # Flags
    f.write(struct.pack('B', 0))
    # Code offset (points to string table for now since we don't have code)
    f.write(struct.pack('<I', header_size))
    # Code size
    f.write(struct.pack('<I', string_table_size))
    # Init offset
    f.write(struct.pack('<I', header_size))
    # Init size
    f.write(struct.pack('<I', string_table_size))
    # Reserved
    f.write(b'\x00' * 8)
    # String table offset and size
    f.write(struct.pack('<I', string_table_offset))
    f.write(struct.pack('<I', string_table_size))
    # Module name (32 bytes, null-padded)
    name_padded = name[:31] + b'\x00' * (32 - min(len(name), 31))
    f.write(name_padded)
    # Padding to reach 80 bytes (we're at 72 bytes after name)
    f.write(b'\x00' * 8)
    # String table
    f.write(string_table)

print(f"Created NKM file: $output ({header_size + len(string_table)} bytes)")
EOF

    mv "$tmpfile" "$output"
    echo "✓ Module $name created: $output"
}

# Build ext2 filesystem module
echo ""
echo "Building ext2 filesystem module..."
create_nkm "ext2" 1 "1.0.0" "ext2 filesystem driver" "$MODULES_DIR/ext2.nkm"

# Future modules can be added here:
# create_nkm "ext4" 1 "1.0.0" "ext4 filesystem driver" "$MODULES_DIR/ext4.nkm"
# create_nkm "virtio_blk" 2 "1.0.0" "VirtIO block device driver" "$MODULES_DIR/virtio_blk.nkm"
# create_nkm "e1000" 4 "1.0.0" "Intel e1000 network driver" "$MODULES_DIR/e1000.nkm"

echo ""
echo "========================================"
echo "Kernel modules built successfully!"
echo "========================================"
echo ""
echo "Modules:"
ls -lh "$MODULES_DIR"/*.nkm 2>/dev/null || echo "  (no modules built)"
echo ""
echo "To include modules in initramfs:"
echo "  1. Run ./scripts/build-userspace.sh"
echo "  2. Run ./scripts/build-iso.sh"
