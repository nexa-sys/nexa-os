#!/bin/bash
# NVM Enterprise Build Script (Standalone)
# 
# NOTE: The preferred way to build NVM is through the NDK build system:
#   ./ndk build nvm              # Build NVM (frontend + backend)
#   ./ndk build nvm --list       # List NVM components
#   ./ndk build steps nvm rootfs # Build NVM and rootfs
#
# This script is kept for standalone development/testing.
# It builds the Vue.js frontend and Rust backend together.
#
# Usage:
#   ./build-nvm.sh          # Full build (frontend + backend)
#   ./build-nvm.sh backend  # Backend only
#   ./build-nvm.sh frontend # Frontend only
#   ./build-nvm.sh dev      # Development build (no frontend embed)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NVM_DIR="$SCRIPT_DIR"
WEBUI_DIR="$NVM_DIR/webui"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Build Vue.js frontend
build_frontend() {
    log_info "Building Vue.js frontend..."
    
    if [ ! -d "$WEBUI_DIR" ]; then
        log_error "Frontend directory not found: $WEBUI_DIR"
        exit 1
    fi
    
    cd "$WEBUI_DIR"
    
    # Install dependencies if needed
    if [ ! -d "node_modules" ]; then
        log_info "Installing frontend dependencies..."
        npm install
    fi
    
    # Build for production
    log_info "Running npm build..."
    npm run build
    
    # Verify build output
    if [ ! -f "dist/index.html" ]; then
        log_error "Frontend build failed - dist/index.html not found"
        exit 1
    fi
    
    local file_count=$(find dist -type f | wc -l)
    log_success "Frontend built successfully ($file_count files)"
    
    cd "$NVM_DIR"
}

# Build Rust backend
build_backend() {
    local mode="${1:-release}"
    
    log_info "Building Rust backend ($mode)..."
    
    cd "$NVM_DIR"
    
    if [ "$mode" = "release" ]; then
        cargo build --release --features "full"
    else
        cargo build --features "full"
    fi
    
    log_success "Backend built successfully"
}

# Full build
full_build() {
    log_info "=== NVM Enterprise Full Build ==="
    echo ""
    
    # Check requirements
    if ! command -v npm &> /dev/null; then
        log_error "npm not found. Please install Node.js and npm."
        exit 1
    fi
    
    if ! command -v cargo &> /dev/null; then
        log_error "cargo not found. Please install Rust."
        exit 1
    fi
    
    # Build frontend first (so it's embedded in binary)
    build_frontend
    
    # Build backend
    build_backend release
    
    echo ""
    log_success "=== Build Complete ==="
    echo ""
    echo "Binaries:"
    echo "  - target/release/nvmctl       (CLI tool)"
    echo "  - target/release/nvm-server   (Web server)"
    echo ""
    echo "Start the server:"
    echo "  ./target/release/nvm-server"
    echo ""
    echo "Default login: admin / admin123"
    echo "Web UI: http://localhost:8006"
}

# Development build (no frontend embed)
dev_build() {
    log_info "=== NVM Development Build ==="
    echo ""
    
    # Build backend only in debug mode
    build_backend debug
    
    echo ""
    log_warn "Development build complete (no frontend embedded)"
    echo "Run frontend dev server separately: cd webui && npm run dev"
}

# Print usage
usage() {
    echo "NVM Enterprise Build Script"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  (default)    Full build (frontend + backend release)"
    echo "  backend      Build Rust backend only (release)"
    echo "  frontend     Build Vue.js frontend only"
    echo "  dev          Development build (debug, no frontend)"
    echo "  help         Show this help"
}

# Main
case "${1:-}" in
    backend)
        build_backend release
        ;;
    frontend)
        build_frontend
        ;;
    dev)
        dev_build
        ;;
    help|--help|-h)
        usage
        ;;
    *)
        full_build
        ;;
esac
