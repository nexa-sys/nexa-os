# NexaOS English Documentation

Welcome to the NexaOS English documentation center! Here you'll find comprehensive technical documentation for the NexaOS hybrid-kernel operating system.

## 📖 Documentation Navigator

### 🚀 Quick Start
- [QUICK-REFERENCE.md](QUICK-REFERENCE.md) - Developer cheat sheet for common tasks
- [Building the System](BUILD-SYSTEM.md) - Complete build process guide
- [System Overview](SYSTEM-OVERVIEW.md) - High-level system description

### 🏗️ Architecture & Design
- [Architecture](ARCHITECTURE.md) - Kernel architecture, memory model, process management
- [System Overview](SYSTEM-OVERVIEW.md) - Complete system walkthrough (6-stage boot, components)
- [Boot Process Details](../zh/启动流程.md) - 6-stage boot phases explained

### 💻 Development Topics
- [Syscall Reference](SYSCALL-REFERENCE.md) - Complete 38+ syscall API documentation
- [Dynamic Linking](DYNAMIC_LINKING.md) - ELF loading, PT_INTERP, linking support
- [Build System](BUILD-SYSTEM.md) - Build automation, Cargo, custom targets
- [Kernel Logging System](kernel-logging-system.md) - TSC-based kernel logging with nanosecond precision

### ⚙️ Subsystems
- [Init System (PID 1)](../zh/init系统/概述.md) - System V init, runlevels, service management (see Chinese docs)
- [Interactive Shell](../zh/shell与用户空间/交互式Shell.md) - Shell features, 19 commands (see Chinese docs)
- [Root Filesystem Boot](ROOTFS-BOOT-IMPLEMENTATION.md) - Ext2 rootfs implementation

### 📊 Debugging & Troubleshooting
- [Debug Build Guide](DEBUG-BUILD.md) - Debug builds and techniques
- [Rust Stdout Hang Diagnosis](RUST_STDOUT_HANG_DIAGNOSIS.md) - I/O deadlock analysis
- [Stdio Println Deadlock Fix](stdio-println-deadlock-fix.md) - Println! deadlock resolution
- [Stdio Enhancements](STDIO_ENHANCEMENTS.md) - Stdio improvements and fixes
- [Bug Fixes & Testing](bugfixes/testing-guide.md) - Testing procedures

### 🔍 Advanced Topics
- [Fork RIP Fix](FORK_RIP_FIX.md) - Fork instruction pointer issue
- [Fork/Wait Issues](FORK_WAIT_ISSUES.md) - Process creation and synchronization problems
- [Configuration System](CONFIG_SYSTEM_SUMMARY.md) - Init configuration details

## 📚 Documentation by Role

### 👨‍💻 Kernel Developers
**Start here**:
1. [ARCHITECTURE.md](ARCHITECTURE.md) - Understand the kernel design
2. [SYSCALL-REFERENCE.md](SYSCALL-REFERENCE.md) - Learn the syscall interface
3. [BUILD-SYSTEM.md](BUILD-SYSTEM.md) - Master the build process
4. [kernel-logging-system.md](kernel-logging-system.md) - Use logging in your code

**Recommended Path**: QUICK-REFERENCE → ARCHITECTURE → SYSTEM-OVERVIEW → specific modules

### 🔧 Userspace Developers
**Start here**:
1. [SYSCALL-REFERENCE.md](SYSCALL-REFERENCE.md) - Understand available system calls
2. [DYNAMIC_LINKING.md](DYNAMIC_LINKING.md) - Learn how programs are loaded
3. [BUILD-SYSTEM.md](BUILD-SYSTEM.md) - Build your programs
4. [QUICK-REFERENCE.md](QUICK-REFERENCE.md) - Quick lookup

**Recommended Path**: QUICK-REFERENCE → SYSCALL-REFERENCE → DYNAMIC_LINKING → BUILD-SYSTEM

