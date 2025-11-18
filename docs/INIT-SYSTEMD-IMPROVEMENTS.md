# Init 系统 Systemd 风格改进

## 概述

根据 systemd 的设计模式，改进了 NexaOS 的 `/sbin/init` 系统，增强了服务管理能力、配置灵活性和诊断能力。

## 核心改进

### 1. 服务类型支持 (Service Type)

新增了对多种服务类型的支持：

```rust
enum ServiceType {
    Simple,     // 默认：直接启动主进程
    Oneshot,    // 服务进程终止后继续执行下一个单元
    Forking,    // 服务进程分叉，父进程退出
    Dbus,       // 服务获取 D-Bus 名称
    Notify,     // 服务发送就绪通知
}
```

配置文件支持：
```ini
[Service]
Type=oneshot
Type=forking
Type=simple
```

### 2. 完整的单元配置选项

#### 新增配置字段

| 字段 | 说明 | 默认值 | 示例 |
|------|------|--------|------|
| `Type` | 服务类型 | `simple` | `oneshot`, `forking` |
| `ExecStop` | 停止命令 | 空 | `/usr/bin/stop-service` |
| `User` | 运行用户 | 空 | `nobody` |
| `Group` | 运行组 | 空 | `nogroup` |
| `WorkingDirectory` | 工作目录 | 空 | `/var/lib/service` |
| `StandardOutput` | 输出目标 | `journal` | `journal`, `file`, `inherit` |
| `TimeoutStartSec` | 启动超时 | 90秒 | `30` |
| `TimeoutStopSec` | 停止超时 | 90秒 | `10` |
| `Before` | 启动顺序前置 | 空 | `multi-user.target` |
| `RequiredBy` | 被依赖的目标 | 空 | `multi-user.target` |

#### 已支持的配置字段（原有）

- `Description` - 单元描述
- `ExecStart` - 启动命令
- `Restart` - 重启策略（no, on-failure, always）
- `RestartSec` - 重启延迟
- `RestartLimitBurst` - 重启限制次数
- `RestartLimitIntervalSec` - 重启限制时间窗口
- `After` - 启动顺序依赖
- `WantedBy` - 所属目标

### 3. 改进的服务状态机

新增了完整的生命周期管理：

```rust
enum ServiceState {
    Inactive,      // 未运行
    Activating,    // 启动中
    Active,        // 运行中
    Deactivating,  // 停止中
    Failed,        // 失败
}
```

#### UnitState 结构

新的 `UnitState` 结构提供了详细的服务状态跟踪：

```rust
struct UnitState {
    state: ServiceState,           // 当前状态
    respawn_count: u32,            // 重启计数
    window_start: Option<Instant>, // 时间窗口开始
    total_starts: u64,             // 总启动次数
    pid: i64,                      // 进程 PID
    start_time: Option<Instant>,   // 启动时间
}
```

新增方法：
- `transition_to()` - 状态转换
- `is_running()` - 检查运行状态
- `set_active()` - 标记为活跃
- `set_inactive()` - 标记为非活跃
- `set_failed()` - 标记为失败
- `uptime()` - 获取运行时间

### 4. 增强的日志和诊断

#### 新增日志函数

```rust
fn log_state_change(unit: &str, old_state: &str, new_state: &str)
fn log_detail(key: &str, value: &str)
```

#### 改进的日志输出

系统现在使用 systemd 风格的彩色输出：

```
✓ [  OK  ] Unit started: getty (PID: 2)
⋄ [ .... ] Starting unit: getty
✗ [FAILED] Failed to start unit: getty
⚠ [ WARN ] Unit terminated: getty
⚡ [STATE ] getty state: inactive -> active
```

### 5. ServiceConfig 结构扩展

从原来的 5 个字段扩展到 18 个字段，支持更丰富的配置：

```rust
struct ServiceConfig {
    name: &'static str,
    description: &'static str,
    exec_start: &'static str,
    exec_stop: &'static str,
    service_type: ServiceType,
    restart: RestartPolicy,
    restart_settings: RestartSettings,
    restart_delay_ms: u64,
    timeout_start_sec: u64,
    timeout_stop_sec: u64,
    after: &'static str,
    before: &'static str,
    wants: &'static str,
    requires: &'static str,
    user: &'static str,
    group: &'static str,
    working_dir: &'static str,
    standard_output: &'static str,
}
```

### 6. 配置文件解析增强

改进的配置文件解析器现在支持：

- `[Service]` 和 `[Init]` 两种部分
- 不区分大小写的键名匹配
- 自动类型转换和默认值处理
- 错误恢复和日志记录
- 安全的内存管理

示例配置文件 (`/etc/ni/ni.conf`):

