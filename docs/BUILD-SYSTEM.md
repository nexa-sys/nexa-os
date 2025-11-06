# NexaOS Build System

This document explains the NexaOS build system and the role of each component.

## Overview

NexaOS uses a two-stage boot process similar to modern Linux systems:

1. **Initramfs** (Initial RAM Filesystem) - Minimal early boot environment
2. **Root Filesystem** (ext2 disk) - Full system with all applications

## Components

### 1. Kernel (`nexa-os`)

The kernel binary that handles hardware initialization, memory management, and boot stages.

**Build:**
```bash
cargo build --release
```

**Output:** `target/x86_64-nexaos/release/nexa-os`

### 2. Initramfs (Minimal)

A tiny RAM-based filesystem loaded by the bootloader. Contains only what's needed for early boot:

- `/init` - Early boot script (currently a stub, kernel handles mounting)
- `/bin/sh` - Emergency shell for recovery
- `/dev`, `/proc`, `/sys` - Mount points
- `/sysroot` - Mount point for real root filesystem

**Purpose:**
- Provide emergency recovery shell
- Set up early environment (mount /proc, /sys)
- Future: Load drivers needed to access root device
- Future: Handle complex storage (LVM, RAID, encryption)

**Build:**
```bash
./scripts/build-userspace.sh
```

**Output:** `build/initramfs.cpio` (~40KB)

### 3. Root Filesystem (Full System)

An ext2-formatted disk image containing the complete operating system:

```
/
├── bin/
│   ├── sh          - Shell
│   └── login       - Login program
├── sbin/
│   ├── ni          - Nexa Init (PID 1)
│   ├── init        - Init symlink
│   └── getty       - Terminal manager
├── etc/
│   ├── ni/
│   │   └── ni.conf - Init configuration
│   ├── inittab     - Init table
│   └── motd        - Message of the day
├── dev/            - Device nodes
├── proc/           - Process info (mount point)
├── sys/            - System info (mount point)
├── tmp/            - Temporary files
└── home/           - User home directories
```

**Build:**
```bash
./scripts/build-rootfs.sh
```

**Output:** `build/rootfs.ext2` (~50MB)

### 4. Bootable ISO

GRUB-based ISO that includes:
- Kernel binary
- Minimal initramfs
- Boot configuration

**Build:**
```bash
./scripts/build-iso.sh
```

**Output:** `dist/nexaos.iso`

## Build Scripts

### `scripts/build-all.sh` (Recommended)

Builds everything in the correct order:

```bash
./scripts/build-all.sh
```

This is equivalent to:
```bash
cargo build --release              # 1. Kernel
./scripts/build-userspace.sh       # 2. Minimal initramfs
./scripts/build-rootfs.sh          # 3. Root filesystem
./scripts/build-iso.sh             # 4. Bootable ISO
```

### `scripts/build-userspace.sh`

Builds the **minimal initramfs** for early boot.

**What it does:**
- Compiles emergency shell (`/bin/sh`)
- Creates init script (`/init`)
- Packages into CPIO archive
- Includes rootfs.ext2 if available (for testing)

**Output:** `build/initramfs.cpio`

### `scripts/build-rootfs.sh`

Builds the **full root filesystem** on ext2.

**What it does:**
- Compiles all userspace programs (init, shell, getty, login)
- Creates ext2 filesystem image (50MB)
- Populates with directory structure
- Copies binaries and configuration
- Creates device nodes

**Output:** `build/rootfs.ext2`

**Requirements:**
- `mkfs.ext2` (e2fsprogs)
- `sudo` (for mounting loop device)

### `scripts/build-iso.sh`

Creates bootable ISO image.

**What it does:**
- Builds kernel (if needed)
- Builds minimal initramfs
- Creates GRUB configuration with boot parameters
- Packages into ISO with `grub-mkrescue`

**Boot parameters:** `root=/dev/vda1 rootfstype=ext2 loglevel=info`

**Output:** `dist/nexaos.iso`

**Requirements:**
- `grub-mkrescue`
- `xorriso`

### `scripts/run-qemu.sh`

Runs NexaOS in QEMU with proper configuration.

