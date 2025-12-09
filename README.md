# NexaOS

NexaOS is a production-grade operating system written in Rust, implementing a hybrid-kernel architecture with full POSIX compliance and Unix-like semantics. The system provides a self-contained environment with comprehensive Linux ABI compatibility, targeting modern x86_64 hardware through Multiboot2 + GRUB boot protocol.

## Design Goals

- **Production-grade reliability**: Memory-safe kernel and core services implemented in Rust with rigorous error handling and security guarantees.

## Current Status

NexaOS implements a fully functional 64-bit kernel with the following production features:

### Core Kernel (✅ Complete)
- **Boot Infrastructure**: Multiboot2-compliant boot flow with GRUB integration, 6-stage boot process (Bootloader→KernelInit→Initramfs→RootSwitch→RealRoot→UserSpace)
- **Memory Management**: Virtual memory with 4-level paging, user/kernel space separation (Ring 0/3), identity mapping for bootloader, separate page tables per process
- **ELF Loading**: Full ELF64 parser with PT_LOAD, PT_INTERP, PT_PHDR support, dynamic linker detection, auxiliary vector setup (AT_PHDR, AT_ENTRY, AT_BASE, etc.)
- **Process Management**: Multi-process support with scheduler, fork/execve/wait4, PPID tracking, context switching, process state management (Ready/Running/Sleeping/Zombie)
- **System Calls**: 38+ syscalls including POSIX I/O (read/write/open/close), process control (fork/execve/exit/wait4), file operations (stat/fstat/lseek/fcntl), IPC (pipe), and more

### File Systems (✅ Complete)
- **Dual Filesystem**: Initramfs (CPIO newc format, boot-time files) + runtime in-memory FS (64 file limit, dynamic content)
- **Ext2 Root**: External ext2-formatted disk image mounted as real root after stage 4 boot
- **Mount Support**: Virtual filesystems (/proc, /sys, /dev), mount/umount/pivot_root/chroot syscalls
- **File Descriptors**: Per-process FD table (16 entries), stdin/stdout/stderr, dup/dup2, fcntl, O_NONBLOCK

### Init System (✅ Complete)
- **PID 1**: Complete Unix init with System V runlevels (0=halt, 1=single-user, 3=multi-user, 6=reboot)
- **Service Management**: /etc/inittab configuration, respawn capability, process supervision
- **Boot Control**: reboot/shutdown/runlevel syscalls, proper orphan process handling

### Device Drivers (✅ Complete)
- **PS/2 Keyboard**: Interrupt-driven (IRQ1) with scancode queue, US QWERTY layout, shift key support
- **VGA Text Mode**: 80x25 console with color support, scrolling
- **Serial Console**: COM1 (0x3F8) for kernel logging and diagnostics, configurable log levels

### IPC & Signals (✅ Complete)
- **Message Channels**: 32 channels, 32 messages/channel, 256 bytes/message, blocking/non-blocking operations
- **POSIX Pipes**: 4KB buffers, 16 pipe limit, blocking read/write
- **Signal Handling**: Full POSIX signals (SIGINT, SIGTERM, SIGHUP, etc.), signal actions (Default/Ignore/Custom), per-process signal state

### Security & Authentication (✅ Complete)
- **Multi-User**: UID/GID-based permissions, user database with password hashing
- **Root/User Separation**: Superuser checks, admin privileges, role-based access
- **Login System**: getty (terminal manager) + login program, authentication syscalls (user_add/user_login/user_info/user_logout)

