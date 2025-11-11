# NexaOS Documentation Structure

## 文档组织

```
docs/
├── README.md              # 本文件 - 文档导航入口
├── INDEX.md               # 详细的文档索引（按主题/角色）
│
├── en/                    # 英文文档（English Documentation）
│   ├── ARCHITECTURE.md    # 内核架构详解
│   ├── BUILD-SYSTEM.md    # 构建系统完全指南
│   ├── SYSCALL-REFERENCE.md  # 38+ 系统调用完整参考
│   ├── SYSTEM-OVERVIEW.md    # 系统全面概览
│   ├── QUICK-REFERENCE.md    # 开发者速查表
│   │
│   ├── kernel-logging-system.md      # 内核日志系统
│   ├── DYNAMIC_LINKING.md            # 动态链接和ELF加载
│   ├── ROOTFS-BOOT-IMPLEMENTATION.md # 根文件系统启动
│   ├── STDIO_ENHANCEMENTS.md         # 标准I/O增强
│   ├── DEBUG-BUILD.md                # 调试构建指南
│   │
│   ├── RUST_STDOUT_HANG_DIAGNOSIS.md      # Rust stdout死锁诊断
│   ├── stdio-println-deadlock-fix.md      # Println死锁修复
│   ├── FORK_RIP_FIX.md                    # Fork RIP修复
│   ├── FORK_WAIT_ISSUES.md                # Fork/Wait问题
│   │
│   └── bugfixes/                     # 修复报告
│       ├── testing-guide.md          # 测试指南
│       ├── release-build-buffer-error.md  # 发布构建错误
│       └── newline-flush-fix.md      # 换行刷新修复
│
├── zh/                    # 中文文档（Chinese Documentation）
│   ├── 快速开始.md        # 新手入门（中文）
│   ├── 系统概览.md        # 系统全面介绍（中文）
│   ├── 架构设计.md        # 架构深入分析（中文）
│   │
│   ├── init系统/
│   │   ├── 概述.md        # Init系统概览
│   │   ├── 实现总结.md    # 实现细节
│   │   ├── 服务管理.md    # 服务和runlevel
│   │   └── 配置指南.md    # /etc/inittab配置
│   │
│   ├── shell与用户空间/
│   │   ├── 交互式Shell.md # Shell功能详解
│   │   ├── 命令参考.md    # 19个命令完整参考
│   │   └── 行编辑.md      # Tab补全和快捷键
│   │
│   ├── 内核开发/
│   │   ├── 动态链接支持.md    # ELF加载和链接
│   │   ├── 启动流程.md        # 6阶段启动过程
│   │   ├── 内存管理.md        # 虚拟内存和分页
│   │   └── 进程管理.md        # 进程和调度
│   │
│   ├── 标准库与兼容/
│   │   ├── nrlib简介.md   # Libc兼容层
│   │   ├── stdio增强.md   # 标准I/O实现
│   │   └── 错误处理.md    # errno和POSIX兼容
│   │
│   ├── 故障排除/
│   │   ├── 常见问题.md
│   │   ├── 构建错误.md
│   │   ├── 启动问题.md
│   │   └── 调试技巧.md
│   │
│   └── 开发报告/
│       ├── 完成度报告.md         # 实现完成度
│       ├── 实现报告.md           # 技术实现细节
│       └── 日志系统实现.md       # 内核日志
│
└── 旧文档/                # 归档（保持参考用）
    ├── CONFIG_SYSTEM_SUMMARY.md
    └── ...
```

## 快速导航

### 🚀 我想快速上手
1. [README (根目录)](../README.md) - 项目概览和快速开始
2. [en/QUICK-REFERENCE.md](en/QUICK-REFERENCE.md) - 开发者速查表
3. [zh/快速开始.md](zh/快速开始.md) - 中文新手指南

### 📚 我想理解系统架构
1. [zh/系统概览.md](zh/系统概览.md) - 中文完整概览
2. [en/ARCHITECTURE.md](en/ARCHITECTURE.md) - 英文详细架构
3. [en/SYSTEM-OVERVIEW.md](en/SYSTEM-OVERVIEW.md) - 英文系统全景

