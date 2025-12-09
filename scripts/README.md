# NexaOS TypeScript Build System

用 TypeScript 重写的 NexaOS 构建系统，提供更好的类型安全、可维护性和扩展性。

## 特性

- 🚀 **类型安全** - 完整的 TypeScript 类型定义
- 📦 **模块化** - 每个构建步骤独立模块
- 🎨 **美观输出** - 彩色日志、进度条、spinner
- ⚡ **并行构建** - 支持并行执行独立任务
- 📋 **YAML 配置** - 模块化配置文件在 `config/` 目录
- 🔧 **灵活** - 支持单独构建任何组件
- 📝 **构建日志** - 自动记录所有构建输出到 `logs/` 目录，保留 ANSI 颜色代码

## 快速开始

### 安装依赖

```bash
cd scripts-ts
npm install
```

### 开发模式

使用 `tsx` 直接运行 TypeScript：

```bash
npm run dev -- full        # 完整构建
npm run dev -- quick       # 快速构建
npm run dev -- kernel      # 仅构建内核
```

或使用 wrapper 脚本：

```bash
./scripts/build-ts.sh full
./scripts/build-ts.sh kernel
```

### 编译生产版本

```bash
npm run build              # 编译 TypeScript
node dist/cli.js full      # 运行编译后的版本
```

## 命令

| 命令 | 别名 | 描述 |
|------|------|------|
| `full` | `all` | 完整系统构建 |
| `quick` | `q` | 快速构建（kernel + initramfs + ISO） |
| `kernel` | `k` | 仅构建内核 |
| `userspace` | `u` | 构建用户空间程序 |
| `libs` | `l` | 构建库 |
| `modules` | `m` | 构建内核模块 |
| `programs` | `p` | 构建用户程序 |
| `initramfs` | `i` | 构建 initramfs |
| `rootfs` | `r` | 构建根文件系统 |
| `iso` | - | 构建 ISO 镜像 |
| `clean` | - | 清理构建产物 |
| `list` | - | 列出可用目标 |
| `info` | - | 显示构建环境信息 |
| `features` | `f` | 管理内核编译时特性 |

### 选项

```bash
# 构建特定程序
npm run dev -- programs --name sh

# 构建特定库
npm run dev -- libs --name nssl

# 构建特定模块
npm run dev -- modules --name ext2

# 列出所有可用程序
npm run dev -- programs --list

# 列出所有目标
npm run dev -- list

# 仅清理 build/ 目录
npm run dev -- clean --build-only

# 运行多个步骤
npm run dev -- run kernel initramfs iso
```

## 内核特性管理 (Features)

使用 `features` 命令管理内核编译时特性（定义在 `config/features.yaml`）：

```bash
# 列出所有特性
./scripts/build.sh features list

# 只显示网络相关特性
./scripts/build.sh features list -c network

# 只显示已启用的特性
./scripts/build.sh features list -e

# 显示详细信息
./scripts/build.sh features list -v

# 查看单个特性详情
./scripts/build.sh features show tcp

# 启用特性
./scripts/build.sh features enable verbose_logging

# 禁用特性
./scripts/build.sh features disable tcp

# 切换特性状态
./scripts/build.sh features toggle ttf

# 列出所有预设
./scripts/build.sh features presets -v

# 应用预设配置
./scripts/build.sh features apply minimal_network
./scripts/build.sh features apply embedded

# 输出当前 RUSTFLAGS
./scripts/build.sh features rustflags
```

### 可用预设

| 预设 | 描述 |
|------|------|
| `full_network` | 完整网络栈 |
| `minimal_network` | 最小网络（仅 UDP） |
| `no_network` | 禁用所有网络 |
| `full_graphics` | 完整图形支持 |
| `minimal_graphics` | 基础帧缓冲 |
| `headless` | 无头服务器 |
| `full_hardware` | 完整硬件支持 (SMP, NUMA) |
| `single_cpu` | 单 CPU 模式 |
| `development` | 开发调试构建 |
| `production` | 生产构建 |
| `embedded` | 嵌入式最小配置 |

### 特性类别

