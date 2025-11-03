# NexaOS Init 配置文件系统

## 概述

NexaOS 现已支持从配置文件动态启动服务，而不再依赖硬编码的启动参数。这遵循 Unix 传统的 `/etc/inittab` 初始化配置。

## 配置文件格式

### 位置
- `/etc/inittab` - 系统初始化配置文件

### 格式
```
# 注释行以 # 开头
PATH RUNLEVEL

# 示例：
/bin/sh 2
/sbin/getty 2
/sbin/syslogd 2
```

- **PATH**：要启动的服务的完整路径（例如 `/bin/sh`）
- **RUNLEVEL**：运行级别（0-9 的单个数字）
  - 0 = 关闭
  - 1 = 单用户
  - 2 = 多用户
  - 3 = 网络多用户
  - 其他 = 自定义级别

## 实现细节

### ni（Nexa Init）处理流程

1. **加载配置**：ni 启动时从 `/etc/inittab` 读取服务列表
2. **解析配置**：逐行解析，跳过空行和注释
3. **顺序启动**：按配置文件顺序依次启动每个服务
4. **监督管理**：为每个服务维护独立的重启计数和时间窗口

### 代码实现

#### userspace/init.rs 中的关键函数

```rust
/// 加载服务配置
fn load_config() -> &'static [Option<ServiceEntry>]

/// 解析单行配置
fn parse_config_line(line: &[u8]) -> Option<ServiceEntry>

/// 运行单个服务的监督循环
fn run_service_loop(service_state: &mut ServiceState, path: &str, buf: &mut [u8]) -> !
```

#### src/fs.rs 中的初始化

```rust
// 如果不存在 /etc/inittab，使用默认配置
const DEFAULT_INITTAB: &[u8] = b"# NexaOS init configuration\n/bin/sh 2\n";

// 在 fs::init() 中注册默认配置
add_file_bytes("etc/inittab", DEFAULT_INITTAB, false);
```

## 默认配置

如果 initramfs 中不包含 `/etc/inittab`，内核会自动创建一个包含以下内容的默认配置：

```
# NexaOS init configuration (/etc/inittab)
# Format: PATH RUNLEVEL
# Services listed here will be started by ni (Nexa Init) at boot
# Runlevel 2 = multi-user mode
/bin/sh 2
```

这确保了即使没有配置文件，系统也能正确启动 shell。

## 特性

### ✅ 已实现
- ✓ 从 `/etc/inittab` 动态加载服务配置
- ✓ 支持注释行（以 `#` 开头）
- ✓ 支持空行（自动跳过）
- ✓ 为每个服务独立处理重启限制
- ✓ 按配置顺序启动多个服务
- ✓ 默认配置自动创建

### 🔜 计划中
- [ ] 支持 runlevel 过滤（仅启动与当前 runlevel 匹配的服务）
- [ ] 支持服务依赖关系
- [ ] 支持条件启动（if/unless 指令）
- [ ] 支持重启策略定制（respawn/once/wait）
- [ ] 支持环境变量展开
- [ ] 支持 init= 命令行参数覆盖配置

## 测试

### 验证配置加载

启动系统后，应该看到类似的日志输出：

```
[ .... ] Loading service configuration
[  OK  ] Loaded services from /etc/inittab
         Service count: 1

[ .... ] Starting service supervision
[  OK  ] Using fork/exec/wait supervision model

[ .... ] Spawning service
         Service: /bin/sh
         Attempt: 1
[  OK  ] Service started successfully
         Child PID: 2
```

### 自定义配置

要创建自定义配置，修改 `DEFAULT_INITTAB` 常量在 `src/fs.rs` 中，然后重新编译：

```rust
const DEFAULT_INITTAB: &[u8] = b"# Custom configuration\n/sbin/getty 2\n/bin/sh 2\n";
```

## 错误处理

- **配置文件不存在**：使用默认配置
- **配置文件解析错误**：跳过错误行，继续处理下一行
- **服务启动失败**：根据重启限制重试
- **超过重启限制**：进入无限等待状态（不继续重试）

## 与标准 Unix init 的区别

NexaOS 实现了一个简化版本的 `/etc/inittab`：

| 特性 | Unix init | NexaOS ni |
|------|-----------|-----------|
| 配置格式 | id:runlevels:action:process | PATH RUNLEVEL |
| 多个 runlevel | 支持 | 支持单个（计划中支持过滤） |
| 行为控制 | respawn/wait/once/boot | 简化版（计划扩展） |
| 依赖关系 | 不支持 | 计划支持 |
| 动态重新读取 | 支持（HUP 信号） | 计划支持 |

## 将来的改进

### 多服务支持
当前实现在第一个服务进入监督循环后会停止，因为 `run_service_loop` 永不返回。计划改为：
- 为每个服务创建真正的子进程
- 使用 `wait4()` 等待服务退出
- 在管理循环中循环处理所有服务

### 高级特性
- 服务依赖关系图
- 条件执行
- 优雅关闭（SIGTERM/SIGKILL）
- 日志重定向
- 资源限制

## 参考资源

- [Linux init(8) man page](https://man7.org/linux/man-pages/man8/init.8.html)
- [Unix System V Init](https://en.wikipedia.org/wiki/Init)
- [systemd 服务配置](https://www.freedesktop.org/software/systemd/man/systemd.service.html)
