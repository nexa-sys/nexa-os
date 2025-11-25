# NexaOS 中文文档中心

**语言**: 中文 | [English](../en/README.md)

> **平台**: x86_64  
> **状态**: 生产级混合内核操作系统  
> **标准**: POSIX.1-2017, Unix-like 语义

---

## 📚 文档导航

### 🚀 新手快速入门

- **[快速开始.md](快速开始.md)** - 环境设置、构建和运行系统
- **[../en/BUILD-SYSTEM.md](../en/BUILD-SYSTEM.md)** - 完整构建流程（英文）
- **[系统概览.md](系统概览.md)** - NexaOS 系统架构和功能概览

### 🏗️ 系统架构与设计

- **[架构设计.md](架构设计.md)** - 混合内核架构、内存管理、进程模型详解
- **[../en/SYSCALL-REFERENCE.md](../en/SYSCALL-REFERENCE.md)** - 38+ 系统调用完整参考（英文）
- **[../en/ARCHITECTURE.md](../en/ARCHITECTURE.md)** - 详细架构文档（英文）
- **[../en/kernel-logging-system.md](../en/kernel-logging-system.md)** - 内核日志系统（英文）
- **[../en/ADVANCED-SCHEDULER.md](../en/ADVANCED-SCHEDULER.md)** - 进程调度器设计（英文）

### ⚙️ Init 系统（PID 1）

- **[init系统/概述.md](init系统/概述.md)** - Init/ni 进程架构
- **[init系统/实现总结.md](init系统/实现总结.md)** - 实现技术细节
- **[init系统/服务管理.md](init系统/服务管理.md)** - System V init、runlevel、服务监管
- **[init系统/配置指南.md](init系统/配置指南.md)** - /etc/inittab 配置语法和示例

### 💻 Shell 与用户空间

- **[shell与用户空间/交互式Shell.md](shell与用户空间/交互式Shell.md)** - Shell 功能详解
- **[shell与用户空间/命令参考.md](shell与用户空间/命令参考.md)** - 19 个内置命令完整参考
- **[shell与用户空间/行编辑.md](shell与用户空间/行编辑.md)** - Tab 补全、历史、快捷键
- **[../en/NRLIB_STD_USAGE_GUIDE.md](../en/NRLIB_STD_USAGE_GUIDE.md)** - 在用户空间使用 std（英文）

### 🔧 内核开发与增强

- **[../en/DYNAMIC_LINKING.md](../en/DYNAMIC_LINKING.md)** - ELF 加载、PT_INTERP、动态链接（英文）
- **[../en/ROOTFS-BOOT-IMPLEMENTATION.md](../en/ROOTFS-BOOT-IMPLEMENTATION.md)** - 根文件系统启动（英文）
- **[../en/CR3-IMPLEMENTATION.md](../en/CR3-IMPLEMENTATION.md)** - 虚拟内存和分页管理（英文）
- **[../en/CONFIG_SYSTEM_SUMMARY.md](../en/CONFIG_SYSTEM_SUMMARY.md)** - 启动配置系统（英文）

### 📡 网络与通信

- **[../en/UDP_NETWORK_STACK.md](../en/UDP_NETWORK_STACK.md)** - 网络栈架构（英文）
- **[../en/UDP-SYSCALL-SUPPORT.md](../en/UDP-SYSCALL-SUPPORT.md)** - UDP 套接字系统调用（英文）
- **[../en/DNS-IMPLEMENTATION-SUMMARY.md](../en/DNS-IMPLEMENTATION-SUMMARY.md)** - DNS 实现概览（英文）
- **[../en/DNS-SUPPORT-ENHANCEMENTS.md](../en/DNS-SUPPORT-ENHANCEMENTS.md)** - DNS 解析器和 nslookup（英文）

### 📊 标准 I/O 与用户库

- **[../en/STDIO_ENHANCEMENTS.md](../en/STDIO_ENHANCEMENTS.md)** - 标准 I/O 实现（英文）
- **[../en/RUST_STDOUT_HANG_DIAGNOSIS.md](../en/RUST_STDOUT_HANG_DIAGNOSIS.md)** - printf/println 死锁诊断（英文）
- **[../en/stdio-println-deadlock-fix.md](../en/stdio-println-deadlock-fix.md)** - 死锁修复方案（英文）

### 🐛 调试与故障排除

