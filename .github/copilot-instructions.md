# NexaOS AI Coding Guide

## Architecture Overview
NexaOS is a Rust `no_std` hybrid kernel with 6-stage boot (`src/boot/stages.rs`): Bootloader → KernelInit → Initramfs → RootSwitch → RealRoot → UserSpace. The kernel runs in Ring 0, userspace in Ring 3 with full POSIX compliance.

### Key Subsystems
| Component | Location | Purpose |
|-----------|----------|---------|
| Boot entry | `src/main.rs` → `src/lib.rs` | Multiboot2 → kernel_main |
| Memory | `src/mm/paging.rs`, `src/process/types.rs` | Identity-mapped kernel, isolated userspace with 4-level paging |
| Scheduler | `src/scheduler/` | Round-robin with priorities |
| Syscalls | `src/syscalls/` | 50+ POSIX syscalls, organized by domain (file, process, signal, network, memory, thread, time) |
| Filesystems | `src/fs/initramfs.rs`, `src/fs/` | CPIO initramfs → ext2 rootfs after pivot_root (stage 4) |
| Safety helpers | `src/safety/` | Centralized unsafe wrappers (volatile, MMIO, port I/O, packet casting) |
| Networking | `src/net/` | Full UDP/IPv4 stack, ARP, DNS resolver; TCP in progress |
| Kernel modules | `modules/`, `src/kmod/` | Loadable `.nkm` modules (ext2, e1000, virtio) with PKCS#7 signing |

### Memory Layout Constants (`src/process/types.rs`)
```rust
USER_VIRT_BASE: 0x1000000   // Userspace code base (16MB)
HEAP_BASE:      0x1200000   // User heap region
STACK_BASE:     0x1400000   // User stack region (2MB)
INTERP_BASE:    0x1600000   // Dynamic linker region (16MB reserved)
```
**Critical**: Changes require coordinated updates in `src/mm/paging.rs` + `src/process/loader.rs`.

## Build & Test Workflows
```bash
./scripts/build.sh all                    # Full: kernel → userspace → rootfs → ISO (use first!)
./scripts/run-qemu.sh                     # Boot in QEMU with serial console
./scripts/build.sh kernel iso             # After kernel-only changes
./scripts/build.sh userspace rootfs iso   # After userspace changes
./scripts/build.sh modules initramfs      # After module changes
```

### Environment Variables
```bash
BUILD_TYPE=debug ./scripts/build.sh all   # Debug build (default, STABLE)
BUILD_TYPE=release ./scripts/build.sh all # Release build (O3 may cause fork/exec crashes!)
LOG_LEVEL=info ./scripts/build.sh kernel  # Set kernel log level (debug|info|warn|error)
```

**Build order matters**: Dependencies are kernel → nrlib → userspace → modules → initramfs → rootfs → iso.

## Coding Conventions

### Kernel Code (`src/`)
- **`no_std` only** — no heap allocations in kernel; use fixed-size buffers
- **Logging**: `kinfo!`, `kwarn!`, `kerror!`, `kdebug!`, `kfatal!` (defined in `src/logger.rs`)
- **Error handling**: Propagate `Errno` from `src/posix.rs`; never panic in syscall paths
- **Unsafe code**: Route through `src/safety/` helpers:
  ```rust
  use crate::safety::{inb, outb, volatile_read, copy_from_user, cast_header};
  ```

### Adding New Syscalls (`src/syscalls/`)
1. Add constant to `numbers.rs`: `pub const SYS_XXX: u64 = N;`
2. Implement in domain file (`file.rs`, `process.rs`, `network.rs`, `memory.rs`, etc.)
3. Wire up in `mod.rs:syscall_dispatch` match arm
4. Update `userspace/nrlib/src/lib.rs` with wrapper if needed by Rust std

### Userspace (`userspace/`)
- Target JSON: `targets/x86_64-nexaos-userspace.json`
- **nrlib** (`userspace/nrlib/`): libc shim for Rust `std` (pthread stubs, TLS, malloc, stdio, socket)
- **ld-nrlib** (`userspace/ld-nrlib/`): Dynamic linker at `/lib64/ld-nrlib-x86_64.so.1`
- **Adding programs**: Create in `userspace/programs/`, add to `userspace/Cargo.toml` workspace members

### Service Registration (`etc/inittab`)
```
id:runlevels:action:process
1:2345:respawn:/sbin/getty 38400 tty1
```
Services use System V runlevels (0=halt, 1=single, 3=multi-user, 6=reboot). Init is `/sbin/ni`.

## Critical Pitfalls
- **Never disable logging** — serial output is essential for boot debugging
- **ProcessState consistency** — scheduler, signals (`src/ipc/signal.rs`), and wait4 must stay synchronized
- **Dynamic linking** — PT_INTERP must match `/lib64/ld-nrlib-x86_64.so.1`
- **Rebuild rootfs** after `userspace/` or `etc/` changes: `./scripts/build.sh userspace rootfs iso`
- **Memory constants** — USER_VIRT_BASE/STACK_BASE changes break ELF loading; coordinate with paging

## Debugging
```bash
./scripts/run-qemu.sh -S -s              # GDB server + pause at start
gdb -ex "target remote :1234"            # Attach GDB
grub-file --is-x86-multiboot2 target/x86_64-nexaos/debug/nexa-os  # Verify boot
```
- Serial console shows all `kinfo!/kerror!` output
- Kernel ring buffer: `dmesg` in userspace (64KB, via `SYS_SYSLOG`)
- Module signing: `./scripts/sign-module.sh` (PKCS#7/CMS)