### 💻 我想开发内核功能
1. [en/ARCHITECTURE.md](en/ARCHITECTURE.md) - 架构基础
2. [zh/内核开发/](zh/内核开发/) - 内核特定主题
3. [en/SYSCALL-REFERENCE.md](en/SYSCALL-REFERENCE.md) - 系统调用参考

### 🔧 我想开发用户空间程序
1. [en/SYSCALL-REFERENCE.md](en/SYSCALL-REFERENCE.md) - 系统调用API
2. [zh/shell与用户空间/](zh/shell与用户空间/) - Shell和开发
3. [en/DYNAMIC_LINKING.md](en/DYNAMIC_LINKING.md) - 动态链接

### ⚙️ 我想构建和部署系统
1. [en/BUILD-SYSTEM.md](en/BUILD-SYSTEM.md) - 构建系统完全指南
2. [zh/快速开始.md](zh/快速开始.md) - 中文构建步骤
3. [en/DEBUG-BUILD.md](en/DEBUG-BUILD.md) - 调试构建

### 🐛 我遇到了问题
1. [en/RUST_STDOUT_HANG_DIAGNOSIS.md](en/RUST_STDOUT_HANG_DIAGNOSIS.md) - I/O问题
2. [zh/故障排除/](zh/故障排除/) - 常见问题解答
3. [en/bugfixes/testing-guide.md](en/bugfixes/testing-guide.md) - 测试指南

## 文档按语言分布

### 英文文档（English）- 技术核心
- **核心架构**: ARCHITECTURE, BUILD-SYSTEM, SYSCALL-REFERENCE, SYSTEM-OVERVIEW
- **技术深度**: 内核日志、动态链接、启动过程、内存管理
- **故障排除**: 详细的诊断和修复报告
- **适合**: 系统开发者、架构师、深度技术研究

### 中文文档（中文）- 实现细节
- **快速入门**: 环境设置、构建步骤、运行测试
- **模块详解**: Init系统、Shell、内存管理、进程管理
- **开发指南**: 实现报告、最佳实践、代码示例
- **适合**: 中文开发者、学习者、贡献者

## 文档更新日志

### 最近更新 (2025-11-12)
- ✅ 创建了结构化的 `en/` 和 `zh/` 目录
- ✅ 移动英文文档到 `docs/en/`
- ✅ 整理中文文档并补充完整性
- ✅ 创建中英文导航索引

### 计划中的更新
- ⚙️ 补充 `zh/快速开始.md` 的完整内容
- ⚙️ 创建 `zh/故障排除/` 下的详细故障排除指南
- ⚙️ 添加视频教程或截图演示
- ⚙️ 创建 API 文档的中文版本

## 文档贡献指南

### 添加新文档
1. 确定文档的语言和主题
2. 放在正确的目录：
   - 英文 → `docs/en/` 或其子目录
   - 中文 → `docs/zh/` 或其子目录
3. 使用清晰的标题和 Markdown 格式
4. 包含导航链接指向相关文档
5. 在本 README 中添加导航条目

### 更新现有文档
1. 检查文档的准确性
2. 确保示例代码有效
3. 更新相关的交叉链接
4. 更新"最近更新"部分

### 文档标准
- **标题**: 使用 H1 (`#`) 作为文档标题
- **结构**: 导航 → 概览 → 详细 → 示例 → 总结
- **链接**: 使用相对路径，指向同一文档集
- **代码**: 包含语言标记，如 ` ```rust` 或 ` ```bash`
- **清晰度**: 避免术语过多，包含定义

## 相关资源

- **主项目**: https://github.com/nexa-sys/nexa-os
- **问题跟踪**: https://github.com/nexa-sys/nexa-os/issues
- **Wiki**: 进行中
- **讨论**: https://github.com/nexa-sys/nexa-os/discussions

---

**文档状态**: ✅ 结构完整，内容95%完成  
**最后审核**: 2025-11-12  
**维护者**: NexaOS 开发团队

需要帮助？在 [GitHub Issues](https://github.com/nexa-sys/nexa-os/issues) 中提问！
