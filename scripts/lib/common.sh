#!/bin/bash
# NexaOS Build System - Common Functions and Variables
# Source this file in other build scripts

set -e

# ============================================================================
# Directory Setup
# ============================================================================
if [ -z "$PROJECT_ROOT" ]; then
    # common.sh is in scripts/lib/, so go up two levels to get project root
    _COMMON_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    PROJECT_ROOT="$(dirname "$(dirname "$_COMMON_DIR")")"
fi

# Scripts directory (scripts/)
SCRIPTS_DIR="$PROJECT_ROOT/scripts"
STEPS_DIR="$SCRIPTS_DIR/steps"

BUILD_DIR="$PROJECT_ROOT/build"
DIST_DIR="$PROJECT_ROOT/dist"
TARGET_DIR="$PROJECT_ROOT/target"

# Build type (debug/release)
# NOTE: Default to debug for kernel - release (O3) causes fork/exec issues
# The original build-iso.sh used debug mode. Keep this for compatibility.
BUILD_TYPE="${BUILD_TYPE:-debug}"
LOG_LEVEL="${LOG_LEVEL:-debug}"
KERNEL_TARGET_DIR="$TARGET_DIR/x86_64-nexaos/$BUILD_TYPE"
USERSPACE_TARGET_DIR="$BUILD_DIR/userspace-build/target/x86_64-nexaos-userspace/$BUILD_TYPE"

# Target specifications
TARGET_KERNEL="$PROJECT_ROOT/targets/x86_64-nexaos.json"
TARGET_USERSPACE="$PROJECT_ROOT/targets/x86_64-nexaos-userspace.json"
TARGET_USERSPACE_PIC="$PROJECT_ROOT/targets/x86_64-nexaos-userspace-pic.json"
TARGET_USERSPACE_DYN="$PROJECT_ROOT/targets/x86_64-nexaos-userspace-dynamic.json"
TARGET_LD="$PROJECT_ROOT/targets/x86_64-nexaos-ld.json"
TARGET_MODULE="$PROJECT_ROOT/targets/x86_64-nexaos-module.json"

# Build artifacts
KERNEL_BIN="$KERNEL_TARGET_DIR/nexa-os"
INITRAMFS_CPIO="$BUILD_DIR/initramfs.cpio"
ROOTFS_IMG="$BUILD_DIR/rootfs.ext2"
ISO_FILE="$DIST_DIR/nexaos.iso"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# ============================================================================
# Logging Functions
# ============================================================================
log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[✓]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*" >&2
}

log_step() {
    echo -e "${CYAN}==>${NC} $*"
}

log_section() {
    echo ""
    echo -e "${CYAN}========================================"
    echo -e "$*"
    echo -e "========================================${NC}"
    echo ""
}

# ============================================================================
# Utility Functions
# ============================================================================

# Check if a command exists
require_cmd() {
    local cmd="$1"
    local package="${2:-$1}"
    if ! command -v "$cmd" >/dev/null 2>&1; then
        log_error "Required tool '$cmd' not found. Please install: $package"
        return 1
    fi
}

