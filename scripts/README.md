# NexaOS Build Scripts

This directory contains the modular build system for NexaOS.

## Quick Start

```bash
# Build everything (kernel, userspace, rootfs, ISO)
./scripts/build.sh

# Run in QEMU
./scripts/run-qemu.sh
```

## Main Build Script

### `build.sh` - Unified Build System

The main entry point for all build operations:

```bash
# Full build (default)
./scripts/build.sh

# Quick build (skip modules)
./scripts/build.sh quick

# Build specific components
./scripts/build.sh kernel          # Kernel only
./scripts/build.sh userspace       # Userspace programs only
./scripts/build.sh modules         # Kernel modules only
./scripts/build.sh rootfs          # Root filesystem only
./scripts/build.sh iso             # ISO image only

# Build multiple components
./scripts/build.sh kernel userspace rootfs iso

# Clean build
./scripts/build.sh clean           # Clean all artifacts
./scripts/build.sh clean-build     # Clean build/ directory only
```

**Environment Variables:**
```bash
BUILD_TYPE=debug|release   # Build mode (default: debug)
LOG_LEVEL=debug|info|warn  # Kernel log level (default: debug)
```

**Example:**
```bash
BUILD_TYPE=release LOG_LEVEL=info ./scripts/build.sh
```

## Directory Structure

```
scripts/
├── build.sh                    # Main entry point
├── run-qemu.sh                 # QEMU launcher
├── build-uefi-loader.sh        # UEFI loader builder
├── sign-module.sh              # Module signing tool
├── lib/
│   └── common.sh               # Shared functions and variables
└── steps/
    ├── build-kernel.sh         # Kernel compilation
    ├── build-nrlib.sh          # nrlib (libc shim) compilation
    ├── build-userspace-programs.sh  # Userspace programs
    ├── build-modules.sh        # Kernel modules (.nkm)
    ├── build-initramfs.sh      # Minimal initramfs
    ├── build-rootfs.sh         # ext2 root filesystem
    └── build-iso.sh            # Bootable ISO creation
```

## Build Components

### Kernel (`build.sh kernel`)

Builds the NexaOS kernel:
- Target: `x86_64-nexaos.json`
- Output: `target/x86_64-nexaos/debug/nexa-os`

### Userspace (`build.sh userspace`)

Builds all userspace components:
1. **nrlib** - Rust libc shim library
   - `lib64/libnrlib.so` - Shared library
   - `lib64/ld-nrlib-x86_64.so.1` - Dynamic linker
2. **Programs** - Shell, init, utilities
   - `/sbin/ni` - Init system
   - `/bin/sh` - Shell
   - `/bin/dhcp`, `/bin/login`, etc.

### Modules (`build.sh modules`)

Builds loadable kernel modules:
- `ext2.nkm` - ext2 filesystem driver
- `e1000.nkm` - Intel E1000 network driver

Modules are signed with PKCS#7/CMS signatures.

### Rootfs (`build.sh rootfs`)

Creates a 50MB ext2 root filesystem image:
- Output: `build/rootfs.ext2`
- Contains all binaries, libraries, and configs
- Attached as virtio disk in QEMU (`/dev/vda`)

### ISO (`build.sh iso`)

Creates a bootable ISO with GRUB:
- Output: `dist/nexaos.iso`
- Supports both BIOS and UEFI boot
- Contains kernel, initramfs, and UEFI loader

## Common Workflows

### First Time Build

```bash
./scripts/build.sh
./scripts/run-qemu.sh
```

### After Kernel Changes

```bash
./scripts/build.sh kernel iso
./scripts/run-qemu.sh
```

### After Userspace Changes

```bash
./scripts/build.sh userspace rootfs
./scripts/run-qemu.sh
```

### After Module Changes

```bash
./scripts/build.sh modules initramfs
./scripts/run-qemu.sh
```

### Clean Rebuild

```bash
./scripts/build.sh clean
./scripts/build.sh
```

## Running in QEMU

```bash
# Default run
./scripts/run-qemu.sh

# With additional QEMU options
./scripts/run-qemu.sh -S -s  # GDB server + pause
```

**QEMU Configuration:**
- 512MB RAM
- 4 CPU cores
- Virtio disk: `build/rootfs.ext2` → `/dev/vda`
- Serial console output
- Network: User-mode or bridge (auto-detected)

## Boot Parameters

Default GRUB command line:
```
root=/dev/vda1 rootfstype=ext2 loglevel=debug
```

To change, set `LOG_LEVEL` before building:
```bash
LOG_LEVEL=info ./scripts/build.sh iso
```

## Build Requirements

### Rust Toolchain
- Rust nightly
- Components: `rust-src`, `llvm-tools-preview`

### System Tools
- `grub-mkrescue`, `xorriso` - ISO creation
- `mkfs.ext2` (e2fsprogs) - Filesystem creation
- `qemu-system-x86_64` - Emulation
- `openssl` - Module signing

## Troubleshooting

### "Kernel not found"
```bash
./scripts/build.sh kernel
```

### "Root filesystem not found"
```bash
./scripts/build.sh rootfs
```

### "ISO image not found"
```bash
./scripts/build.sh iso
```

### Fork/exec crashes in userspace
Ensure kernel is built in debug mode (default). Release mode with O3 optimization causes issues:
```bash
BUILD_TYPE=debug ./scripts/build.sh kernel iso
```

### Clean everything and rebuild
```bash
./scripts/build.sh clean
rm -rf target/
./scripts/build.sh
```

## Boot Flow

```
1. GRUB/UEFI loads kernel + initramfs
   ↓
2. Kernel parses: root=/dev/vda rootfstype=ext2
   ↓
3. Kernel mounts virtual filesystems (/proc, /sys, /dev)
   ↓
4. Kernel loads modules from initramfs (ext2.nkm, e1000.nkm)
   ↓
5. Kernel mounts rootfs.ext2 at /sysroot
   ↓
6. Kernel performs pivot_root to real root
   ↓
7. Kernel starts /sbin/ni (init system)
   ↓
8. Init reads /etc/inittab and starts services
```

## See Also

- `../docs/BUILD-SYSTEM.md` - Detailed build system documentation
- `../docs/zh/rootfs-boot-process.md` - Boot process guide (Chinese)
- `../README.md` - Project overview
