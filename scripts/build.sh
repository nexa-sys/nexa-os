#!/bin/bash
# NexaOS Build System - Main Build Script
# Orchestrates the complete build process with step selection
#
# Usage:
#   ./build.sh              # Full build (all steps)
#   ./build.sh quick        # Quick build (kernel + initramfs + ISO)
#   ./build.sh kernel       # Kernel only
#   ./build.sh userspace    # Userspace programs only
#   ./build.sh iso          # ISO only (requires kernel)
#   ./build.sh clean        # Clean all build artifacts
#
# Environment Variables:
#   BUILD_TYPE=debug|release  # Build type (default: release)
#   PARALLEL=1                # Enable parallel builds where safe
#   LOG_LEVEL=debug|info      # Kernel log level

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/common.sh"

init_build_env

# ============================================================================
# Build Profiles
# ============================================================================

# Full build: everything
build_full() {
    log_section "Full NexaOS System Build"
    
    local start_time=$(date +%s)
    
    timed_run "Building kernel" bash "$STEPS_DIR/build-kernel.sh" "$BUILD_TYPE"
    timed_run "Building UEFI loader" bash "$SCRIPTS_DIR/build-uefi-loader.sh"
    timed_run "Building kernel modules" bash "$STEPS_DIR/build-modules.sh" all
    timed_run "Building rootfs" bash "$STEPS_DIR/build-rootfs.sh" all
    timed_run "Building initramfs" bash "$STEPS_DIR/build-initramfs.sh" all
    timed_run "Building ISO" bash "$STEPS_DIR/build-iso.sh" "$BUILD_TYPE"
    
    local end_time=$(date +%s)
    local total_time=$((end_time - start_time))
    
    print_summary "$total_time"
}

# Quick build: kernel + initramfs + ISO (no rootfs rebuild)
build_quick() {
    log_section "Quick NexaOS Build"
    
    local start_time=$(date +%s)
    
    timed_run "Building kernel" bash "$STEPS_DIR/build-kernel.sh" "$BUILD_TYPE"
    timed_run "Building initramfs" bash "$STEPS_DIR/build-initramfs.sh" all
    timed_run "Building ISO" bash "$STEPS_DIR/build-iso.sh" "$BUILD_TYPE"
    
    local end_time=$(date +%s)
    local total_time=$((end_time - start_time))
    
    print_summary "$total_time"
}

# Kernel only
build_kernel_only() {
    timed_run "Building kernel" bash "$STEPS_DIR/build-kernel.sh" "$BUILD_TYPE"
}

# Userspace only
build_userspace_only() {
    log_section "Building Userspace"
    
    timed_run "Building nrlib" bash "$STEPS_DIR/build-nrlib.sh" all
    timed_run "Building programs" bash "$STEPS_DIR/build-userspace-programs.sh" all
}

# ISO only (assumes kernel exists)
build_iso_only() {
    timed_run "Building ISO" bash "$STEPS_DIR/build-iso.sh" "$BUILD_TYPE"
}

# Modules only
build_modules_only() {
    timed_run "Building modules" bash "$STEPS_DIR/build-modules.sh" all
}

# Initramfs only
build_initramfs_only() {
    timed_run "Building initramfs" bash "$STEPS_DIR/build-initramfs.sh" all
}

# Rootfs only
build_rootfs_only() {
    timed_run "Building rootfs" bash "$STEPS_DIR/build-rootfs.sh" all
}

# ============================================================================
# Clean
# ============================================================================

