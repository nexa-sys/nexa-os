#!/usr/bin/env bash
set -euo pipefail

if command -v ld.bfd >/dev/null 2>&1; then
    exec ld.bfd "$@"
else
    exec ld "$@"
fi
