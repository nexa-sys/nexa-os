# NexaOS English Documentation

**Language**: English | [中文](../zh/README.md)

> **Platform**: x86_64  
> **Status**: Production-grade hybrid kernel OS  
> **Standards**: POSIX.1-2017, Unix-like semantics

---

## 📚 Documentation Index

### Quick Start for New Developers

- **[QUICK-REFERENCE.md](QUICK-REFERENCE.md)** - Essential commands and syscall reference
- **[BUILD-SYSTEM.md](BUILD-SYSTEM.md)** - Complete build process and scripts
- **[SYSTEM-OVERVIEW.md](SYSTEM-OVERVIEW.md)** - High-level system architecture and features

### Core Architecture & Design

- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Detailed hybrid kernel design, memory management, process model
- **[SYSCALL-REFERENCE.md](SYSCALL-REFERENCE.md)** - Complete 38+ system call API reference with examples
- **[DYNAMIC_LINKING.md](DYNAMIC_LINKING.md)** - ELF loading, PT_INTERP, dynamic linker implementation
- **[Kernel Logging System](kernel-logging-system.md)** - TSC timestamps, kinfo!/kdebug!/kerror! macros
- **[ADVANCED-SCHEDULER.md](ADVANCED-SCHEDULER.md)** - Scheduler design, process states, priority handling

### Boot & Initialization

- **[ROOTFS-BOOT-IMPLEMENTATION.md](ROOTFS-BOOT-IMPLEMENTATION.md)** - ext2 root filesystem mounting and init process
- **[UEFI_COMPAT_BOOT_FLOW.md](UEFI_COMPAT_BOOT_FLOW.md)** - UEFI loader, fallback driver, network boot
- **[CONFIG_SYSTEM_SUMMARY.md](CONFIG_SYSTEM_SUMMARY.md)** - Configuration system and boot parameters
- **[CR3-IMPLEMENTATION.md](CR3-IMPLEMENTATION.md)** - Page table management and virtual memory

### Standard I/O & User Libraries

- **[STDIO_ENHANCEMENTS.md](STDIO_ENHANCEMENTS.md)** - stdin/stdout implementation, buffering, synchronization
- **[RUST_STDOUT_HANG_DIAGNOSIS.md](RUST_STDOUT_HANG_DIAGNOSIS.md)** - Debugging printf/println hangs
- **[stdio-println-deadlock-fix.md](stdio-println-deadlock-fix.md)** - Deadlock prevention in libc/nrlib
- **[NRLIB_STD_USAGE_GUIDE.md](NRLIB_STD_USAGE_GUIDE.md)** - Using std library in userspace, pthread/TLS setup

### File System

- **[EXT2-WRITE-SUPPORT.md](EXT2-WRITE-SUPPORT.md)** - ext2 filesystem write implementation
- **[EXT2-WRITE-IMPLEMENTATION.md](EXT2-WRITE-IMPLEMENTATION.md)** - Detailed implementation notes
- **[README-EXT2-WRITE.md](README-EXT2-WRITE.md)** - Quickstart for ext2 operations

### Process Management & Debugging

- **[FORK_RIP_FIX.md](FORK_RIP_FIX.md)** - RIP corruption fix in fork syscall
- **[FORK_WAIT_ISSUES.md](FORK_WAIT_ISSUES.md)** - wait4, SIGCHLD handling, zombie processes
- **[EXTERNAL-COMMAND-EXECUTION.md](EXTERNAL-COMMAND-EXECUTION.md)** - Shell command execution, child process management
- **[EXTERNAL-COMMAND-STATUS.md](EXTERNAL-COMMAND-STATUS.md)** - Exit status and error handling
- **[EXECVE-GP-FAULT-BUG.md](EXECVE-GP-FAULT-BUG.md)** - General protection fault debugging in execve

### Networking (UDP/TCP)

- **[UDP-SYSCALL-SUPPORT.md](UDP-SYSCALL-SUPPORT.md)** - UDP socket syscalls and protocol implementation
- **[UDP_NETWORK_STACK.md](UDP_NETWORK_STACK.md)** - Network stack architecture
- **[UEFI_COMPAT_NETWORK_TCP.md](UEFI_COMPAT_NETWORK_TCP.md)** - TCP support in UEFI compatibility layer
- **[DNS-IMPLEMENTATION-SUMMARY.md](DNS-IMPLEMENTATION-SUMMARY.md)** - DNS implementation overview
- **[DNS-SUPPORT-ENHANCEMENTS.md](DNS-SUPPORT-ENHANCEMENTS.md)** - DNS resolver and nslookup utility
- **[NSLOOKUP-IMPROVEMENTS.md](NSLOOKUP-IMPROVEMENTS.md)** - Query types, caching, performance

