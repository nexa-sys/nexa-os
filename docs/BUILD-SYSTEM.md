# NexaOS Build System

> **Last Updated**: 2025年11月12日  
> **Build Architecture**: Two-stage boot with ext2 root filesystem

This document explains the NexaOS build system and the role of each component.

## Overview

NexaOS uses a three-layer boot process similar to modern Linux systems:

1. **Kernel** - Core operating system binary (Multiboot2-compliant ELF)
2. **Initramfs** (Initial RAM Filesystem) - Minimal early boot environment (CPIO archive)
3. **Root Filesystem** (ext2 disk) - Full system with all applications and data

## Build Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                  Build System Overview                       │
└─────────────────────────────────────────────────────────────┘

Step 1: Build Kernel
   cargo build --release --target x86_64-nexaos.json
   ↓
   target/x86_64-nexaos/release/nexa-os (ELF binary, ~2 MB)

Step 2: Build Userspace Programs (for initramfs)
   ./scripts/build-userspace.sh
   ↓
   build/initramfs/bin/sh       (emergency shell)
   build/initramfs/lib64/ld-linux.so  (dynamic linker)
   ↓
   Create CPIO archive:
   build/initramfs.cpio (~40 KB)

Step 3: Build Root Filesystem (full system)
   ./scripts/build-rootfs.sh
   ↓
   Compile userspace programs (ni, shell, getty, login, nrlib)
   ↓
   Create ext2 disk image (50 MB):
   build/rootfs.ext2
   ↓
   Layout:
   /bin/sh       - Full-featured shell
   /sbin/ni      - Init system (PID 1)
   /sbin/getty   - Terminal manager
   /bin/login    - Authentication
   /etc/inittab  - Init configuration
   /etc/ni/      - Service configs
   /home/        - User directories

Step 4: Create Bootable ISO
   ./scripts/build-iso.sh
   ↓
   GRUB bootloader + kernel + initramfs
   ↓
   dist/nexaos.iso (~3 MB)

Boot Process:
   GRUB → Kernel → Unpack initramfs → Mount ext2 root → Start init
