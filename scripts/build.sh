#!/usr/bin/env bash
# NexaOS Build System - TypeScript Entry Point

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Check if we have node
if ! command -v node &> /dev/null; then
    echo "Error: Node.js is required but not installed." >&2
    echo "Install Node.js 20+ and try again." >&2
    exit 1
fi

# Check if dependencies are installed
if [ ! -d "$SCRIPT_DIR/node_modules" ]; then
    echo "Installing dependencies..."
    cd "$SCRIPT_DIR"
    npm install
fi

# Check if we should use development mode (tsx) or production mode
if [ -f "$SCRIPT_DIR/dist/cli.js" ]; then
    # Production mode - use compiled JavaScript
    exec node "$SCRIPT_DIR/dist/cli.js" "$@"
else
    # Development mode - use tsx
    cd "$SCRIPT_DIR"
    exec npx tsx src/cli.ts "$@"
fi