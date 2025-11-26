# NexaOS AI Coding Guide

## Architecture Overview
NexaOS is a Rust `no_std` hybrid kernel with 6-stage boot (`src/boot_stages.rs`): Bootloader → KernelInit → Initramfs → RootSwitch → RealRoot → UserSpace.

### Key Subsystems
| Component | Location | Purpose |
|-----------|----------|---------|
| Boot entry | `src/main.rs` → `src/lib.rs` | Multiboot2 → kernel_main |
| Memory | `src/paging.rs`, `src/process/types.rs` | Identity-mapped kernel, isolated userspace |
| Scheduler | `src/scheduler/` | Round-robin with priorities |
| Syscalls | `src/syscalls/` | 38+ POSIX syscalls, organized by domain |
| Filesystems | `src/initramfs.rs`, `src/fs.rs` | CPIO initramfs + ext2 rootfs after stage 4 |
| Safety helpers | `src/safety/` | Centralized unsafe wrappers (volatile, MMIO, port I/O) |

### Memory Layout Constants (`src/process/types.rs`)
```rust
USER_VIRT_BASE: 0x400000   // Userspace code base
STACK_BASE:     0x800000   // User stack region
HEAP_BASE:      0x600000   // User heap region
INTERP_BASE:    0xA00000   // Dynamic linker region
```
**Critical**: Changes to these require coordinated updates in paging + ELF loader.

## Build & Test Workflows
```bash
./scripts/build-all.sh      # Full: kernel → userspace → rootfs → ISO (always use this)
./scripts/run-qemu.sh       # Boot in QEMU with serial console
cargo build --release --target x86_64-nexaos.json  # Kernel-only iteration
./scripts/build-rootfs.sh   # After userspace changes (BEFORE build-iso.sh!)
```
**Build order matters**: `build-rootfs.sh` → `build-iso.sh`, or ISO embeds stale rootfs.

## Coding Conventions

### Kernel Code (`src/`)
- **`no_std` only** — no heap allocations; use fixed-size buffers/arrays
- **Logging**: Use `kinfo!`, `kwarn!`, `kerror!`, `kdebug!`, `kfatal!` (not `println!`)
- **Error handling**: Propagate `Errno` equivalents; never panic in syscall paths
- **Unsafe code**: Route through `src/safety/` helpers when possible

### Userspace (`userspace/`)
- Target: `x86_64-nexaos-userspace.json`
- Programs expose `_start`; build via `scripts/build-userspace.sh`
- `userspace/nrlib/` — libc shim for Rust `std` (pthread stubs, TLS, syscall wrappers)
- Adding services: create binary in `userspace/`, add to `scripts/build-rootfs.sh`, register in `etc/inittab`

### Synchronization
Use `src/safety/` patterns with:
- Timeout/bailout mechanisms (prevent infinite hangs)
- Atomic Acquire/Release semantics
- Exponential backoff under contention
- Re-entrancy detection

## Critical Pitfalls
- **Never disable logging** — use log levels; serial output is essential for boot debugging
- **ProcessState consistency** — scheduler, signals, and IPC must stay synchronized when adding states
- **Dynamic linking** — PT_INTERP path (`/lib64/ld-linux.so`) must match rootfs layout
- **Rebuild rootfs** after touching `userspace/` or `etc/` before ISO packaging

## Debugging
- Serial console: enabled via `src/serial.rs`, logs to QEMU terminal
- Boot verification: `grub-file --is-x86-multiboot2 target/x86_64-nexaos/release/nexa-os`
- Use `git bisect` for regression hunting; verify all 6 boot stages complete