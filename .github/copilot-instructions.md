# NexaOS AI Coding Guide

## Architecture Overview
NexaOS is a Rust `no_std` hybrid kernel with 6-stage boot (`src/boot/stages.rs`): Bootloader → KernelInit → Initramfs → RootSwitch → RealRoot → UserSpace.

### Key Subsystems
| Component | Location | Purpose |
|-----------|----------|---------|
| Boot entry | `src/main.rs` → `src/lib.rs` | Multiboot2 → kernel_main |
| Memory | `src/mm/paging.rs`, `src/process/types.rs` | Identity-mapped kernel, isolated userspace |
| Scheduler | `src/scheduler/` | Round-robin with priorities |
| Syscalls | `src/syscalls/` | 38+ POSIX syscalls, organized by domain (file, process, signal, network, etc.) |
| Filesystems | `src/fs/initramfs.rs`, `src/fs/` | CPIO initramfs + ext2 rootfs after stage 4 |
| Safety helpers | `src/safety/` | Centralized unsafe wrappers (volatile, MMIO, port I/O, packet casting) |
| Networking | `src/net/` | UDP/IPv4 stack, ARP, DNS; TCP in progress |

### Memory Layout Constants (`src/process/types.rs`)
```rust
USER_VIRT_BASE: 0x1000000  // Userspace code base (16MB, after kernel)
STACK_BASE:     0x1400000  // User stack region
HEAP_BASE:      0x1200000  // User heap region
INTERP_BASE:    0x1600000  // Dynamic linker region (16MB reserved)
```
**Critical**: Changes require coordinated updates in `src/mm/paging.rs` + `src/process/loader.rs`.

## Build & Test Workflows
```bash
./scripts/build.sh all      # Full: kernel → userspace → rootfs → ISO (always use this first)
./scripts/run-qemu.sh       # Boot in QEMU with serial console
./scripts/build.sh kernel iso             # After kernel-only changes
./scripts/build.sh userspace rootfs iso   # After userspace changes
```

### Environment Variables
```bash
BUILD_TYPE=debug ./scripts/build.sh all   # Debug build (default, stable)
BUILD_TYPE=release ./scripts/build.sh all # Release build (may cause fork/exec issues)
LOG_LEVEL=info ./scripts/build.sh kernel  # Set kernel log level (debug|info|warn|error)
```

**Build order matters**: Use `./scripts/build.sh all` or specify steps in dependency order.
**Important**: Debug builds recommended. Release (O3) may cause fork/exec crashes.

## Coding Conventions

### Kernel Code (`src/`)
- **`no_std` only** — no heap allocations; use fixed-size buffers/arrays
- **Logging**: Use `kinfo!`, `kwarn!`, `kerror!`, `kdebug!`, `kfatal!` (defined in `src/logger.rs`)
- **Error handling**: Propagate `Errno` values from `src/posix.rs`; never panic in syscall paths
- **Unsafe code**: Route through `src/safety/` helpers:
  - `safety::x86::{inb, outb, rdtsc}` — port I/O and CPU intrinsics
  - `safety::volatile::{volatile_read, volatile_write}` — MMIO
  - `safety::ptr::{copy_from_user, copy_to_user}` — user/kernel memory transfer
  - `safety::packet::{cast_header, cast_header_mut}` — network packet parsing

### Syscall Implementation (`src/syscalls/`)
Syscalls are organized by domain in separate files:
- `file.rs` (read/write/open/stat), `process.rs` (fork/exec/exit), `signal.rs`, `network.rs`, `memory.rs`
- Add new syscall numbers to `numbers.rs`, implement in domain file, wire up in `mod.rs:syscall_dispatch`

### Userspace (`userspace/`)
- Target: `targets/x86_64-nexaos-userspace.json`
- Programs expose `_start`; build via `./scripts/build.sh userspace`
- **nrlib** (`userspace/nrlib/`): libc shim enabling Rust `std` — provides pthread stubs, TLS, malloc, stdio, socket API
- **ld-nrlib** (`userspace/ld-nrlib/`): Dynamic linker for shared libraries
- Adding programs: create in `userspace/programs/`, add to workspace `Cargo.toml`, register in `etc/inittab` if service

### Service Registration (`etc/inittab`)
```
id:runlevels:action:process
1:2345:respawn:/sbin/getty 38400 tty1
```
Services use System V runlevels (0=halt, 3=multi-user, 6=reboot). Init is at `/sbin/ni`.

## Critical Pitfalls
- **Never disable logging** — serial output is essential for boot debugging; use log levels instead
- **ProcessState consistency** — scheduler (`src/scheduler/`), signals (`src/ipc/signal.rs`), and IPC must stay synchronized
- **Dynamic linking** — PT_INTERP path must match rootfs layout (`/lib64/ld-nrlib-x86_64.so.1`)
- **Rebuild rootfs** after touching `userspace/` or `etc/` (use `./scripts/build.sh userspace rootfs iso`)
- **Memory constants** — changing `USER_VIRT_BASE` or `STACK_BASE` breaks ELF loading; coordinate with paging

## Debugging
- Serial console: QEMU terminal receives all `kinfo!/kerror!` output
- Boot verification: `grub-file --is-x86-multiboot2 target/x86_64-nexaos/debug/nexa-os`
- GDB attach: `./scripts/run-qemu.sh -S -s` then `gdb -ex "target remote :1234"`
- Kernel logs: `dmesg` in userspace reads ring buffer via `SYS_SYSLOG`