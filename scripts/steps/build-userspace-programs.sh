#!/bin/bash
# NexaOS Build System - Userspace Programs Builder
# Build userspace binaries for rootfs

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"

init_build_env

# ============================================================================
# Configuration
# ============================================================================

USERSPACE_BUILD_DIR="$BUILD_DIR/userspace-build"
SYSROOT_LIB="$USERSPACE_BUILD_DIR/sysroot/lib"
SYSROOT_PIC_LIB="$USERSPACE_BUILD_DIR/sysroot-pic/lib"

# Programs to build with std
STD_PROGRAMS=(
    "ni:sbin:init.rs:--no-default-features"
    "getty:sbin:getty.rs:--no-default-features"
    "sh:bin:shell.rs:"
    "login:bin:login.rs:"
    "nslookup:bin:nslookup.rs:--no-default-features --features use-nrlib-std"
    "uefi-compatd:sbin:uefi_compatd.rs:--no-default-features --features use-nrlib-std"
    "ip:bin:ip.rs:"
    "dhcp:bin:dhcp.rs:"
    "nurl:bin:nurl.rs:"
    "dmesg:bin:dmesg.rs:"
    "crashtest:bin:crashtest.rs:"
)

# Programs to build with dynamic linking
DYN_PROGRAMS=(
    "hello:bin:hello_dynamic.rs:"
)

# ============================================================================
# Setup
# ============================================================================

setup_userspace_cargo() {
    log_step "Setting up userspace build environment..."
    
    ensure_dir "$USERSPACE_BUILD_DIR"
    
    cat > "$USERSPACE_BUILD_DIR/Cargo.toml" << 'EOF'
[package]
name = "userspace"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "ni"
path = "../../userspace/init.rs"
required-features = []

[[bin]]
name = "sh"
path = "../../userspace/shell.rs"

[[bin]]
name = "getty"
path = "../../userspace/getty.rs"
required-features = []

[[bin]]
name = "login"
path = "../../userspace/login.rs"

[[bin]]
name = "nslookup"
path = "../../userspace/nslookup.rs"

[[bin]]
name = "dhcp"
path = "../../userspace/dhcp.rs"

[[bin]]
name = "uefi-compatd"
path = "../../userspace/uefi_compatd.rs"

[[bin]]
name = "ip"
path = "../../userspace/ip.rs"

[[bin]]
name = "nurl"
path = "../../userspace/nurl.rs"

[[bin]]
name = "hello"
path = "../../userspace/hello_dynamic.rs"

[[bin]]
name = "dmesg"
path = "../../userspace/dmesg.rs"

[[bin]]
name = "crashtest"
path = "../../userspace/crashtest.rs"

[profile.release]
panic = "abort"
opt-level = 2
lto = false

[dependencies]
nrlib = { path = "../../userspace/nrlib", optional = true, default-features = false }
nexa_boot_info = { path = "../../boot/boot-info" }

[features]
default = ["use-nrlib"]
use-nrlib = ["nrlib", "nrlib/panic-handler"]
use-nrlib-std = ["nrlib", "nrlib/std"]
use-std = ["nrlib"]
EOF
}

# ============================================================================
# Build Functions
# ============================================================================

build_std_program() {
    local name="$1"
    local dest_subdir="$2"
    local extra_args="$3"
    
    log_step "Building $name (std)..."
    
    cd "$USERSPACE_BUILD_DIR"
    
    local rustflags
    rustflags="$(get_std_rustflags "$SYSROOT_LIB")"
    
    # shellcheck disable=SC2086
    RUSTFLAGS="$rustflags" \
        cargo build -Z build-std=std,panic_abort \
            --target "$TARGET_USERSPACE" \
            --release \
            --bin "$name" \
            $extra_args
    
    return $?
}

build_dyn_program() {
    local name="$1"
    local dest_subdir="$2"
    local extra_args="$3"
    
    log_step "Building $name (dynamic)..."
    
    cd "$USERSPACE_BUILD_DIR"
    
    local rustflags
    # Use PIC sysroot for dynamic/PIE programs
    rustflags="$(get_dyn_rustflags "$SYSROOT_PIC_LIB")"
    
    # shellcheck disable=SC2086
    RUSTFLAGS="$rustflags" \
        cargo build -Z build-std=std,panic_abort \
            --target "$TARGET_USERSPACE_DYN" \
            --release \
            --bin "$name" \
            $extra_args
    
    return $?
}

install_program() {
    local name="$1"
    local dest_subdir="$2"
    local dest_dir="$3"
    local target="${4:-x86_64-nexaos-userspace}"
    
    local src="$USERSPACE_BUILD_DIR/target/$target/release/$name"
    local dst="$dest_dir/$dest_subdir/$name"
    
    ensure_dir "$dest_dir/$dest_subdir"
    
    if [ -f "$src" ]; then
        cp "$src" "$dst"
        strip --strip-all "$dst" 2>/dev/null || true
        chmod 755 "$dst"
        log_success "$name installed to /$dest_subdir ($(file_size "$dst"))"
        return 0
    else
        log_error "Failed to build $name"
        return 1
    fi
}

# ============================================================================
# Main Build Functions
# ============================================================================

build_all_std_programs() {
    local dest_dir="$1"
    local failed=0
    
    for entry in "${STD_PROGRAMS[@]}"; do
        IFS=':' read -r name subdir source extra <<< "$entry"
        
        if build_std_program "$name" "$subdir" "$extra"; then
            install_program "$name" "$subdir" "$dest_dir"
        else
            ((failed++))
        fi
    done
    
    return $failed
}

build_all_dyn_programs() {
    local dest_dir="$1"
    local failed=0
    
    for entry in "${DYN_PROGRAMS[@]}"; do
        IFS=':' read -r name subdir source extra <<< "$entry"
        
        if build_dyn_program "$name" "$subdir" "$extra"; then
            install_program "$name" "$subdir" "$dest_dir" "x86_64-nexaos-userspace-dynamic"
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
    
    setup_userspace_cargo
    
    build_all_std_programs "$dest_dir"
    build_all_dyn_programs "$dest_dir"
    
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
    
    setup_userspace_cargo
    
    # Find program in lists
    for entry in "${STD_PROGRAMS[@]}"; do
        IFS=':' read -r prog_name subdir source extra <<< "$entry"
        if [ "$prog_name" = "$name" ]; then
            build_std_program "$name" "$subdir" "$extra"
            install_program "$name" "$subdir" "$dest_dir"
            return $?
        fi
    done
    
    for entry in "${DYN_PROGRAMS[@]}"; do
        IFS=':' read -r prog_name subdir source extra <<< "$entry"
        if [ "$prog_name" = "$name" ]; then
            build_dyn_program "$name" "$subdir" "$extra"
            install_program "$name" "$subdir" "$dest_dir" "x86_64-nexaos-userspace-dynamic"
            return $?
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
            echo "Available std programs:"
            for entry in "${STD_PROGRAMS[@]}"; do
                IFS=':' read -r name subdir _ _ <<< "$entry"
                echo "  $name (/$subdir)"
            done
            echo ""
            echo "Available dynamic programs:"
            for entry in "${DYN_PROGRAMS[@]}"; do
                IFS=':' read -r name subdir _ _ <<< "$entry"
                echo "  $name (/$subdir)"
            done
            ;;
        *)
            echo "Usage: $0 {all|single|list} [args...]"
            exit 1
            ;;
    esac
fi
