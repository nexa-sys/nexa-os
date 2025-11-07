# NexaOS

NexaOS is a production-grade operating system written in Rust, implementing a hybrid-kernel architecture with full POSIX compliance and Unix-like semantics. The system provides a self-contained environment with comprehensive Linux ABI compatibility, targeting modern x86_64 hardware through Multiboot2 + GRUB boot protocol.

## Design Goals

- **Production-grade reliability**: Memory-safe kernel and core services implemented in Rust with rigorous error handling and security guarantees.
- **Hybrid kernel architecture**: Combines microkernel-style modularity with monolithic kernel performance, optimizing for both isolation and efficiency. Critical subsystems run in kernel space while maintaining clean interfaces and minimal coupling.
- **Full POSIX compliance**: Comprehensive implementation of POSIX.1-2017 standards including process management, file systems, signals, IPC mechanisms, and threading primitives.
- **Unix-like semantics**: Everything-is-a-file philosophy, hierarchical filesystem, shell integration, and standard Unix conventions.
- **Linux ABI compatibility**: Binary compatibility layer enabling unmodified Linux applications to run natively through syscall translation and compatible userspace libraries.
- **Enterprise-ready features**: Multi-user support with authentication, capability-based security, resource isolation, and comprehensive logging infrastructure.

## Current Status

NexaOS implements a fully functional 64-bit kernel with the following production features:

- **Boot Infrastructure**: Multiboot2-compliant boot flow with GRUB integration, complete 64-bit long mode initialization
- **Memory Management**: Virtual memory with paging, user/kernel space separation, ELF binary loading with proper address space isolation, PT_INTERP detection for dynamic linking
- **Process Management**: Ring 0/3 privilege separation, user mode process execution, process state tracking, PPID support, dynamic linker support
- **Init System**: Complete Unix-like init (PID 1) with System V runlevels, service management, respawn capability, /etc/inittab support
- **System Calls**: Production syscall interface including POSIX I/O, process control, and system management (reboot/shutdown/runlevel)
- **File Systems**: Initramfs support with CPIO parsing, runtime in-memory filesystem for dynamic content
- **Device Drivers**: PS/2 keyboard driver with interrupt handling, VGA text mode, serial console for diagnostics
- **IPC Mechanisms**: Message-passing channels for inter-process communication
- **Security**: Multi-user authentication system with role-based access control, root/user privilege separation, superuser checks
- **POSIX Compliance**: Error number definitions (errno), file metadata structures, standard file types, process hierarchy
- **Interactive Shell**: Full command-line environment with POSIX commands (ls, cat, echo, pwd, ps, etc.)

## Architecture Overview

### Hybrid Kernel Design

NexaOS implements a hybrid kernel architecture that balances the security and modularity of microkernels with the performance characteristics of monolithic kernels:

| Component | Architecture | Rationale |
|-----------|--------------|-----------|
| **Core Kernel** | Monolithic (Ring 0) | Memory management, scheduling, core syscalls run in kernel space for maximum performance |
| **Device Drivers** | Hybrid | Critical drivers (keyboard, console) in kernel space; future drivers may run as isolated services |
| **File Systems** | Kernel Space | VFS layer and core filesystems in kernel for performance; future network filesystems may be userspace |
| **IPC Layer** | Kernel-Mediated | Message-passing primitives implemented in kernel, enforcing security policies and isolation |
| **System Services** | User Space (Ring 3) | Authentication, logging, and higher-level services run as isolated user processes |

### POSIX Compliance

| Standard | Status | Implementation |
|----------|--------|----------------|
| **Process Management** | ✅ Full | Fork/exec semantics, process lifecycle, signal handling framework |
| **File I/O** | ✅ Full | Open/close/read/write, file descriptors, standard streams |
| **File System** | ✅ Core | Hierarchical directory structure, file metadata, permissions |
| **Error Handling** | ✅ Full | Comprehensive errno values matching POSIX specifications |
| **System Calls** | ⚙️ Growing | Core syscalls implemented, expanding toward full POSIX.1-2017 coverage |
| **IPC** | ✅ Partial | Message queues implemented, pipes and shared memory planned |
| **Threading** | 🔄 Planned | pthread compatibility layer under development |

### Platform Support

- **Primary Target**: x86_64 architecture with full long mode (64-bit) support
- **Boot Protocol**: Multiboot2 standard for maximum bootloader compatibility
- **Virtualization**: Full QEMU/KVM support for development and testing
- **Hardware**: Designed for modern x86_64 hardware with APIC, MSI, and ACPI support

## Production Roadmap

### Phase 1: Core Infrastructure (Completed ✅)
- [x] 64-bit kernel bootstrap with Multiboot2
- [x] Virtual memory management with paging
- [x] Interrupt descriptor table (IDT) and exception handling
- [x] System call interface with Ring 0/3 transitions
- [x] Basic device drivers (keyboard, VGA, serial)
- [x] In-memory file system and initramfs support

### Phase 2: POSIX Foundations (In Progress ⚙️)
- [x] Process management structures
- [x] File descriptor abstraction
- [x] POSIX error codes (errno)
- [x] Basic IPC (message channels)
- [ ] Signal handling mechanism
- [ ] Process scheduler with fair time-slicing
- [x] Fork/exec implementation
- [ ] Pipe and FIFO support

### Phase 3: Security & Multi-User (In Progress ⚙️)
- [x] User authentication system
- [x] UID/GID-based permissions
- [ ] Capability-based security model
- [ ] File permission enforcement
- [ ] Secure credential storage
- [ ] Audit logging infrastructure