### 🧪 System Testers
**Start here**:
1. [BUILD-SYSTEM.md](BUILD-SYSTEM.md) - Build the system
2. [bugfixes/testing-guide.md](bugfixes/testing-guide.md) - Test procedures
3. [DEBUG-BUILD.md](DEBUG-BUILD.md) - Debug failing tests
4. [RUST_STDOUT_HANG_DIAGNOSIS.md](RUST_STDOUT_HANG_DIAGNOSIS.md) - Diagnose I/O issues

**Recommended Path**: BUILD-SYSTEM → testing-guide → specific issues

### 📖 Documentation Readers
**Start here**:
1. [SYSTEM-OVERVIEW.md](SYSTEM-OVERVIEW.md) - Understand the big picture
2. [ARCHITECTURE.md](ARCHITECTURE.md) - Deep dive into design
3. [QUICK-REFERENCE.md](QUICK-REFERENCE.md) - Find specific information
4. Other modules as needed

**Recommended Path**: SYSTEM-OVERVIEW → ARCHITECTURE → topic of interest

## 📋 Complete File Listing

### Core Documentation
- ✅ `ARCHITECTURE.md` - Hybrid kernel architecture, memory model, process management
- ✅ `SYSTEM-OVERVIEW.md` - Complete system guide (6-stage boot, subsystems, performance)
- ✅ `BUILD-SYSTEM.md` - Build process, scripts, compilation
- ✅ `SYSCALL-REFERENCE.md` - 38+ system call complete reference with C signatures
- ✅ `QUICK-REFERENCE.md` - Developer cheat sheet for quick lookup

### Technical Deep Dives
- ✅ `kernel-logging-system.md` - TSC-based logging, nanosecond timestamps
- ✅ `DYNAMIC_LINKING.md` - ELF loading, PT_INTERP, linking support
- ✅ `ROOTFS-BOOT-IMPLEMENTATION.md` - Ext2 root filesystem in initramfs
- ✅ `STDIO_ENHANCEMENTS.md` - Userspace stdio improvements
- ✅ `DEBUG-BUILD.md` - Debug build configuration and techniques

### Bug Analysis & Fixes
- ✅ `RUST_STDOUT_HANG_DIAGNOSIS.md` - Rust stdout deadlock analysis
- ✅ `stdio-println-deadlock-fix.md` - Println deadlock resolution
- ✅ `FORK_RIP_FIX.md` - Fork instruction pointer issue
- ✅ `FORK_WAIT_ISSUES.md` - Process creation synchronization
- ✅ `CONFIG_SYSTEM_SUMMARY.md` - Configuration system details

### Testing & Validation
- ✅ `bugfixes/testing-guide.md` - System testing procedures
- ✅ `bugfixes/release-build-buffer-error.md` - Release build error analysis
- ✅ `bugfixes/newline-flush-fix.md` - Line buffering fixes

## 🔗 Key Links

### Core Resources
- **Main Repository**: https://github.com/nexa-sys/nexa-os
- **Issue Tracker**: https://github.com/nexa-sys/nexa-os/issues
- **Discussions**: https://github.com/nexa-sys/nexa-os/discussions
- **Build Scripts**: ../../scripts/
- **Source Code**: ../../src/

### Build Commands
```bash
# Complete system build
./scripts/build-all.sh

# Run in QEMU
./scripts/run-qemu.sh

# Debug build
./scripts/build-rootfs-debug.sh

# Monitor QEMU serial output
tail -f /tmp/qemu-serial.log
```

### Related Chinese Documentation
For implementation details and learning materials in Chinese, see:
- [Chinese Docs Index](../zh/README.md) - Complete Chinese documentation center
- [Architecture (Chinese)](../zh/架构设计.md) - Architecture in Chinese
- [Init System (Chinese)](../zh/init系统/概述.md) - Init system detailed guide
- [Shell Guide (Chinese)](../zh/shell与用户空间/交互式Shell.md) - Shell complete guide

## 🛠️ Documentation Structure