- **network**: IPv4, UDP, TCP, ARP, DNS, DHCP, Netlink
- **kernel**: SMP, NUMA, 内核模块, 模块签名
- **filesystem**: initramfs, devfs, procfs, sysfs
- **security**: 栈保护, ASLR
- **graphics**: TTF 字体, 合成器, 帧缓冲
- **debug**: 详细日志, 内存调试, 网络调试

### 通过环境变量覆盖

构建时可以使用环境变量临时覆盖特性设置：

```bash
FEATURE_TCP=false ./scripts/build.sh kernel
FEATURE_TTF=false FEATURE_COMPOSITOR=false ./scripts/build.sh kernel
```

## 环境变量

| 变量 | 默认值 | 描述 |
|------|--------|------|
| `BUILD_TYPE` | `debug` | 构建类型 (debug/release) |
| `LOG_LEVEL` | `debug` | 内核日志级别 |
| `ROOTFS_SIZE_MB` | `50` | 根文件系统大小 (MB) |

## 项目结构

```
scripts/
├── src/
│   ├── cli.ts           # 命令行接口
│   ├── builder.ts       # 主构建器
│   ├── types.ts         # 类型定义
│   ├── config.ts        # YAML 配置解析
│   ├── features.ts      # 特性管理
│   ├── env.ts           # 构建环境
│   ├── logger.ts        # 日志输出
│   ├── exec.ts          # 命令执行
│   └── steps/           # 构建步骤
│       ├── kernel.ts    # 内核构建
│       ├── nrlib.ts     # nrlib 构建
│       ├── libs.ts      # 库构建
│       ├── programs.ts  # 程序构建
│       ├── modules.ts   # 模块构建
│       ├── rootfs.ts    # rootfs 构建
│       ├── initramfs.ts # initramfs 构建
│       ├── iso.ts       # ISO 构建
│       ├── uefi.ts      # UEFI loader 构建
│       └── clean.ts     # 清理
├── package.json
├── tsconfig.json
└── README.md
```

## 与 Shell 脚本的对比

| 特性 | Shell 脚本 | TypeScript |
|------|------------|------------|
| 类型检查 | ❌ | ✅ |
| IDE 支持 | 基础 | 完整 |
| 错误处理 | 基础 | 结构化 |
| 配置解析 | 手动正则 | YAML 库 |
| 并行构建 | 困难 | 简单 |
| 测试 | 困难 | 简单 |
| 可维护性 | 中等 | 高 |

## 扩展

### 添加新的构建步骤

1. 在 `src/steps/` 创建新文件
2. 导出构建函数
3. 在 `src/steps/index.ts` 添加导出
4. 在 `src/builder.ts` 添加方法
5. 在 `src/cli.ts` 添加命令

### 添加新的程序/模块/库

在 `config/` 目录的对应配置文件中添加配置即可，构建系统会自动识别：

- `config/programs.yaml` - 用户空间程序
- `config/modules.yaml` - 内核模块
- `config/libraries.yaml` - 共享库
- `config/build.yaml` - 构建配置文件和全局设置

### 使用构建配置文件

通过 `BUILD_PROFILE` 环境变量选择配置文件：

```bash
BUILD_PROFILE=minimal ./scripts/build.sh all  # 最小构建
BUILD_PROFILE=full ./scripts/build.sh all     # 完整构建
BUILD_PROFILE=dev ./scripts/build.sh all      # 开发构建
```

## 依赖

- Node.js 20+
- npm 或 yarn
- Rust 工具链
- 标准 Linux 构建工具 (gcc, make, etc.)

## 构建日志

所有构建输出都会自动保存到 `logs/` 目录，每个组件都有独立的日志文件：

- 保留完整的 ANSI 转义字符（颜色、格式等）
- 构建失败时自动显示相关日志
- 方便调试和问题追踪

详细信息请参阅 [构建日志文档](../docs/BUILD-LOGS.md)。

### 查看日志

```bash
# 查看内核构建日志
cat logs/kernel.log

# 查看所有模块日志
ls logs/module-*.log

# 使用 less 查看（保留颜色）
less -R logs/kernel.log
```
