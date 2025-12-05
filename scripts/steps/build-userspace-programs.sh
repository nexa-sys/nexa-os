#!/bin/bash
# NexaOS Build System - Userspace Programs Builder
# Build userspace binaries for rootfs using workspace structure

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"

init_build_env

# ============================================================================
# Configuration
# ============================================================================

USERSPACE_DIR="$PROJECT_ROOT/userspace"
SYSROOT_LIB="$BUILD_DIR/userspace-build/sysroot/lib"
SYSROOT_PIC_LIB="$BUILD_DIR/userspace-build/sysroot-pic/lib"

# Programs definition: package_name:binary_name:dest_subdir:extra_args:link_type
# link_type: std (static linking) or dyn (dynamic linking)
PROGRAMS=(
    "ni:ni:sbin::std"
    "getty:getty:sbin::std"
    "sh:sh:bin::dyn"
    "login:login:bin::dyn"
    "nslookup:nslookup:bin:--features use-nrlib-std:dyn"
    "uefi_compatd:uefi-compatd:sbin:--features use-nrlib-std:dyn"
    "ip:ip:bin::dyn"
    "dhcp:dhcp:bin::dyn"
    "ntpd:ntpd:sbin::dyn"
    "nurl:nurl:bin::dyn"
    "dmesg:dmesg:bin::dyn"
    "crashtest:crashtest:bin::dyn"
    "thread_test:thread_test:bin::dyn"
    "pthread_test:pthread_test:bin:--features use-nrlib-std:dyn"
    "hello_dynamic:hello:bin::dyn"
)

# ============================================================================
# Build Functions
# ============================================================================

build_program() {
    local package="$1"
    local binary="$2"
    local extra_args="$3"
    local link_type="$4"
    
    local target
    local rustflags
    
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
        IFS=':' read -r package binary subdir extra link_type <<< "$entry"
        
        if build_program "$package" "$binary" "$extra" "$link_type"; then
            install_program "$package" "$binary" "$subdir" "$dest_dir" "$link_type"
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
        IFS=':' read -r package binary subdir extra link_type <<< "$entry"
        if [ "$package" = "$name" ] || [ "$binary" = "$name" ]; then
            if build_program "$package" "$binary" "$extra" "$link_type"; then
                install_program "$package" "$binary" "$subdir" "$dest_dir" "$link_type"
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
                IFS=':' read -r package binary subdir _ link_type <<< "$entry"
                echo "  $package -> $binary (/$subdir) [$link_type]"
            done
            ;;
        *)
            echo "Usage: $0 {all|single|list} [args...]"
            exit 1
            ;;
    esac
fi