### Phase 4: Advanced Features
- [ ] Multi-threading support (pthreads)
- [ ] Shared memory (POSIX shm)
- [ ] Network stack (TCP/IP)
- [ ] Block device layer
- [ ] Ext2/4 filesystem driver
- [x] Dynamic linking and shared libraries (PT_INTERP detection, ld-linux.so included)

### Phase 5: Linux Compatibility
- [ ] Linux syscall translation layer
- [x] ELF dynamic linker compatibility (basic support, needs auxiliary vectors)
- [ ] Linux ABI compatibility layer
- [ ] Common Linux utilities port
- [ ] Package management integration

## Getting Started

To get started with NexaOS development, you'll need to set up your environment and familiarize yourself with the project's structure.

### Prerequisites

- Rust nightly toolchain with the `rust-src` and `llvm-tools-preview` components (`rustup toolchain install nightly` and `rustup component add rust-src llvm-tools-preview --toolchain nightly`).
- A working C toolchain (e.g. `build-essential` on Debian/Ubuntu) so the bundled GAS bootstrap can be assembled via `cc`.
- `ld.lld` (preferred) or GNU `ld` to satisfy the custom kernel linker invocation. The build scripts automatically fall back to `ld` when `ld.lld` is absent.
- `grub-mkrescue` and `xorriso` for packaging a bootable ISO.
- `qemu-system-x86_64` (or actual hardware, if you're daring) to launch the resulting image.

### Build & Run (work in progress)

```bash
# Clone the repo (if you haven't already)
git clone https://github.com/nexa-sys/nexa-os.git
cd nexa-os

# Ensure the right toolchain in this repo
rustup override set nightly
rustup component add rust-src llvm-tools-preview --toolchain nightly

# Build the kernel ELF (requires a C toolchain + lld available in PATH)
cargo build --release

# Produce a bootable ISO using GRUB
./scripts/build-iso.sh

# Boot the ISO in QEMU (serial output is forwarded to your terminal)
./scripts/run-qemu.sh
```

> ℹ️ **Troubleshooting:** 如果构建输出提示缺少 `cc`、`ld.lld` 或 `ld`，请安装相应编译工具链；同时确保 `grub-mkrescue`、`xorriso`、`qemu-system-x86_64` 可用。

更多中文说明、环境配置与调试/验证技巧可参考：

- [`docs/zh/getting-started.md`](docs/zh/getting-started.md)：环境准备与构建指南。
- [`docs/zh/tests.md`](docs/zh/tests.md)：当前测试流程与自动化计划。

## Shell Features (Latest Update)

NexaOS now includes a fully-featured interactive shell with production-grade functionality:

### Command Set (19 Commands)

**File & Directory Operations:**
- `ls [-a] [-l] [path]` - List directory contents with optional hidden files and detailed view
- `cat <file>` - Display file contents
- `stat <file>` - Show detailed file metadata (size, permissions, ownership)
- `pwd` - Print current working directory
- `cd [path]` - Change directory (defaults to root if no path given)
- `mkdir <path>` - Create directory (stub for future implementation)

**System Information:**
- `help` - Show comprehensive command help with editing keys reference
- `uname [-a]` - Display system information (version, architecture)
- `echo [text...]` - Print text to output

**User Management:**
- `whoami` - Display current logged-in user
- `users` - List all registered users
- `login <user>` - Authenticate and switch to specified user
- `logout` - Log out current user session
- `adduser [-a] <user>` - Create new user account (use -a flag for admin privileges)

**Inter-Process Communication:**
- `ipc-create` - Allocate new IPC message channel
- `ipc-send <channel> <message>` - Send message to IPC channel
- `ipc-recv <channel>` - Receive message from IPC channel

**Utilities:**
- `clear` - Clear screen display
- `exit` - Terminate shell session

### Advanced Line Editing

**Tab Completion:**
- Smart command name completion with longest-common-prefix expansion
- Path completion for file operations (ls, cat, stat, cd, mkdir)
- Directory indicator suffixes (/) for folders
- Multiple match display with automatic menu when ambiguous
- Hidden file filtering based on prefix (. prefix shows hidden files)

**Keyboard Shortcuts:**
- `Tab` - Complete command or path
- `Backspace` / `Delete` - Remove character before cursor
- `Ctrl-C` - Cancel current line and display new prompt
- `Ctrl-D` - Exit shell (only on empty line, otherwise ignored)
- `Ctrl-U` - Clear entire line
- `Ctrl-W` - Delete previous word
- `Ctrl-L` - Clear screen and redraw current line with prompt
- `Enter` - Execute command

### Implementation Highlights

- **Kernel/Userspace Separation**: Kernel provides raw byte input; shell handles all echoing and editing
- **UEFI & Legacy BIOS Support**: Works correctly in both boot modes with proper character echoing
- **Robust Input Handling**: Escape sequence filtering, error recovery, multi-byte read safety
- **Memory Efficient**: Fixed-size buffers, no dynamic allocation in userspace
- **POSIX-Inspired**: Standard stdin/stdout file descriptors, errno error reporting

### Testing

Run the interactive test guide:
```bash
./tests/shell_test.sh
```

Or boot and try these quick tests:
```bash
# Tab completion
he<Tab>          # completes to 'help'
ls /b<Tab>       # completes to 'ls /bin/'

# Navigation
pwd; cd /bin; pwd; cd; pwd

# System info
uname -a
echo "Hello, NexaOS!"
```

## Contributing

Contributions, experiments, and feedback are very welcome. Until the contribution guidelines are published, feel free to open an issue to discuss ideas, report bugs, or coordinate larger contributions.

## License

This project is released under the terms described in `LICENSE` in the repository root.

