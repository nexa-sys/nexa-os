# NexaOS AI Coding Guide

### Architecture Snapshot
- **Hybrid Kernel**: `src/main.rs` initializes `src/lib.rs` (6-stage boot in `src/boot_stages.rs`).
- **Memory & Scheduling**: `src/paging.rs` (identity-mapped kernel + isolated user space), `src/scheduler.rs` (round-robin).
- **Filesystems**: `src/initramfs.rs` (CPIO), `src/fs.rs` (VFS + in-memory), `rootfs.ext2` (mounted at stage 4).
- **Networking**: `src/net/` implements a custom stack (ARP, IPv4, UDP, Netlink) with driver abstraction (`drivers::DriverInstance`).
- **Bootloader**: `boot/uefi-loader` (Rust-based UEFI app) loads `KERNEL.ELF`, `INITRAMFS.CPIO`, and `ROOTFS.EXT2`, passing `BootInfo` to the kernel.
- **Syscalls**: `src/syscall.rs` dispatches to subsystems (`auth`, `ipc`, `signal`, `pipe`, `net`).

### Key Workflows
- **Full Build**: `./scripts/build-all.sh` (Builds RootFS → UEFI Loader → Kernel → ISO). Always use this for complete builds.
- **Kernel Dev**: `cargo build --release --target x86_64-nexaos.json`.
- **Userspace Dev**: `./scripts/build-userspace.sh` (builds binaries in `userspace/` and packs `initramfs.cpio`).
- **UEFI Loader Dev**: `./scripts/build-uefi-loader.sh` (builds `BootX64.EFI`).
- **Run/Test**: `./scripts/run-qemu.sh` (boots ISO with `rootfs.ext2` attached as `/dev/vda`).
- **Verify Multiboot**: `grub-file --is-x86-multiboot2 target/x86_64-nexaos/release/nexa-os`.

### Coding Conventions
- **Kernel (`src/`)**: `no_std`, fixed buffers (no heap in critical paths), use `kinfo!`/`kerror!` macros.
- **Userspace (`userspace/`)**: `no_std` binaries, linked against `nrlib` (libc shim).
- **Networking**: Use `NetStack` in `src/net/stack.rs` for packet processing. Drivers implement `drivers::NetworkDriver`.
- **Error Handling**: Propagate `Errno` (from `src/posix.rs`) for syscalls. Explicit error paths required.
- **Memory Layout**: Respect constants in `src/process.rs` (`USER_BASE`, `STACK_BASE`).

### Integration Points
- **Syscalls**: Add new syscalls in `src/syscall.rs` and `userspace/nrlib/src/syscalls.rs`.
- **Drivers**: Register new network drivers in `src/net/mod.rs` (`DeviceSlot`).
- **Init System**: Add services to `etc/inittab` and ensure binaries are in `userspace/`.
- **Boot Info**: Changes to `BootInfo` struct must be synced between `boot/boot-info/` and `src/bootinfo.rs`.

### Common Pitfalls
- **RootFS Staleness**: Always run `./scripts/build-rootfs.sh` if you change userspace binaries or `etc/` config.
- **UEFI Paths**: The loader expects files at `\EFI\BOOT\` in the ISO.
- **Logging**: Serial output is vital. Don't disable `src/serial.rs` logging.
- **Concurrency**: Use `spin::Mutex` with caution. Follow locking hierarchy to avoid deadlocks.

### Synchronization & Concurrency
When implementing locks or synchronization primitives:
1. **Always include timeout/bailout mechanisms** - prevent infinite hangs
2. **Use atomic operations with Acquire/Release semantics** - ensure memory consistency
3. **Implement exponential backoff** - reduce CPU contention
4. **Detect re-entrancy issues** - prevent deadlocks from nested lock attempts
5. **Add safety checks** - validate lock state and detect anomalies
6. **Test with load** - verify behavior under contention and stress

### Debugging & Observability
- **Never disable logging in critical paths** - instead, use conditional compilation or log levels
- **If a diagnostic tool breaks functionality** (e.g., debug_log causing deadlock), fix the underlying issue, don't just disable it
- **Maintain tracing capability** - system state must be observable for production debugging
- **Document why things are the way they are** - future maintainers need to understand design decisions

### System Reliability
- **Buffer overflows must never occur** - use bounds checking everywhere
- **Syscall failures must be handled** - every syscall can fail, check return values
- **Process state must be consistent** - scheduler, memory, and permission state must always be coherent
- **Recovery must be automatic where possible** - systems should heal themselves, not get stuck
- **Failures must be detectable** - use panic, assertions, or error logging to catch problems early

Remember: This is production-grade code targeting real systems. Every design decision impacts system reliability and debuggability.
- Runtime filesystem handles dynamic content and temporary files
- Root filesystem (ext2) contains full system after boot stage 4
- Virtual filesystems (/proc, /sys, /dev) mounted during initramfs stage

Remember: This is experimental code. Changes can break the entire system. Always test builds with `./scripts/build-all.sh` and boot with `./scripts/run-qemu.sh` after modifications. Use `git bisect` for regression hunting, and verify all boot stages complete successfully.