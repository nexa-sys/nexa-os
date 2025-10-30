# NexaOS AI Coding Guidelines

## Project Overview
NexaOS is an experimental Rust-based operating system implementing a hybrid-kernel architecture. It targets x86_64 with Multiboot2 + GRUB boot, providing POSIX-inspired interfaces and limited Linux userspace compatibility.

## Architecture Fundamentals

### Kernel Structure
- **Hybrid kernel**: Combines microkernel-style isolation with monolithic performance optimizations
- **Boot flow**: Multiboot2 → GRUB → Rust kernel entry (`kmain`) → user mode transition
- **Memory model**: Identity-mapped paging with separate kernel/user spaces (Ring 0/3)
- **Process model**: Single user process execution (currently `/bin/sh` from initramfs)

### Key Components
- `src/main.rs`: Multiboot entry point, calls `nexa_os::kernel_main()`
- `src/lib.rs`: Core kernel initialization sequence
- `src/process.rs`: ELF loading and Ring 3 user mode switching
- `src/syscall.rs`: System call dispatch (write/read/exit/getpid/open/close)
- `src/paging.rs`: Virtual memory setup for user space
- `src/initramfs.rs`: CPIO archive parsing for initial filesystem
- `src/fs.rs`: Simple in-memory filesystem for runtime file operations
- `userspace/shell.rs`: Minimal shell using syscall interface

## Critical Developer Workflows

### Build Process
```bash
# Kernel build (requires nightly Rust toolchain)
cargo build --release

# Userspace programs (creates initramfs)
./scripts/build-userspace.sh

# ISO creation with GRUB
./scripts/build-iso.sh

# QEMU testing
./scripts/run-qemu.sh
```

### Prerequisites
- Rust nightly: `rustup override set nightly && rustup component add rust-src llvm-tools-preview`
- System deps: `build-essential lld grub-pc-bin xorriso qemu-system-x86`
- Custom target: Uses `x86_64-nexaos.json` for bare-metal compilation

## Code Patterns & Conventions

### Logging
Use kernel logging macros instead of `println!`:
```rust
kinfo!("Kernel initialized successfully");
kerror!("Failed to load ELF: {}", error);
kdebug!("Memory region: {:#x} - {:#x}", start, end);
```

### No-Std Environment
- `#![no_std]` with custom panic handler
- Core types from `core::` instead of `std::`
- No heap allocation in kernel (stack-only)
- Custom `#[lang_items]` for userspace

### Memory Management
- Physical addresses for kernel, virtual for userspace
- User space layout: 0x400000-0x800000 (code), 0x600000-0x700000 (stack), heap after code
- Identity mapping for bootloader compatibility
- No dynamic allocation in kernel core

### Syscall Interface
```rust
// Kernel side (syscall.rs) - uses syscall instruction
pub const SYS_WRITE: u64 = 1;
pub const SYS_READ: u64 = 0;
pub const SYS_EXIT: u64 = 60;
pub const SYS_GETPID: u64 = 39;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;

// Assembly handler with register preservation
#[no_mangle]
pub extern "C" fn syscall_dispatch(nr: u64, arg1: u64, arg2: u64, arg3: u64) -> u64

// Userspace side (shell.rs) - syscall instruction
fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("syscall", in("rax") n, in("rdi") a1, in("rsi") a2, in("rdx") a3, lateout("rax") ret);
    }
    ret
}
```

### ELF Loading
- Custom ELF parser in `src/elf.rs`
- Loads to fixed physical addresses (0x400000+)
- Entry point calculation: `header.entry_point()`
- No dynamic linking support
- Process creation: `Process::from_elf(data)` returns executable process

### Filesystem Architecture
- **Initramfs**: CPIO newc format parsing for boot-time files
- **Runtime FS**: Simple in-memory filesystem (64 file limit) for dynamic content
- Dual filesystem design: initramfs for initial programs, memory fs for runtime data

## Build System Details

### Cargo Configuration
- Custom target: `x86_64-nexaos.json` (no OS, soft-float)
- Release profile: `panic = "abort"` (no unwinding)
- Build dependencies: `cc` for assembly compilation

### Linker Script
- `linker.ld`: Places Multiboot header at 0x100000
- Custom sections: `.multiboot`, `.bootstrap_stack`
- Assembly bootstrap: `boot/long_mode.S`

### Initramfs Creation
- Userspace binaries built with `-Z build-std=core`
- CPIO archive format for GRUB modules
- Stripped binaries: `strip --strip-all`

## Testing & Debugging

### Verification Steps
```bash
# Multiboot validation
grub-file --is-x86-multiboot2 target/x86_64-nexaos/release/nexa-os

# Serial output monitoring
./scripts/run-qemu.sh  # Check for kernel logs
```

### Common Issues
- **Build fails**: Check `lld` availability, Rust nightly components
- **No output**: Verify VGA buffer initialization, serial port setup
- **Boot hangs**: Check Multiboot header, GRUB configuration
- **Syscall fails**: Verify GS register setup, IDT configuration

## File Organization

### Key Directories
- `src/`: Kernel source (lib.rs entry point)
- `userspace/`: User programs (shell.rs)
- `boot/`: Assembly bootstrap code
- `scripts/`: Build automation
- `docs/zh/`: Chinese documentation
- `target/x86_64-nexaos/`: Custom target builds

### Configuration Files
- `x86_64-nexaos.json`: Rust target specification
- `linker.ld`: Kernel linking layout
- `rust-toolchain.toml`: Nightly version pinning

## Development Guidelines

### When Adding Kernel Features
1. Initialize in `kernel_main()` sequence
2. Use `kinfo!` for boot progress logging
3. Handle failures gracefully (halt vs panic)
4. Test with QEMU serial output

### When Adding Syscalls
1. Define constant in `syscall.rs`
2. Add dispatch case in `syscall_dispatch()`
3. Update userspace syscall wrappers
4. Test with shell integration

### When Modifying Memory Layout
1. Update `paging.rs` user space mappings
2. Adjust `process.rs` address constants
3. Verify with memory map logging
4. Test ELF loading functionality

### When Adding Filesystem Features
1. Consider initramfs vs runtime filesystem usage
2. Update both `initramfs.rs` and `fs.rs` if needed
3. Test file operations in userspace shell

## Cross-Component Communication

### Kernel ↔ Userspace
- Syscalls via `syscall` instruction
- GS register for kernel data access
- Fixed memory layout contracts

### Bootloader Integration
- Multiboot2 tags for memory map, modules
- GRUB modules for initramfs
- Serial console for debugging

### Process Management
- Single process execution model
- ELF loading with fixed address allocation
- Ring 3 transition via `iretq`
- Process state tracking (Ready/Running/Sleeping/Zombie)

Remember: This is experimental code. Changes can break the entire system. Always test boots after modifications, and use `git bisect` for regression hunting.