### Debugging & Build Variants

- **[DEBUG-BUILD.md](DEBUG-BUILD.md)** - Debug mode compilation, logging levels, GDB setup
- **[bugfixes/stdio-println-deadlock-fix.md](bugfixes/stdio-println-deadlock-fix.md)** - Detailed fix for libc deadlock
- **[bugfixes/release-build-buffer-error.md](bugfixes/release-build-buffer-error.md)** - Release mode buffer issues
- **[bugfixes/testing-guide.md](bugfixes/testing-guide.md)** - Testing procedures and validation

### Legacy & Reference

- **[UEFI_COMPAT_FALLBACK_DRIVER.md](UEFI_COMPAT_FALLBACK_DRIVER.md)** - UEFI fallback driver design

---

## 📖 Documentation by Role

### 👨‍💻 For Kernel Developers

**Recommended Reading Order**:
1. [QUICK-REFERENCE.md](QUICK-REFERENCE.md) - Essential commands (5 min)
2. [ARCHITECTURE.md](ARCHITECTURE.md) - Deep architecture dive (30 min)
3. [SYSCALL-REFERENCE.md](SYSCALL-REFERENCE.md) - API reference (20 min)
4. [kernel-logging-system.md](kernel-logging-system.md) - Logging in code (10 min)
5. [ADVANCED-SCHEDULER.md](ADVANCED-SCHEDULER.md) - Process scheduling (20 min)

**Problem-Specific Docs**:
- Stuck on boot? → [ROOTFS-BOOT-IMPLEMENTATION.md](ROOTFS-BOOT-IMPLEMENTATION.md)
- Syscall failing? → [SYSCALL-REFERENCE.md](SYSCALL-REFERENCE.md)
- Memory issues? → [CR3-IMPLEMENTATION.md](CR3-IMPLEMENTATION.md)
- Fork problems? → [FORK_RIP_FIX.md](FORK_RIP_FIX.md)

### 💻 For Userspace Developers

**Recommended Reading Order**:
1. [QUICK-REFERENCE.md](QUICK-REFERENCE.md) - Quick start (5 min)
2. [SYSCALL-REFERENCE.md](SYSCALL-REFERENCE.md) - Available syscalls (20 min)
3. [DYNAMIC_LINKING.md](DYNAMIC_LINKING.md) - Program loading (15 min)
4. [NRLIB_STD_USAGE_GUIDE.md](NRLIB_STD_USAGE_GUIDE.md) - Using std in userspace (15 min)
5. [BUILD-SYSTEM.md](BUILD-SYSTEM.md) - Build your programs (10 min)

**Quick Answers**:
- How do I call a syscall? → [SYSCALL-REFERENCE.md](SYSCALL-REFERENCE.md)
- My program won't load? → [DYNAMIC_LINKING.md](DYNAMIC_LINKING.md)
- I/O is hanging? → [RUST_STDOUT_HANG_DIAGNOSIS.md](RUST_STDOUT_HANG_DIAGNOSIS.md)

### 🧪 For System Testers & QA

**Recommended Reading Order**:
1. [QUICK-REFERENCE.md](QUICK-REFERENCE.md) - Start here (5 min)
2. [BUILD-SYSTEM.md](BUILD-SYSTEM.md) - Build the system (10 min)
3. [bugfixes/testing-guide.md](bugfixes/testing-guide.md) - Test procedures (15 min)
4. [DEBUG-BUILD.md](DEBUG-BUILD.md) - Debug mode (10 min)
5. Issue-specific docs as needed

---

## 🗂️ Documentation by Problem

| Problem | Solution |
|---------|----------|
| "How do I build?" | [BUILD-SYSTEM.md](BUILD-SYSTEM.md) |
| "Build fails" | [BUILD-SYSTEM.md](BUILD-SYSTEM.md) → relevant bugfix docs |
| "System won't boot" | [ROOTFS-BOOT-IMPLEMENTATION.md](ROOTFS-BOOT-IMPLEMENTATION.md) |
| "System hangs" | [DEBUG-BUILD.md](DEBUG-BUILD.md) |
| "Syscall not working" | [SYSCALL-REFERENCE.md](SYSCALL-REFERENCE.md) |
| "Shell command hangs" | [EXTERNAL-COMMAND-EXECUTION.md](EXTERNAL-COMMAND-EXECUTION.md) |
| "printf/println hangs" | [RUST_STDOUT_HANG_DIAGNOSIS.md](RUST_STDOUT_HANG_DIAGNOSIS.md) |
| "Child process issues" | [FORK_RIP_FIX.md](FORK_RIP_FIX.md) + [FORK_WAIT_ISSUES.md](FORK_WAIT_ISSUES.md) |
| "File operations fail" | [EXT2-WRITE-SUPPORT.md](EXT2-WRITE-SUPPORT.md) |
| "Network not working" | [UDP_NETWORK_STACK.md](UDP_NETWORK_STACK.md) |

