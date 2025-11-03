#!/bin/bash
# Quick test of NexaOS with init system

cd /home/hanxi-cat/dev/nexa-os

echo "Testing NexaOS with /sbin/init..."
echo ""
echo "Expected behavior:"
echo "1. Kernel boots and loads init ramfs"
echo "2. /sbin/init starts as PID 1"
echo "3. Init spawns /bin/sh"
echo "4. Shell becomes interactive"
echo ""
echo "Press Ctrl-C to exit QEMU"
echo ""

./scripts/run-qemu.sh
