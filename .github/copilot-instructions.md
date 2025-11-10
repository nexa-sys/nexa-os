# NexaOS AI Coding Guidelines

## Project Overview
NexaOS is a production-grade operating system written in Rust, implementing a hybrid-kernel architecture with full POSIX compliance and Unix-like semantics. The system provides a self-contained environment with comprehensive Linux ABI compatibility, targeting modern x86_64 hardware through Multiboot2 + GRUB boot protocol.

## Architecture Fundamentals

### Kernel Structure
- **Hybrid kernel**: Combines microkernel-style isolation with monolithic performance optimizations
- **Boot flow**: Multiboot2 → GRUB → Rust kernel entry (`kmain`) → 6-stage boot process → user mode transition
- **Memory model**: Identity-mapped paging with separate kernel/user spaces (Ring 0/3)
- **Process model**: Multi-process support with scheduler, authentication, IPC, signals, and pipes

### Key Components
- `src/main.rs`: Multiboot entry point, calls `nexa_os::kernel_main()`
- `src/lib.rs`: Core kernel initialization sequence with 6-stage boot process
- `src/boot_stages.rs`: Multi-stage boot management (Bootloader→KernelInit→Initramfs→RootSwitch→RealRoot→UserSpace)
- `src/process.rs`: ELF loading and Ring 3 user mode switching with dynamic linking support
- `src/scheduler.rs`: Round-robin process scheduler with time slicing and priority support
- `src/syscall.rs`: System call dispatch (write/read/exit/getpid/open/close + POSIX extensions)
- `src/auth.rs`: Multi-user authentication system with UID/GID and role-based access
- `src/ipc.rs`: Message-passing channels for inter-process communication
- `src/signal.rs`: POSIX signal handling with 32 signal types and signal actions
- `src/pipe.rs`: POSIX pipe implementation for IPC with 4KB buffers
- `src/paging.rs`: Virtual memory setup for user space with address space isolation
- `src/initramfs.rs`: CPIO archive parsing for initial filesystem with PT_INTERP detection
- `src/fs.rs`: Dual filesystem: initramfs (immutable boot files) + runtime in-memory filesystem
- `src/init.rs`: Complete init system (PID 1) with System V runlevels, service management, and /etc/inittab
- `src/interrupts.rs`: IDT setup, PIC/APIC configuration, syscall interrupt handling
- `src/keyboard.rs`: PS/2 keyboard driver with scancode processing and US QWERTY layout
- `src/gdt.rs`: Global Descriptor Table for privilege separation (Ring 0/3)
- `src/serial.rs`: Serial console driver for debugging and logging
- `src/vga_buffer.rs`: VGA text mode driver for console output

## Critical Developer Workflows

### Build Process
NexaOS uses a two-stage build: minimal initramfs for early boot + full ext2 root filesystem.

```bash
# Complete system build (kernel + initramfs + rootfs + ISO) - RECOMMENDED
./scripts/build-all.sh

# Individual components - CRITICAL BUILD ORDER:
# The ext2 root filesystem is embedded in initramfs, so you MUST:
# 1. Build rootfs FIRST (creates build/rootfs.ext2)
# 2. Then build ISO (embeds rootfs.ext2 into initramfs)
./scripts/build-rootfs.sh       # Step 1: Build full ext2 root filesystem
./scripts/build-iso.sh          # Step 2: Create bootable ISO with GRUB
# NEVER build ISO without building rootfs first - you'll get stale filesystem!

./scripts/build-userspace.sh    # Build initramfs only (minimal boot environment)

# Kernel-only build
cargo build --release

# QEMU testing with full system
./scripts/run-qemu.sh
```

### Prerequisites
- Rust nightly: `rustup override set nightly && rustup component add rust-src llvm-tools-preview`
- System deps: `build-essential lld grub-pc-bin xorriso qemu-system-x86 mtools`
- Custom target: Uses `x86_64-nexaos.json` for bare-metal compilation (no OS, soft-float, PIC model)

## Code Patterns & Conventions

### Logging
Use kernel logging macros instead of `println!`:
```rust
kinfo!("Kernel initialized successfully");
kerror!("Failed to load ELF: {}", error);
kdebug!("Memory region: {:#x} - {:#x}", start, end);
kwarn!("Using fallback configuration");
kfatal!("Critical error, halting system");
```

### No-Std Environment
- `#![no_std]` with custom panic handler
- Core types from `core::` instead of `std::`
- No heap allocation in kernel (stack-only, fixed-size arrays)
- Custom `#[lang_items]` for userspace programs

### Memory Management
- Physical addresses for kernel, virtual for userspace
- User space layout constants (`src/process.rs`):
  - `USER_BASE: u64 = 0x200000` (code segment start)
  - `STACK_BASE: u64 = 0x600000` (stack segment start)
  - `STACK_SIZE: u64 = 0x100000` (1MB stack)
  - `HEAP_SIZE: u64 = 0x100000` (1MB heap after code)
