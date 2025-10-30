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
- `src/syscall.rs`: System call dispatch (write/read/exit/getpid)
- `src/paging.rs`: Virtual memory setup for user space
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
- User space: 0x400000-0x800000 (code), 0x600000-0x700000 (stack)
- Identity mapping for bootloader compatibility
- No dynamic allocation in kernel core

### Syscall Interface
```rust
// Kernel side (syscall.rs)
pub const SYS_WRITE: u64 = 1;
// Assembly handler with register preservation

// Userspace side (shell.rs)
fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    unsafe { asm!("syscall", /* params */) }
}
```

### ELF Loading
- Custom ELF parser in `src/elf.rs`
- Loads to fixed physical addresses (0x400000+)
- Entry point calculation: `header.entry_point()`
- No dynamic linking support

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

## Cross-Component Communication

### Kernel ↔ Userspace
- Syscalls via `syscall` instruction
- GS register for kernel data access
- Fixed memory layout contracts

### Bootloader Integration
- Multiboot2 tags for memory map, modules
- GRUB modules for initramfs
- Serial console for debugging

Remember: This is experimental code. Changes can break the entire system. Always test boots after modifications, and use `git bisect` for regression hunting.