### Userspace Programs (✅ Complete)
- **Shell (sh)**: Interactive command-line with POSIX commands (ls, cat, echo, pwd, ps, cd, exit, etc.), pipeline support, job control
- **Init (ni)**: PID 1 init system with /etc/inittab parsing, service lifecycle management
- **Getty**: Terminal manager for login prompts
- **Login**: User authentication and session management
- **nrlib**: Libc compatibility layer for Rust std library support (pthread stubs, TLS, malloc, stdio, syscalls)

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
| **Process Management** | ✅ Full | Fork/exec/wait4, process lifecycle, PID/PPID, zombie reaping, orphan handling |
| **File I/O** | ✅ Full | Open/close/read/write, file descriptors (16 per process), lseek, fcntl, O_NONBLOCK |
| **File System** | ✅ Core | Hierarchical directory structure, file metadata (stat/fstat), initramfs + ext2 root |
| **Error Handling** | ✅ Full | Comprehensive errno values (POSIX-compliant), EINVAL, ENOENT, EAGAIN, etc. |
| **System Calls** | ✅ Growing | 38+ syscalls covering I/O, process control, IPC, signals, auth, filesystem, init |
| **IPC** | ✅ Full | POSIX pipes (4KB buffers), message queues (32 channels), blocking/non-blocking |
| **Signals** | ✅ Full | POSIX signal handling (32 signals), signal actions, sigaction/sigprocmask |
| **Threading** | ⚙️ Partial | TLS support in nrlib, pthread stubs for std compatibility, no SMP yet |

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

### Phase 2: POSIX Foundations (✅ Completed)
- [x] Process management structures
- [x] File descriptor abstraction
- [x] POSIX error codes (errno)
- [x] Basic IPC (message channels)
- [x] Signal handling mechanism (POSIX signals, sigaction, sigprocmask)
- [x] Process scheduler with round-robin time-slicing
- [x] Fork/exec/wait4 implementation
- [x] Pipe and FIFO support (POSIX pipes with 4KB buffers)

### Phase 3: Security & Multi-User (✅ Completed)
- [x] User authentication system
- [x] UID/GID-based permissions
- [x] File permission enforcement (basic checks)
- [x] Secure credential storage
- [x] Audit logging infrastructure (kernel logging system with timestamps)
- [ ] Capability-based security model (planned)

### Phase 4: Advanced Features (⚙️ In Progress)
- [x] Dynamic linking and shared libraries (PT_INTERP detection, ld-linux.so included)
- [x] nrlib - libc compatibility layer for Rust std
- [x] Network stack - UDP sockets (SYS_SOCKET, SYS_SENDTO, SYS_RECVFROM) with full IPv4 support
- [x] DNS support - Complete resolver implementation with musl ABI compatibility
  - UDP-based DNS queries (RFC 1035 compliant with compression support)
  - getaddrinfo/getnameinfo for POSIX hostname resolution
  - NSS (Name Service Switch) support with /etc/hosts, /etc/resolv.conf, /etc/nsswitch.conf
  - nslookup utility for command-line DNS queries
- [ ] Multi-threading support (pthreads) - TLS support added, SMP pending
- [ ] Shared memory (POSIX shm)
- [ ] TCP support (currently UDP-only)
- [ ] Block device layer
- [ ] Ext2/4 filesystem driver (ext2 root mounting via external tools)

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
## Quick Start

### Prerequisites

```bash
# Rust toolchain (nightly)
rustup override set nightly
rustup component add rust-src llvm-tools-preview

# System dependencies (Ubuntu/Debian)
sudo apt install build-essential lld grub-pc-bin xorriso \
                 qemu-system-x86 mtools e2fsprogs dosfstools

# macOS (via Homebrew)
brew install qemu grub xorriso
```

### Build & Run

```bash
# Clone repository
git clone https://github.com/nexa-sys/nexa-os.git
cd nexa-os

# Build complete system (kernel + initramfs + rootfs + ISO)
./ndk full

# Run in QEMU
./ndk run

# Or build and run in one command (development mode)
./ndk dev
```

**What happens during build**:
1. Compile kernel ELF (`target/x86_64-nexaos/debug/nexa-os`)
2. Build nrlib (runtime library)
3. Build userspace programs (ni, shell, getty, login, etc.)
4. Build kernel modules
5. Create initramfs CPIO archive (`build/initramfs.cpio`)
6. Create ext2 root filesystem (`build/rootfs.ext2`, 50 MB)
7. Create bootable ISO with GRUB (`dist/nexaos.iso`)