- Identity mapping for bootloader compatibility
- No dynamic allocation in kernel core

### Syscall Interface
```rust
// Kernel side (syscall.rs) - uses syscall instruction
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_EXIT: u64 = 60;
pub const SYS_GETPID: u64 = 39;
// ... additional POSIX syscalls

// Assembly handler with register preservation
#[no_mangle]
pub extern "C" fn syscall_dispatch(nr: u64, arg1: u64, arg2: u64, arg3: u64) -> u64

// GS_DATA structure for syscall context (interrupts.rs)
pub static mut GS_DATA: [u64; 16] = [0; 16];
// GS_DATA[0] = user RSP, GS_DATA[1] = kernel RSP
// GS_DATA[2] = user entry point, GS_DATA[3] = user stack base
// GS_DATA[4-6] = segment selectors (CS, SS, DS)

// Userspace syscall wrapper (userspace/shell.rs)
fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("syscall", in("rax") n, in("rdi") a1, in("rsi") a2, in("rdx") a3, lateout("rax") ret);
    }
    ret
}
```

### Userspace Program Structure
- `#![no_std]` with custom panic handler and lang items
- Entry point: `#[no_mangle] pub extern "C" fn _start()`
- Syscall wrappers for kernel communication
- No standard library, manual memory management
- Built with `-Z build-std=core` and custom target

### Process Scheduling
- Round-robin scheduler with configurable time slices
- Process table with 32 process limit
- Priority-based scheduling (0=highest, 255=lowest)
- Process state tracking: Ready/Running/Sleeping/Zombie

### Authentication & Security
- Multi-user system with UID/GID-based permissions
- Root user (UID 0) with admin privileges
- User database with password hashing
- Credential passing between processes

### IPC Mechanisms
- Message-passing channels (32 channels, 32 messages/channel, 256 bytes/message)
- POSIX pipes (4KB buffers, 16 pipe limit)
- Blocking/non-blocking operations

### Signal Handling
- Full POSIX signal support (32 signals including SIGINT, SIGTERM, SIGHUP, etc.)
- Signal actions: Default/Ignore/Custom Handler
- Per-process signal state with pending/blocked masks

### ELF Loading & Dynamic Linking
- Custom ELF parser in `src/elf.rs` with PT_INTERP detection
- Supports both static and dynamically linked executables
- Static binaries: Load to fixed physical addresses (0x400000+)
- Dynamic binaries: Load program at 0x400000, interpreter at 0xA00000
- Entry point calculation: `header.entry_point()`
- Dynamic linking: Detects PT_INTERP, loads ld-linux.so from `/lib64/`
- Process creation: `Process::from_elf(data)` returns executable process
- Note: Auxiliary vectors not yet implemented for full dynamic linking support

### Init System Architecture
- System V-style init with runlevels (0=halt, 1=single-user, 3=multi-user, 6=reboot)
- Service management with respawn capability
- `/etc/inittab` configuration file support
- PID 1 management with proper orphan process handling

### Filesystem Architecture
- **Dual filesystem design**: Initramfs for boot-time files, memory filesystem for runtime data
- **Initramfs**: CPIO newc format parsing (`src/initramfs.rs`) for immutable boot files
- **Runtime FS**: Simple in-memory filesystem (64 file limit) in `src/fs.rs` for dynamic content
- Files registered via `fs::add_file_bytes(name, content, is_dir)`
- Initramfs loaded from GRUB modules, parsed at boot time

### Keyboard Input
- PS/2 keyboard driver with scancode queue (128 bytes)
- US QWERTY layout with shift key support
- Blocking character and line reading
- Interrupt-driven (IRQ1) with PIC handling

## Build System Details

### Cargo Configuration
- Custom target: `x86_64-nexaos.json` (bare-metal, no OS, soft-float, PIC relocation model)
- Release profile: `panic = "abort"` (no unwinding)
- Build dependencies: `cc` for assembly compilation
- Userspace builds: `-Z build-std=core` for minimal std replacement

### Linker Script
- `linker.ld`: Places Multiboot header at 0x100000
- Custom sections: `.boot.header`, `.boot`, `.text`, `.rodata`, `.data`, `.bss`
- Assembly bootstrap: `boot/long_mode.S` with identity-mapped page tables

### Multi-Stage Build Process
- **Initramfs Creation**: Userspace binaries built with `-Z build-std=core`, CPIO newc archive format
- **Root Filesystem**: ext2-formatted disk image (50MB) with full directory structure
- **ISO Creation**: GRUB configuration with kernel, initramfs, and rootfs disk image
- **Boot Parameters**: `root=/dev/vda1 rootfstype=ext2 loglevel=debug`