```ini
[Init]
DefaultTarget=multi-user.target
FallbackTarget=rescue.target

[Service "getty"]
Description=Virtual Terminal Service
Type=simple
ExecStart=/sbin/getty
Restart=always
RestartSec=5
TimeoutStartSec=30
After=systemd-setup.service
WantedBy=multi-user.target

[Service "uefi-compatd"]
Description=UEFI Compatibility Daemon
Type=simple
ExecStart=/sbin/uefi-compatd
Restart=on-failure
RestartSec=10
After=multi-user.target
StandardOutput=journal
```

## 实现细节

### 内存布局

配置存储在固定大小的缓冲区中以避免动态分配：

```rust
const CONFIG_BUFFER_SIZE: usize = 4096;
const MAX_SERVICES: usize = 12;
const MAX_FIELD_LEN: usize = 256;
const SERVICE_FIELD_COUNT: usize = 13;

static CONFIG_BUFFER: ConfigBuffer = ConfigBuffer::new();
static mut SERVICE_CONFIGS: [ServiceConfig; MAX_SERVICES] = [...];
static mut STRING_STORAGE: [[u8; MAX_FIELD_LEN]; MAX_SERVICES * SERVICE_FIELD_COUNT] = [...];
```

### 状态转换图

```
      Inactive
        |  |
        |  |-- (fork & exec) --> Activating --> Active
        |                          |
        |                          +-- (startup timeout) --> Failed
        |
        +-- (process exits) --> Deactivating --> Inactive
                                                    |
                                                    +-- (respawn check) --> Activating
```

## API 更新

### 配置解析

```rust
fn load_service_catalog() -> ServiceCatalog
fn parse_unit_file(len: usize) -> usize
fn handle_service_key_value(line: &[u8], current: &mut ServiceConfig)
```

### 服务管理

```rust
fn start_service(service: &ServiceConfig, buf: &mut [u8]) -> i64
fn parallel_service_supervisor(
    running_services: &mut [Option<RunningService>; MAX_SERVICES],
    service_count: usize,
    buf: &mut [u8],
) -> !
```

### 日志和诊断

```rust
fn log_info(msg: &str)
fn log_start(msg: &str)
fn log_fail(msg: &str)
fn log_warn(msg: &str)
fn log_detail(key: &str, value: &str)
fn log_state_change(unit: &str, old_state: &str, new_state: &str)
```

## 兼容性

### 向后兼容

所有原有的配置选项仍然完全支持，现有的配置文件无需修改。

### Systemd 兼容性

支持的 systemd 概念：
- ✅ 服务类型（Type）
- ✅ 重启策略（Restart, RestartSec）
- ✅ 启动顺序（After, Before）
- ✅ 超时管理（TimeoutStartSec, TimeoutStopSec）
- ✅ 单位依赖（Wants, Requires）
- ✅ 用户/组指定（User, Group）
- ✅ 工作目录（WorkingDirectory）
- ✅ 标准输出配置（StandardOutput）

不支持的 systemd 特性（可在后续版本实现）：
- ❌ Socket 激活
- ❌ Timer 单位
- ❌ Mount 单位
- ❌ Device 单位
- ❌ Target 单位

## 测试验证

系统已成功在 QEMU 中引导并通过以下验证：

1. ✅ 配置文件解析成功
2. ✅ 服务启动成功
3. ✅ 进程管理和重启工作正常
4. ✅ 改进的日志输出清晰可读
5. ✅ 状态转换正确执行
6. ✅ 系统登录和 shell 交互正常

## 性能影响

- 内存占用：轻微增加（配置缓冲区 ~4KB）
- CPU 使用：无显著变化（解析仅在启动时执行）
- 启动时间：保持不变

## 未来改进方向

1. **依赖解析** - 实现 After/Before 的完整拓扑排序
2. **条件化启动** - 支持 `ConditionPathExists` 等条件
3. **环境变量** - 支持 `EnvironmentFile` 和 `Environment`
4. **资源限制** - 支持 `LimitNOFILE`, `LimitCPU` 等
5. **守护进程** - 更好地支持 Type=forking
6. **健康检查** - Type=notify 和心跳机制
7. **日志持久化** - 完整的日志系统集成

## 文件变更

主要修改文件：
- `userspace/init.rs` - 核心 init 实现

新增结构体和枚举：
- `ServiceType` - 服务类型枚举
- `UnitState` - 单位状态结构
- `ServiceState` - 服务状态枚举
- 扩展后的 `ServiceConfig`

## 技术笔记

### 内存安全

- 所有字符串存储在固定大小的缓冲区中
- 使用 `unsafe` 块仅在必要时，并带有清晰的注释
- 边界检查和长度验证在所有指针操作中进行

### 并发安全

虽然当前实现是单线程的，但：
- `UnitState` 是 `Copy` 类型，适合在多线程场景中使用
- 状态转换是原子操作
- 未来可以扩展为多线程监督器

### 配置验证

- 所有必需的字段都有默认值
- 无效的配置值会被替换为安全的默认值
- 解析错误被记录但不会导致系统崩溃

## 致谢

这些改进基于 systemd 项目的设计原则和最佳实践，适应了 NexaOS 的特定约束条件。