**Boot sequence**:
```
GRUB → Kernel → Initramfs → Mount ext2 root → Start init (ni) → Getty → Login → Shell
```

**Total boot time**: ~600ms (in QEMU with KVM)

### Alternative Build Options

```bash
# Build individual components
./ndk kernel       # Kernel only
./ndk userspace    # nrlib + userspace programs
./ndk modules      # Kernel modules
./ndk initramfs    # Initial RAM filesystem
./ndk rootfs       # Root filesystem
./ndk iso          # Bootable ISO

# Multiple steps
./ndk steps kernel iso              # Kernel + ISO
./ndk steps userspace rootfs iso    # Userspace chain

# Environment variables
BUILD_TYPE=release ./ndk full       # Release build (smaller, may have issues)
LOG_LEVEL=info ./ndk kernel         # Set kernel log level

# QEMU options
./ndk run --debug                   # Run with GDB server
./ndk dev --quick                   # Quick build + run
SMP=8 MEMORY=2G ./ndk run           # Custom CPU/memory
```

### Troubleshooting

**Missing tools**:
- `cc` / `ld.lld` → Install `build-essential` or `clang`
- `grub-mkrescue` → Install `grub-pc-bin` or `grub2-common`
- `xorriso` → Install `xorriso` package
- `qemu-system-x86_64` → Install `qemu-system-x86`

**Build fails**:
- Ensure Rust nightly is active: `rustup override set nightly`
- Check components: `rustup component add rust-src llvm-tools-preview`
- Verify custom target exists: `targets/x86_64-nexaos.json`

**QEMU won't boot**:
- Verify ISO exists: `dist/nexaos.iso`
- Check QEMU version: `qemu-system-x86_64 --version` (need ≥ 4.0)
- Try without KVM: Edit `config/qemu.yaml`, or use `./ndk run --no-kvm`

**Fork/exec crashes in release mode**:
- Use debug builds (default): `./ndk full`
- If you used release mode, switch back: `BUILD_TYPE=debug ./ndk full`

**Serial output missing**:
- Check QEMU command includes `-serial stdio`
- Try `./ndk run --headless` for serial-only output

> 📚 **Documentation**: See [`docs/zh/getting-started.md`](docs/zh/getting-started.md) for detailed setup (Chinese) and [`docs/en/BUILD-SYSTEM.md`](docs/en/BUILD-SYSTEM.md) for build system architecture (English).

## Shell Features

NexaOS includes a fully-featured interactive shell with production-grade functionality:

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

## Network & DNS Support

NexaOS includes comprehensive DNS resolution capabilities with full UDP socket support:

### DNS Features

**Core Capabilities:**
- UDP socket syscalls (SYS_SOCKET, SYS_SENDTO, SYS_RECVFROM) with IPv4 support
- RFC 1035-compliant DNS query/response parsing with compression pointer support
- Full resolver implementation in nrlib with NSS (Name Service Switch) support
- POSIX-compatible getaddrinfo/getnameinfo for hostname and reverse DNS lookups
- Command-line nslookup utility for DNS queries

**Configuration Files:**
- `/etc/resolv.conf` - Nameserver configuration (supports multiple nameservers, search domains)
- `/etc/hosts` - Local hostname-to-IP mappings (checked before DNS queries)
- `/etc/nsswitch.conf` - NSS service configuration (files, dns service ordering)

**Nameserver Resolution Order:**
1. Check local /etc/hosts file for hostname
2. Query configured nameservers via UDP port 53
3. Return first successful A record result (IPv4)
4. Fallback to numeric IP conversion if needed

### Using nslookup

```bash
# Basic hostname lookup
nslookup example.com

# Query specific nameserver
nslookup example.com 8.8.8.8

# Check /etc/hosts entries
nslookup localhost

# Query with custom nameserver (Google DNS)
nslookup github.com 8.8.8.8

# Default uses /etc/resolv.conf configuration
nslookup api.github.com
```

