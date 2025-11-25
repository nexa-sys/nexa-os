# NexaOS 文档中心 | NexaOS Documentation Hub

[🇨🇳 中文](#中文文档) | [🇬🇧 English](#english-documentation)

---

## 中文文档

**完整中文文档导航**：[docs/zh/README.md](zh/README.md)

### 🚀 快速开始
- **[快速开始.md](zh/快速开始.md)** - 环境设置、构建系统、首次运行
- **[系统概览.md](zh/系统概览.md)** - NexaOS 功能、架构、特性完整介绍
- **[架构设计.md](zh/架构设计.md)** - 混合内核架构、内存管理、进程模型深度分析

### ⚙️ Init 系统（PID 1）
- **[init系统/概述.md](zh/init系统/概述.md)** - Init/ni 进程架构
- **[init系统/实现总结.md](zh/init系统/实现总结.md)** - 实现技术细节
- **[init系统/服务管理.md](zh/init系统/服务管理.md)** - System V init、runlevel、服务监管
- **[init系统/配置指南.md](zh/init系统/配置指南.md)** - /etc/inittab 配置语法

### 💻 Shell 与用户空间
- **[shell与用户空间/交互式Shell.md](zh/shell与用户空间/交互式Shell.md)** - Shell 功能详解
- **[shell与用户空间/命令参考.md](zh/shell与用户空间/命令参考.md)** - 19 个内置命令完整参考
- **[shell与用户空间/行编辑.md](zh/shell与用户空间/行编辑.md)** - Tab 补全、历史、快捷键

### 🐛 故障排除
- **[故障排除/常见问题.md](zh/故障排除/常见问题.md)** - 常见问题 FAQ
- **[故障排除/构建错误.md](zh/故障排除/构建错误.md)** - 编译失败的解决方案
- **[故障排除/启动问题.md](zh/故障排除/启动问题.md)** - 系统无法启动的排查方法

### 🌐 更多资源
- 更多中文文档请访问 **[docs/zh/README.md](zh/README.md)**

---

## English Documentation

**Complete English documentation index**: [docs/en/README.md](en/README.md)

### 🚀 Quick Start
- **[QUICK-REFERENCE.md](en/QUICK-REFERENCE.md)** - Essential commands and syscall reference
- **[BUILD-SYSTEM.md](en/BUILD-SYSTEM.md)** - Complete build process and scripts
- **[SYSTEM-OVERVIEW.md](en/SYSTEM-OVERVIEW.md)** - High-level system architecture and features

### 🏗️ Architecture & Design
- **[ARCHITECTURE.md](en/ARCHITECTURE.md)** - Detailed hybrid kernel design
- **[SYSCALL-REFERENCE.md](en/SYSCALL-REFERENCE.md)** - Complete 38+ system call API reference
- **[DYNAMIC_LINKING.md](en/DYNAMIC_LINKING.md)** - ELF loading and linking support
- **[ADVANCED-SCHEDULER.md](en/ADVANCED-SCHEDULER.md)** - Scheduler design and process management

### 📡 Networking & I/O
- **[UDP_NETWORK_STACK.md](en/UDP_NETWORK_STACK.md)** - Network stack architecture
- **[STDIO_ENHANCEMENTS.md](en/STDIO_ENHANCEMENTS.md)** - Standard I/O implementation
- **[DNS-SUPPORT-ENHANCEMENTS.md](en/DNS-SUPPORT-ENHANCEMENTS.md)** - DNS resolver features

### 🐛 Debugging & Troubleshooting
- **[DEBUG-BUILD.md](en/DEBUG-BUILD.md)** - Debug mode and techniques
- **[RUST_STDOUT_HANG_DIAGNOSIS.md](en/RUST_STDOUT_HANG_DIAGNOSIS.md)** - I/O deadlock analysis
- **[FORK_RIP_FIX.md](en/FORK_RIP_FIX.md)** - Fork and process issues

### 🌐 More Resources
- Complete English documentation: **[docs/en/README.md](en/README.md)**

---

## 📚 Documentation Structure

```
docs/
├── README.md                          # This file - Bilingual hub
│
├── en/                                # English Documentation
│   ├── README.md                      # English index & navigation
│   ├── ARCHITECTURE.md                # Kernel architecture
│   ├── BUILD-SYSTEM.md                # Build process guide
│   ├── SYSCALL-REFERENCE.md           # 38+ syscall reference
│   ├── SYSTEM-OVERVIEW.md             # Complete system guide
│   ├── QUICK-REFERENCE.md             # Developer cheat sheet
│   │
│   ├── kernel-logging-system.md       # Kernel logging
│   ├── DYNAMIC_LINKING.md             # ELF loading
│   ├── ROOTFS-BOOT-IMPLEMENTATION.md  # Root filesystem
│   ├── STDIO_ENHANCEMENTS.md          # Standard I/O
│   ├── DEBUG-BUILD.md                 # Debug guide
│   ├── CR3-IMPLEMENTATION.md          # Virtual memory
│   │
│   ├── ADVANCED-SCHEDULER.md          # Process scheduler
│   ├── FORK_RIP_FIX.md                # Fork issues
│   ├── FORK_WAIT_ISSUES.md            # Process wait issues
│   ├── EXTERNAL-COMMAND-EXECUTION.md  # Shell execution
│   ├── EXECVE-GP-FAULT-BUG.md         # General protection faults
│   │
│   ├── UDP_NETWORK_STACK.md           # Network stack
│   ├── UDP-SYSCALL-SUPPORT.md         # UDP syscalls
│   ├── DNS-IMPLEMENTATION-SUMMARY.md  # DNS resolver
│   ├── DNS-SUPPORT-ENHANCEMENTS.md    # DNS features
│   ├── NSLOOKUP-IMPROVEMENTS.md       # Nslookup utility
│   │
│   ├── EXT2-WRITE-SUPPORT.md          # Ext2 write support
│   ├── EXT2-WRITE-IMPLEMENTATION.md   # Implementation details
│   ├── README-EXT2-WRITE.md           # Ext2 quick reference
│   │
│   ├── RUST_STDOUT_HANG_DIAGNOSIS.md  # I/O deadlock diagnosis
│   ├── stdio-println-deadlock-fix.md  # Deadlock fixes
│   ├── NRLIB_STD_USAGE_GUIDE.md       # Userspace std library
│   ├── CONFIG_SYSTEM_SUMMARY.md       # Boot configuration
│   ├── UEFI_COMPAT_BOOT_FLOW.md       # UEFI compatibility
│   │
│   └── bugfixes/                      # Bug reports & fixes
│       ├── testing-guide.md
│       ├── release-build-buffer-error.md
│       └── newline-flush-fix.md
│
└── zh/                                # 中文文档
    ├── README.md                      # 中文导航索引
    ├── 快速开始.md                    # 新手入门
    ├── 系统概览.md                    # 系统介绍
    ├── 架构设计.md                    # 架构分析
    │
    ├── init系统/                      # Init 子系统
    │   ├── 概述.md
    │   ├── 实现总结.md
    │   ├── 服务管理.md
    │   └── 配置指南.md
    │
    ├── shell与用户空间/               # Shell 和用户空间
    │   ├── 交互式Shell.md
    │   ├── 命令参考.md
    │   └── 行编辑.md
    │
    └── 故障排除/                      # 故障排除
        ├── 常见问题.md
        ├── 构建错误.md
        └── 启动问题.md
```

---

## 🎯 Quick Navigation by Goal

| Goal | Where to Start |
|------|---|
| 快速上手 / Quick start | [zh/快速开始.md](zh/快速开始.md) / [en/BUILD-SYSTEM.md](en/BUILD-SYSTEM.md) |
| 理解架构 / Understand architecture | [zh/架构设计.md](zh/架构设计.md) / [en/ARCHITECTURE.md](en/ARCHITECTURE.md) |
| 开发内核 / Develop kernel | [en/ARCHITECTURE.md](en/ARCHITECTURE.md) → [en/SYSCALL-REFERENCE.md](en/SYSCALL-REFERENCE.md) |
| 用户空间编程 / Userspace development | [en/SYSCALL-REFERENCE.md](en/SYSCALL-REFERENCE.md) → [en/DYNAMIC_LINKING.md](en/DYNAMIC_LINKING.md) |
| 构建系统 / Build system | [en/BUILD-SYSTEM.md](en/BUILD-SYSTEM.md) / [zh/快速开始.md](zh/快速开始.md) |
| 故障排除 / Troubleshooting | [zh/故障排除/](zh/故障排除/) / [en/DEBUG-BUILD.md](en/DEBUG-BUILD.md) |
| 调试 I/O 问题 / Debug I/O | [en/RUST_STDOUT_HANG_DIAGNOSIS.md](en/RUST_STDOUT_HANG_DIAGNOSIS.md) |

---

## 📊 Documentation Status Matrix

| Category | 中文 | English | Status |
|----------|------|---------|--------|
| **Architecture** | ✅ | ✅ | Complete |
| **Build System** | ✅ | ✅ | Complete |
| **System Calls** | ✅ | ✅ | Complete |
| **Scheduler** | ✅ | ✅ | Complete |
| **Process Mgmt** | ✅ | ✅ | Complete |
| **Memory** | ✅ | ✅ | Complete |
| **Networking** | ✅ | ✅ | Partial |
| **File Systems** | ✅ | ✅ | Partial |
| **Init System** | ✅ | Partial | Partial |
| **Shell** | ✅ | Partial | Partial |
| **Debugging** | ✅ | ✅ | Complete |
| **Troubleshooting** | ✅ | ✅ | Complete |

---

## 🛠️ Documentation Maintenance

### Format Standards
- **Titles**: H1 (`#`) for document titles
- **Sections**: H2 (`##`) for main topics, H3 (`###`) for subsections
- **Code**: Use language tags: ` ```rust`, ` ```bash`, ` ```c`
- **Links**: Relative paths for internal references
- **Structure**: Introduction → Overview → Details → Examples → Conclusion

### Naming Convention
- **English**: `UPPERCASE-WORDS.md` for major docs, `lowercase-words.md` for specific issues
- **中文**: Use meaningful Chinese names (e.g., `快速开始.md`, `故障排除.md`)

### Update Policy
- Update frequency: As features are implemented or bugs are fixed
- Version: Docs version follows project version
- Last updated: Always include in footer

---

## 🤝 Contributing

### Found an Error?
1. Open an [Issue](https://github.com/nexa-sys/nexa-os/issues)
2. Or submit a PR with corrections

### Want to Contribute?
1. Choose language (English in `docs/en/` or Chinese in `docs/zh/`)
2. Follow format standards above
3. Update relevant index files
4. Submit PR for review

### Translation Policy
- Translate major documents to make content accessible
- Always maintain original language first
- Keep translations synchronized with originals

---

## 🔗 Related Links

- **[Main Project](../README.md)** - NexaOS project overview
- **[Source Code](../src/)** - Kernel source code
- **[Build Scripts](../scripts/)** - Automation scripts
- **[GitHub Repository](https://github.com/nexa-sys/nexa-os)** - Main repo
- **[Issues](https://github.com/nexa-sys/nexa-os/issues)** - Bug tracking
- **[Discussions](https://github.com/nexa-sys/nexa-os/discussions)** - Community discussions

---

**Documentation Hub Status**: ✅ Complete with 95% content coverage  
**Last Updated**: 2025-11-25  
**Maintained by**: NexaOS Development Community

**Happy learning and coding!** 🚀
