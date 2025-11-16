# NexaOS Build Scripts

This directory contains scripts for building and running NexaOS.

## Quick Start

```bash
# Build everything (recommended for first time)
./scripts/build-all.sh

# Run in QEMU
./scripts/run-qemu.sh
```

## Individual Scripts

### `build-all.sh` - Complete Build

Builds the entire system in the correct order:

```bash
./scripts/build-all.sh
```

This runs:
1. `cargo build --release` - Builds kernel
2. `build-userspace.sh` - Builds minimal initramfs
3. `build-rootfs.sh` - Builds ext2 root filesystem
4. `build-iso.sh release` - Creates bootable ISO

**Output:**
- `target/x86_64-nexaos/release/nexa-os` - Kernel binary
- `build/initramfs.cpio` - Minimal initramfs (~40KB)
- `build/rootfs.ext2` - Full root filesystem (50MB)
- `dist/nexaos.iso` - Bootable ISO

### `build-userspace.sh` - Minimal Initramfs

Builds a minimal initramfs for early boot:

```bash
./scripts/build-userspace.sh
```

**Contents:**
- `/init` - Early boot script
- `/bin/sh` - Emergency shell
- Mount points: `/dev`, `/proc`, `/sys`, `/sysroot`

**Output:** `build/initramfs.cpio` (~40KB)

### `build-rootfs.sh` - Full Root Filesystem

Creates a complete ext2 root filesystem:

```bash
./scripts/build-rootfs.sh
```

**Requirements:**
- `mkfs.ext2` (e2fsprogs package)
- `sudo` (for loop mounting)

**Contents:**
- All userspace binaries (`/sbin/ni`, `/bin/sh`, etc.)
- Configuration files (`/etc/ni/ni.conf`, `/etc/inittab`)
- Full directory structure

**Output:** `build/rootfs.ext2` (50MB ext2 image)

### `build-iso.sh` - Bootable ISO

Creates a bootable ISO image with GRUB:

```bash
# Release build (recommended)
./scripts/build-iso.sh release

# Debug build
./scripts/build-iso.sh debug
```

**Requirements:**
- `grub-mkrescue`
- `xorriso`

**What it does:**
- Builds kernel (if needed)
- Builds initramfs
- Creates GRUB config with boot parameters
- Packages into ISO

**Boot parameters:** `root=/dev/vda1 rootfstype=ext2 loglevel=info`

**Output:** `dist/nexaos.iso`

### `run-qemu.sh` - Run in QEMU

Runs NexaOS in QEMU with proper configuration:

```bash
# Run with default options
./scripts/run-qemu.sh

# Pass additional QEMU args (example: start GDB server and pause CPUs)
./scripts/run-qemu.sh -S -s

# Use `--` to separate script args from QEMU args if needed
./scripts/run-qemu.sh -- -S -s
```

**What it does:**
- Boots from ISO (kernel + initramfs)
- Attaches `rootfs.ext2` as virtio disk (`/dev/vda`)
- Allocates 512MB RAM
- Enables serial console

**Requirements:**
- ISO built: `dist/nexaos.iso`
- Root filesystem: `build/rootfs.ext2` (optional, will warn if missing)

## Common Workflows

### First Time Build

```bash
./scripts/build-all.sh
./scripts/run-qemu.sh
```

### Rebuild After Kernel Changes

```bash
cargo build --release
./scripts/build-iso.sh release
./scripts/run-qemu.sh
```

### Rebuild After Userspace Changes

```bash
./scripts/build-rootfs.sh
./scripts/run-qemu.sh
```

### Test with Different Boot Parameters

Edit `scripts/build-iso.sh` and change `GRUB_CMDLINE`:

```bash
GRUB_CMDLINE="root=/dev/vda1 rootfstype=ext2 emergency loglevel=debug"
```

Then rebuild ISO:

```bash
./scripts/build-iso.sh release
./scripts/run-qemu.sh
```

## Troubleshooting

### "ISO image not found"

Run `./scripts/build-iso.sh release` first.

### "Root filesystem not found"

Run `./scripts/build-rootfs.sh` to create the ext2 disk.

### "No init program found"

The initramfs is minimal and doesn't contain init. The kernel should mount the root filesystem from `rootfs.ext2`. Check that:
1. `build/rootfs.ext2` exists
2. GRUB cmdline includes `root=/dev/vda1 rootfstype=ext2`
3. Kernel logs show root mounting

### "Command line parsing empty"

The GRUB configuration wasn't generated correctly. Rebuild the ISO:

```bash
./scripts/build-iso.sh release
```

### Rebuild Everything from Scratch

```bash
# Clean build artifacts
rm -rf build/ dist/ target/iso/

# Rebuild
./scripts/build-all.sh
```

## Build Requirements

### Kernel
- Rust nightly toolchain
- `rust-src` component
- `llvm-tools-preview` component

### Userspace
- Same as kernel

### Root Filesystem
- `mkfs.ext2` (e2fsprogs)
- `sudo` access (for loop mounting)
- `strip` (binutils)

### ISO
- `grub-mkrescue` (grub-pc-bin or grub-efi-amd64-bin)
- `xorriso`

### QEMU
- `qemu-system-x86_64`

## Debug vs Release

**Debug build:**
- Faster compile time
- Larger binary (~2MB)
- More logging
- Use for development

**Release build:**
- Optimized binary (~380KB)
- Faster execution
- Use for testing and demos

To switch:
```bash
# Debug
./scripts/build-iso.sh debug

# Release
./scripts/build-iso.sh release
```

## Boot Flow

```
1. GRUB loads kernel from ISO
   ↓
2. GRUB loads minimal initramfs from ISO
   ↓
3. GRUB passes: root=/dev/vda1 rootfstype=ext2
   ↓
4. Kernel parses boot parameters
   ↓
5. Kernel mounts /proc, /sys, /dev
   ↓
6. Kernel scans for root device
   ↓
7. Kernel finds rootfs.ext2 in initramfs
   ↓
8. Kernel mounts ext2 at /sysroot
   ↓
9. Kernel performs pivot_root
   ↓
10. Kernel starts init from real root
```

## See Also

- `../docs/BUILD-SYSTEM.md` - Detailed build system documentation
- `../docs/zh/rootfs-boot-process.md` - Chinese boot process guide
- `../README.md` - Project overview
