# NexaOS Build System Complete Guide

> **Last Updated**: 2025-11-25  
> **Build Architecture**: Three-stage boot (Kernel + Initramfs + ext2 rootfs)  
> **Supported**: x86_64, UEFI/BIOS boot modes

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Build Architecture](#build-architecture)
3. [Build Scripts](#build-scripts)
4. [Build Components](#build-components)
5. [Customization](#customization)
6. [Troubleshooting](#troubleshooting)
7. [Advanced](#advanced)

---

## Quick Start

### Complete Build
```bash
# Build entire system at once
./scripts/build-all.sh

# Output artifacts:
# - target/x86_64-nexaos/release/nexa-os (kernel)
# - build/initramfs.cpio (bootstrap environment)
# - build/rootfs.ext2 (full filesystem)
# - dist/nexaos.iso (bootable image)

# Run in QEMU
./scripts/run-qemu.sh

# Exit QEMU: Ctrl-A X (or Ctrl-C)
```

### Incremental Development

```bash
# Modify kernel code
vim src/lib.rs

# Rebuild kernel only (fast)
cargo build --release --target x86_64-nexaos.json

# Test without rebuilding filesystem
./scripts/run-qemu.sh
```

---

## Build Architecture

### Three-Stage Boot Process

```
┌─────────────────────────────────────────────────────────────┐
│                    Boot Flow Diagram                         │
└─────────────────────────────────────────────────────────────┘

FIRMWARE (BIOS/UEFI)
    ↓
BOOTLOADER (GRUB)
    ↓ loads from ISO
KERNEL BINARY (nexa-os)
    ├─ 6-stage boot process
    ├─ Setup: GDT, IDT, paging, interrupts
    ├─ Initialize subsystems
    └─ Switch to Ring 3 (userspace)
    ↓
INITRAMFS (cpio archive)
    ├─ Minimal environment: /init, /bin/sh, dynamic linker
    ├─ Mount /proc, /sys, /dev
    ├─ Parse GRUB modules
    └─ Parse /etc/inittab
    ↓
ROOT FILESYSTEM (ext2 disk)
    ├─ Load if kernel parameter specifies: root=/dev/vda1
    ├─ Unmount initramfs
    ├─ Mount rootfs on /
    └─ Run /sbin/ni (init system)
    ↓
USER SPACE
    ├─ /sbin/ni manages services
    ├─ /sbin/getty spawns login prompts
    ├─ /bin/shell user interface
    └─ Applications and daemons
```

### Component Sizes (Debug/Release)

| Component | Debug | Release | Purpose |
|-----------|-------|---------|---------|
| Kernel | ~50 MB | ~2 MB | Core OS binary |
| Initramfs | ~400 KB | ~380 KB | Bootstrap environment |
| Root FS | ~60 MB | ~50 MB | Full system filesystem |
| ISO | ~80 MB | ~65 MB | Bootable image |

---

## Build Scripts

### Main Build Script

**File**: `./scripts/build-all.sh`

**What it does**:
1. Verifies prerequisites (cargo, grub-mkrescue, etc.)
2. Builds kernel
3. Builds userspace programs
4. Creates initramfs archive
5. Builds ext2 root filesystem
6. Packages bootable ISO
7. Validates ISO

**Typical runtime**: 2-5 minutes (release build)

### Component Build Scripts

#### Kernel Only
```bash
cargo build --release --target x86_64-nexaos.json
# Output: target/x86_64-nexaos/release/nexa-os
# Time: 30 seconds - 2 minutes (depends on changes)
```

#### Userspace Programs
```bash
./scripts/build-userspace.sh
# Builds: ni, getty, shell, login, and support binaries
# Time: 30 seconds
```

#### Initramfs (Bootstrap Environment)
```bash
# Handled by build-rootfs.sh, but individual steps:
# 1. Compile emergency shell
# 2. Copy dynamic linker
# 3. Create CPIO archive
```

#### Root Filesystem
```bash
./scripts/build-rootfs.sh
# Creates ext2 disk image with full system
# Time: 1-2 minutes
```

#### ISO Image
```bash
./scripts/build-iso.sh
# Creates bootable ISO with GRUB, kernel, initramfs
# Time: 30 seconds
```

---

## Build Components

### 1. Kernel Build

**Source**: `src/` directory

**Build command**:
```bash
cargo build --release --target x86_64-nexaos.json
```

**What's included**:
- Core kernel: process management, memory, scheduling
- Syscall interface (38+ syscalls)
- Network stack (UDP/TCP)
- Filesystem (ext2 read/write)
- Device drivers
- Interrupt handlers

**Verification**:
```bash
grub-file --is-x86-multiboot2 target/x86_64-nexaos/release/nexa-os
```

### 2. Userspace Programs

**Location**: `userspace/*.rs`

**Programs built**:
- `init.rs` → `/sbin/ni` (PID 1 init system)
- `getty.rs` → `/sbin/getty` (terminal manager)
- `shell.rs` → `/bin/shell` (interactive shell)
- `login.rs` → `/bin/login` (authentication)
- Plus utilities: `ip`, `nslookup`, `dhcp`, etc.

**Compiled as**:
- Static binaries (for initramfs)
- Dynamic binaries (for rootfs, linked against `nrlib`)

### 3. Initramfs (Initial RAM Filesystem)

**Type**: CPIO archive

**Contents**:
```
/init                    # Bootstrap script
/bin/shell              # Emergency shell
/lib64/ld-linux.so      # Dynamic linker
/dev/                   # Device nodes
/proc/                  # Virtual filesystem mount point
/sys/                   # Virtual filesystem mount point
/sysroot/               # Temporary rootfs mount point
```

**Size**: ~380 KB (compressed in ISO)

**Purpose**:
- Minimal environment for early boot
- Discovers and mounts root filesystem
- Loads necessary drivers
- Initializes virtual filesystems

### 4. Root Filesystem (ext2)

**Type**: ext2 disk image

**Location**: `build/rootfs.ext2` (50 MB)

**Layout**:
```
/bin/              # User binaries (shell, utilities)
/sbin/             # System binaries (ni, getty, login)
/lib/              # System libraries
/lib64/            # 64-bit libraries and linker
/etc/              # Configuration files
  inittab          # Init configuration
  ni/              # Service configs
/home/             # User directories
/root/             # Root user home
/tmp/              # Temporary files
/var/              # Variable data
/proc/             # Virtual: processes
/sys/              # Virtual: system info
/dev/              # Virtual: devices
/mnt/              # Mount points
```

**Build process**:
1. Create empty ext2 image (50 MB)
2. Mount as loop device
3. Copy files from `build/rootfs/`
4. Compile dynamic binaries
5. Copy to mounted filesystem
6. Unmount

### 5. Bootable ISO

**Tool**: xorriso + GRUB

**Components**:
- GRUB bootloader (BIOS + UEFI support)
- Kernel binary (embedded in ISO)
- Initramfs archive (embedded in ISO)
- Boot parameters

**Boot methods supported**:
- BIOS MBR
- UEFI (GPT)

---

## Customization

### Change Kernel Features

**File**: `src/lib.rs` (feature flags at top)

```rust
// Enable/disable features
#[cfg(feature = "net")]
pub mod net;

#[cfg(feature = "ext2_write")]
pub mod ext2_write;
```

**Rebuild**:
```bash
cargo build --release --target x86_64-nexaos.json --features "net,ext2_write"
```

### Add User Program to Rootfs

1. Create `userspace/myprogram.rs`:
```rust
fn main() {
    println!("Hello from myprogram!");
}
```

2. Add to `build/rootfs/bin/`:
```bash
# Done automatically during rootfs build
./scripts/build-rootfs.sh
```

3. Rebuild ISO:
```bash
./scripts/build-iso.sh
```

### Change Boot Parameters

**File**: `scripts/build-iso.sh`

**Current parameters**:
```bash
root=/dev/vda1 rootfstype=ext2 loglevel=debug
```

**Available parameters**:
- `root=/dev/vda1` - Root device (for QEMU: vda, real: sda, etc.)
- `rootfstype=ext2` - Root filesystem type
- `loglevel=debug` - Logging level (debug/info/warn/error)
- `init=/sbin/ni` - Init program to run

### Enable Debug Logging

**Option 1: Debug build**
```bash
./scripts/build-rootfs-debug.sh
```

**Option 2: Boot parameter**
```
loglevel=debug
```

**Monitor logs**:
```bash
tail -f /tmp/qemu-serial.log  # In separate terminal
```

---

## Troubleshooting

### Build Fails

**Error**: `cargo build` fails

**Steps**:
1. Check Rust version: `rustc --version` (should be nightly)
2. Update toolchain: `rustup update`
3. Clean build: `cargo clean && cargo build --release --target x86_64-nexaos.json`

**Error**: `grub-mkrescue not found`

**Fix**: Install GRUB tools
```bash
sudo apt-get install grub2-common grub-pc-bin  # Linux
brew install grub xorriso                       # macOS
```

### ISO Won't Boot

**Symptoms**: QEMU error, system hangs at startup

**Steps**:
1. Verify kernel is Multiboot2: `grub-file --is-x86-multiboot2 target/x86_64-nexaos/release/nexa-os`
2. Rebuild completely: `./scripts/build-all.sh`
3. Check QEMU command: Ensure `-drive file=build/rootfs.ext2` for rootfs
4. Monitor serial output: `./scripts/run-qemu.sh` watches serial automatically

### System Boots But Hangs

**Problem**: Kernel loads but init doesn't start

**Debugging**:
1. Enable debug logging: `loglevel=debug` in boot params
2. Watch serial: `tail -f /tmp/qemu-serial.log`
3. Look for: Last syscall, interrupt, or error message
4. Check init binary: Ensure `/sbin/ni` exists in rootfs

### Rootfs Changes Not Appearing

**Problem**: Modified program doesn't run

**Solution**:
1. Rebuild rootfs: `./scripts/build-rootfs.sh`
2. Rebuild ISO: `./scripts/build-iso.sh`
3. Verify files: `ls -la build/rootfs/bin/`

---

## Advanced

### Rebuild Individual Steps

```bash
# Just kernel
cargo build --release --target x86_64-nexaos.json

# Kernel + initramfs only (skip rootfs)
cargo build --release --target x86_64-nexaos.json
./scripts/build-userspace.sh
# ... manually create initramfs if needed

# Rootfs only
./scripts/build-rootfs.sh

# ISO only (use existing kernel + initramfs)
./scripts/build-iso.sh
```

### Cross-Compile

**Prerequisites**: 
- LLVM with x86_64 support
- Custom linker script: `linker.ld`

**Build for different target**:
```bash
# Current: x86_64 using custom JSON target
cargo build --release --target x86_64-nexaos.json

# Could add: aarch64, riscv64, etc. (with appropriate target files)
```

### Performance Profiling

```bash
# Release build (optimized)
cargo build --release

# Debug build (symbols, logging)
cargo build

# Debug with profiling
# Use qemu -d trace_help for instruction tracing
```

### CI/CD Integration

**GitHub Actions example**:
```yaml
- name: Build NexaOS
  run: ./scripts/build-all.sh

- name: Run tests
  run: ./scripts/test-*.sh

- name: Upload artifacts
  uses: actions/upload-artifact@v2
  with:
    name: nexaos-iso
    path: dist/nexaos.iso
```

---

## Architecture Details

### Target Specification

**File**: `x86_64-nexaos.json`

**Key settings**:
```json
{
  "llvm-target": "x86_64-unknown-none",
  "cpu": "x86-64",
  "features": "+mmx,+sse,+sse2",
  "code-model": "kernel",
  "data-layout": "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
}
```

### Linking

**Linker script**: `linker.ld`

**Sections**:
```
.text     - Code (0x400000 in kernel space)
.data     - Initialized data
.rodata   - Read-only data
.bss      - Uninitialized data
```

### Memory Layout

**Kernel space**:
- `0x0000000000000000` - I/O and memory-mapped regions
- `0xffffffff80000000` - Kernel code and data (higher half)

**User space**:
- `0x0000000000000000` - User heap, stack, code
- `0x0000400000000000` - Large allocations (future)

---

## Performance Tips

1. **Use release builds** (`--release`) for final system
2. **Incremental kernelcompilation**: Only rebuild changed modules
3. **Parallel builds**: Cargo uses all cores by default
4. **Enable LTO**: Add to `Cargo.toml`:
   ```toml
   [profile.release]
   lto = true
   ```

---

## Related Documentation

- [QUICK-REFERENCE.md](QUICK-REFERENCE.md) - Build commands at a glance
- [ARCHITECTURE.md](ARCHITECTURE.md) - System design
- [DEBUG-BUILD.md](DEBUG-BUILD.md) - Debug mode details
- [../README.md](../README.md) - Full documentation index

---

**Last Reviewed**: 2025-11-25  
**Maintainer**: NexaOS Development Team  
**Status**: ✅ Complete and up-to-date