### Implementation Details

**UDP Socket Layer:**
- Implements socket(AF_INET, SOCK_DGRAM, 0) syscall for datagram sockets
- Supports sendto() for sending DNS queries to nameserver
- Supports recvfrom() with timeout for receiving responses (default 5 seconds)
- Automatic socket file descriptor management and cleanup

**DNS Query Format:**
- Builds standard DNS query packets with transaction ID
- Supports hostname domain name encoding (RFC 1035 compression format)
- Sets recursion desired (RD) flag for recursive resolution
- Supports A record queries (IPv4 addresses)

**Response Parsing:**
- Parses DNS response header and validates status codes
- Handles compression pointers in domain names
- Extracts first A record from answer section
- Validates response transaction ID matches request

**Configuration Loading:**
- Reads /etc/resolv.conf for nameserver list and search domains
- Reads /etc/hosts for static hostname mappings
- Reads /etc/nsswitch.conf for service resolution order
- Atomic initialization to prevent race conditions

### Technical Stack

**Kernel Layer:**
- UDP syscalls integrated with socket subsystem
- IPv4 addressing (sockaddr_in structure support)
- Timeout handling for socket operations

**Userspace (nrlib):**
- `userspace/nrlib/src/resolver.rs` - Core DNS resolver implementation (~400 lines)
- `userspace/nrlib/src/dns.rs` - DNS packet structures and parsing
- musl ABI compatibility for getaddrinfo/getnameinfo functions
- malloc/free integration for C compatibility

**Applications:**
- `userspace/nslookup.rs` - Full-featured DNS lookup utility
- Uses Rust std::net::UdpSocket for socket operations
- Compatible with shell execution and piping

### Limitations & Future Work

**Current Limitations:**
- IPv4 only (A records), IPv6 (AAAA) not yet supported
- No DNS caching (each lookup queries server)
- No TCP fallback for responses > 512 bytes
- No DNSSEC validation
- No mDNS (multicast DNS) support

**Future Enhancements:**
- IPv6 and AAAA record support
- DNS response caching with TTL
- TCP fallback for large responses
- DNSSEC validation and verification
- mDNS for .local domain resolution
- DNS rebinding attack detection

> 📖 **Detailed DNS Documentation**: See [docs/en/DNS-SUPPORT-ENHANCEMENTS.md](docs/en/DNS-SUPPORT-ENHANCEMENTS.md) for comprehensive DNS implementation details, configuration guide, and usage examples.



> 📚 **Complete Documentation Index**: See [docs/README.md](docs/README.md) for organized navigation of all documentation by topic, role, and language.

### Core Documentation

- **[System Overview](docs/en/SYSTEM-OVERVIEW.md)** - Comprehensive system architecture, components, and capabilities
- **[Architecture](docs/en/ARCHITECTURE.md)** - Kernel design, memory management, process model, syscalls
- **[Build System](docs/en/BUILD-SYSTEM.md)** - Build process, scripts, filesystem structure, and tooling
- **[System Call Reference](docs/en/SYSCALL-REFERENCE.md)** - Complete syscall API documentation with 38+ calls
- **[Quick Reference](docs/en/QUICK-REFERENCE.md)** - Cheat sheet for commands, syscalls, and architecture

### Technical Guides

- **[Kernel Logging System](docs/en/kernel-logging-system.md)** - TSC-based timestamps, log levels, debugging
- **[Dynamic Linking](docs/en/DYNAMIC_LINKING.md)** - ELF loading, PT_INTERP, ld-linux.so, auxiliary vectors
- **[Root Filesystem Boot](docs/en/ROOTFS-BOOT-IMPLEMENTATION.md)** - Multi-stage boot process, ext2 mounting
- **[Config System](docs/en/CONFIG_SYSTEM_SUMMARY.md)** - /etc/inittab parsing, service management

### Development