# Check multiple commands
require_cmds() {
    local missing=()
    for cmd in "$@"; do
        if ! command -v "$cmd" >/dev/null 2>&1; then
            missing+=("$cmd")
        fi
    done
    if [ ${#missing[@]} -gt 0 ]; then
        log_error "Missing required tools: ${missing[*]}"
        return 1
    fi
}

# Create directory if not exists
ensure_dir() {
    for dir in "$@"; do
        mkdir -p "$dir"
    done
}

# Get file size in human readable format
file_size() {
    stat -c%s "$1" 2>/dev/null | numfmt --to=iec-i 2>/dev/null || echo "unknown"
}

# Check if file is newer than another
is_newer() {
    local src="$1"
    local dst="$2"
    [ ! -f "$dst" ] || [ "$src" -nt "$dst" ]
}

# Check if any source file in directory is newer than target
needs_rebuild() {
    local target="$1"
    shift
    local dirs=("$@")
    
    [ ! -f "$target" ] && return 0
    
    for dir in "${dirs[@]}"; do
        if [ -d "$dir" ]; then
            if find "$dir" -type f \( -name "*.rs" -o -name "Cargo.toml" \) -newer "$target" 2>/dev/null | grep -q .; then
                return 0
            fi
        fi
    done
    return 1
}

# Run command with timing
timed_run() {
    local desc="$1"
    shift
    local start_time=$(date +%s)
    log_step "$desc"
    "$@"
    local end_time=$(date +%s)
    local duration=$((end_time - start_time))
    log_success "$desc (${duration}s)"
}

# ============================================================================
# Build Flags
# ============================================================================

# Common RUSTFLAGS for userspace with std
get_std_rustflags() {
    local sysroot_lib="${1:-$BUILD_DIR/userspace-build/sysroot/lib}"
    echo "-C opt-level=2 -C panic=abort -C linker=rust-lld -C link-arg=--image-base=0x01000000 -C link-arg=--entry=_start -L $sysroot_lib -C link-arg=-upthread_mutexattr_settype -C link-arg=-upthread_mutexattr_init -C link-arg=-upthread_mutexattr_destroy -C link-arg=-upthread_mutex_init -C link-arg=-upthread_mutex_lock -C link-arg=-upthread_mutex_unlock -C link-arg=-upthread_mutex_destroy -C link-arg=-upthread_once -C link-arg=-u__libc_single_threaded"
}

# RUSTFLAGS for dynamic linking
get_dyn_rustflags() {
    local sysroot_lib="${1:-$BUILD_DIR/userspace-build/sysroot/lib}"
    echo "-C opt-level=2 -C panic=abort -C linker=rust-lld -C link-arg=--image-base=0x01000000 -C link-arg=--entry=_start -L $sysroot_lib -C link-arg=--undefined=_start -C link-arg=-lc -C link-arg=--undefined=pthread_mutexattr_settype -C link-arg=--undefined=pthread_mutexattr_init -C link-arg=--undefined=pthread_mutexattr_destroy -C link-arg=--undefined=pthread_mutex_init -C link-arg=--undefined=pthread_mutex_lock -C link-arg=--undefined=pthread_mutex_unlock -C link-arg=--undefined=pthread_mutex_destroy -C link-arg=--undefined=pthread_once -C link-arg=--undefined=__libc_single_threaded"
}

# RUSTFLAGS for nrlib no-std
get_nrlib_rustflags() {
    echo "-C opt-level=2 -C panic=abort"
}

# RUSTFLAGS for PIC (shared libraries)
# Export _start and _start_c symbols so they appear in .dynsym for dynamic linking
# Use -u to keep symbols referenced and --export-dynamic to make them visible
get_pic_rustflags() {
    echo "-C opt-level=2 -C panic=abort -C relocation-model=pic -C link-arg=-u_start -C link-arg=-u_start_c -C link-arg=--export-dynamic"
}

# RUSTFLAGS for dynamic linker
get_ld_rustflags() {
    echo "-C opt-level=s -C panic=abort -C linker=rust-lld -C link-arg=--pie -C link-arg=-e_start -C link-arg=--no-dynamic-linker -C link-arg=-soname=ld-nrlib-x86_64.so.1"
}

# RUSTFLAGS for kernel modules
get_module_rustflags() {
    echo "-C relocation-model=static -C code-model=kernel -C panic=abort"
}

# ============================================================================
# Lock File Management (for parallel builds)
# ============================================================================
LOCK_DIR="$BUILD_DIR/.locks"

acquire_lock() {
    local name="$1"
    local timeout="${2:-300}"  # 5 minutes default
    local lockfile="$LOCK_DIR/$name.lock"
    
    ensure_dir "$LOCK_DIR"
    
    local waited=0
    while ! mkdir "$lockfile" 2>/dev/null; do
        if [ $waited -ge $timeout ]; then
            log_error "Timeout waiting for lock: $name"
            return 1
        fi
        sleep 1
        ((waited++))
    done
    
    echo $$ > "$lockfile/pid"
}

release_lock() {
    local name="$1"
    local lockfile="$LOCK_DIR/$name.lock"
    rm -rf "$lockfile" 2>/dev/null || true
}

# ============================================================================
# Artifact Caching
# ============================================================================
CACHE_DIR="$BUILD_DIR/.cache"

cache_artifact() {
    local name="$1"
    local source="$2"
    local hash_file="$3"
    
    ensure_dir "$CACHE_DIR"
    
    if [ -f "$source" ] && [ -f "$hash_file" ]; then
        local hash=$(sha256sum "$hash_file" | cut -d' ' -f1)
        local cache_path="$CACHE_DIR/$name-$hash"
        cp "$source" "$cache_path"
        log_info "Cached: $name"
    fi
}

restore_from_cache() {
    local name="$1"
    local dest="$2"
    local hash_file="$3"
    
    if [ ! -f "$hash_file" ]; then
        return 1
    fi
    
    local hash=$(sha256sum "$hash_file" | cut -d' ' -f1)
    local cache_path="$CACHE_DIR/$name-$hash"
    
    if [ -f "$cache_path" ]; then
        cp "$cache_path" "$dest"
        log_info "Restored from cache: $name"
        return 0
    fi
    return 1
}

# ============================================================================
# Initialization
# ============================================================================
init_build_env() {
    ensure_dir "$BUILD_DIR" "$DIST_DIR" "$CACHE_DIR" "$LOCK_DIR"
    
    # Export for sub-scripts
    export PROJECT_ROOT BUILD_DIR DIST_DIR TARGET_DIR BUILD_TYPE
    export KERNEL_TARGET_DIR USERSPACE_TARGET_DIR
    export TARGET_KERNEL TARGET_USERSPACE TARGET_USERSPACE_PIC TARGET_USERSPACE_DYN TARGET_LD TARGET_MODULE
}
