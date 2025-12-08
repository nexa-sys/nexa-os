#!/bin/bash
# NexaOS Build System - Kernel Modules Builder
# Build loadable kernel modules (.nkm)
#
# Module definitions are loaded from scripts/build-config.yaml

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"
source "$SCRIPT_DIR/../lib/config-parser.sh"

init_build_env

# ============================================================================
# Configuration
# ============================================================================

MODULES_BUILD_DIR="$BUILD_DIR/modules"
INITRAMFS_MODULES_DIR="$BUILD_DIR/initramfs/lib/modules"

# Load modules from YAML config
# Format: "name:type:description"
load_modules_array

# ============================================================================
# Helper Functions
# ============================================================================

create_simple_nkm() {
    local name="$1"
    local type="$2"
    local version="$3"
    local description="$4"
    local output="$5"

    python3 << EOF
import struct

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

print(f"Created simple NKM: $output ({header_size + len(string_table)} bytes)")
EOF
}

# ============================================================================
# Build Functions
# ============================================================================

build_module() {
    local name="$1"
    local type="$2"
    local description="$3"
    
    local module_src="$PROJECT_ROOT/modules/$name"
    local output_nkm="$MODULES_BUILD_DIR/$name.nkm"
    
    log_step "Building $name module..."
    
    if [ ! -d "$module_src" ]; then
        log_warn "Module source not found: $module_src"
        log_info "Creating metadata-only module"
        create_simple_nkm "$name" "$type" "1.0.0" "$description (built-in)" "$output_nkm"
        return 0
    fi
    
    cd "$module_src"
    
    # Clean previous builds
    cargo clean 2>/dev/null || true
    
    # Build as staticlib using kernel target
    log_info "Compiling $name module as staticlib..."
    
    local rustflags
    rustflags="$(get_module_rustflags)"
    
    if RUSTFLAGS="$rustflags" \
        cargo +nightly build \
            -Z build-std=core,alloc,compiler_builtins \
            -Z build-std-features=compiler-builtins-mem \
            --target "$TARGET_KERNEL" \
            --release 2>&1; then
        
        # Find the built staticlib
        local staticlib
        staticlib=$(find "$module_src/target" -name "lib${name}_module.a" 2>/dev/null | head -1)
        
        if [ -n "$staticlib" ] && [ -f "$staticlib" ]; then
            log_info "Found staticlib: $staticlib"
            
            # Create relocatable ELF by partially linking
            cd "$MODULES_BUILD_DIR"
            rm -f ./*.o 2>/dev/null || true
            
            # Extract object files
            ar x "$staticlib" 2>/dev/null || true
            
            local obj_files
            obj_files=$(ls -1 ./*.o 2>/dev/null | tr '\n' ' ')
            
            if [ -n "$obj_files" ]; then
                log_info "Linking $(echo "$obj_files" | wc -w) object files..."
                
                # shellcheck disable=SC2086
                ld.lld -r --gc-sections -o "$output_nkm" $obj_files 2>/dev/null || \
                ld -r --gc-sections -o "$output_nkm" $obj_files 2>/dev/null || \
                ld.lld -r -o "$output_nkm" $obj_files 2>/dev/null || \
                ld -r -o "$output_nkm" $obj_files 2>/dev/null || {
                    local main_obj
                    main_obj=$(ls -1 ./*.o 2>/dev/null | grep -i "${name}_module" | head -1)
                    if [ -n "$main_obj" ]; then
                        mv "$main_obj" "$output_nkm"
                    fi
                }
                
                rm -f ./*.o 2>/dev/null || true
                
                if [ -f "$output_nkm" ]; then
                    strip --strip-debug "$output_nkm" 2>/dev/null || true
                    log_success "$name.nkm built ($(file_size "$output_nkm"))"
                else
                    log_warn "Link failed, creating metadata-only module"
                    create_simple_nkm "$name" "$type" "1.0.0" "$description (built-in)" "$output_nkm"
                fi
            else
                log_warn "No object files found"
                create_simple_nkm "$name" "$type" "1.0.0" "$description (built-in)" "$output_nkm"
            fi
        else
            log_warn "No staticlib found"
            create_simple_nkm "$name" "$type" "1.0.0" "$description (built-in)" "$output_nkm"
        fi
    else
        log_warn "Cargo build failed"
        create_simple_nkm "$name" "$type" "1.0.0" "$description (built-in)" "$output_nkm"
    fi
    
    # Install to initramfs
    ensure_dir "$INITRAMFS_MODULES_DIR"
    cp "$output_nkm" "$INITRAMFS_MODULES_DIR/"
    log_info "Installed to initramfs: /lib/modules/$name.nkm"
}

sign_modules() {
    local sign_script="$PROJECT_ROOT/scripts/sign-module.sh"
    local key_file="$PROJECT_ROOT/certs/signing_key.pem"
    
    if [ ! -x "$sign_script" ] || [ ! -f "$key_file" ]; then
        log_info "Module signing skipped (no key or script)"
        return 0
    fi
    
    log_step "Signing kernel modules..."
    
    for nkm in "$MODULES_BUILD_DIR"/*.nkm; do
        if [ -f "$nkm" ]; then
            local name
            name=$(basename "$nkm")
            if "$sign_script" -i "$nkm" 2>/dev/null; then
                log_success "Signed: $name"
                cp "$nkm" "$INITRAMFS_MODULES_DIR/$name"
            else
                log_warn "Failed to sign: $name"
            fi
        fi
    done
}

build_all_modules() {
    log_section "Building Kernel Modules"
    
    ensure_dir "$MODULES_BUILD_DIR" "$INITRAMFS_MODULES_DIR"
    
    for entry in "${MODULES[@]}"; do
        IFS=':' read -r name type desc <<< "$entry"
        build_module "$name" "$type" "$desc"
    done
    
    sign_modules
    
    log_success "All modules built"
    echo ""
    log_info "Built modules:"
    ls -lh "$MODULES_BUILD_DIR"/*.nkm 2>/dev/null || echo "  (none)"
}

# ============================================================================
# Main
# ============================================================================

if [ "${BASH_SOURCE[0]}" == "${0}" ]; then
    case "${1:-all}" in
        all)
            build_all_modules
            ;;
        single)
            if [ -z "$2" ]; then
                echo "Usage: $0 single <module_name>"
                exit 1
            fi
            for entry in "${MODULES[@]}"; do
                IFS=':' read -r name type desc <<< "$entry"
                if [ "$name" = "$2" ]; then
                    ensure_dir "$MODULES_BUILD_DIR" "$INITRAMFS_MODULES_DIR"
                    build_module "$name" "$type" "$desc"
                    exit $?
                fi
            done
            log_error "Unknown module: $2"
            exit 1
            ;;
        list)
            echo "Available modules:"
            for entry in "${MODULES[@]}"; do
                IFS=':' read -r name type desc <<< "$entry"
                echo "  $name (type $type): $desc"
            done
            ;;
        sign)
            sign_modules
            ;;
        *)
            echo "Usage: $0 {all|single|list|sign} [args...]"
            exit 1
            ;;
    esac
fi
