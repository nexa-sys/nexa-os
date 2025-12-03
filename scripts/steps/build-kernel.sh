#!/bin/bash
# NexaOS Build System - Kernel Builder
# Build the NexaOS kernel

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib/common.sh"

init_build_env

# ============================================================================
# Kernel Build
# ============================================================================

build_kernel() {
    # Default to debug mode for kernel - matches original build-iso.sh behavior
    # Release mode (O3) causes fork/exec issues; debug mode (O2) works correctly
    local mode="${1:-debug}"
    
    log_section "Building NexaOS Kernel ($mode)"
    
    cd "$PROJECT_ROOT"
    
    local cargo_args=()
    [ "$mode" = "release" ] && cargo_args+=(--release)
    
    log_step "Compiling kernel..."
    cargo build "${cargo_args[@]}"
    
    local kernel_path="$TARGET_DIR/x86_64-nexaos/$mode/nexa-os"
    
    if [ -f "$kernel_path" ]; then
        local size=$(file_size "$kernel_path")
        log_success "Kernel built: $kernel_path ($size)"
        
        # Verify multiboot2 header
        if command -v grub-file >/dev/null 2>&1; then
            if grub-file --is-x86-multiboot2 "$kernel_path"; then
                log_success "Multiboot2 header verified"
            else
                log_warn "Multiboot2 header verification failed"
            fi
        fi
        
        # Generate kernel symbols if objcopy available
        if command -v objcopy >/dev/null 2>&1; then
            local syms_path="$BUILD_DIR/kernel.syms"
            objcopy --only-keep-debug "$kernel_path" "$syms_path" 2>/dev/null || true
            if [ -f "$syms_path" ]; then
                log_info "Symbols exported: $syms_path"
            fi
        fi
        
        return 0
    else
        log_error "Kernel build failed"
        return 1
    fi
}

# Main
if [ "${BASH_SOURCE[0]}" == "${0}" ]; then
    build_kernel "$@"
fi