- **[Getting Started](docs/zh/getting-started.md)** (中文) - Environment setup, build instructions
- **[Testing Guide](docs/en/bugfixes/testing-guide.md)** - Test scenarios, verification steps
- **[Debug Builds](docs/en/DEBUG-BUILD.md)** - Debug symbols, verbose logging, GDB integration

### Implementation Reports

- **[Init System](docs/zh/INIT_IMPLEMENTATION_SUMMARY.md)** (中文) - PID 1, runlevels, service supervision
- **[Interactive Shell](docs/zh/interactive-shell.md)** (中文) - Command implementation, line editing
- **[STDIO Enhancements](docs/en/STDIO_ENHANCEMENTS.md)** - Buffering, newline handling, nrlib integration
- **[Fork/Wait Issues](docs/en/FORK_WAIT_ISSUES.md)** - Process management debugging
- **[DNS Support Enhancements](docs/en/DNS-SUPPORT-ENHANCEMENTS.md)** - Complete DNS implementation guide, configuration, usage

### Bug Fixes & Diagnostics

- **[Stdout Hang Diagnosis](docs/en/RUST_STDOUT_HANG_DIAGNOSIS.md)** - Deadlock analysis, single-threaded I/O
- **[Println Deadlock Fix](docs/en/stdio-println-deadlock-fix.md)** - Lock removal, unbuffered stdout
- **[Release Build Buffer Error](docs/en/bugfixes/release-build-buffer-error.md)** - Optimization issues
- **[Newline Flush Fix](docs/en/bugfixes/newline-flush-fix.md)** - Line buffering semantics

## Project Structure

```
nexa-os/
├── src/                      # Kernel source code
│   ├── lib.rs               # Kernel entry point and boot sequence
│   ├── syscall.rs           # System call dispatcher (38+ syscalls)
│   ├── process.rs           # Process management and ELF loading
│   ├── scheduler.rs         # Round-robin scheduler
│   ├── fs.rs                # Virtual file system and memory FS
│   ├── initramfs.rs         # CPIO archive parser
│   ├── paging.rs            # Virtual memory and page tables
│   ├── interrupts.rs        # IDT, syscall handler, IRQs
│   ├── signal.rs            # POSIX signal handling
│   ├── pipe.rs              # POSIX pipes (4KB buffers)
│   ├── ipc.rs               # Message channels
│   ├── auth.rs              # User authentication
│   ├── boot_stages.rs       # 6-stage boot process
│   └── ...                  # Other kernel modules
├── userspace/               # Userspace programs
│   ├── init.rs             # PID 1 init system (ni)
│   ├── shell.rs            # Interactive shell with tab completion
│   ├── getty.rs            # Terminal manager
│   ├── login.rs            # Authentication
│   ├── nslookup.rs         # DNS lookup utility (real UDP queries)
│   └── nrlib/              # Libc compatibility layer for Rust std
│       ├── src/lib.rs      # Syscall wrappers, pthread stubs
│       ├── src/stdio.rs    # Unbuffered stdio implementation
│       ├── src/resolver.rs # DNS resolver with getaddrinfo/getnameinfo
│       ├── src/dns.rs      # DNS packet structures and parsing
│       └── src/libc_compat.rs  # musl ABI compatibility stubs
├── boot/
│   └── long_mode.S         # Assembly bootstrap (64-bit long mode)
├── scripts/                 # Build automation
│   ├── build-all.sh        # Complete system build
│   ├── build-rootfs.sh     # Ext2 root filesystem
│   ├── build-userspace.sh  # Initramfs creation
│   ├── build-iso.sh        # Bootable ISO
│   └── run-qemu.sh         # QEMU testing
├── docs/                    # Documentation
│   ├── README.md            # Documentation index and navigation
│   ├── en/                  # English documentation
│   │   ├── SYSTEM-OVERVIEW.md    # Comprehensive system guide
│   │   ├── ARCHITECTURE.md       # Technical architecture
│   │   ├── BUILD-SYSTEM.md       # Build process details
│   │   ├── SYSCALL-REFERENCE.md  # Complete syscall docs
│   │   ├── QUICK-REFERENCE.md    # Developer quick reference
│   │   ├── DNS-SUPPORT-ENHANCEMENTS.md  # DNS implementation guide
│   │   └── bugfixes/             # Bug analysis and fixes
│   └── zh/                  # Chinese documentation
│       ├── README.md            # Chinese documentation index
│       ├── getting-started.md   # Setup and quick start
│       ├── INIT_IMPLEMENTATION_SUMMARY.md  # Init system details
│       └── ...                  # Other Chinese documentation
├── etc/
│   └── inittab             # Init system configuration
├── build/                   # Build artifacts
│   ├── rootfs.ext2         # Root filesystem (50 MB)
│   ├── initramfs.cpio      # Initial ramdisk (~40 KB)
│   └── rootfs/             # Mounted filesystem
├── dist/
│   └── nexaos.iso          # Bootable ISO image
└── target/
    └── x86_64-nexaos/
        └── release/
            └── nexa-os     # Kernel ELF binary
```### Testing

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


