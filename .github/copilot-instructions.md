# NexaOS AI Coding Guidelines

## Project Overview
NexaOS is an experimental Rust-based operating system implementing a hybrid-kernel architecture. It targets x86_64 with Multiboot2 + GRUB boot, providing POSIX-inspired interfaces and limited Linux userspace compatibility.

## Architecture Fundamentals

### Kernel Structure
- **Hybrid kernel**: Combines microkernel-style isolation with monolithic performance optimizations
- **Boot flow**: Multiboot2 → GRUB → Rust kernel entry (`kmain`) → user mode transition
- **Memory model**: Identity-mapped paging with separate kernel/user spaces (Ring 0/3)
- **Process model**: Single user process execution (currently `/bin/sh` from initramfs) with structures for future multi-process support

### Key Components
- `src/main.rs`: Multiboot entry point, calls `nexa_os::kernel_main()`
- `src/lib.rs`: Core kernel initialization sequence
- `src/process.rs`: ELF loading and Ring 3 user mode switching
- `src/syscall.rs`: System call dispatch (write/read/exit/getpid/open/close)
- `src/paging.rs`: Virtual memory setup for user space
- `src/initramfs.rs`: CPIO archive parsing for initial filesystem
- `src/fs.rs`: Simple in-memory filesystem for runtime file operations
- `src/interrupts.rs`: IDT setup, PIC configuration, syscall interrupt handling
- `src/keyboard.rs`: PS/2 keyboard driver with scancode processing
- `src/gdt.rs`: Global Descriptor Table for privilege separation

## Critical Developer Workflows

### Build Process
```bash
# Set up Rust nightly toolchain
rustup override set nightly
rustup component add rust-src llvm-tools-preview --toolchain nightly

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
  - `USER_BASE: u64 = 0x400000` (code segment start)
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

### ELF Loading
- Custom ELF parser in `src/elf.rs`
- Loads to fixed physical addresses (0x400000+)
- Entry point calculation: `header.entry_point()`
- No dynamic linking support
- Process creation: `Process::from_elf(data)` returns executable process

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
- Custom target: `x86_64-nexaos.json` (no OS, soft-float, PIC relocation model)
- Release profile: `panic = "abort"` (no unwinding)
- Build dependencies: `cc` for assembly compilation
- Userspace builds: `-Z build-std=core` for minimal std replacement

### Linker Script
- `linker.ld`: Places Multiboot header at 0x100000
- Custom sections: `.boot.header`, `.boot`, `.text`, `.rodata`, `.data`, `.bss`
- Assembly bootstrap: `boot/long_mode.S` with identity-mapped page tables

### Initramfs Creation
- Userspace binaries built with `-Z build-std=core`
- CPIO newc archive format for GRUB modules
- Stripped binaries: `strip --strip-all` to minimize size
- Build script: `./scripts/build-userspace.sh` creates `build/initramfs.cpio`

### Command Line Parsing
- GRUB command line support: `init=/path/to/program`
- Parsed in `kernel_main()` for custom init programs
- Fallback to default init paths: `/sbin/init`, `/etc/init`, `/bin/init`, `/bin/sh`

## Testing & Debugging

### Verification Steps
```bash
# Multiboot validation
grub-file --is-x86-multiboot2 target/x86_64-nexaos/release/nexa-os

# Serial output monitoring
./scripts/run-qemu.sh  # Check for kernel logs

# Build validation
cargo build --release && ./scripts/build-userspace.sh && ./scripts/build-iso.sh
```

### Common Issues
- **Build fails**: Check `lld` availability, Rust nightly components
- **No output**: Verify VGA buffer initialization, serial port setup
- **Boot hangs**: Check Multiboot header, GRUB configuration
- **Syscall fails**: Verify GS register setup, IDT configuration
- **Keyboard not working**: Check PIC initialization, IRQ handling
- **Userspace won't start**: Check ELF loading, paging setup, GDT configuration

## File Organization

### Key Directories
- `src/`: Kernel source (lib.rs entry point)
- `userspace/`: User programs (shell.rs)
- `boot/`: Assembly bootstrap code
- `scripts/`: Build automation
- `docs/zh/`: Chinese documentation
- `target/x86_64-nexaos/`: Custom target builds

### Configuration Files
- `x86_64-nexaos.json`: Rust target specification (bare-metal, no OS)
- `linker.ld`: Kernel linking layout with Multiboot header
- `rust-toolchain.toml`: Nightly version pinning
- `Cargo.toml`: Dependencies and build configuration

## Development Guidelines

### When Adding Kernel Features
1. Initialize in `kernel_main()` sequence in `src/lib.rs`
2. Use `kinfo!` for boot progress logging
3. Handle failures gracefully (halt vs panic)
4. Test with QEMU serial output and verify boot

### When Adding Syscalls
1. Define constant in `src/syscall.rs`
2. Add dispatch case in `syscall_dispatch()` function
3. Update userspace syscall wrappers in `userspace/shell.rs`
4. Test with shell integration and verify syscall numbers

### When Modifying Memory Layout
1. Update `src/paging.rs` user space mappings
2. Adjust address constants in `src/process.rs`
3. Update `linker.ld` if kernel memory layout changes
4. Verify with memory map logging and ELF loading tests

### When Adding Filesystem Features
1. Consider initramfs vs runtime filesystem usage
2. Update both `src/initramfs.rs` and `src/fs.rs` if needed
3. Test file operations in userspace shell
4. Rebuild initramfs with `./scripts/build-userspace.sh`

### When Adding Device Drivers
1. Initialize interrupts in `interrupts::init_interrupts()`
2. Configure PIC for device IRQs
3. Add interrupt handlers with proper masking
4. Test with QEMU device emulation

## Cross-Component Communication

### Kernel ↔ Userspace
- Syscalls via `syscall` instruction (x86_64 fast syscall)
- GS register points to GS_DATA array for kernel data access
- Fixed memory layout contracts (user code at 0x400000+)

### Bootloader Integration
- Multiboot2 tags for memory map, command line, modules
- GRUB modules for initramfs (CPIO archives)
- Serial console for debugging output

### Process Management
- Single process execution model (currently)
- Process structures support future multi-process (PID, state, memory layout)
- ELF loading with fixed address allocation
- Ring 3 transition via `iretq` instruction
- Process state tracking (Ready/Running/Sleeping/Zombie)

Remember: This is experimental code. Changes can break the entire system. Always test boots after modifications, and use `git bisect` for regression hunting.