```
docs/
├── README.md                          # Main navigation (this file)
│
├── en/                                # English documentation
│   ├── ARCHITECTURE.md                # Kernel architecture
│   ├── BUILD-SYSTEM.md                # Build process
│   ├── SYSCALL-REFERENCE.md           # Syscall API reference
│   ├── SYSTEM-OVERVIEW.md             # System complete guide
│   ├── QUICK-REFERENCE.md             # Developer cheat sheet
│   │
│   ├── kernel-logging-system.md       # Kernel logging
│   ├── DYNAMIC_LINKING.md             # ELF loading
│   ├── ROOTFS-BOOT-IMPLEMENTATION.md  # Root filesystem
│   ├── STDIO_ENHANCEMENTS.md          # Stdio improvements
│   ├── DEBUG-BUILD.md                 # Debug guide
│   │
│   ├── RUST_STDOUT_HANG_DIAGNOSIS.md  # Diagnosis
│   ├── stdio-println-deadlock-fix.md  # Deadlock fix
│   ├── FORK_RIP_FIX.md                # Fork RIP
│   ├── FORK_WAIT_ISSUES.md            # Fork/Wait issues
│   │
│   ├── CONFIG_SYSTEM_SUMMARY.md       # Configuration
│   │
│   └── bugfixes/                      # Bug fixes and testing
│       ├── testing-guide.md
│       ├── release-build-buffer-error.md
│       └── newline-flush-fix.md
│
├── zh/                                # Chinese documentation
│   ├── README.md                      # Chinese index
│   ├── 快速开始.md                    # Quick start
│   ├── 系统概览.md                    # System overview
│   ├── 架构设计.md                    # Architecture
│   │
│   ├── init系统/                      # Init subsystem
│   ├── shell与用户空间/               # Shell & userspace
│   ├── 内核开发/                      # Kernel development
│   │
│   ├── 故障排除/                      # Troubleshooting
│   └── 开发报告/                      # Development reports
│
└── (legacy files)                     # To be archived
```

## 📝 Documentation Standards

### Format Requirements
- Use **H1** (`#`) for document titles
- Use **H2** (`##`) and **H3** (`###`) for sections
- Use code fences with language tags: ` ```rust`, ` ```bash`, etc.
- Use relative links for internal references
- Include **table of contents** for long documents

### Content Requirements
- **Clear structure**: Introduction → Concepts → Details → Examples → Summary
- **Examples**: Provide working code samples when applicable
- **Cross-references**: Link to related documents for context
- **Clarity**: Avoid excessive jargon; define technical terms
- **Accuracy**: Verify against actual implementation in source code

### File Naming
- Use **kebab-case** for file names: `kernel-logging-system.md`
- Use **descriptive names**: `SYSCALL-REFERENCE.md` not `syscalls.md`
- Use **UPPERCASE.md** for major documents, `lowercase.md` for secondary

## ✏️ How to Contribute

### Found an Error?
1. Open an [Issue](https://github.com/nexa-sys/nexa-os/issues)
2. Or submit a Pull Request with corrections

### Want to Add Documentation?
1. Create the file in appropriate directory (`docs/en/` or `docs/zh/`)
2. Follow documentation standards (see above)
3. Update navigation in `docs/README.md` and `docs/en/README.md`
4. Ensure all links are correct (relative paths)

### Translation Policy
- **English documents** go in `docs/en/`
- **Chinese documents** go in `docs/zh/`
- Consider translating major documents to make content accessible
- Always maintain documentation in the original language first

## 📞 Support & Feedback

- **Questions about documentation?** → Open [Issue](https://github.com/nexa-sys/nexa-os/issues)
- **Suggestions?** → Use [Discussions](https://github.com/nexa-sys/nexa-os/discussions)
- **Want to help?** → Check [Chinese Index](../zh/开发报告/完成度报告.md) for TODO items

---

**Documentation Status**: ✅ Structure complete, 95% content ready  
**Last Updated**: 2025-11-12  
**Maintained by**: NexaOS Development Community

Happy learning and developing! 🚀
