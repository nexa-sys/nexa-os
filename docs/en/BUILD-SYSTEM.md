# NexaOS Build System Complete Guide

> **Last Updated**: 2025-12-04  
> **Build Architecture**: Six-stage boot (Kernel + Initramfs + ext2 rootfs)  
> **Supported**: x86_64, UEFI/BIOS boot modes

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Build Architecture](#build-architecture)
3. [Build Scripts](#build-scripts)
4. [Build Components](#build-components)
5. [Environment Variables](#environment-variables)
6. [Customization](#customization)
7. [Troubleshooting](#troubleshooting)
8. [Advanced](#advanced)

---

## Quick Start

### Complete Build
```bash
# Build entire system at once (modular build system)
./scripts/build.sh all

# Output artifacts:
# - target/x86_64-nexaos/debug/nexa-os (kernel, default)
# - build/initramfs.cpio (bootstrap environment)
# - build/rootfs.ext2 (full filesystem)
# - target/iso/nexaos.iso (bootable image)

# Run in QEMU
./scripts/run-qemu.sh

# Exit QEMU: Ctrl-A X (or Ctrl-C)
```

### Incremental Development

```bash
# Modify kernel code
vim src/lib.rs

# Rebuild kernel only (fast)
./scripts/build.sh kernel

# Test without rebuilding filesystem
./scripts/run-qemu.sh

# Rebuild just userspace and rootfs
./scripts/build.sh userspace rootfs iso
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
| Kernel | ~18 MB | ~6 MB | Core OS binary |
| Initramfs | ~400 KB | ~380 KB | Bootstrap environment |
| Root FS | ~60 MB | ~50 MB | Full system filesystem |
| ISO | ~80 MB | ~65 MB | Bootable image |

> **Note**: Debug builds (default) are recommended for stability. Release builds may cause fork/exec issues due to O3 optimization level.

---

## Build Scripts

### Modular Build System

The build system uses a modular architecture:

```
scripts/
├── build.sh              # Main orchestrator
├── lib/
│   └── common.sh         # Shared functions & variables
└── steps/
    ├── build-kernel.sh           # Kernel compilation
    ├── build-nrlib.sh            # Runtime library
    ├── build-userspace-programs.sh # User programs
    ├── build-modules.sh          # Kernel modules
    ├── build-initramfs.sh        # Initial ramdisk
    ├── build-rootfs.sh           # Root filesystem
    └── build-iso.sh              # Bootable ISO
```

### Main Build Script

**File**: `./scripts/build.sh`

**Usage**:
```bash
./scripts/build.sh <command> [command2] [command3] ...

# Available commands:
./scripts/build.sh kernel      # Build kernel only
./scripts/build.sh userspace   # Build nrlib + userspace programs
./scripts/build.sh modules     # Build kernel modules
./scripts/build.sh initramfs   # Build initramfs
./scripts/build.sh rootfs      # Build root filesystem
./scripts/build.sh iso         # Build bootable ISO
./scripts/build.sh all         # Build everything (full chain)

# Combine multiple steps:
./scripts/build.sh kernel iso           # Just kernel and ISO
./scripts/build.sh userspace rootfs iso # Userspace chain
```

**What `all` does**:
1. Builds kernel
2. Builds nrlib (runtime library)
3. Builds userspace programs
4. Builds kernel modules
5. Creates initramfs archive
6. Builds ext2 root filesystem
7. Packages bootable ISO

**Typical runtime**: 2-5 minutes (debug build)

### Component Build Scripts

#### Kernel Only
```bash
./scripts/build.sh kernel
# Or directly:
# cargo build --target targets/x86_64-nexaos.json (debug, default)
# cargo build --release --target targets/x86_64-nexaos.json (release)
# Output: target/x86_64-nexaos/{debug,release}/nexa-os
# Time: 30 seconds - 2 minutes (depends on changes)
```

#### Userspace Programs
```bash
./scripts/build.sh userspace
# Builds: nrlib + ni, getty, shell, login, and utilities
# Time: 30 seconds
```

#### Initramfs (Bootstrap Environment)
```bash
./scripts/build.sh initramfs
# Creates CPIO archive with minimal boot environment
# Time: 5 seconds
```

#### Root Filesystem
```bash
./scripts/build.sh rootfs
# Creates ext2 disk image with full system
# Time: 1-2 minutes
```

#### ISO Image
```bash
./scripts/build.sh iso
# Creates bootable ISO with GRUB, kernel, initramfs
# Time: 30 seconds
```

---

## Environment Variables

Configure build behavior via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `BUILD_TYPE` | `debug` | Build mode: `debug` or `release` |
| `LOG_LEVEL` | `debug` | Kernel log level: `debug`, `info`, `warn`, `error` |

**Examples**:
```bash
# Release build (faster runtime, smaller binaries)
BUILD_TYPE=release ./scripts/build.sh all

# Debug build with info logging
LOG_LEVEL=info ./scripts/build.sh kernel iso

# Combined settings
BUILD_TYPE=debug LOG_LEVEL=debug ./scripts/build.sh all
```

> **Warning**: Release builds (O3 optimization) may cause stability issues with fork/exec. Use debug builds for development.

---

## Build Components

### 1. Kernel Build

**Source**: `src/` directory

**Build command**:
```bash
./scripts/build.sh kernel
# Or: cargo build --target targets/x86_64-nexaos.json
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
grub-file --is-x86-multiboot2 target/x86_64-nexaos/debug/nexa-os
# Or for release:
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
./scripts/build.sh rootfs
```

3. Rebuild ISO:
```bash
./scripts/build.sh iso
```

### Change Boot Parameters

**File**: `scripts/steps/build-iso.sh`

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

**Option 1: Environment variable**
```bash
LOG_LEVEL=debug ./scripts/build.sh kernel iso
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
3. Clean build: `cargo clean && ./scripts/build.sh kernel`

**Error**: `grub-mkrescue not found`

**Fix**: Install GRUB tools
```bash
sudo apt-get install grub2-common grub-pc-bin xorriso  # Linux
brew install grub xorriso                               # macOS
```

### ISO Won't Boot

**Symptoms**: QEMU error, system hangs at startup

**Steps**:
1. Verify kernel is Multiboot2: `grub-file --is-x86-multiboot2 target/x86_64-nexaos/debug/nexa-os`
2. Rebuild completely: `./scripts/build.sh all`
3. Check QEMU command: Ensure `-drive file=build/rootfs.ext2` for rootfs
4. Monitor serial output: `./scripts/run-qemu.sh` watches serial automatically

### Fork/Exec Crashes in Release Mode

**Problem**: Child processes crash or hang with release builds

**Cause**: O3 optimization level causes issues with process creation

**Solution**: Use debug builds (default)
```bash
# This is the default and recommended:
./scripts/build.sh all

# If you explicitly used release, switch back:
BUILD_TYPE=debug ./scripts/build.sh kernel iso
```

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
1. Rebuild rootfs: `./scripts/build.sh rootfs`
2. Rebuild ISO: `./scripts/build.sh iso`
3. Verify files: `ls -la build/rootfs/bin/`

---

## Advanced

### Rebuild Individual Steps

```bash
# Just kernel
./scripts/build.sh kernel

# Userspace only (nrlib + programs)
./scripts/build.sh userspace

# Rootfs only
./scripts/build.sh rootfs

# ISO only (use existing kernel + initramfs)
./scripts/build.sh iso

# Multiple steps
./scripts/build.sh kernel iso
./scripts/build.sh userspace rootfs iso
```

### Cross-Compile

**Prerequisites**: 
- LLVM with x86_64 support
- Custom linker script: `linker.ld`

**Build for different target**:
```bash
# Current: x86_64 using custom JSON target
./scripts/build.sh kernel
# Uses: targets/x86_64-nexaos.json

# Could add: aarch64, riscv64, etc. (with appropriate target files)
```

### Performance Profiling

```bash
# Debug build (symbols, logging) - DEFAULT
./scripts/build.sh all

# Release build (optimized, may have stability issues)
BUILD_TYPE=release ./scripts/build.sh all

# Use qemu -d trace_help for instruction tracing
```

### CI/CD Integration

**GitHub Actions example**:
```yaml
- name: Build NexaOS
  run: ./scripts/build.sh all

- name: Run tests
  run: ./scripts/test-*.sh

- name: Upload artifacts
  uses: actions/upload-artifact@v2
  with:
    name: nexaos-iso
    path: target/iso/nexaos.iso
```

---

## Architecture Details

### Target Specification

**File**: `targets/x86_64-nexaos.json`

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

1. **Use debug builds** (default) for stability during development
2. **Incremental kernel compilation**: Only rebuild changed modules
3. **Parallel builds**: Cargo uses all cores by default
4. **Release builds**: Only use for final deployment after thorough testing
   ```toml
   # In Cargo.toml, release profile uses opt-level = 2 for stability
   [profile.release]
   opt-level = 2
   ```

---

## Related Documentation

- [QUICK-REFERENCE.md](QUICK-REFERENCE.md) - Build commands at a glance
- [ARCHITECTURE.md](ARCHITECTURE.md) - System design
- [DEBUG-BUILD.md](DEBUG-BUILD.md) - Debug mode details
- [../README.md](../README.md) - Full documentation index
- [../../scripts/README.md](../../scripts/README.md) - Build script quick reference

---

**Last Reviewed**: 2025-12-04  
**Maintainer**: NexaOS Development Team  
**Status**: ✅ Complete and up-to-date

