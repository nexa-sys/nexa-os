#!/bin/bash
# NexaOS Build System - Unified Library Builder
# Build NexaOS userspace libraries (libcrypto, libssl, etc.)
#
# Usage:
#   ./build-libs.sh                    # Build all libraries
#   ./build-libs.sh ncryptolib         # Build specific library
#   ./build-libs.sh nssl               # Build specific library
#   ./build-libs.sh ncryptolib static  # Build only static lib
#   ./build-libs.sh nssl shared        # Build only shared lib
#   ./build-libs.sh list               # List available libraries
#
# Adding new libraries:
#   1. Add entry to LIBRARIES array below
#   2. Library must have Cargo.toml with cdylib+staticlib crate-types
#   3. Library must use x86_64-nexaos-userspace-lib.json target

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"

init_build_env

# ============================================================================
# Library Registry
# Format: "name:source_dir:output_name:so_version:dependencies"
# - name: Library identifier (cargo package name)
# - source_dir: Path relative to userspace/
# - output_name: Output library name (libXXX)
# - so_version: Shared library version (e.g., "3" for libssl.so.3)
# - dependencies: Comma-separated list of dependent libs (built first)
# ============================================================================

declare -A LIBRARIES=(
    ["ncryptolib"]="ncryptolib:crypto:3:"
    ["nssl"]="nssl:ssl:3:ncryptolib"
    # Add more libraries here:
    # ["nzlib"]="nzlib:z:1:"
    # ["npng"]="npng:png:16:nzlib"
)

# Build order (topologically sorted based on dependencies)
# Libraries without dependencies come first
BUILD_ORDER=(
    "ncryptolib"
    "nssl"
    # Add new libraries in dependency order
)

# ============================================================================
# Configuration
# ============================================================================

USERSPACE_DIR="$PROJECT_ROOT/userspace"
SYSROOT_DIR="$BUILD_DIR/userspace-build/sysroot"
TARGET_LIB="$PROJECT_ROOT/targets/x86_64-nexaos-userspace-lib.json"

# ============================================================================
# Library Build Functions
# ============================================================================

parse_lib_config() {
    local lib_name="$1"
    local config="${LIBRARIES[$lib_name]}"
    
    if [ -z "$config" ]; then
        log_error "Unknown library: $lib_name"
        return 1
    fi
    
    IFS=':' read -r LIB_SRC_DIR LIB_OUTPUT_NAME LIB_SO_VERSION LIB_DEPS <<< "$config"
    LIB_SRC_PATH="$USERSPACE_DIR/$LIB_SRC_DIR"
    LIB_STATIC_NAME="lib${LIB_OUTPUT_NAME}.a"
    LIB_SHARED_NAME="lib${LIB_OUTPUT_NAME}.so"
}

check_dependencies() {
    local lib_name="$1"
    parse_lib_config "$lib_name"
    
    if [ -n "$LIB_DEPS" ]; then
        IFS=',' read -ra deps <<< "$LIB_DEPS"
        for dep in "${deps[@]}"; do
            local dep_config="${LIBRARIES[$dep]}"
            IFS=':' read -r _ dep_output _ _ <<< "$dep_config"
            local dep_lib="$SYSROOT_DIR/lib/lib${dep_output}.so"
            
            if [ ! -f "$dep_lib" ]; then
                log_warn "Dependency $dep not found, building it first..."
                build_library "$dep" "all"
            fi
        done
    fi
}

build_library_static() {
    local lib_name="$1"
    parse_lib_config "$lib_name"
    
    log_step "Building $lib_name staticlib ($LIB_STATIC_NAME)..."
    
    ensure_dir "$SYSROOT_DIR/lib"
    
    cd "$LIB_SRC_PATH"
    
    # Build static library
    RUSTFLAGS="-C opt-level=2 -C panic=abort -L $SYSROOT_DIR/lib" \
        cargo build -Z build-std=std,core,alloc,panic_abort \
        --target "$TARGET_LIB" --release 2>&1 | grep -v "^warning:" || true
    
    local staticlib="$USERSPACE_DIR/target/x86_64-nexaos-userspace-lib/release/lib${lib_name}.a"
    
    if [ -f "$staticlib" ]; then
        cp "$staticlib" "$SYSROOT_DIR/lib/$LIB_STATIC_NAME"
        log_success "$LIB_STATIC_NAME installed ($(file_size "$staticlib"))"
    else
        log_error "Failed to build $lib_name staticlib"
        return 1
    fi
}

