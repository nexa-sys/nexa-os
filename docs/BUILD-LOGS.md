# 构建日志系统

## 概述

NexaOS 构建系统现在会自动将所有构建日志保存到 `logs/` 目录中。每个模块、程序和库都有独立的日志文件，失败的构建会在构建结束时自动显示详细的错误信息。

## 功能特性

### 1. 自动日志记录
- 所有 `cargo build` 命令的输出都会被记录
- 日志文件包含完整的编译输出，**保留 ANSI 转义字符**（颜色、格式等）
- 每个组件都有独立的日志文件

### 2. 日志文件组织

```
logs/
├── kernel.log                    # 内核构建日志
├── module-e1000.log              # e1000 网卡驱动模块
├── module-ext2.log               # ext2 文件系统模块
├── module-virtio_blk.log         # virtio 块设备模块
├── program-init.log              # init 程序
├── program-shell.log             # shell 程序
├── library-ncryptolib-static.log # ncryptolib 静态库
├── library-ncryptolib-shared.log # ncryptolib 共享库
├── nrlib-static.log              # nrlib 静态库
├── nrlib-shared.log              # nrlib 共享库
├── ld-nrlib.log                  # 动态链接器
├── initramfs-*.log               # initramfs 组件
└── ...
```

### 3. 失败日志自动显示

当构建失败时，系统会：
1. 记录失败的组件和日志路径
2. 在构建结束时自动打印失败日志（最后 100 行）
3. 保留 ANSI 颜色代码，方便阅读错误信息
4. 显示完整日志文件路径供进一步查看

示例输出：
```
========================================
Build Failures
========================================

[ERROR] 2 build(s) failed

================================================================================
Failed: program-myapp
Log file: /home/user/nexa-os-1/logs/program-myapp.log
================================================================================
error[E0425]: cannot find value `foo` in this scope
  --> src/main.rs:10:5
   |
10 |     foo();
   |     ^^^ not found in this scope

error: aborting due to previous error

================================================================================
Build logs saved in: logs/
================================================================================
```

## 使用方式

### 查看构建日志

构建后，所有日志都保存在 `logs/` 目录：

```bash
# 查看内核构建日志
less logs/kernel.log

# 使用 cat -A 查看包含 ANSI 代码的原始日志
cat logs/kernel.log

# 查看特定模块的构建日志
tail -100 logs/module-e1000.log

# 搜索错误信息
grep -r "error" logs/
```

### 保留颜色输出

日志文件保留了原始的 ANSI 转义字符，可以使用支持 ANSI 的工具查看：

```bash
# 使用 less 查看（保留颜色）
less -R logs/kernel.log

# 使用 cat 直接输出（终端会解释颜色）
cat logs/kernel.log

# 使用 bat（如果安装）
bat logs/kernel.log
```

### 清理日志

日志文件会在每次构建时重新生成。如需手动清理：

```bash
rm -rf logs/
```

## 技术细节

### 日志文件格式

每个日志文件包含：
1. **头部信息**：组件名称和时间戳
2. **命令行**：完整的 `cargo build` 命令
3. **环境变量**：相关的 RUSTFLAGS 等
4. **构建输出**：完整的 stdout 和 stderr

示例：
```
=== Build Log: kernel ===
Timestamp: 2025-12-09T16:21:42.310Z
================================================================================

$ cargo build --target /path/to/target.json --features net_ipv4,net_udp,...
RUSTFLAGS=-C opt-level=2 ...

warning: unused import: `core::ptr`
  --> src/drivers/block/mod.rs:25:5
   |
25 | use core::ptr;
   |     ^^^^^^^^^
...
```

### ANSI 转义字符

日志文件保留原始的 ANSI 转义序列，包括：
- 颜色代码（红色错误、黄色警告等）
- 格式化（粗体、斜体等）
- 光标控制序列

这使得日志在终端中查看时能够保持原始的视觉效果。

### 失败检测

构建系统通过以下方式检测失败：
1. 监控 `cargo build` 的退出码
2. 记录非零退出码的构建
3. 在所有构建步骤完成后统一显示失败日志

## 配置

日志系统会在构建初始化时自动创建 `logs/` 目录。该目录已添加到 `.gitignore`，不会提交到版本控制系统。

## 故障排查

### 日志文件未生成

如果日志文件未生成，检查：
1. 是否有权限创建 `logs/` 目录
2. 构建系统是否正确初始化（查看 `[INFO] Build logs directory:` 输出）

### 日志文件过大

默认情况下，失败时只显示最后 100 行。如需查看完整日志：

```bash
less logs/<component>.log
```

### ANSI 字符显示异常

某些文本编辑器可能不能正确显示 ANSI 转义字符。使用以下工具查看：
- `less -R`：保留颜色
- `cat`：在支持 ANSI 的终端中直接显示
- `bat`：语法高亮和 ANSI 支持
- VS Code：使用 ANSI Colors 扩展

## 相关文件

- `scripts/src/exec.ts` - 日志管理核心逻辑
- `scripts/src/builder.ts` - 构建器集成和失败日志显示
- `scripts/src/steps/*.ts` - 各个构建步骤的日志命名