---

## 🔗 Cross-Links & Resources

### Quick Commands

```bash
./scripts/build-all.sh        # Complete system build
./scripts/run-qemu.sh         # Run in QEMU
cargo build --release         # Kernel only
./scripts/build-userspace.sh  # Userspace only
./scripts/build-rootfs.sh     # Root filesystem only
```

### Related Documentation

- **[Main Docs](../README.md)** - Bilingual navigation
- **[Chinese Docs](../zh/README.md)** - Complete Chinese documentation center
- **[Project README](../../README.md)** - Main project description
- **[Build Scripts](../../scripts/)** - Automation scripts
- **[Source Code](../../src/)** - Kernel source

---

## 📋 Complete File Listing

### Navigation & Index
- `README.md` - This file (navigation hub)
- `../README.md` - Bilingual navigation

### Core System
- `ARCHITECTURE.md` - Kernel architecture
- `SYSTEM-OVERVIEW.md` - Complete system walkthrough
- `BUILD-SYSTEM.md` - Build process
- `QUICK-REFERENCE.md` - Commands cheat sheet
- `SYSCALL-REFERENCE.md` - 38+ syscall API

### Boot & Memory
- `ROOTFS-BOOT-IMPLEMENTATION.md` - Root filesystem boot
- `CR3-IMPLEMENTATION.md` - Virtual memory
- `DYNAMIC_LINKING.md` - ELF loading
- `CONFIG_SYSTEM_SUMMARY.md` - Boot configuration

### Subsystems
- `kernel-logging-system.md` - TSC-based logging
- `STDIO_ENHANCEMENTS.md` - Standard I/O
- `UEFI_COMPAT_BOOT_FLOW.md` - UEFI compatibility
- `ADVANCED-SCHEDULER.md` - Process scheduling

### Networking
- `UDP_NETWORK_STACK.md` - Network architecture
- `UDP-SYSCALL-SUPPORT.md` - UDP syscalls
- `DNS-IMPLEMENTATION-SUMMARY.md` - DNS resolver
- `DNS-SUPPORT-ENHANCEMENTS.md` - DNS features
- `NSLOOKUP-IMPROVEMENTS.md` - Nslookup utility

### Debugging & Issues
- `DEBUG-BUILD.md` - Debug mode and tools
- `RUST_STDOUT_HANG_DIAGNOSIS.md` - I/O deadlock
- `stdio-println-deadlock-fix.md` - Deadlock fixes
- `FORK_RIP_FIX.md` - Fork instruction pointer
- `FORK_WAIT_ISSUES.md` - Process wait issues
- `EXTERNAL-COMMAND-EXECUTION.md` - Shell execution
- `EXTERNAL-COMMAND-STATUS.md` - Exit status
- `EXECVE-GP-FAULT-BUG.md` - General protection fault
- `NRLIB_STD_USAGE_GUIDE.md` - Userspace std

### File System
- `EXT2-WRITE-SUPPORT.md` - Ext2 write capability
- `EXT2-WRITE-IMPLEMENTATION.md` - Implementation details
- `README-EXT2-WRITE.md` - Quick reference

### Testing & Bug Reports
- `bugfixes/testing-guide.md` - Test procedures
- `bugfixes/release-build-buffer-error.md` - Release issues
- `bugfixes/newline-flush-fix.md` - Line buffering fixes

---

## ✏️ Contributing

### Found an Error?
1. Open an [Issue](https://github.com/nexa-sys/nexa-os/issues)
2. Or submit a Pull Request

### Want to Add Documentation?
1. Create file in `docs/en/`
2. Follow format standards (see below)
3. Update this README with entry
4. Ensure all relative links work

### Format Standards
- Use **H1** (`#`) for titles only
- Use **H2** (`##`), **H3** (`###`) for sections
- Use code fences with language: ` ```rust`, ` ```bash`
- Use relative links for internal references
- Include TOC for documents >500 lines

---

**Documentation Status**: ✅ Complete  
**Last Updated**: 2025-11-25  
**Maintained by**: NexaOS Development Community

🚀 Happy coding!