build_library_shared() {
    local lib_name="$1"
    local dest_dir="${2:-$SYSROOT_DIR/lib}"
    
    parse_lib_config "$lib_name"
    
    log_step "Building $lib_name shared library ($LIB_SHARED_NAME)..."
    
    ensure_dir "$dest_dir"
    
    cd "$LIB_SRC_PATH"
    
    # Build shared library with PIC
    RUSTFLAGS="-C opt-level=2 -C panic=abort -C relocation-model=pic -L $SYSROOT_DIR/lib" \
        cargo build -Z build-std=std,core,alloc,panic_abort \
        --target "$TARGET_LIB" --release 2>&1 | grep -v "^warning:" || true
    
    local sharedlib="$USERSPACE_DIR/target/x86_64-nexaos-userspace-lib/release/lib${lib_name}.so"
    
    if [ -f "$sharedlib" ]; then
        cp "$sharedlib" "$dest_dir/$LIB_SHARED_NAME"
        strip --strip-unneeded "$dest_dir/$LIB_SHARED_NAME" 2>/dev/null || true
        
        # Create version symlinks
        if [ -n "$LIB_SO_VERSION" ]; then
            ln -sf "$LIB_SHARED_NAME" "$dest_dir/${LIB_SHARED_NAME}.${LIB_SO_VERSION}"
            ln -sf "$LIB_SHARED_NAME" "$dest_dir/${LIB_SHARED_NAME}.${LIB_SO_VERSION}.0.0"
        fi
        
        log_success "$LIB_SHARED_NAME installed ($(file_size "$dest_dir/$LIB_SHARED_NAME"))"
    else
        log_error "Failed to build $lib_name shared library"
        return 1
    fi
}

build_library() {
    local lib_name="$1"
    local build_type="${2:-all}"
    local dest_dir="$3"
    
    log_section "Building library: $lib_name"
    
    # Check and build dependencies first
    check_dependencies "$lib_name"
    
    case "$build_type" in
        static)
            build_library_static "$lib_name"
            ;;
        shared)
            build_library_shared "$lib_name" "$dest_dir"
            ;;
        all)
            build_library_static "$lib_name"
            build_library_shared "$lib_name" "$dest_dir"
            ;;
        *)
            log_error "Unknown build type: $build_type (use static|shared|all)"
            return 1
            ;;
    esac
}

build_all_libraries() {
    log_section "Building All NexaOS Libraries"
    
    local start_time=$(date +%s)
    local success_count=0
    local fail_count=0
    
    for lib_name in "${BUILD_ORDER[@]}"; do
        if build_library "$lib_name" "all"; then
            ((success_count++))
        else
            ((fail_count++))
            log_error "Failed to build $lib_name"
        fi
    done
    
    local end_time=$(date +%s)
    local total_time=$((end_time - start_time))
    
    echo ""
    log_section "Library Build Summary"
    echo "  Total time: ${total_time}s"
    echo "  Successful: $success_count"
    echo "  Failed:     $fail_count"
    echo ""
    echo "  Installed to: $SYSROOT_DIR/lib/"
    ls -la "$SYSROOT_DIR/lib/"/*.so "$SYSROOT_DIR/lib/"/*.a 2>/dev/null | awk '{print "    " $9 " (" $5 " bytes)"}'
}

list_libraries() {
    echo "Available libraries:"
    echo ""
    printf "  %-15s %-12s %-8s %s\n" "NAME" "OUTPUT" "VERSION" "DEPENDENCIES"
    printf "  %-15s %-12s %-8s %s\n" "----" "------" "-------" "------------"
    
    for lib_name in "${BUILD_ORDER[@]}"; do
        local config="${LIBRARIES[$lib_name]}"
        IFS=':' read -r src_dir output_name so_ver deps <<< "$config"
        printf "  %-15s lib%-9s %-8s %s\n" "$lib_name" "$output_name" "${so_ver:-N/A}" "${deps:-none}"
    done
    
    echo ""
    echo "Usage: $0 <library_name> [static|shared|all] [dest_dir]"
}

# ============================================================================
# Main Entry Point
# ============================================================================

main() {
    case "${1:-all}" in
        list|--list|-l)
            list_libraries
            ;;
        all)
            build_all_libraries
            ;;
        help|--help|-h)
            echo "NexaOS Library Builder"
            echo ""
            echo "Usage: $0 [command|library_name] [build_type] [dest_dir]"
            echo ""
            echo "Commands:"
            echo "  all              Build all libraries (default)"
            echo "  list             List available libraries"
            echo "  help             Show this help"
            echo ""
            echo "Build types:"
            echo "  static           Build static library (.a) only"
            echo "  shared           Build shared library (.so) only"
            echo "  all              Build both (default)"
            echo ""
            echo "Examples:"
            echo "  $0                           # Build all libraries"
            echo "  $0 ncryptolib                # Build ncryptolib"
            echo "  $0 nssl shared               # Build nssl shared library only"
            echo "  $0 list                      # List available libraries"
            ;;
        *)
            # Check if it's a known library
            if [ -n "${LIBRARIES[$1]}" ]; then
                build_library "$1" "${2:-all}" "$3"
            else
                log_error "Unknown command or library: $1"
                echo "Use '$0 list' to see available libraries"
                exit 1
            fi
            ;;
    esac
}

main "$@"
