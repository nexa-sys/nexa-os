# Swap Subsystem Implementation for NexaOS

## Overview

NexaOS implements swap support as a loadable kernel module (`.nkm`), following the modular design philosophy of the kernel. This allows swap functionality to be loaded on demand and keeps the core kernel lean.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    User Space                           │
│  swapon /dev/swap0    swapoff /dev/swap0    free -m     │
└───────────────────────┬─────────────────────────────────┘
                        │ syscall (SYS_SWAPON=167, SYS_SWAPOFF=168)
┌───────────────────────▼─────────────────────────────────┐
│                  Kernel Core                            │
│  src/mm/swap.rs - Registration API, swap operations     │
│  src/syscalls/swap.rs - System call handlers            │
└───────────────────────┬─────────────────────────────────┘
                        │ kmod API (kmod_swap_register/unregister)
┌───────────────────────▼─────────────────────────────────┐
│              Swap Module (modules/swap/)                │
│  - Linux-compatible swap header parsing                 │
│  - Bitmap-based slot allocation                         │
│  - Block device I/O for page swap                       │
│  - Multi-device support with priorities                 │
└─────────────────────────────────────────────────────────┘
```

## Components

### 1. Kernel Swap API (`src/mm/swap.rs`)

The kernel provides:

- **Module registration**: `kmod_swap_register()` / `kmod_swap_unregister()`
- **Swap operations**: `swap_out()`, `swap_in()`, `swap_free()`
- **Statistics**: `get_swap_stats()`, `get_swap_info()`
- **Swap entry encoding**: 64-bit entry with device ID and offset

### 2. Swap Module (`modules/swap/`)

The loadable module implements:

- **Device management**: Up to 8 swap devices
- **Header parsing**: Linux-compatible `SWAPSPACE2` format
- **Slot allocation**: Bitmap-based tracking (64-bit words)
- **I/O operations**: Read/write pages via `kmod_blk_*` APIs
- **Priority scheduling**: Higher priority devices preferred

### 3. System Calls

| Syscall | Number | Arguments | Description |
|---------|--------|-----------|-------------|
| `swapon` | 167 | path, flags | Activate swap device |
| `swapoff` | 168 | path | Deactivate swap device |

### 4. Proc Filesystem Integration

- `/proc/meminfo`: Shows `SwapTotal` and `SwapFree`
- `/proc/swaps`: Lists active swap devices

## Usage

### Loading the Swap Module

```bash
# Load swap module (from initramfs during boot or manually)
modprobe swap
```

### Creating a Swap File/Partition

```bash
# Create swap file (in userspace)
dd if=/dev/zero of=/swapfile bs=1M count=256
mkswap /swapfile

# Or use a partition
mkswap /dev/sda2
```

### Activating Swap

```bash
# Enable swap
swapon /dev/sda2

# With priority (0-32767, higher = preferred)
swapon -p 10 /dev/sda2
```

### Checking Swap Status

```bash
# View swap devices
cat /proc/swaps

# View memory including swap
free -m
cat /proc/meminfo | grep Swap
```

### Deactivating Swap

```bash
swapoff /dev/sda2
```

## Swap Header Format

The module uses Linux-compatible swap header format:

```
Offset    Size    Field
0         1024    Boot block (reserved)
1024      4       Version (1)
1028      4       Last page number
1032      4       Number of bad pages
1036      16      UUID
1052      16      Volume name
...
4086      10      Magic "SWAPSPACE2"
```

## Kernel Module API

### Operations Table

```c
struct SwapModuleOps {
    int (*swapon)(path, len, flags);    // Activate device
    int (*swapoff)(path, len);          // Deactivate device
    int (*alloc_slot)(out_dev, out_off);// Allocate swap slot
    int (*free_slot)(dev, offset);      // Free swap slot
    int (*write_page)(dev, off, data);  // Write page to swap
    int (*read_page)(dev, off, data);   // Read page from swap
    int (*get_stats)(total, free);      // Get statistics
};
```

### Exported Kernel Symbols

| Symbol | Description |
|--------|-------------|
| `kmod_swap_register` | Register swap module |
| `kmod_swap_unregister` | Unregister swap module |
| `kmod_blk_read_bytes` | Read from block device |
| `kmod_blk_write_bytes` | Write to block device |

## Swap Entry Encoding

Swap entries are 64-bit values encoded as:

```
[63:56] Device index (8 bits, 0-255)
[55:0]  Page offset within device (56 bits)
```

This allows addressing up to 256 swap devices with 2^56 pages each (256PB per device).

## Memory Integration

### Page Table Entries

Swapped pages use special PTE encoding:
- Present bit (bit 0) = 0
- Swap entry in bits [63:1]

### Helper Functions

```rust
pte_is_swap(pte)       // Check if PTE is a swap entry
make_swap_pte(entry)   // Create swap PTE from entry
pte_to_swap_entry(pte) // Extract swap entry from PTE
```

## Future Enhancements

1. **Swap Priority Scheduling**: Implement more sophisticated scheduling
2. **Swap Compression**: zswap/zram-like compressed swap cache
3. **Swap Encryption**: Encrypted swap for security
4. **Swap Clustering**: Group related pages for better I/O
5. **TRIM Support**: SSD discard for freed swap pages
6. **Memory Pressure Notifications**: OOM killer integration

## Building

```bash
# Build swap module
cd modules/swap
cargo build --release --target ../../targets/x86_64-nexaos-module.json

# Or use the build system
./scripts/build.sh modules
```

## References

- Linux Kernel Swap Implementation
- `mm/swapfile.c`, `mm/swap_state.c` in Linux source
- `SWAPSPACE2` header format specification