```

## Components

### 1. Kernel (`nexa-os`)

The kernel binary that handles hardware initialization, memory management, and boot stages.

**Source Files**:
- `src/lib.rs` - Kernel entry point and 6-stage boot sequence
- `src/main.rs` - Multiboot header stub
- `boot/long_mode.S` - Assembly bootstrap (long mode setup)
- `linker.ld` - Linker script (Multiboot header placement)

**Build Command**:
```bash
cargo build --release
```

**Target**: `x86_64-nexaos.json` (custom bare-metal target)
- No OS, no std
- Soft-float (no SSE, no MMX)
- Red zone disabled (for interrupt safety)
- Panic = abort (no unwinding)

**Output**: 
- `target/x86_64-nexaos/release/nexa-os` (~2 MB ELF binary)

**Features**:
- Multiboot2-compliant header
- 6-stage boot process
- Memory management (paging, virtual memory)
- Process management (scheduler, context switching)
- System call interface (38+ syscalls)
- Device drivers (keyboard, VGA, serial)
- File systems (initramfs, memory FS, ext2 root)

### 2. Initramfs (Minimal Boot Environment)

A tiny RAM-based filesystem loaded by the bootloader. Contains only what's needed for early boot:

**Purpose**:
- Emergency recovery shell
- Dynamic linker for dynamically-linked executables
- Minimal tools for root filesystem detection/mounting
- Future: Load drivers needed to access root device (RAID, LVM, encryption)

**Contents**:
```
build/initramfs/
├── init                 # Early boot script (currently minimal)
├── README.txt           # Build info
├── bin/
│   └── sh              # Emergency shell (statically or dynamically linked)
├── lib64/
│   └── ld-linux-x86-64.so.2  # Dynamic linker (if sh is dynamic)
├── dev/                # Device nodes (empty, populated at runtime)
├── proc/               # Process info mount point
├── sys/                # System info mount point
└── sysroot/            # Mount point for real root filesystem
```

**Build Script**:
```bash
./scripts/build-userspace.sh
```

**Build Process**:
1. Compile minimal shell (userspace/shell.rs) with `-Z build-std=core`
2. Copy to `build/initramfs/bin/sh`
3. If dynamically linked, copy `/lib64/ld-linux-x86-64.so.2` from host
4. Create directory structure
5. Generate CPIO newc archive

**Output**: 
- `build/initramfs.cpio` (~40 KB compressed)

**Format**: CPIO newc (ASCII format, compatible with Linux kernel)

**Loading**: 
- Loaded by GRUB as Multiboot module
- Parsed and unpacked by kernel in `src/initramfs.rs`
- Mounted in memory at boot stage 2

### 3. Root Filesystem (Full System)

An ext2-formatted disk image containing the complete operating system following Unix FHS (Filesystem Hierarchy Standard):

**Structure**:
```
build/rootfs.ext2 (50 MB ext2 image)
/
├── bin/                  # User binaries
│   ├── sh               # Full-featured interactive shell
│   └── login            # Login program
├── sbin/                # System binaries
│   ├── ni               # Nexa Init (PID 1)
│   ├── init → ni        # Symlink for compatibility
│   └── getty            # Terminal manager
├── etc/                 # System configuration
│   ├── inittab          # Init configuration (System V style)
│   ├── motd             # Message of the day
│   └── ni/
│       └── ni.conf      # Init system config
├── lib64/               # Shared libraries
│   └── ld-linux-x86-64.so.2  # Dynamic linker
├── dev/                 # Device nodes (populated at runtime)
├── proc/                # Process information (virtual FS, mount point)
├── sys/                 # System information (virtual FS, mount point)
├── tmp/                 # Temporary files
├── var/                 # Variable data
│   ├── log/             # Log files
│   └── run/             # Runtime data
├── home/                # User home directories
│   └── user/            # Default user home
└── root/                # Root user home directory
```

**Build Script**:
```bash
./scripts/build-rootfs.sh
```

**Build Process**:
1. **Create build workspace**: `build/userspace-build/`
2. **Compile userspace programs** with nrlib (libc compatibility):
   - `userspace/init.rs` → `sbin/ni` (PID 1 init)
   - `userspace/shell.rs` → `bin/sh` (interactive shell)
   - `userspace/getty.rs` → `sbin/getty` (terminal manager)
   - `userspace/login.rs` → `bin/login` (authentication)
   - `userspace/nrlib/` → libc compatibility layer
3. **Create ext2 filesystem**: 50 MB, 4096 byte blocks
   ```bash
   dd if=/dev/zero of=build/rootfs.ext2 bs=1M count=50
   mkfs.ext2 -F build/rootfs.ext2
   ```
4. **Mount and populate**:
   ```bash
   mkdir -p build/rootfs
   sudo mount -o loop build/rootfs.ext2 build/rootfs
   sudo cp binaries to build/rootfs/...
   sudo mkdir -p build/rootfs/{dev,proc,sys,tmp,var,home,root}
   sudo cp etc/inittab build/rootfs/etc/
   sudo umount build/rootfs
   ```
5. **Set permissions**: Proper ownership and execute bits

**Output**: 
- `build/rootfs.ext2` (50 MB ext2 disk image)

**Format**: Standard ext2 filesystem (Linux-compatible)

**Mounting**:
- Kernel mounts via `mount()` syscall during boot stage 4
- Device: `/dev/vda1` (QEMU virtio-blk)
- Mount point: `/sysroot` initially, then pivot to `/`
- Options: Read-write, standard ext2 semantics

**Persistence**: 
- Changes persist across reboots (in QEMU with `-drive` option)
- Can be inspected from host: `sudo mount -o loop build/rootfs.ext2 /mnt`

### 4. Bootable ISO

GRUB-based ISO that includes:
- GRUB bootloader (BIOS mode)
- Kernel binary
- Initramfs CPIO archive
- Boot configuration (grub.cfg)

**Build Script**:
```bash
./scripts/build-iso.sh
```

**Build Process**:
1. Create ISO staging directory: `target/iso/`
2. Copy kernel: `target/x86_64-nexaos/release/nexa-os` → `target/iso/boot/kernel.bin`
3. Copy initramfs: `build/initramfs.cpio` → `target/iso/boot/initramfs.cpio`
4. Generate GRUB config:
   ```
   menuentry "NexaOS" {
       multiboot2 /boot/kernel.bin root=/dev/vda1 rootfstype=ext2 loglevel=debug
       module2 /boot/initramfs.cpio initramfs
   }
   ```
5. Create ISO with GRUB bootloader:
   ```bash
   grub-mkrescue -o dist/nexaos.iso target/iso/
   ```

**Output**: 
- `dist/nexaos.iso` (~3 MB)

**Testing**:
```bash
./scripts/run-qemu.sh
# or
qemu-system-x86_64 -cdrom dist/nexaos.iso -m 512M -drive file=build/rootfs.ext2,format=raw
```

## Build Scripts

### `scripts/build-all.sh` (✅ RECOMMENDED)

Builds everything in the correct order with proper dependency tracking.

**Command**:
```bash
./scripts/build-all.sh
```

**Steps**:
1. **Build ext2 root filesystem** (`build-rootfs.sh`)
   - Compiles userspace programs (ni, shell, getty, login)
   - Creates 50 MB ext2 disk image
   - Populates with binaries and config files
   - Output: `build/rootfs.ext2`

2. **Build bootable ISO** (`build-iso.sh`)
   - Builds kernel (if not already built)
   - Creates initramfs CPIO archive
   - Generates GRUB ISO image
   - Output: `dist/nexaos.iso`

**Total Time**: ~30 seconds (first build), ~10 seconds (incremental)

**Output Summary**:
```
System components:
  - Kernel: target/x86_64-nexaos/release/nexa-os (2.1 MiB)
  - Initramfs: build/initramfs.cpio (40 KiB)
  - Root FS: build/rootfs.ext2 (50 MiB)
  - ISO: dist/nexaos.iso (3.2 MiB)