**What it does:**
- Boots from ISO (contains kernel + initramfs)
- Attaches rootfs.ext2 as virtio disk (`/dev/vda`)
- Configures 512MB RAM
- Enables serial console

**Requirements:**
- ISO built (`dist/nexaos.iso`)
- Root filesystem built (`build/rootfs.ext2`)

## Boot Process

### Stage 1: Bootloader (GRUB)

1. GRUB loads kernel from ISO
2. GRUB loads initramfs from ISO
3. GRUB passes boot parameters: `root=/dev/vda1 rootfstype=ext2`
4. Kernel starts

### Stage 2: Kernel Init

1. Hardware detection and initialization
2. Memory management setup
3. Unpack initramfs into tmpfs
4. Parse boot parameters from cmdline

### Stage 3: Initramfs Stage

1. Mount `/proc`, `/sys`, `/dev`
2. Scan for root device (`/dev/vda1`)
3. Find and validate ext2 disk image
4. Mount ext2 filesystem at `/sysroot`

### Stage 4: Root Switch (pivot_root)

1. Verify `/sysroot` is mounted
2. Remount ext2 as new root
3. Replace VFS root mount point
4. Old initramfs remains accessible for emergency

### Stage 5: Real Root

1. Search for init in real root filesystem:
   - `/sbin/ni` (preferred)
   - `/sbin/init`
   - `/bin/sh` (fallback)
2. Execute init as PID 1
3. Init starts services per configuration

### Stage 6: User Space

1. Getty provides login prompt
2. User authentication
3. Shell spawned
4. System ready

## Differences from Standard Linux

### Similarities
- Initramfs is minimal and temporary
- Real root is on persistent storage (ext2)
- Boot parameters follow Linux conventions
- Uses pivot_root concept to switch roots

### Current Limitations
- **No hardware drivers yet:** Uses disk image from initramfs instead of real block devices
- **No module loading:** Drivers will be built into kernel for now
- **Simplified device detection:** No udev, just image file scanning
- **No LVM/RAID/encryption:** Direct ext2 mounting only

### Future Enhancements
- virtio-blk driver for QEMU
- AHCI driver for real hardware
- Module loading from initramfs
- udev for dynamic device management
- Support for complex storage setups

## Testing

### Quick Test (Existing Rootfs)
```bash
./scripts/build-iso.sh
./scripts/run-qemu.sh
```

### Full Build
```bash
./scripts/build-all.sh
./scripts/run-qemu.sh
```

### Expected Boot Output
```
[INFO] Stage 1: Bootloader - Complete
[INFO] Stage 2: Kernel Init - Starting...
[INFO] Boot config: root=/dev/vda1
[INFO] Stage 3: Initramfs Stage - Starting...
[INFO] Mounting /proc...
[INFO] Mounting /sys...
[INFO] Mounting /dev...
[INFO] Stage 4: Root Mounting - Starting...
[INFO] Scanning for block device: /dev/vda1
[INFO] Found rootfs.ext2 in initramfs (52428800 bytes)
[INFO] Successfully parsed ext2 filesystem
[INFO] Real root mounted at /sysroot (ext2, read-only)
[INFO] Stage 5: Root Switch - Starting...
[INFO] Remounting ext2 filesystem as new root
[INFO] Stage 6: User Space - Starting init process
[INFO] Trying init program: /sbin/ni
```

### Troubleshooting

**"Root filesystem not found"**
- Build rootfs: `./scripts/build-rootfs.sh`
- Verify: `ls -lh build/rootfs.ext2`

**"Init binary not found"**
- Rootfs not built or incomplete
- Rebuild: `./scripts/build-rootfs.sh`

**Emergency shell**
- System dropped to emergency shell
- Check logs for error messages
- Type `exit` to try continuing

## Summary

| Component | Size | Purpose | Location |
|-----------|------|---------|----------|
| Kernel | ~380KB | Core OS | ISO |
| Initramfs | ~40KB | Early boot | ISO |
| Root FS | ~50MB | Full system | Disk (VDA) |
| ISO | ~2MB | Bootable image | dist/ |

The initramfs is now truly minimal, containing only what's needed for early boot and emergency recovery. The full system lives on the ext2 root filesystem, which is mounted during boot via the pivot_root process.
