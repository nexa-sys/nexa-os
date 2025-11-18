# Init 系统快速参考

## 文件概览

- **文件** `userspace/init.rs`
- **行数** 1284
- **函数数** 69
- **结构体数** 6
- **枚举数** 4
- **编译目标** `x86_64-nexaos-userspace.json`
- **生成二进制** `/sbin/ni`

## 快速配置指南

### 基本单元文件格式

配置文件位置：`/etc/ni/ni.conf`

```ini
[Init]
# 系统启动时运行的默认目标
DefaultTarget=multi-user.target
# 无单元可用时的备用目标
FallbackTarget=rescue.target

[Service "getty"]
# 单元描述
Description=Virtual Terminal Service
# 服务类型：simple, oneshot, forking, dbus, notify
Type=simple
# 启动命令
ExecStart=/sbin/getty
# 停止命令（可选）
ExecStop=/sbin/getty-stop
# 重启策略：no, on-failure, always
Restart=always
# 重启延迟（秒）
RestartSec=5
# 启动超时（秒）
TimeoutStartSec=30
# 停止超时（秒）
TimeoutStopSec=10
# 启动顺序：在某个单元之后启动
After=systemd-setup.service
# 所属目标
WantedBy=multi-user.target
# 当某个单元启动时启动此单元
Wants=keyboard-setup.service
# 强制依赖
Requires=network.service
# 用户名
User=root
# 用户组
Group=root
# 工作目录
WorkingDirectory=/root
# 标准输出：journal, file, inherit
StandardOutput=journal
```

## 配置字段详解

### 必需字段
- `ExecStart` - 启动命令（必需，其他所有字段都是可选的）

### 常用字段

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `Description` | 字符串 | 空 | 单元描述，用于日志 |
| `Type` | simple\|oneshot\|forking\|dbus\|notify | simple | 服务类型 |
| `Restart` | no\|on-failure\|always | always | 重启策略 |
| `RestartSec` | 数字 | 1000ms | 重启延迟（秒） |
| `RestartLimitBurst` | 数字 | 5 | 时间窗口内最多重启次数 |
| `RestartLimitIntervalSec` | 数字 | 60 | 重启计数时间窗口（秒） |
| `After` | 字符串 | 空 | 启动依赖（在此单元之后） |
| `Before` | 字符串 | 空 | 启动顺序（在此单元之前） |
| `WantedBy` | 字符串 | multi-user.target | 所属目标 |
| `RequiredBy` | 字符串 | 空 | 被依赖的目标 |
| `TimeoutStartSec` | 数字 | 90 | 启动超时（秒） |
| `TimeoutStopSec` | 数字 | 90 | 停止超时（秒） |
| `User` | 字符串 | 空 | 运行用户（当前不实现） |
| `Group` | 字符串 | 空 | 运行组（当前不实现） |
| `WorkingDirectory` | 字符串 | 空 | 工作目录（当前不实现） |
| `StandardOutput` | journal\|file\|inherit | journal | 输出目标（当前不实现） |

## 服务类型说明

### Type=simple（默认）
- init 分叉子进程启动服务
- init 继续管理该进程
- 进程退出时根据 Restart 策略决定是否重启

```ini
[Service "main-app"]
Type=simple
ExecStart=/usr/bin/myapp
Restart=always
```

### Type=oneshot
- 服务启动并运行至完成
- init 等待服务进程退出
- 通常用于一次性初始化任务
- 不支持重启（Restart 被忽略）

```ini
[Service "system-setup"]
Type=oneshot
ExecStart=/usr/bin/setup-system
After=network.service
```

### Type=forking
- 服务在后台分叉
- init 等待主进程退出
- 常用于传统守护进程

```ini
[Service "daemon"]
Type=forking
ExecStart=/usr/sbin/mydaemon
Restart=on-failure
```

### Type=dbus
- 服务通过 D-Bus 激活（未来实现）

### Type=notify
- 服务使用 systemd 通知协议（未来实现）

## 重启策略

### no / never / none / false
- 服务退出后不重启
- 通常用于 oneshot 服务

### on-failure
- 仅在失败时重启（非零退出码或被信号终止）
- 推荐用于关键服务

```ini
Restart=on-failure
RestartSec=10
RestartLimitBurst=3
RestartLimitIntervalSec=60
```

### always / true / yes
- 无论如何退出都重启
- 添加限制防止重启风暴

