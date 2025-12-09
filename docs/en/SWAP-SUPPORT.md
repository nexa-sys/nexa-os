# NexaOS Swap Support

## Overview

NexaOS implements swap support as a loadable kernel module, providing:

- **Linux-compatible swap format** (SWAPSPACE2 magic signature)
- **Multiple swap devices** with configurable priorities
- **Priority-based scheduling** (higher priority = preferred)
- **Standard syscalls**: `swapon(2)` and `swapoff(2)` (Linux x86_64 compatible)
- **Userspace tools**: `swapon`, `swapoff`, `mkswap`, `free`

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                    Userspace Tools                           │
│  ┌─────────┐  ┌──────────┐  ┌────────┐  ┌──────┐            │
│  │ swapon  │  │ swapoff  │  │ mkswap │  │ free │            │
│  └────┬────┘  └────┬─────┘  └────────┘  └──┬───┘            │
│       │            │                        │                │
│       │ syscall(167)│ syscall(168)         │ /proc/meminfo  │
└───────┼────────────┼───────────────────────┼────────────────┘
        │            │                        │
┌───────▼────────────▼────────────────────────▼────────────────┐
│                    Kernel (src/syscalls/)                    │
│  ┌────────────────────────────────────────────────────────┐  │
│  │                    sys_swapon / sys_swapoff             │  │
│  └──────────────────────────┬─────────────────────────────┘  │
│                              │                                │
│  ┌──────────────────────────▼─────────────────────────────┐  │
│  │               Swap Registration API (src/mm/swap.rs)    │  │
│  │    kmod_swap_register() / kmod_swap_unregister()        │  │
│  └──────────────────────────┬─────────────────────────────┘  │
└─────────────────────────────┼────────────────────────────────┘
                              │
┌─────────────────────────────▼────────────────────────────────┐
│               Swap Kernel Module (modules/swap/)             │
│  ┌────────────────────────────────────────────────────────┐  │
│  │  • Header parsing (SWAPSPACE2)                         │  │
│  │  • Bitmap-based slot allocation                        │  │
│  │  • Multi-device management with priorities             │  │
│  │  • Read/Write operations via block device              │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

## Files

### Kernel Module
- `modules/swap/Cargo.toml` - Module package definition
- `modules/swap/src/lib.rs` - Swap module implementation

### Kernel Integration
- `src/mm/swap.rs` - Swap registration API and page operations
- `src/syscalls/swap.rs` - swapon/swapoff syscall handlers
- `src/syscalls/numbers.rs` - SYS_SWAPON=167, SYS_SWAPOFF=168
- `src/kmod/symbols.rs` - Exported symbols for module

### Userspace Tools
- `userspace/programs/swapon/` - Enable swap devices
- `userspace/programs/swapoff/` - Disable swap devices
- `userspace/programs/mkswap/` - Create swap signature
- `userspace/programs/free/` - Display memory usage

### Configuration
- `etc/fstab` - Swap device entry (`/dev/vdb`)
- `scripts/run-qemu.sh` - Creates swap.img and attaches as virtio device

## Usage

### Quick Start

```bash
# 1. Build everything
./scripts/build.sh all

# 2. Run QEMU (automatically creates 256MB swap.img)
./scripts/run-qemu.sh

# 3. In NexaOS shell, enable swap
swapon /dev/vdb

# 4. Check swap status
swapon -s
free -h
cat /proc/swaps
```

### Userspace Commands

#### swapon - Enable swap
```bash
swapon /dev/vdb           # Enable swap on device
swapon -p 10 /dev/vdb     # Enable with priority 10
swapon -a                 # Enable all from /etc/fstab
swapon -s                 # Show swap summary
```

#### swapoff - Disable swap
```bash
swapoff /dev/vdb          # Disable specific device
swapoff -a                # Disable all swap
swapoff -v /dev/vdb       # Verbose mode
```

#### mkswap - Create swap signature
```bash
mkswap /dev/vdb           # Format device as swap
mkswap -L myswap /dev/vdb # With label
mkswap /swapfile 256M     # Create swap file
```

#### free - Display memory
```bash
free                      # Show in kibibytes
free -m                   # Show in mebibytes
free -h                   # Human readable
free -t                   # Include total line
```

## Swap Entry Format

Each swap entry is encoded as a 64-bit value:

```
┌────────────────────────────────────────────────────────────────┐
│  Bit 63-52 (12 bits)  │  Bit 51-0 (52 bits)                   │
│  Device Index         │  Slot Index                            │
└────────────────────────────────────────────────────────────────┘
```

- **Device Index**: Supports up to 4096 swap devices
- **Slot Index**: 4PB addressable swap space per device

## Swap Header Format

Linux-compatible SWAPSPACE2 format:

```
Offset      Size    Field
---------------------------------------
0           1024    Reserved (bootblock)
1024        4       version (1)
1028        4       last_page
1032        4       nr_badpages
1036        16      uuid
1052        16      volume_name
...
4086        10      magic ("SWAPSPACE2")
```

## Configuration

### QEMU Swap Size

Set `SWAP_SIZE` environment variable:

```bash
SWAP_SIZE=512M./ndk run  # 512MB swap
SWAP_SIZE=1G./ndk run    # 1GB swap
```

### /etc/fstab Entry

```
/dev/vdb   none   swap   defaults,pri=10   0   0
```

Options:
- `pri=N` - Priority (0-32767, higher = preferred)
- `discard` - Enable TRIM/discard

### Automatic Swap at Boot

Add to init scripts:

```bash
# In /etc/rc.local or init script
swapon -a
```

## procfs Interface

### /proc/swaps
```
Filename              Type        Size      Used      Priority
/dev/vdb              partition   262140    0         10
```

### /proc/meminfo
```
SwapTotal:       262140 kB
SwapFree:        262140 kB
SwapCached:          0 kB
```

## Building

### Module Only
```bash
./scripts/build.sh modules
```

### Full Build
```bash
./scripts/build.sh all
```

## Limitations

Current implementation limitations:

1. **No swap file support** - Only block devices (swap files require filesystem I/O)
2. **No hibernation** - Suspend-to-disk not implemented
3. **No swap compression** - zswap/zram not available
4. **Fixed page size** - 4KB pages only
5. **No memory pressure notifications** - OOM killer basic

## Future Enhancements

- [ ] Swap file support (via VFS layer)
- [ ] zswap compressed cache
- [ ] Memory pressure notifications
- [ ] Swap encryption
- [ ] Better OOM handling
- [ ] Swap accounting per-process

## Troubleshooting

### "Invalid swap signature"
The device doesn't have a valid SWAPSPACE2 header. Run `mkswap` first:
```bash
mkswap /dev/vdb
```

### "Device or resource busy"
Swap is already enabled. Check with `swapon -s`.

### "Operation not permitted"
Run as root or check capabilities.

### Module not loading
Check kernel log with `dmesg`:
```bash
dmesg | grep -i swap
```
