# NexaOS Debug Build Script - 使用指南

## 概述

NexaOS 构建系统默认使用 debug 模式构建内核，以确保稳定性。本文档说明如何控制构建类型以及如何进行调试。

## 构建类型

### 环境变量控制

```bash
# Debug 构建（默认，推荐用于开发）
./scripts/build.sh all

# 显式指定 debug
BUILD_TYPE=debug ./scripts/build.sh all

# Release 构建（更小、更快，但可能有 fork/exec 问题）
BUILD_TYPE=release ./scripts/build.sh all
```

## 关键特性

| 特性 | Debug 版本 | Release 版本 |
|------|-----------|---------|
| 构建命令 | `./scripts/build.sh all` | `BUILD_TYPE=release ./scripts/build.sh all` |
| 内核输出 | `target/x86_64-nexaos/debug/nexa-os` | `target/x86_64-nexaos/release/nexa-os` |
| 内核大小 | ~18 MB | ~6 MB |
| 优化级别 | 默认 (O0-O1) | O2 (Cargo.toml 中配置) |
| 符号处理 | ❌ **保留** | ✅ **保留** |
| Fork/Exec | ✅ 稳定 | ⚠️ 可能有问题 |
| GDB支持 | ✅ 完整符号 | ✅ 有符号 |

> **重要**: Release 构建曾经使用 O3 优化，导致 fork/exec 子进程崩溃。现在已修改为 O2，但仍建议使用 debug 构建进行开发。

## 调试符号验证

Debug 版本的二进制文件包含以下调试部分：

```
.debug_gdb_scripts    # GDB 脚本
.debug_loc           # 调试位置信息
.debug_abbrev        # 调试缩写
.debug_info          # 调试信息
.debug_aranges       # 调试地址范围
```

可以用以下命令验证：

```bash
# 查看文件信息
file target/x86_64-nexaos/debug/nexa-os
# 输出: with debug_info, not stripped

# 检查调试段
readelf -S target/x86_64-nexaos/debug/nexa-os | grep -E "(\.debug|\.symtab)"

# 使用 GDB 查看符号
gdb -batch -ex "file target/x86_64-nexaos/debug/nexa-os" -ex "info functions"
```

## 使用方法

### 构建 Debug 版本

```bash
# 完整构建（默认 debug）
./scripts/build.sh all

# 仅构建内核
./scripts/build.sh kernel
```

### 用 GDB 调试

```bash
gdb ./target/x86_64-nexaos/debug/nexa-os

# GDB 命令示例
(gdb) file ./target/x86_64-nexaos/debug/nexa-os
(gdb) info functions
(gdb) list
(gdb) break kernel_main
```

## 技术细节

### Cargo 配置

Release 版本的 Cargo.toml 使用以下配置：

```toml
[profile.release]
panic = "abort"
opt-level = 2       # 使用 O2 而非 O3，避免 fork/exec 问题
debug = true        # 启用调试符号
lto = false         # 禁用链接时优化
```

### 构建系统架构

构建系统使用模块化设计：

```
scripts/
├── build.sh              # 主入口
├── lib/
│   └── common.sh         # 共享变量和函数
└── steps/
    ├── build-kernel.sh           # 内核构建
    ├── build-nrlib.sh            # 运行时库
    ├── build-userspace-programs.sh # 用户程序
    ├── build-modules.sh          # 内核模块
    ├── build-initramfs.sh        # initramfs
    ├── build-rootfs.sh           # 根文件系统
    └── build-iso.sh              # ISO 镜像
```

## 文件结构

```
build/
├── rootfs/              # rootfs 内容
├── rootfs.ext2          # rootfs 镜像 (50MB)
├── initramfs/           # initramfs 内容
├── initramfs.cpio       # initramfs 归档
└── userspace-build/     # 用户空间构建目录

target/
├── x86_64-nexaos/
│   ├── debug/
│   │   └── nexa-os      # Debug 内核（默认）
│   └── release/
│       └── nexa-os      # Release 内核
└── iso/
    └── nexaos.iso       # 可启动 ISO
## 性能特点

| 指标 | Debug | Release |
|------|-------|---------|
| 内核大小 | ~18 MB | ~6 MB |
| 运行时性能 | 较慢 | 较快 |
| Fork/Exec 稳定性 | ✅ 稳定 | ⚠️ 可能有问题 |
| 调试能力 | ✅ 完整 | ✅ 有限 |

## 常见问题

**Q: 为什么默认使用 Debug 构建？**  
A: 因为 Release 构建（O3 优化）会导致 fork/exec 子进程崩溃。Debug 构建更稳定，适合开发。

**Q: Debug 版本的内核大小比较大？**  
A: 是的，约 18MB vs 6MB。这是因为包含了完整的调试符号和未优化的代码。

**Q: 如何切换到 Release 构建？**  
A: 使用 `BUILD_TYPE=release ./scripts/build.sh all`，但请注意可能的稳定性问题。

**Q: 日志级别如何控制？**  
A: 使用 `LOG_LEVEL` 环境变量：`LOG_LEVEL=info ./scripts/build.sh kernel`

**Q: 在 QEMU 中如何查看内核日志？**  
A: 日志输出到串口，可以用 `tail -f /tmp/qemu-serial.log` 查看。

## 相关文档

- [BUILD-SYSTEM.md](BUILD-SYSTEM.md) - 完整构建系统指南
- [QUICK-REFERENCE.md](QUICK-REFERENCE.md) - 快速参考卡
- [../../scripts/README.md](../../scripts/README.md) - 构建脚本参考