- **[../en/DEBUG-BUILD.md](../en/DEBUG-BUILD.md)** - 调试构建指南（英文）
- **[故障排除/常见问题.md](故障排除/常见问题.md)** - 常见问题 FAQ
- **[故障排除/构建错误.md](故障排除/构建错误.md)** - 编译失败的解决方案
- **[故障排除/启动问题.md](故障排除/启动问题.md)** - 内核启动故障排查
- **[../en/FORK_RIP_FIX.md](../en/FORK_RIP_FIX.md)** - Fork 指令指针问题（英文）
- **[../en/FORK_WAIT_ISSUES.md](../en/FORK_WAIT_ISSUES.md)** - Fork/wait 问题（英文）
- **[../en/EXTERNAL-COMMAND-EXECUTION.md](../en/EXTERNAL-COMMAND-EXECUTION.md)** - Shell 命令执行（英文）
- **[../en/EXECVE-GP-FAULT-BUG.md](../en/EXECVE-GP-FAULT-BUG.md)** - execve 常规保护故障（英文）

### 📁 文件系统

- **[../en/EXT2-WRITE-SUPPORT.md](../en/EXT2-WRITE-SUPPORT.md)** - ext2 文件系统写支持（英文）
- **[../en/EXT2-WRITE-IMPLEMENTATION.md](../en/EXT2-WRITE-IMPLEMENTATION.md)** - 实现细节（英文）
- **[../en/README-EXT2-WRITE.md](../en/README-EXT2-WRITE.md)** - ext2 快速参考（英文）

### 🧪 测试与验证

- **[../en/bugfixes/testing-guide.md](../en/bugfixes/testing-guide.md)** - 测试程序和流程（英文）
- **[../en/bugfixes/release-build-buffer-error.md](../en/bugfixes/release-build-buffer-error.md)** - 发布构建错误（英文）

---

## 👥 按角色推荐阅读

### 👨‍💻 内核开发者

**推荐阅读顺序**:
1. [../en/QUICK-REFERENCE.md](../en/QUICK-REFERENCE.md) - 快速参考（5 分钟）
2. [架构设计.md](架构设计.md) 或 [../en/ARCHITECTURE.md](../en/ARCHITECTURE.md) - 架构深度分析（30 分钟）
3. [../en/SYSCALL-REFERENCE.md](../en/SYSCALL-REFERENCE.md) - API 参考（20 分钟）
4. [../en/kernel-logging-system.md](../en/kernel-logging-system.md) - 日志系统（10 分钟）
5. [../en/ADVANCED-SCHEDULER.md](../en/ADVANCED-SCHEDULER.md) - 进程调度（20 分钟）

**问题排查**:
- 启动失败？→ [../en/ROOTFS-BOOT-IMPLEMENTATION.md](../en/ROOTFS-BOOT-IMPLEMENTATION.md)
- 系统调用失败？→ [../en/SYSCALL-REFERENCE.md](../en/SYSCALL-REFERENCE.md)
- 内存问题？→ [../en/CR3-IMPLEMENTATION.md](../en/CR3-IMPLEMENTATION.md)
- Fork 问题？→ [../en/FORK_RIP_FIX.md](../en/FORK_RIP_FIX.md)

### 💻 用户空间开发者

**推荐阅读顺序**:
1. [快速开始.md](快速开始.md) - 快速开始（10 分钟）
2. [../en/SYSCALL-REFERENCE.md](../en/SYSCALL-REFERENCE.md) - 可用系统调用（20 分钟）
3. [../en/DYNAMIC_LINKING.md](../en/DYNAMIC_LINKING.md) - 程序加载（15 分钟）
4. [../en/NRLIB_STD_USAGE_GUIDE.md](../en/NRLIB_STD_USAGE_GUIDE.md) - 在用户空间使用 std（15 分钟）
5. [../en/BUILD-SYSTEM.md](../en/BUILD-SYSTEM.md) - 构建你的程序（10 分钟）

**快速答案**:
- 如何调用系统调用？→ [../en/SYSCALL-REFERENCE.md](../en/SYSCALL-REFERENCE.md)
- 程序无法加载？→ [../en/DYNAMIC_LINKING.md](../en/DYNAMIC_LINKING.md)
- I/O 挂起？→ [../en/RUST_STDOUT_HANG_DIAGNOSIS.md](../en/RUST_STDOUT_HANG_DIAGNOSIS.md)

### 🧪 系统测试员

