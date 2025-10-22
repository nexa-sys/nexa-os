# NexaOS 开发入门指南

> 更新时间：2025-10-23

NexaOS 是一个采用 Rust 编写的实验性操作系统，目标是提供混合内核架构、POSIX 风格接口以及与 Linux 用户态的有限兼容。本文档为中文开发者提供快速上手指导，涵盖必备依赖、构建流程、调试方式与常见问题。

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

## 后续开发方向

- **内存管理**：基于当前的内存映射日志构建物理帧分配器与内核堆。
- **中断与异常**：实现 IDT、APIC 初始化以及基础的异常处理流程。
- **任务调度**：设计最小可用的线程/进程抽象与时间片调度。
- **用户态实验**：探索 Linux 兼容层或提供自定义用户态运行时。

欢迎通过 Issue 或 Pull Request 贡献设计思路与代码改进！