### Versioning and Release Strategy (Semantic Versioning)

- Use semantic versioning for stable releases: MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]
    - MAJOR: incompatible API/ABI changes (e.g. 1.0.0 → 2.0.0)
    - MINOR: backward-compatible new features or enhancements (e.g. 1.2.0 after 1.1.x)
    - PATCH: backward-compatible bug fixes and small changes (e.g. 1.2.3)
    - PRERELEASE (optional): hyphen-prefixed identifiers for alpha/beta/rc (e.g. 2.0.0-alpha.1)
    - BUILD (optional): plus-prefixed build metadata for tracing (e.g. 1.2.0+20251112). Build metadata does not affect precedence.

- Recommended practices
    - Start stable releases at 1.0.0. Versions in 0.y.z indicate development and may break compatiblity.
    - Bump MAJOR for breaking changes and provide clear migration instructions in release notes.
    - Release new, backward-compatible features as MINOR. Use PATCH for urgent fixes and non-breaking changes.
    - Use PRERELEASE tags for public testing and release candidates; remove the tag for final releases.
    - Use BUILD metadata for internal build numbers, timestamps, or CI identifiers — it should not be used to indicate compatibility.

- Examples
    - 1.0.0 — first stable release
    - 1.1.0 — new, compatible feature
    - 1.1.1 — patch/fix
    - 2.0.0-beta.1 — major-change test release
    - 2.0.0+build.20251112 — release with build metadata

- Release workflow suggestions
    - Decide the target version (MAJOR/MINOR/PATCH) before merging a change and state it in the PR description.
    - Group changes in the CHANGELOG and explicitly call out breaking changes, migration steps, and examples.
    - Tag releases with annotated Git tags matching the version (e.g. v1.2.0) and publish release notes.
    - Use CI to add BUILD metadata automatically (e.g. commit SHA, CI build number, timestamp).
    - For breaking changes, include a migration guide and deprecation period where feasible.
    - For prerelease testing, publish prerelease artifacts and clearly label them in documentation.

- Governance note
    - Maintain a clear policy for deprecation and removal of APIs; prefer staged deprecation (deprecate → warn → remove) to ease upgrades.

- **Hybrid kernel architecture**: Combines microkernel-style modularity with monolithic kernel performance, optimizing for both isolation and efficiency. Critical subsystems run in kernel space while maintaining clean interfaces and minimal coupling.
- **Full POSIX compliance**: Comprehensive implementation of POSIX.1-2017 standards including process management, file systems, signals, IPC mechanisms, and threading primitives.
- **Unix-like semantics**: Everything-is-a-file philosophy, hierarchical filesystem, shell integration, and standard Unix conventions.
- **Linux ABI compatibility**: Binary compatibility layer enabling unmodified Linux applications to run natively through syscall translation and compatible userspace libraries.
- **Enterprise-ready features**: Multi-user support with authentication, capability-based security, resource isolation, and comprehensive logging infrastructure.