To run in QEMU:
  ./scripts/run-qemu.sh

Boot parameters (in GRUB):
  root=/dev/vda1 rootfstype=ext2 loglevel=debug
```

### Individual Build Scripts

#### `scripts/build-userspace.sh` - Build Initramfs Only

**Purpose**: Create minimal initramfs for emergency boot

**Command**:
```bash
./scripts/build-userspace.sh
```

**Process**:
1. Create workspace: `build/userspace-build/`
2. Compile minimal shell: `cargo build --release --target x86_64-nexaos-userspace.json`
3. Copy shell to `build/initramfs/bin/sh`
4. Copy dynamic linker (if needed)
5. Create CPIO archive: `build/initramfs.cpio`

**Use Case**: 
- Quick iteration on shell code
- Emergency recovery testing
- Initramfs debugging

**Output**: `build/initramfs.cpio` (~40 KB)

#### `scripts/build-rootfs.sh` - Build Full Root Filesystem

**Purpose**: Create complete ext2 root filesystem with all userspace programs

**Command**:
```bash
./scripts/build-rootfs.sh
```

**Process**:
1. Create workspace: `build/userspace-build/`
2. Compile all userspace programs:
   ```bash
   cd build/userspace-build
   cargo build --release --target x86_64-nexaos-userspace.json
   ```
   - `ni` (init system, PID 1)
   - `shell` (interactive shell)
   - `getty` (terminal manager)
   - `login` (authentication)
3. Create ext2 filesystem:
   ```bash
   dd if=/dev/zero of=build/rootfs.ext2 bs=1M count=50
   mkfs.ext2 -F -b 4096 build/rootfs.ext2
   ```
4. Mount and populate:
   ```bash
   sudo mount -o loop build/rootfs.ext2 build/rootfs
   sudo mkdir -p build/rootfs/{bin,sbin,etc,dev,proc,sys,tmp,var,home,root}
   sudo cp binaries...
   sudo cp etc/inittab build/rootfs/etc/
   sudo ln -s ni build/rootfs/sbin/init
   sudo umount build/rootfs
   ```

**Use Case**:
- Full system testing
- Init system development
- Multi-user testing

**Output**: `build/rootfs.ext2` (50 MB)

**Dependencies**: Requires `sudo` for mounting ext2 image

#### `scripts/build-rootfs-debug.sh` - Debug Root Filesystem

**Purpose**: Create root filesystem with debug symbols and verbose logging

**Command**:
```bash
./scripts/build-rootfs-debug.sh
```

**Differences from Release**:
- Compile with `cargo build` (no `--release`)
- Include debug symbols (larger binaries)
- Enable verbose logging (kinfo!, kdebug!)
- Output: `build/rootfs-debug.ext2`

**Use Case**:
- Debugging userspace programs
- Tracing system calls
- Investigating crashes

#### `scripts/build-iso.sh` - Build Bootable ISO

**Purpose**: Create GRUB-based bootable ISO with kernel and initramfs

**Command**:
```bash
./scripts/build-iso.sh
```

**Process**:
1. Build kernel (if needed):
   ```bash
   cargo build --release
   ```
2. Build initramfs (if needed):
   ```bash
   ./scripts/build-userspace.sh
   ```
3. Create ISO staging directory:
   ```bash
   mkdir -p target/iso/boot/grub
   ```
4. Copy files:
   ```bash
   cp target/x86_64-nexaos/release/nexa-os target/iso/boot/kernel.bin
   cp build/initramfs.cpio target/iso/boot/initramfs.cpio
   ```
5. Generate GRUB config (`target/iso/boot/grub/grub.cfg`):
   ```
   set timeout=3
   set default=0
   
   menuentry "NexaOS" {
       multiboot2 /boot/kernel.bin root=/dev/vda1 rootfstype=ext2 loglevel=debug
       module2 /boot/initramfs.cpio initramfs
   }
   ```
6. Create ISO with GRUB:
   ```bash
   grub-mkrescue -o dist/nexaos.iso target/iso/
   ```

**Output**: `dist/nexaos.iso` (~3 MB)

**Use Case**:
- Boot on real hardware (burn to USB/CD)
- VM testing (VirtualBox, VMware)
- Distribution packaging

#### `scripts/build-iso-release.sh` - Release ISO with Debug Rootfs

**Purpose**: Create ISO with release kernel but debug rootfs

**Command**:
```bash
./scripts/build-iso-release.sh
```

**Use Case**: Debugging userspace while keeping kernel optimized

#### `scripts/run-qemu.sh` - Run in QEMU

**Purpose**: Boot NexaOS in QEMU with proper configuration

**Command**:
```bash
./scripts/run-qemu.sh
```

**QEMU Configuration**:
```bash
qemu-system-x86_64 \
    -cdrom dist/nexaos.iso \
    -m 512M \
    -serial stdio \
    -drive file=build/rootfs.ext2,format=raw,if=virtio \
    -display curses \
    -enable-kvm  # if available
```

**Options**:
- `-cdrom dist/nexaos.iso`: Boot from ISO
- `-m 512M`: 512 MB RAM
- `-serial stdio`: Serial console to terminal
- `-drive file=build/rootfs.ext2,format=raw,if=virtio`: Attach root disk as /dev/vda
- `-display curses`: Text UI (alternative: `-nographic`)
- `-enable-kvm`: Hardware acceleration (Linux only)

**Use Case**: Primary development and testing environment
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