**推荐阅读顺序**:
1. [../en/QUICK-REFERENCE.md](../en/QUICK-REFERENCE.md) - 从这里开始（5 分钟）
2. [../en/BUILD-SYSTEM.md](../en/BUILD-SYSTEM.md) - 构建系统（10 分钟）
3. [../en/bugfixes/testing-guide.md](../en/bugfixes/testing-guide.md) - 测试程序（15 分钟）
4. [../en/DEBUG-BUILD.md](../en/DEBUG-BUILD.md) - 调试模式（10 分钟）
5. 根据需要查看具体问题文档

---

## 🗂️ 按问题类型查找

| 问题 | 解决方案 |
|------|--------|
| "如何构建？" | [../en/BUILD-SYSTEM.md](../en/BUILD-SYSTEM.md) |
| "构建失败" | [../en/BUILD-SYSTEM.md](../en/BUILD-SYSTEM.md) → 相关故障排查 |
| "系统无法启动" | [../en/ROOTFS-BOOT-IMPLEMENTATION.md](../en/ROOTFS-BOOT-IMPLEMENTATION.md) |
| "系统挂起" | [../en/DEBUG-BUILD.md](../en/DEBUG-BUILD.md) |
| "系统调用不工作" | [../en/SYSCALL-REFERENCE.md](../en/SYSCALL-REFERENCE.md) |
| "Shell 命令挂起" | [../en/EXTERNAL-COMMAND-EXECUTION.md](../en/EXTERNAL-COMMAND-EXECUTION.md) |
| "printf/println 挂起" | [../en/RUST_STDOUT_HANG_DIAGNOSIS.md](../en/RUST_STDOUT_HANG_DIAGNOSIS.md) |
| "子进程问题" | [../en/FORK_RIP_FIX.md](../en/FORK_RIP_FIX.md) + [../en/FORK_WAIT_ISSUES.md](../en/FORK_WAIT_ISSUES.md) |
| "文件操作失败" | [../en/EXT2-WRITE-SUPPORT.md](../en/EXT2-WRITE-SUPPORT.md) |
| "网络不工作" | [../en/UDP_NETWORK_STACK.md](../en/UDP_NETWORK_STACK.md) |

---

## 🔗 快速命令

```bash
./scripts/build-all.sh        # 完整系统构建
./scripts/run-qemu.sh         # 在 QEMU 运行
cargo build --release         # 仅内核
./scripts/build-userspace.sh  # 仅用户空间
./scripts/build-rootfs.sh     # 仅根文件系统
```

---

## 📋 完整文件列表

### 导航与索引
- `README.md` - 本文件（导航中心）
- `../README.md` - 双语导航
- `../en/README.md` - 英文文档索引

### 核心系统
- `系统概览.md` - 系统完整功能介绍
- `架构设计.md` - 架构深入分析
- `快速开始.md` - 新手快速入门

### Init 系统
- `init系统/概述.md` - Init 进程架构
- `init系统/实现总结.md` - 实现细节
- `init系统/服务管理.md` - 服务和 runlevel
- `init系统/配置指南.md` - inittab 配置

### Shell 与用户空间
- `shell与用户空间/交互式Shell.md` - Shell 完整指南
- `shell与用户空间/命令参考.md` - 19 个命令参考
- `shell与用户空间/行编辑.md` - 编辑和补全功能

### 故障排除
- `故障排除/常见问题.md` - 常见问题 FAQ
- `故障排除/构建错误.md` - 编译错误解决
- `故障排除/启动问题.md` - 启动故障排查

---

## ✏️ 如何贡献

### 发现错误？
1. 打开 [Issue](https://github.com/nexa-sys/nexa-os/issues)
2. 或提交 Pull Request 修正

### 想添加文档？
1. 在 `docs/zh/` 创建文件
2. 遵循格式标准（见下）
3. 更新本 README 中的条目
4. 确保所有相对链接正确

### 格式标准
- 使用 **H1** (`#`) 作为标题
- 使用 **H2** (`##`)、**H3** (`###`) 作为章节
- 代码块使用语言标签：` ```rust`、` ```bash`
- 内部链接使用相对路径
- 长文档（>500 行）包含目录

---

## 🔗 相关资源

- **[英文文档](../en/README.md)** - 完整英文文档索引
- **[主文档](../README.md)** - 双语导航中心
- **[项目 README](../../README.md)** - 项目主页
- **[构建脚本](../../scripts/)** - 自动化脚本
- **[源代码](../../src/)** - 内核源码

---

**文档状态**: ✅ 完整  
**最后更新**: 2025-11-25  
**维护者**: NexaOS 开发社区

🚀 祝你开发愉快！