### Command Line Parsing
- GRUB command line support: `init=/path/to/program root=/dev/vda1`
- Parsed in `kernel_main()` for custom init programs
- Fallback to standard Unix init paths: `/sbin/ni`, `/sbin/init`, `/etc/init`, `/bin/init`, `/bin/sh`

## Testing & Debugging

### Verification Steps
```bash
# Multiboot validation
grub-file --is-x86-multiboot2 target/x86_64-nexaos/release/nexa-os

# Serial output monitoring
./scripts/run-qemu.sh  # Check for kernel logs

# Build validation
./scripts/build-all.sh && ./scripts/run-qemu.sh

# Individual component testing
./scripts/test-boot-stages.sh    # Test boot stage progression
./scripts/test-init.sh           # Test init system
./scripts/test-shell-exit.sh     # Test shell and process cleanup
```

### Common Issues
- **Build fails**: Check `lld` availability, Rust nightly components, `x86_64-nexaos.json` target
- **No output**: Verify VGA buffer initialization, serial port setup, framebuffer activation
- **Boot hangs**: Check Multiboot header, GRUB configuration, boot stage progression
- **Syscall fails**: Verify GS register setup, IDT configuration, syscall dispatch table
- **Keyboard not working**: Check PIC initialization, IRQ handling, PS/2 controller setup
- **Userspace won't start**: Check ELF loading, paging setup, GDT configuration, dynamic linking
- **Init system fails**: Check `/etc/inittab` syntax, service paths, runlevel configuration
- **Filesystem issues**: Verify initramfs CPIO format, rootfs ext2 mounting, mount point setup

## File Organization

### Key Directories
- `src/`: Kernel source (lib.rs entry point with 6-stage boot)
- `userspace/`: User programs (shell.rs, init.rs, login.rs, getty.rs)
- `boot/`: Assembly bootstrap code for long mode initialization
- `scripts/`: Build automation (build-all.sh, build-iso.sh, build-rootfs.sh, build-userspace.sh)
- `docs/`: Architecture and implementation documentation
- `docs/zh/`: Chinese documentation
- `build/`: Build artifacts (initramfs.cpio, rootfs.ext2, userspace binaries)
- `target/x86_64-nexaos/`: Custom target builds
- `etc/`: System configuration files (inittab, ni.conf)

### Configuration Files
- `x86_64-nexaos.json`: Rust target specification (bare-metal, no OS)
- `x86_64-nexaos-userspace.json`: Userspace target for `-Z build-std=core`
- `linker.ld`: Kernel linking layout with Multiboot header
- `rust-toolchain.toml`: Nightly version pinning
- `Cargo.toml`: Dependencies and build configuration
- `etc/inittab`: Init system configuration (System V style)

## Development Guidelines

### When Adding Kernel Features
1. Initialize in `kernel_main()` sequence in `src/lib.rs` (after existing subsystems)
2. Use `kinfo!` for boot progress logging with timing information
3. Handle failures gracefully (halt vs panic) with proper error reporting
4. Test with QEMU serial output and verify boot stage completion
5. Update boot stage tracking if adding new initialization phases

### When Adding Syscalls
1. Define constant in `src/syscall.rs` (follow POSIX numbering where possible)
2. Add dispatch case in `syscall_dispatch()` function with parameter validation
3. Update userspace syscall wrappers in appropriate userspace programs
4. Test with shell integration and verify syscall numbers match POSIX standards

### When Adding Processes/Services
1. Implement as userspace program in `userspace/` directory
2. Add to build system in `scripts/build-userspace.sh` or `scripts/build-rootfs.sh`
3. Configure in `/etc/inittab` for init system management
4. Test process lifecycle: spawn, execution, cleanup, signal handling

### When Modifying Memory Layout
1. Update `src/paging.rs` user space mappings and address space calculations
2. Adjust address constants in `src/process.rs` (USER_BASE, STACK_BASE, etc.)
3. Update `linker.ld` if kernel memory layout changes
4. Verify with memory map logging and ELF loading tests
5. Test with both static and dynamically linked executables

### When Adding Filesystem Features
1. Consider initramfs vs runtime filesystem usage (initramfs for boot, runtime for dynamic content)
2. Update both `src/initramfs.rs` and `src/fs.rs` if needed
3. Test file operations in userspace shell with various file types
4. Rebuild initramfs with `./scripts/build-userspace.sh` and test boot

### When Adding Device Drivers
1. Initialize interrupts in `interrupts::init_interrupts()` with proper IRQ routing
2. Configure PIC/APIC for device IRQs with mask/unmask logic
3. Add interrupt handlers with proper register preservation and error handling
4. Test with QEMU device emulation and verify interrupt delivery

### When Modifying Authentication/Security
1. Update user database structures in `src/auth.rs`
2. Implement credential validation and permission checking
3. Add UID/GID handling in syscall implementations
4. Test with multi-user scenarios and privilege escalation prevention

