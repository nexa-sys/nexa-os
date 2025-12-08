#!/bin/bash
# NexaOS Build System - Userspace Programs Builder
# Build userspace binaries for rootfs using workspace structure
#
# Program definitions are loaded from scripts/build-config.yaml

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"
source "$SCRIPT_DIR/../lib/config-parser.sh"

init_build_env

# ============================================================================
# Configuration
# ============================================================================

USERSPACE_DIR="$PROJECT_ROOT/userspace"
SYSROOT_LIB="$BUILD_DIR/userspace-build/sysroot/lib"
SYSROOT_PIC_LIB="$BUILD_DIR/userspace-build/sysroot-pic/lib"

# Load programs from YAML config
# Format: "package:binary:dest:features:link"
load_programs_array

# ============================================================================
# Build Functions
# ============================================================================

build_program() {
    local package="$1"
    local binary="$2"
    local features="$3"
    local link_type="$4"
    
    local target
    local rustflags
    local extra_args=""
    
    # Convert features to cargo argument
    if [ -n "$features" ]; then
        extra_args="--features $features"
    fi
    
    if [ "$link_type" = "dyn" ]; then
        target="$TARGET_USERSPACE_DYN"
        rustflags="$(get_dyn_rustflags "$SYSROOT_PIC_LIB")"
    else
        target="$TARGET_USERSPACE"
        rustflags="$(get_std_rustflags "$SYSROOT_LIB")"
    fi
    
    log_step "Building $package ($link_type)..."
    
    cd "$USERSPACE_DIR"
    
    # shellcheck disable=SC2086
    RUSTFLAGS="$rustflags" \
        cargo build -Z build-std=std,panic_abort \
            --target "$target" \
            --release \
            --package "$package" \
            $extra_args
    
    return $?
}

install_program() {
    local package="$1"
    local binary="$2"
    local dest_subdir="$3"
    local dest_dir="$4"
    local link_type="$5"
    
    local target_name
    if [ "$link_type" = "dyn" ]; then
        target_name="x86_64-nexaos-userspace-dynamic"
    else
        target_name="x86_64-nexaos-userspace"
    fi
    
    local src="$USERSPACE_DIR/target/$target_name/release/$binary"
    local dst="$dest_dir/$dest_subdir/$binary"
    
    ensure_dir "$dest_dir/$dest_subdir"
    
    if [ -f "$src" ]; then
        cp "$src" "$dst"
        strip --strip-all "$dst" 2>/dev/null || true
        chmod 755 "$dst"
        log_success "$binary installed to /$dest_subdir ($(file_size "$dst"))"
        return 0
    else
        log_error "Failed to find $binary at $src"
        return 1
    fi
}

# ============================================================================
# Main Build Functions
# ============================================================================

build_all_programs() {
    local dest_dir="$1"
    local failed=0
    
    for entry in "${PROGRAMS[@]}"; do
        IFS=':' read -r package binary dest features link_type <<< "$entry"
        
        if build_program "$package" "$binary" "$features" "$link_type"; then
            install_program "$package" "$binary" "$dest" "$dest_dir" "$link_type"
        else
            ((failed++))
        fi
    done
    
    return $failed
}

build_userspace_programs() {
    local dest_dir="${1:-$BUILD_DIR/rootfs}"
    
    log_section "Building Userspace Programs"
    
    # Ensure nrlib is built first (both static and PIC versions)
    if [ ! -f "$SYSROOT_LIB/libc.a" ] || [ ! -f "$SYSROOT_PIC_LIB/libc.a" ]; then
        log_info "Building nrlib first..."
        bash "$SCRIPT_DIR/build-nrlib.sh" all
    fi
    
    build_all_programs "$dest_dir"
    
    log_success "All userspace programs built"
}

# Build a single program
build_single() {
    local name="$1"
    local dest_dir="${2:-$BUILD_DIR/rootfs}"
    
    # Ensure nrlib is built first (both static and PIC versions)
    if [ ! -f "$SYSROOT_LIB/libc.a" ] || [ ! -f "$SYSROOT_PIC_LIB/libc.a" ]; then
        log_info "Building nrlib first..."
        bash "$SCRIPT_DIR/build-nrlib.sh" all
    fi
    
    # Find program in list
    for entry in "${PROGRAMS[@]}"; do
        IFS=':' read -r package binary dest features link_type <<< "$entry"
        if [ "$package" = "$name" ] || [ "$binary" = "$name" ]; then
            if build_program "$package" "$binary" "$features" "$link_type"; then
                install_program "$package" "$binary" "$dest" "$dest_dir" "$link_type"
                return $?
            fi
            return 1
        fi
    done
    
    log_error "Unknown program: $name"
    return 1
}

# ============================================================================
# Main
# ============================================================================

if [ "${BASH_SOURCE[0]}" == "${0}" ]; then
    case "${1:-all}" in
        all)
            build_userspace_programs "${2:-}"
            ;;
        single)
            if [ -z "$2" ]; then
                echo "Usage: $0 single <program_name> [dest_dir]"
                exit 1
            fi
            build_single "$2" "${3:-}"
            ;;
        list)
            echo "Available programs:"
            for entry in "${PROGRAMS[@]}"; do
                IFS=':' read -r package binary dest _ link_type <<< "$entry"
                echo "  $package -> $binary (/$dest) [$link_type]"
            done
            ;;
        *)
            echo "Usage: $0 {all|single|list} [args...]"
            exit 1
            ;;
    esac
fi
