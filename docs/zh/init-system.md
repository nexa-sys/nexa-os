# NexaOS Init System 设计文档

## 概述

NexaOS 的 init 系统遵循传统 Unix System V 和现代 Linux 的设计理念，同时适配混合内核架构的特点。

## 设计目标

1. **POSIX 兼容性**: 遵循 POSIX 标准的进程管理和系统初始化规范
2. **Unix-like 行为**: 实现传统 Unix 的 init (PID 1) 概念
3. **混合内核适配**: 在混合内核架构下实现高效的进程管理
4. **服务管理**: 支持服务的启动、停止、重启和监控
5. **运行级别**: 实现 System V 风格的运行级别管理

## 架构设计

### 核心组件

#### 1. Init 进程 (PID 1)

Init 进程是系统启动后的第一个用户态进程，具有以下特点：

- **PID 固定为 1**: 符合 Unix 标准
- **PPID 为 0**: 表示没有父进程
- **永不退出**: Init 进程退出会导致系统 panic
- **孤儿进程的养父**: 所有孤儿进程会被 init 接管

#### 2. 运行级别系统

实现 System V 风格的 7 个运行级别：

```
0 - Halt (系统关机)
1 - Single User (单用户模式，维护模式)
2 - Multi-User (多用户模式，无网络)
3 - Multi-User + Network (多用户模式，带网络)
4 - Unused (保留)
5 - Multi-User + GUI (多用户图形界面)
6 - Reboot (重启)
```

#### 3. 服务管理

每个服务包含以下属性：

- **名称**: 服务标识符
- **路径**: 可执行文件路径
- **运行级别**: 该服务应该在哪些运行级别运行
- **重生标志**: 服务退出后是否自动重启
- **优先级**: 启动顺序（数字越小越早启动）

### 初始化流程

```
Bootloader (GRUB)
    ↓
Kernel Entry (boot/long_mode.S)
    ↓
kernel_main() (src/lib.rs)
    ↓
├─ 硬件初始化
├─ 内存管理初始化
├─ 中断系统初始化
├─ 文件系统初始化
├─ 认证系统初始化
├─ IPC 系统初始化
├─ 信号系统初始化
├─ 调度器初始化
└─ Init 系统初始化
    ↓
加载 /etc/inittab 配置
    ↓
启动 init 进程 (PID 1)
    ↓
根据运行级别启动服务
    ↓
系统运行
```

## 关键特性

### 1. 进程管理 (POSIX)

#### 进程创建
- 使用 `fork()` 创建子进程
- 新进程继承父进程的 PPID
- 子进程获得新的 PID

#### 进程终止
- `exit()` 系统调用终止进程
- 通知 init 系统处理进程退出
- 自动重启标记为 respawn 的服务

#### 孤儿进程处理
- 父进程退出时，子进程被 init (PID 1) 接管
- Init 负责回收僵尸进程

### 2. 服务重生 (Respawn)

实现防止 fork 炸弹的重生限制：

```rust
const MAX_RESPAWN_COUNT: u32 = 5;      // 最多重生 5 次
const RESPAWN_WINDOW_MS: u64 = 60000;  // 1 分钟内
```

如果服务在 1 分钟内重生超过 5 次，init 将放弃重启该服务。

### 3. 运行级别切换

运行级别切换流程：

1. 停止不在新运行级别中的服务
2. 更新当前运行级别
3. 按优先级启动新运行级别的服务
4. 特殊处理 halt (0) 和 reboot (6)

### 4. 系统调用接口

新增的系统调用：

```rust
// 169 - reboot (Linux 兼容)
SYS_REBOOT: u64 = 169

// 230 - shutdown (关机)
SYS_SHUTDOWN: u64 = 230

// 231 - runlevel (获取/设置运行级别)
SYS_RUNLEVEL: u64 = 231
```

#### reboot() 系统调用

```c
// 命令参数 (Linux 兼容)
#define LINUX_REBOOT_CMD_RESTART    0x01234567  // 重启
#define LINUX_REBOOT_CMD_HALT       0x4321FEDC  // 停机
#define LINUX_REBOOT_CMD_POWER_OFF  0xCDEF0123  // 关机

int reboot(int cmd);
```

需要超级用户权限 (UID 0 或 CAP_SYS_BOOT)。

#### shutdown() 系统调用

```c
int shutdown(void);
```

关闭系统，需要超级用户权限。

#### runlevel() 系统调用

```c
// 获取当前运行级别
int runlevel = syscall(SYS_RUNLEVEL, -1);

// 设置运行级别 (需要 root)
syscall(SYS_RUNLEVEL, 3);  // 切换到运行级别 3
```

