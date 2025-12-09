#!/usr/bin/env bash
# NexaOS QEMU Launcher - Compatibility Wrapper
#
# This script is deprecated. Please use ./ndk run instead.
# The actual run script is now generated at build/run-qemu.sh
# from config/qemu.yaml.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "⚠️  Warning: scripts/run-qemu.sh is deprecated." >&2
echo "   Use: ./ndk run" >&2
echo "   Or:  ./ndk dev (build + run)" >&2
echo "" >&2

# Check if build/run-qemu.sh exists
if [[ -f "$ROOT_DIR/build/run-qemu.sh" ]]; then
    exec "$ROOT_DIR/build/run-qemu.sh" "$@"
else
    # Generate and run via ndk
    exec "$ROOT_DIR/ndk" run "$@"
fi