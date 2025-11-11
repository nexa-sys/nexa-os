# Rootfs Boot Process Implementation

This document describes the implementation of the complete rootfs boot process in NexaOS, following standard Linux boot conventions.

## Overview

The rootfs boot process has been implemented to follow the standard Linux 6-stage boot flow:

1. **Bootloader Stage** - GRUB loads kernel and initramfs
2. **Kernel Init Stage** - Hardware detection, memory setup, subsystem initialization
3. **Initramfs Stage** - Virtual filesystem mounting, device detection
4. **Root Switch Stage** - Pivot to real root filesystem
5. **Real Root Stage** - Remount and init startup
6. **User Space Stage** - Login, shell, services

## Implementation Status

### ✅ Completed Features

#### Boot Stage Management (`src/boot_stages.rs`)
- Complete state machine for tracking boot progress
- Stage transition logging
- Boot configuration parsing from kernel command line
- Emergency mode with recovery shell

#### Kernel Command Line Parsing
Supports the following parameters:
- `root=<device>` - Root device specification (e.g., `/dev/vda1`, `UUID=...`)
- `rootfstype=<type>` - Root filesystem type (e.g., `ext2`, `ext4`)
- `rootflags=<options>` - Mount options (`rw`, `ro`)
- `init=<path>` - Custom init program path
- `emergency` / `single` / `1` - Boot into emergency mode

#### Virtual Filesystem Support
- `/proc` - Process information pseudo-filesystem
- `/sys` - System and device information
  - `/sys/block` - Block device information
  - `/sys/class` - Device classes
  - `/sys/devices` - Device hierarchy
  - `/sys/kernel` - Kernel parameters
- `/dev` - Device nodes
  - `/dev/null`, `/dev/zero`, `/dev/console`

#### Emergency Mode
- Automatic activation on boot failures
- Diagnostic information display
- Emergency shell spawning
- Manual recovery instructions
- Device inspection capabilities

#### System Calls for Boot Process
- `SYS_MOUNT (165)` - Mount filesystems
- `SYS_UMOUNT (166)` - Unmount filesystems
- `SYS_PIVOT_ROOT (155)` - Change root filesystem
- `SYS_CHROOT (161)` - Change root directory

All syscalls require superuser privileges and include proper error handling.

### ⚙️ Partially Implemented

#### Device Detection
- Basic framework in place
- Simulated device waiting
- No actual udev implementation yet

#### Root Device Mounting
- Framework for mounting at `/sysroot`
- Currently a stub implementation
- Requires actual block device drivers

#### Pivot Root
- Syscall interface implemented
- Framework for root switch
- Full implementation requires:
  - Mount point tracking
  - Root filesystem migration
  - Memory cleanup of initramfs

### ❌ Future Work

#### Block Device Support
- Block device layer
- IDE/SATA/NVMe drivers
- virtio-blk driver for QEMU

#### Filesystem Drivers
- ext2/ext3/ext4 support
- FAT32 support
- Filesystem checking (fsck)

#### Device Management
- Proper udev implementation
- Device hotplug support
- Module autoloading

#### Advanced Features
- Real pivot_root implementation
- Mount namespace support
- Multiple filesystem mounts
- Device mapper support

## Architecture

### Boot Flow Diagram

```
┌──────────────────────────────────────────────────────────────┐
│                    BOOT FLOW SEQUENCE                         │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  1. GRUB → Loads kernel + initramfs                          │
│           ↓                                                   │
│  2. kernel_main() → Initialize hardware                      │
│                   → Parse boot config                         │
│                   → Load initramfs                            │
│                   → Initialize subsystems                     │
│           ↓                                                   │
│  3. initramfs_stage() → Mount /proc, /sys, /dev             │
│                        → Wait for root device                 │
│                        → Detect and prepare root              │
│           ↓                                                   │
│  4. mount_real_root() → Mount root at /sysroot              │
│           ↓                                                   │
│  5. pivot_to_real_root() → Switch to new root               │
│                          → Move mount points                  │
│                          → Clean up initramfs                 │
│           ↓                                                   │
│  6. start_real_root_init() → Remount rw                     │
│                            → Start init process               │
│           ↓                                                   │
│  7. User Space → Login → Shell → Services                    │
│                                                               │
└──────────────────────────────────────────────────────────────┘
```

### Module Interactions

```
src/lib.rs (kernel_main)
    ↓
src/boot_stages.rs (state management)
    ↓
    ├─→ src/fs.rs (virtual fs)
    ├─→ src/initramfs.rs (CPIO parsing)
    ├─→ src/syscall.rs (mount/pivot_root)
    └─→ src/init.rs (init process)
```