clean_all() {
    log_section "Cleaning Build Artifacts"
    
    log_step "Removing build directory..."
    rm -rf "$BUILD_DIR"
    
    log_step "Removing dist directory..."
    rm -rf "$DIST_DIR"
    
    log_step "Running cargo clean..."
    cd "$PROJECT_ROOT"
    cargo clean 2>/dev/null || true
    
    # Clean userspace nrlib
    cd "$PROJECT_ROOT/userspace/nrlib"
    cargo clean 2>/dev/null || true
    
    # Clean modules
    for module_dir in "$PROJECT_ROOT/modules"/*; do
        if [ -d "$module_dir" ]; then
            cd "$module_dir"
            cargo clean 2>/dev/null || true
        fi
    done
    
    log_success "Clean complete"
}

clean_build() {
    log_step "Cleaning build directory only..."
    rm -rf "$BUILD_DIR"
    rm -rf "$DIST_DIR"
    log_success "Build artifacts cleaned"
}

# ============================================================================
# Summary
# ============================================================================

print_summary() {
    local total_time="${1:-0}"
    
    echo ""
    log_section "Build Complete! (${total_time}s)"
    
    echo "System components:"
    
    local kernel_path="$TARGET_DIR/x86_64-nexaos/$BUILD_TYPE/nexa-os"
    if [ -f "$kernel_path" ]; then
        echo "  - Kernel: $kernel_path ($(file_size "$kernel_path"))"
    fi
    
    if [ -f "$INITRAMFS_CPIO" ]; then
        echo "  - Initramfs: $INITRAMFS_CPIO ($(file_size "$INITRAMFS_CPIO"))"
    fi
    
    if [ -f "$ROOTFS_IMG" ]; then
        echo "  - Root FS: $ROOTFS_IMG ($(file_size "$ROOTFS_IMG"))"
    fi
    
    if [ -f "$ISO_FILE" ]; then
        echo "  - ISO: $ISO_FILE ($(file_size "$ISO_FILE"))"
    fi
    
    echo ""
    echo "To run in QEMU:"
    echo "  ./scripts/run-qemu.sh"
    echo ""
    echo "Boot parameters (in GRUB):"
    echo "  root=/dev/vda1 rootfstype=ext2 loglevel=$LOG_LEVEL"
    echo ""
}

# ============================================================================
# Help
# ============================================================================

show_help() {
    cat << 'EOF'
NexaOS Build System

Usage: ./scripts/build.sh [command] [options]

Commands:
  full, all       Full system build (default)
  quick, q        Quick build (kernel + initramfs + ISO, no rootfs)
  kernel, k       Build kernel only
  userspace, u    Build userspace programs only
  modules, m      Build kernel modules only
  initramfs, i    Build initramfs only
  rootfs, r       Build root filesystem only
  iso             Build ISO only (requires existing kernel)
  clean           Clean all build artifacts
  clean-build     Clean build/ and dist/ only (keep cargo cache)
  help, -h        Show this help

Environment Variables:
  BUILD_TYPE      debug or release (default: release)
  LOG_LEVEL       Kernel log level: debug, info, warn (default: debug)
  ROOTFS_SIZE_MB  Root filesystem size in MB (default: 50)

Examples:
  ./scripts/build.sh                    # Full release build
  BUILD_TYPE=debug ./scripts/build.sh   # Full debug build
  ./scripts/build.sh quick              # Quick iteration build
  ./scripts/build.sh kernel             # Rebuild kernel only
  ./scripts/build.sh userspace          # Rebuild userspace only

Build Order (for full build):
  1. Kernel (cargo build)
  2. UEFI Loader
  3. Kernel Modules (.nkm)
  4. Root Filesystem (ext2)
  5. Initramfs (CPIO)
  6. Bootable ISO

Notes:
  - Use 'quick' for rapid iteration during development
  - After userspace changes, run 'rootfs' then 'iso' or just 'full'
  - The ISO embeds initramfs; rootfs.ext2 is attached via QEMU
EOF
}

# ============================================================================
# Main
# ============================================================================

run_step() {
    local cmd="$1"
    
    case "$cmd" in
        full|all)
            build_full
            ;;
        quick|q)
            build_quick
            ;;
        kernel|k)
            build_kernel_only
            ;;
        userspace|u)
            build_userspace_only
            ;;
        modules|m)
            build_modules_only
            ;;
        initramfs|i)
            build_initramfs_only
            ;;
        rootfs|r)
            build_rootfs_only
            ;;
        iso)
            build_iso_only
            ;;
        clean)
            clean_all
            ;;
        clean-build)
            clean_build
            ;;
        help|-h|--help)
            show_help
            ;;
        *)
            log_error "Unknown command: $cmd"
            return 1
            ;;
    esac
}

main() {
    # No arguments = full build
    if [ $# -eq 0 ]; then
        build_full
        return
    fi
    
    # Handle special cases that shouldn't be combined
    case "$1" in
        help|-h|--help)
            show_help
            return
            ;;
        clean|clean-build)
            run_step "$1"
            return
            ;;
        full|all)
            build_full
            return
            ;;
        quick|q)
            build_quick
            return
            ;;
    esac
    
    # Run multiple steps in sequence
    for cmd in "$@"; do
        log_info "Running step: $cmd"
        if ! run_step "$cmd"; then
            log_error "Step '$cmd' failed"
            exit 1
        fi
    done
}

main "$@"
