# NexaOS 开发入门指南

> 更新时间：2025-11-03

NexaOS 是一个采用 Rust 编写的生产级操作系统，实现了完整的混合内核架构、全面的 POSIX 标准兼容以及 Linux ABI 二进制兼容层。本文档为中文开发者提供快速上手指导，涵盖必备依赖、构建流程、调试方式与常见问题。

## 系统定位

NexaOS 不是教育性质的实验系统，而是面向生产环境的现代操作系统，具备以下特性：

- **混合内核架构**：结合微内核的模块化与宏内核的性能优势
- **POSIX 标准合规**：完整实现 POSIX.1-2017 核心接口
- **Unix-like 语义**：遵循传统 Unix 设计哲学和约定
- **企业级特性**：多用户支持、基于能力的安全模型、资源隔离

## 环境准备

### 系统依赖

请确保使用 64 位 Linux 环境，并安装以下软件：

- Rust 夜ly 工具链（自动通过 `rustup override` 配置）
- GCC/Clang 编译工具链（建议安装 `build-essential` 或对应发行版的开发套件）
- GNU Binutils（尤其是 `ld.bfd`）：链接脚本依赖 GNU LD 的 `AT()` 语义，`scripts/linker.sh` 默认优先调用。
- `lld`：可选的 LLVM 链接器（若安装仍可手动切换以获得更快的链接速度）。
- `grub-mkrescue` 与 `xorriso`：用于创建 Multiboot2 兼容的 ISO 镜像
- `qemu-system-x86_64`：虚拟机调试首选

```bash
# 以 Debian/Ubuntu 为例
sudo apt update
sudo apt install build-essential lld grub-pc-bin xorriso qemu-system-x86
```

### Rust 工具链

`rust-toolchain.toml` 已锁定 nightly 版本，无需全局切换默认工具链，但建议安装必要组件：

```bash
cd nexa-os
rustup override set nightly
rustup component add rust-src llvm-tools-preview --toolchain nightly
```

## 构建与运行

1. **构建内核 ELF**
   ```bash
   cargo build --release
   ```
   构建过程会自动将 `boot/long_mode.S` 编译为目标文件，并链接自定义的 `linker.ld`，不再依赖 `nasm`。

2. **生成可引导 ISO**
   ```bash
   ./scripts/build-iso.sh
   ```
   该脚本会在 `dist/nexaos.iso` 下生成镜像，内部包含 GRUB 配置和内核映像。

3. **使用 QEMU 启动**
   ```bash
   ./scripts/run-qemu.sh
   ```
   内核启动信息会同时输出到 VGA 文本模式和串口，脚本将串口重定向到当前终端，便于调试。

## 启动流程概览

1. Multiboot2 头让 GRUB 在受保护模式下加载内核。
2. `boot/long_mode.S` 构造初始页表，启用 PAE 与长模式，并跳转到 64 位入口 `kmain`。
3. Rust 内核初始化串口与 VGA，并解析 Multiboot2 提供的命令行、内存映射、模块信息。
4. 当前阶段将内存区域信息打印到串口，为后续内存管理器实现提供基础数据。

## 常见问题排查

| 问题 | 可能原因 | 解决方案 |
|------|----------|----------|
| 构建阶段提示找不到 `grub-mkrescue` 或 `xorriso` | ISO 工具未安装 | 参考“系统依赖”章节安装相关软件 |
| 链接阶段报错 `ld.lld: not found` | `lld` 未安装或不在 `PATH` | 通过包管理器安装 `lld`，或修改 `.cargo/config.toml` 使用其他链接器 |
| QEMU 启动后黑屏且无输出 | 未正确加载长模式或串口 | 检查 `boot/long_mode.S` 是否改动；确认脚本使用的 ISO 与最新构建一致 |
| 终端无法显示串口日志 | Shell 未支持 ANSI 或 QEMU 未开启 `-serial stdio` | 使用仓库自带脚本或在手动运行 QEMU 时追加 `-serial stdio` 参数 |

## 架构实现

### 已完成的生产功能

- **内存管理**：完整的虚拟内存系统，包括分页机制、用户/内核空间隔离
- **中断与异常**：IDT 完整配置、异常处理器、设备中断管理
- **进程管理**：Ring 0/3 特权级分离、用户态进程执行、进程状态跟踪
- **系统调用**：生产级系统调用接口，支持标准 POSIX 操作
- **文件系统**：Initramfs 支持、运行时内存文件系统
- **IPC 机制**：消息传递通道，支持进程间通信
- **安全模型**：多用户认证系统、基于角色的访问控制

### 开发规范

本项目严格遵守以下标准和约定：

1. **POSIX 标准**：所有系统调用和 API 必须符合 POSIX.1-2017 规范
2. **Unix-like 语义**：保持与传统 Unix 系统的语义兼容性
3. **混合内核设计**：
   - 核心子系统（内存、调度）运行在内核空间以保证性能
   - 系统服务（认证、日志）运行在用户空间以提供隔离
   - 设备驱动根据安全和性能需求灵活部署
4. **代码质量**：生产级代码标准，完整的错误处理、安全检查和文档

欢迎通过 Issue 或 Pull Request 贡献符合生产标准的代码改进！