```ini
Restart=always
RestartSec=5
RestartLimitBurst=5
RestartLimitIntervalSec=60
```

## 启动顺序管理

### 简单依赖
```ini
[Service "app"]
After=network.service database.service
```

### 多个目标
```ini
[Service "logger"]
Before=main-app.service
Wants=syslog.service
```

## 日志和诊断

### 日志输出格式

```
[  OK  ] Unit started: getty (PID: 2)              # 成功
[ .... ] Starting unit: getty                      # 启动中
[FAILED] Failed to start unit: getty               # 失败
[ WARN ] Unit terminated: getty                    # 警告
[STATE ] getty state: inactive -> active           # 状态变化
```

### 调试

启动 init 时查看完整输出：
```bash
# 查看启动日志
dmesg | grep -E "\[ni\]|Unit|getty"

# 监控进程
ps aux | grep getty
```

## 常见配置模式

### Web 服务器
```ini
[Service "nginx"]
Description=Nginx Web Server
Type=simple
ExecStart=/usr/sbin/nginx
ExecStop=/usr/sbin/nginx -s quit
Restart=on-failure
RestartSec=5
After=network.service
WantedBy=multi-user.target
```

### 数据库服务
```ini
[Service "postgresql"]
Description=PostgreSQL Database
Type=simple
ExecStart=/usr/lib/postgresql/bin/postgres
Restart=on-failure
RestartSec=10
TimeoutStartSec=120
TimeoutStopSec=30
WantedBy=multi-user.target
```

### 初始化脚本
```ini
[Service "hostname-setup"]
Description=Set system hostname
Type=oneshot
ExecStart=/usr/bin/setup-hostname
ExecStop=
After=network.service
WantedBy=multi-user.target
```

### 后台守护进程
```ini
[Service "sshd"]
Description=OpenSSH Daemon
Type=forking
ExecStart=/usr/sbin/sshd
Restart=always
RestartSec=10
WantedBy=multi-user.target
```

## 性能调优

### 并行启动
多个不相关的服务会自动并行启动：

```ini
# 这些可以同时启动
[Service "nginx"]
After=network.service

[Service "mysql"]
After=network.service

[Service "redis"]
After=network.service
```

### 启动超时
为长时间启动的服务增加超时：

```ini
[Service "slow-app"]
Type=simple
ExecStart=/usr/bin/slow-app
TimeoutStartSec=300  # 5分钟
```

## 故障排查

### 服务不启动
1. 检查 ExecStart 路径是否存在
2. 检查配置文件语法（使用 `journalctl -xe`）
3. 查看权限问题

### 频繁重启
检查重启限制设置：
```bash
# 增加重启限制
RestartLimitBurst=10
RestartLimitIntervalSec=300
```

### 启动顺序问题
使用 After/Before 明确指定依赖关系：
```ini
[Service "app"]
After=network.service database.service
```

## 实现细节

### 状态机
```
Inactive --> Activating --> Active --> Deactivating --> Inactive
              |
              +--> Failed (重启或停止)
```

### 重启限制算法
- 时间窗口内的重启次数受限
- 超过窗口则计数重置
- 达到限制则单元进入失败状态

### 配置存储
- 配置缓冲区: 4KB
- 最大服务数: 12
- 最大字段长度: 256 字节

## API 使用示例

### 自定义重启策略
```rust
RestartSettings {
    burst: 5,           // 最多5次
    interval_sec: 60,   // 60秒内
}
```

### 服务状态检查
```rust
let is_running = state.is_running();
let uptime = state.uptime();
```

### 手动状态转换
```rust
state.transition_to(ServiceState::Activating);
state.set_active(pid);
```

## 限制和已知问题

1. **User/Group** - 当前配置被忽略，所有服务以 root 运行
2. **WorkingDirectory** - 当前配置被忽略
3. **StandardOutput** - 当前配置被忽略，所有输出到内核日志
4. **并行启动** - After/Before 目前不实现完整的拓扑排序
5. **最大服务数** - 限制为 12 个（可配置）
6. **Type=forking** - 需要更精细的进程追踪

## 未来增强计划

- [ ] 实现 User/Group 支持
- [ ] 完整的 After/Before 拓扑排序
- [ ] Type=forking 的完整支持
- [ ] ExecStop 的正确处理
- [ ] 条件化启动（ConditionPathExists等）
- [ ] 环境变量支持
- [ ] 资源限制（LimitNOFILE等）