### When Adding IPC Mechanisms
1. Implement in appropriate module (`ipc.rs` for channels, `pipe.rs` for pipes)
2. Add syscall support in `syscall.rs` dispatch table
3. Implement blocking/non-blocking semantics with proper synchronization
4. Test inter-process communication between multiple userspace programs

### When Modifying Signal Handling
1. Add signal constants and default actions in `src/signal.rs`
2. Update signal delivery mechanism in process scheduler
3. Implement signal masking and pending signal queues
4. Test signal delivery, blocking, and custom handlers

## Cross-Component Communication

### Kernel ↔ Userspace
- Syscalls via `syscall` instruction (x86_64 fast syscall) with GS register context
- Fixed memory layout contracts (user code at 0x200000+, stack at 0x600000+)
- Signal delivery through process control structures
- IPC through kernel-mediated message passing and pipes

### Bootloader Integration
- Multiboot2 tags for memory map, command line, modules (initramfs)
- GRUB modules for initramfs (CPIO archives) and root filesystem (ext2 disk)
- Serial console for debugging output with configurable log levels

### Process Management
- Scheduler integration with process table and time slicing
- Authentication system for user credential management
- Signal handling for inter-process communication
- IPC channels and pipes for data exchange
- Init system for service lifecycle management

### Filesystem Integration
- Initramfs provides boot-time files (binaries, configs)
- Runtime filesystem handles dynamic content and temporary files
- Root filesystem (ext2) contains full system after boot stage 4
- Virtual filesystems (/proc, /sys, /dev) mounted during initramfs stage

## Critical Design Principles

### NexaOS Userspace Library (nrlib)
**CRITICAL**: `nrlib` is specifically designed to provide libc compatibility for Rust's `std` library. It is NOT a standalone userspace library.

**Core Design Goals**:
1. **Enable Rust std**: Primary purpose is to make Rust's standard library work in NexaOS userspace
2. **Provide libc ABI**: Implements C ABI functions that `std` expects from libc (pthread, malloc, syscalls, etc.)
3. **Transparent Integration**: Programs using `std` should work without modifications

**DO NOT**:
- ❌ Bypass `std` to use nrlib directly (defeats the purpose)
- ❌ Implement workarounds that avoid std I/O (println!, io::stdout, etc.)
- ❌ Create custom non-std alternatives when std functionality is needed

**DO**:
- ✅ Fix issues in nrlib to make std work correctly
- ✅ Debug std behavior by instrumenting nrlib implementations
- ✅ Ensure pthread, TLS, futex, and other libc primitives work for std's needs
- ✅ Test with standard Rust patterns (println!, io::Write, std::sync, etc.)

**When std I/O fails**: The problem is in nrlib's libc compatibility layer, not in std itself. Fix the underlying libc implementation rather than working around it.

### Build System Architecture
**CRITICAL**: The ext2 root filesystem is embedded in the initramfs CPIO archive.

**Build Dependencies**:
```
rootfs.ext2 → initramfs.cpio → nexaos.iso
```

**Correct Build Order**:
1. `./scripts/build-rootfs.sh` creates `build/rootfs.ext2`
2. `./scripts/build-iso.sh` embeds `rootfs.ext2` into `build/initramfs.cpio`
3. ISO contains kernel + initramfs (which includes rootfs.ext2)

**Common Mistake**: Building ISO without building rootfs first results in missing or stale filesystem.

**Always use**: `./scripts/build-all.sh` to ensure correct build order, or manually follow: rootfs → ISO.

Remember: This is experimental code. Changes can break the entire system. Always test builds with `./scripts/build-all.sh` and boot with `./scripts/run-qemu.sh` after modifications. Use `git bisect` for regression hunting, and verify all boot stages complete successfully.

## Production-Grade System Standards

### Correctness Over Simplicity
**CRITICAL PRINCIPLE**: NexaOS is designed as a production-grade operating system. **NEVER sacrifice correctness, robustness, or safety for the sake of simplification.**

**DO NOT**:
- ❌ Remove error handling to simplify code
- ❌ Use simplified locking mechanisms that can deadlock (e.g., unbounded spin loops)
- ❌ Skip timeout mechanisms in lock acquisition
- ❌ Disable debug/diagnostic logging without comprehensive monitoring alternatives
- ❌ Assume single-threaded behavior to bypass synchronization
- ❌ Short-cut system call error handling
- ❌ Ignore corner cases or edge conditions

**DO**:
- ✅ Implement robust error handling with proper errno propagation
- ✅ Use bounded locks with timeout detection to prevent deadlocks
- ✅ Maintain comprehensive diagnostic capabilities
- ✅ Implement proper synchronization primitives (mutexes, spinlocks with bounds)
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