### 5. inittab 配置文件

位置: `/etc/inittab`

格式: `id:runlevels:action:process`

示例:
```bash
# 系统初始化
si::sysinit:/etc/init.d/rcS

# 默认运行级别
id:3:initdefault:

# Getty (登录终端)
1:2345:respawn:/sbin/getty 38400 tty1
2:2345:respawn:/sbin/getty 38400 tty2

# Ctrl-Alt-Del 处理
ca::ctrlaltdel:/sbin/shutdown -r now
```

支持的 action 类型:
- `sysinit`: 系统初始化时运行
- `wait`: 等待进程完成
- `respawn`: 进程退出后自动重启
- `initdefault`: 默认运行级别
- `ctrlaltdel`: Ctrl-Alt-Del 按键处理

## 混合内核特性

### 1. 内核态 Init 管理

与微内核不同，NexaOS 的 init 系统部分功能在内核态实现：

- **服务表管理**: 内核维护服务列表
- **进程监控**: 内核直接监控进程状态
- **快速响应**: 进程退出时立即处理，无需上下文切换

### 2. 用户态 Init 进程

Init 进程本身运行在用户态 (Ring 3)：

- **标准进程**: Init 是一个普通的用户态进程
- **特殊权限**: 拥有最高优先级和特殊系统调用权限
- **策略实现**: 实现具体的启动和管理策略

### 3. 特权检查

所有 init 相关系统调用都需要权限检查：

```rust
if !crate::auth::is_superuser() {
    return Err(EPERM);  // Operation not permitted
}
```

检查条件:
- UID == 0 (root 用户)
- 或具有 is_admin 标志

## 与 Linux 的兼容性

### 相似之处

1. **PID 1 概念**: Init 进程固定为 PID 1
2. **运行级别**: 7 个运行级别兼容 System V
3. **inittab 格式**: 配置文件格式类似
4. **系统调用**: reboot() 系统调用号和参数兼容 Linux

### 差异之处

1. **简化实现**: 不支持 systemd 的复杂依赖管理
2. **单核设计**: 当前版本仅支持单 CPU 核心
3. **有限服务**: 服务表大小固定 (16 个服务)
4. **同步启动**: 服务按顺序启动，不支持并行

## 安全考虑

### 1. 权限隔离

- 只有 root 用户可以切换运行级别
- 只有 root 用户可以重启/关机
- Init 进程不能被普通用户杀死

### 2. Fork 炸弹防护

- 限制服务重生次数
- 时间窗口限制
- 达到限制后自动放弃重启

### 3. 信号处理

Init 进程特殊的信号处理：

- `SIGTERM`: 忽略 (防止意外终止)
- `SIGKILL`: 不可捕获，但内核拒绝杀死 PID 1
- `SIGCHLD`: 处理子进程退出

## 未来扩展

### 短期目标

1. 实现完整的 fork/exec 支持
2. 添加进程间通信的 init 支持
3. 实现 cgroup 风格的资源限制
4. 添加服务依赖管理

### 长期目标

1. 支持 systemd 风格的 unit 文件
2. 并行服务启动
3. 动态服务注册/注销
4. 支持容器化服务

## 调试和监控

### 日志输出

Init 系统使用内核日志系统：

```rust
kinfo!("Init process started with PID {}", pid);
kdebug!("Starting service: {}", path);
kerror!("Service '{}' respawn limit reached", name);
```

### 状态查询

通过系统调用查询：

```c
// 获取当前运行级别
int level = syscall(SYS_RUNLEVEL, -1);

// 获取当前进程的父进程
pid_t ppid = getppid();
```

## 代码结构

```
src/
├── init.rs          # Init 系统核心实现
├── process.rs       # 进程管理 (PPID 支持)
├── scheduler.rs     # 进程调度器
├── syscall.rs       # 系统调用 (reboot/shutdown/runlevel)
└── auth.rs          # 权限检查 (is_superuser)

etc/
└── inittab          # Init 配置文件

docs/zh/
└── init-system.md   # 本文档
```

## 总结

NexaOS 的 init 系统实现了：

✅ POSIX 兼容的进程管理  
✅ Unix-like 的 init 概念  
✅ 混合内核优化的实现  
✅ System V 风格的运行级别  
✅ 服务管理和监控  
✅ 安全的权限检查  
✅ Fork 炸弹防护  

这是一个完整的、符合规范的 init 系统实现，适合混合内核架构的操作系统。
