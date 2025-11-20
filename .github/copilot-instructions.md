# NexaOS AI Coding Guide
### Architecture Snapshot
- Hybrid kernel: `src/main.rs` hands off to `src/lib.rs` (6-stage boot defined in `src/boot_stages.rs`) before switching to Ring3 via `src/process.rs`.
- Memory & scheduling: `src/paging.rs` wires identity-mapped kernel + isolated user space; `src/scheduler.rs` round-robin with priorities; keep physical vs virtual addresses straight.
- Dual filesystem: `src/initramfs.rs` parses CPIO modules loaded by GRUB, `src/fs.rs` provides mutable in-memory runtime store; rootfs.ext2 gets mounted after stage 4.
- Syscall surface in `src/syscall.rs` dispatches into auth/ipc/signal subsystems (`src/auth.rs`, `src/ipc.rs`, `src/signal.rs`, `src/pipe.rs`); userspace wrappers live alongside binaries in `userspace/`.

### Key Workflows
- **End-to-end build: `./scripts/build-all.sh`** (the CORRECT comprehensive build script that handles kernel → userspace → rootfs → ISO chain). Always use this for complete builds.
- If splitting, run `./scripts/build-rootfs.sh` **before** `./scripts/build-iso.sh` or ISO will embed stale rootfs.
- Kernel-only iteration: `cargo build --release --target x86_64-nexaos.json`; userspace binaries via `./scripts/build-userspace.sh`.
- Boot/test quickly with `./scripts/run-qemu.sh`; inspect logs over serial (enabled by `src/serial.rs`). Sanity scripts: `./scripts/test-boot-stages.sh`, `./scripts/test-init.sh`, `./scripts/test-shell-exit.sh`.
- Verify multiboot status with `grub-file --is-x86-multiboot2 target/x86_64-nexaos/release/nexa-os`.

### Coding Conventions
- Kernel crates use the `no_std` attribute; no heap allocations—stick to fixed buffers/arrays. Use kernel log macros (`kinfo!/kerror!/kfatal!`) instead of `println!`.
- Respect address constants in `src/process.rs` (`USER_BASE`, `STACK_BASE`); adjust paging + ELF loader together when touching memory layout.
- Interrupts and syscall context rely on `interrupts::init_interrupts()` and the `GS_DATA` scratchpad; keep frame setup/restoration symmetrical.
- Authentication, IPC, and signal code expect explicit error paths—propagate `Errno` equivalents rather than panicking.

### Userspace & NRLib
- Userspace targets `x86_64-nexaos-userspace.json` and also uses `no_std`; programs expose `_start` and build via `scripts/build-userspace.sh`.
- `userspace/nrlib` exists solely to satisfy Rust `std` expectations (pthread, TLS, malloc, syscalls). Fix std issues inside nrlib; do not bypass `std` APIs.
- Dynamic binaries rely on PT_INTERP loading (`/lib64/ld-linux.so`) handled in `src/elf.rs`; keep interpreter path synchronized with rootfs layout.

### Common Pitfalls
- Never disable logging on hot paths; use log levels instead. Losing serial output makes boot debugging painful.
- When adding services, wire binaries into `userspace/`, ensure `scripts/build-rootfs.sh` copies them, and register via `etc/inittab`.
- Scheduler/process changes require keeping signal delivery and IPC queues consistent; audit `ProcessState` transitions if adding new states.
- Always rebuild rootfs before packaging ISO after touching userspace binaries or configs.
- Treat locking as production-grade: bounded waits, backoff, and clear comments when deviating from existing patterns (`src/safety/` helpers).
- ✅ Handle all error paths explicitly
- ✅ Add detailed comments explaining non-obvious synchronization logic
- ✅ Test edge cases and failure scenarios
- ✅ Document assumptions and invariants

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