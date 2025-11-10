# 内核日志系统实现文档

## 概述

本文档描述了 NexaOS 内核的日志系统实现，该系统支持在不同启动阶段使用不同的日志输出策略。

## 功能特性

### 启动前阶段（Init 未启动）
- **输出目标**：显示器、串口、环形缓冲区
- **用途**：在系统启动阶段提供完整的可见性，便于调试和问题诊断
- 所有日志级别的消息都会被输出到控制台

### 启动后阶段（Init 已启动）
- **输出目标**：环形缓冲区（除了 PANIC）
- **用途**：减少系统运行时的控制台输出，提高性能
- 只有 PANIC 级别的消息会仍然输出到显示器和串口
- 其他日志被保存到环形缓冲区，供日志查询使用

## 实现细节

### 1. 环形缓冲区（Ring Buffer）

**位置**：`src/logger.rs`

环形缓冲区用于存储内核日志，大小为 64KB（RINGBUF_SIZE = 65536）。

```rust
/// 内核日志环形缓冲区
struct RingBuffer {
    buf: [u8; RINGBUF_SIZE],
    write_pos: usize,
}
```

**特点**：
- 固定大小的循环缓冲区
- 写入位置自动环绕
- 线程安全（使用 `Mutex` 保护）

### 2. Init 启动状态标志

**变量名**：`INIT_STARTED` (AtomicBool)

```rust
static INIT_STARTED: AtomicBool = AtomicBool::new(false);
```

用原子类型来追踪 init 进程是否已启动，避免锁开销。

### 3. 日志输出逻辑

**函数**：`logger::log()`（修改后）

根据 init 启动状态，日志被路由到不同的输出目标：

```rust
pub fn log(level: LogLevel, args: fmt::Arguments<'_>) {
    // ...
    let init_started = INIT_STARTED.load(Ordering::Relaxed);
    
    // Init 启动前：正常输出到显示器/串口
    // Init 启动后：只有 PANIC 输出到显示器/串口
    let emit_serial = if init_started {
        level.priority() <= LogLevel::PANIC.priority()
    } else {
        should_emit_serial(level)
    };
    
    let emit_vga = if init_started {
        level.priority() <= LogLevel::PANIC.priority()
    } else {
        should_emit_vga(level)
    };
    
    // 总是向环形缓冲区写入
    if let Some(buffer) = plain_line.as_ref() {
        let mut ringbuf = RINGBUF.lock();
        ringbuf.write_bytes(buffer.as_bytes());
    }
}
```

### 4. API 接口

#### 标记 Init 已启动
```rust
pub fn mark_init_started()
```
- **位置**：`src/logger.rs`
- **用途**：切换日志输出模式
- **调用时机**：在启动 init 进程调度器之前

#### 读取日志环形缓冲区
```rust
pub fn read_ringbuffer() -> [u8; RINGBUF_SIZE]
```
- **位置**：`src/logger.rs`
- **返回**：完整的环形缓冲区内容
- **用途**：用于日志查询和分析

#### 获取缓冲区写入位置
```rust
pub fn ringbuffer_write_pos() -> usize
```
- **位置**：`src/logger.rs`
- **返回**：当前的写入位置
- **用途**：确定有效日志数据的范围

## 使用示例

### 在内核中启用日志切换

**位置**：`src/lib.rs` 中的 `kernel_main()` 函数

```rust
if let Some(pid) = init_pid {
    // ... 初始化 init 进程 ...
    
    // 标记 init 已启动 - 此后内核日志将只输出到环形缓冲区
    logger::mark_init_started();
    
    // 启动调度器
    kinfo!("Starting process scheduler");
    scheduler::do_schedule();
}
```

**执行流程**：
1. 找到 init 进程并将其加入调度器
2. 设置 init 进程为当前进程
3. 调用 `logger::mark_init_started()` 切换日志模式
4. 启动调度器，转入用户空间

## 日志级别

系统支持以下日志级别（按优先级从高到低）：

| 级别 | 优先级 | 启动前输出 | 启动后输出 |
|------|--------|---------|---------|
| PANIC | 0 | ✓ 显示器+串口 | ✓ 显示器+串口 |
| FATAL | 1 | ✓ 显示器+串口 | ✗ 只环形缓冲区 |
| ERROR | 2 | ✓ 显示器+串口 | ✗ 只环形缓冲区 |
| WARN | 3 | ✓ 显示器+串口 | ✗ 只环形缓冲区 |
| INFO | 4 | ✓ 显示器+串口 | ✗ 只环形缓冲区 |
| DEBUG | 5 | ✓ 显示器+串口 | ✗ 只环形缓冲区 |
| TRACE | 6 | ✓ 显示器+串口 | ✗ 只环形缓冲区 |

## 性能考虑

### 优势
1. **启动阶段**：完整的调试信息可见
2. **运行阶段**：减少显示器/串口的 I/O 开销
3. **低开销**：环形缓冲区写入操作极轻量级
4. **非阻塞**：日志操作不会长时间持有锁

### 注意事项
- 环形缓冲区有固定的 64KB 大小，长时间运行后会覆盖旧日志
- PANIC 消息始终输出到控制台，不受模式切换影响
- 使用原子操作避免启动状态检查的性能开销

## 集成点

### 内核代码修改
1. **src/logger.rs**
   - 添加 `RingBuffer` 结构体
   - 添加 `RINGBUF` 静态变量
   - 添加 `INIT_STARTED` 原子标志
   - 修改 `log()` 函数的输出逻辑
   - 添加 `mark_init_started()` 公共函数
   - 添加 `read_ringbuffer()` 公共函数

2. **src/lib.rs**
   - 在启动 init 之前调用 `logger::mark_init_started()`

## 特殊考虑

### PANIC 处理
PANIC 消息总是输出到显示器和串口，即使 init 已启动。这确保了致命错误总是可见的。

### 锁策略
- 使用 `spin::Mutex` 保护环形缓冲区
- 日志输出时短期持有锁（仅写入操作）
- 启动状态检查使用原子操作（无锁）

### 向后兼容性
现有的日志宏（`kinfo!`, `kwarn!` 等）无需修改，完全透明地支持新的日志路由逻辑。

## 未来改进

1. **环形缓冲区访问**
   - 实现用户空间 syscall 来读取日志
   - 添加日志查询和过滤功能

2. **日志级别动态控制**
   - 在运行时调整 init 启动后的日志级别
   - 临时启用更详细的调试日志

3. **持久化存储**
   - 将关键日志保存到磁盘
   - 系统崩溃时的日志恢复

4. **性能优化**
   - 无锁环形缓冲区实现
   - 异步日志写入
