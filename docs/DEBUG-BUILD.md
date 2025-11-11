# NexaOS Debug Build Script - 使用指南

## 概述

`build-rootfs-debug.sh` 是一个用于构建带调试符号的根文件系统的脚本。它基于 `build-rootfs.sh`，但保留了完整的调试信息，便于使用 GDB 进行调试。

## 关键特性

| 特性 | Release版本 | Debug版本 |
|------|-----------|---------|
| 脚本 | `build-rootfs.sh` | `build-rootfs-debug.sh` |
| 输出目录 | `build/rootfs/` | `build/rootfs-debug/` |
| 镜像文件 | `build/rootfs.ext2` | `build/rootfs-debug.ext2` |
| 镜像大小 | 50MB | 100MB |
| 优化级别 | `-O2` | `-O2` |
| 符号处理 | ✅ **剥离** | ❌ **保留** |
| 调试信息 | ❌ 无 | ✅ .debug_info |
| GDB支持 | ❌ 无符号 | ✅ 完整符号 |

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
file build/rootfs-debug/sbin/ni
# 输出: with debug_info, not stripped

# 检查调试段
readelf -S build/rootfs-debug/sbin/ni | grep -E "(\.debug|\.symtab)"

# 使用 GDB 查看符号
gdb -batch -ex "file build/rootfs-debug/sbin/ni" -ex "info functions"
```

## 使用方法

### 构建 Debug 版本根文件系统

```bash
./scripts/build-rootfs-debug.sh
```

### 用 GDB 调试 Debug 版本的二进制

```bash
gdb ./build/rootfs-debug/sbin/ni

# GDB 命令示例
(gdb) file ./build/rootfs-debug/sbin/ni
(gdb) info functions
(gdb) list
(gdb) break main
(gdb) run
```

## 技术细节

### Cargo 配置

Debug 版本的 Cargo.toml 使用以下配置：

```toml
[profile.release]
panic = "abort"
opt-level = 2       # 保持优化，避免编译时间过长
debug = true        # 启用调试符号 ← 关键！
lto = false         # 禁用链接时优化
```

### 构建差异

- **Release 版本**：使用 `--release` 构建，然后 `strip --strip-all` 移除符号
- **Debug 版本**：使用 `--release` 构建，**不进行** `strip` 操作，保留所有符号

这种方法提供了：
- ✅ 性能优化的二进制（`-O2` 优化）
- ✅ 完整的调试信息（可用 GDB 调试）
- ✅ 避免 debug 构建的符号冲突问题
- ✅ 合理的编译时间

## 文件结构

```
build/
├── rootfs/              # Release 版本 rootfs
├── rootfs.ext2          # Release 版本镜像 (50MB)
├── rootfs-debug/        # Debug 版本 rootfs
├── rootfs-debug.ext2    # Debug 版本镜像 (100MB，含符号）
├── userspace-build/     # Release 版本构建目录
└── userspace-build-debug/  # Debug 版本构建目录
```

## 性能特点

| 指标 | Release | Debug |
|------|---------|-------|
| 二进制大小 | 小（已strip） | 大（含符号） |
| 运行时性能 | 相同 | 相同 |
| 编译时间 | 标准 | 标准 |
| 调试能力 | ❌ | ✅ |

## 常见问题

**Q: 为什么 Debug 版本的大小比较大？**  
A: 因为包含了完整的调试符号信息（.debug_* 段）。这些信息只用于调试，不影响运行时性能。

**Q: Debug 版本的运行时性能会变慢吗？**  
A: 不会。Debug 版本使用相同的 `-O2` 优化级别编译，性能与 Release 版本相同。调试符号是额外的元数据，不影响可执行代码。

**Q: 可以同时存在 Release 和 Debug 版本吗？**  
A: 可以。两个版本有独立的目录和镜像文件，互不干扰。

**Q: 如何在 QEMU 中测试 Debug 版本？**  
A: 修改启动脚本或 grub.cfg，使用 `rootfs-debug.ext2` 而不是 `rootfs.ext2`。

