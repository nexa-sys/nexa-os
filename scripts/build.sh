#!/usr/bin/env bash
# NexaOS Build System - Compatibility Wrapper
#
# This script is deprecated. Please use ./ndk instead.
# This wrapper is kept for backward compatibility.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "⚠️  Warning: scripts/build.sh is deprecated. Use ./ndk instead." >&2
echo "" >&2

# Forward to ndk
exec "$ROOT_DIR/ndk" "$@"