## Usage Examples

### Basic Boot (initramfs as root)
```bash
# No kernel parameters needed - uses initramfs
qemu-system-x86_64 -cdrom nexaos.iso
```

### Boot with Real Root
```bash
# GRUB command line
multiboot2 /boot/nexa-os.elf root=/dev/vda1 rootfstype=ext2 rw
module2 /boot/initramfs.cpio
```

### Emergency Mode
```bash
# Force emergency shell
multiboot2 /boot/nexa-os.elf emergency loglevel=debug
module2 /boot/initramfs.cpio
```

### Custom Init
```bash
# Use shell as init
multiboot2 /boot/nexa-os.elf init=/bin/sh loglevel=info
module2 /boot/initramfs.cpio
```

## Testing

### Build and Verify
```bash
# Build kernel with boot stages
cargo build --release

# Verify boot_stages module is included
./scripts/test-boot-stages.sh basic

# Build userspace and initramfs
./scripts/build-userspace.sh

# Create ISO (requires xorriso)
./scripts/build-iso.sh

# Run in QEMU
./scripts/run-qemu.sh
```

### Boot Stage Verification

Look for these log messages during boot:

```
[INFO] Stage 1: Bootloader - Complete
[INFO] Stage 2: Kernel Init - Starting...
[INFO] Boot config: root=/dev/vda1
[INFO] Stage 2: Kernel Init - Complete
[INFO] Stage 3: Initramfs Stage - Starting...
[INFO] Mounting /proc...
[INFO] /proc mounted successfully
[INFO] Mounting /sys...
[INFO] /sys mounted successfully
[INFO] Mounting /dev...
[INFO] /dev mounted successfully
[INFO] Stage 3: Initramfs Stage - Complete
[INFO] Stage 4: Root Mounting - Starting...
[INFO] Stage 5: Root Switch - Starting...
[INFO] Stage 6: User Space - Starting init process
```

## Troubleshooting

### Emergency Mode Activation

If boot fails, the system enters emergency mode:

```
==========================================================
EMERGENCY MODE: System cannot complete boot
Reason: Root device /dev/vda1 not found
==========================================================

Available actions:
  - Inspect /sys/block for available block devices
  - Check kernel log for error messages
  - Type 'exit' to attempt boot continuation
```

### Common Issues

**Issue**: No root device specified
- **Solution**: Add `root=` parameter to kernel command line

**Issue**: Root device not found
- **Cause**: Block device drivers not loaded or device doesn't exist
- **Solution**: Use initramfs as root or add proper drivers

**Issue**: Init process not found
- **Cause**: Initramfs doesn't contain init program
- **Solution**: Rebuild initramfs with `/bin/sh` or `/sbin/init`

## Implementation Notes

### Memory Safety
- All boot stage functions use Rust's safety guarantees
- Unsafe code is limited to:
  - Syscall parameter reading (validated pointers)
  - Low-level hardware access
  - Boot data structures

### Error Handling
- Every stage has proper error handling
- Failures trigger emergency mode
- Detailed error messages in kernel log
- No silent failures

### Performance
- Minimal overhead for stage tracking
- Virtual filesystem creation is fast (in-memory)
- No unnecessary I/O during boot

### Security
- All mount/pivot_root syscalls require root
- Proper permission checks
- No arbitrary code execution from initramfs

## Future Enhancements

### Short Term
1. Implement basic block device driver (virtio-blk)
2. Add ext2 filesystem support
3. Implement real device waiting
4. Complete pivot_root implementation

### Medium Term
1. Add udev-like device management
2. Implement module loading
3. Add filesystem checking
4. Support multiple mount points

### Long Term
1. Full Linux ABI compatibility for mount
2. Advanced filesystem support (ext4, btrfs)
3. Network root filesystem (NFS)
4. Encrypted root support

## References

- Linux Boot Process: https://www.kernel.org/doc/html/latest/admin-guide/initrd.html
- pivot_root(2): https://man7.org/linux/man-pages/man2/pivot_root.2.html
- Multiboot2 Specification: https://www.gnu.org/software/grub/manual/multiboot2/
- POSIX System Calls: https://pubs.opengroup.org/onlinepubs/9699919799/

## Contributing

When adding boot-related features:

1. Update `src/boot_stages.rs` if adding new stages
2. Add syscalls to `src/syscall.rs` if needed
3. Update documentation in `docs/zh/rootfs-boot-process.md`
4. Add tests for new functionality
5. Verify boot sequence still works

## License

Same as NexaOS project - see LICENSE file in repository root.
