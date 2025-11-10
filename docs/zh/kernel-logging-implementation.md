# 内核日志系统实现 - 环形缓冲区与启动阶段日志管理

## 需求分析

你需要实现一个内核日志系统，其中：

1. **启动前（Init 未拉起）**：内核日志应被打印到：
   - 显示器（VGA）
   - 串口（Serial）
   - 环形缓冲区（Ring Buffer）- 用于保存历史日志

2. **启动后（Init 已拉起）**：内核日志应该：
   - 只输出到环形缓冲区
   - **例外**：PANIC 仍然会输出到显示器和串口

## 实现方案

### 核心设计

```
启动流程：
  内核初始化 → 所有日志输出到控制台+环形缓冲区 
     ↓
  logger::mark_init_started() 被调用
     ↓
  此后的内核日志只输出到环形缓冲区（PANIC 除外）
     ↓
  用户空间运行，内核日志保存在缓冲区中
```

### 1. 环形缓冲区结构

**文件**：`src/logger.rs`

```rust
const RINGBUF_SIZE: usize = 65536;  // 64KB 缓冲区

struct RingBuffer {
    buf: [u8; RINGBUF_SIZE],
    write_pos: usize,  // 循环写入位置
}

impl RingBuffer {
    fn write_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.buf[self.write_pos] = byte;
            self.write_pos = (self.write_pos + 1) % RINGBUF_SIZE;
        }
    }
}

static RINGBUF: Mutex<RingBuffer> = Mutex::new(RingBuffer::new());
```

**特点**：
- 固定大小 64KB
- 新日志覆盖旧日志（FIFO）
- 通过 Mutex 保护，线程安全
- 自动环绕，无需手动管理

### 2. Init 启动状态追踪

```rust
static INIT_STARTED: AtomicBool = AtomicBool::new(false);
```

使用原子类型而非 Mutex，因为：
- 频繁的状态检查
- 只需要简单的布尔值
- 无锁操作，性能高

### 3. 日志路由逻辑

修改 `logger::log()` 函数：

```rust
pub fn log(level: LogLevel, args: fmt::Arguments<'_>) {
    let init_started = INIT_STARTED.load(Ordering::Relaxed);
    
    // 启动前：正常输出；启动后：只有 PANIC 输出到控制台
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
    
    // ... 根据 emit_serial 和 emit_vga 输出到控制台 ...
    
    // 总是向环形缓冲区写入
    if let Some(buffer) = plain_line.as_ref() {
        let mut ringbuf = RINGBUF.lock();
        ringbuf.write_bytes(buffer.as_bytes());
    }
}
```

### 4. 启动触发点

**文件**：`src/lib.rs` 中的 `kernel_main()` 函数

```rust
if let Some(pid) = init_pid {
    // 初始化 init 进程...
    scheduler::set_current_pid(Some(pid));
    let _ = scheduler::set_process_state(pid, process::ProcessState::Ready);
    
    // ★ 关键点：标记 init 已启动
    logger::mark_init_started();
    
    // 启动调度器（不再返回）
    scheduler::do_schedule();
}
```

**调用时机**：
- 在调用 `scheduler::do_schedule()` 之前
- 此时 init 进程已添加到调度器，但尚未执行
- 转换点之后的任何内核代码都只会输出到环形缓冲区

## 关键特性

### 日志级别表

| 级别 | 启动前 | 启动后 | PANIC |
|------|--------|--------|-------|
| PANIC | 控制台+缓冲 | 控制台+缓冲 | ✓ |
| ERROR/WARN/INFO/DEBUG | 控制台+缓冲 | 缓冲 | ✗ |

### 线程安全性

- **原子操作**：Init 启动状态的检查（无锁，高性能）
- **Mutex**：环形缓冲区的写入（短期锁，低争用）
- **线程安全通过**：原子操作+Mutex 的正确组合

### 向后兼容性

现有的日志宏（`kinfo!`, `kwarn!`, `kpanic!` 等）无需修改，完全透明地适应新的日志输出策略。

## API 接口

### 标记 Init 启动
```rust
pub fn mark_init_started()
```
- 调用一次即可，立即切换日志输出模式

### 读取日志缓冲
```rust
pub fn read_ringbuffer() -> [u8; RINGBUF_SIZE]
```
- 返回完整的 64KB 缓冲区
- 用于日志查询、系统诊断等

```rust
pub fn ringbuffer_write_pos() -> usize
```
- 返回当前的写入位置
- 用于确定有效日志数据的范围

## 执行流程示例

### 系统启动日志序列

```
[启动阶段]
00:00.000 [INFO] NexaOS Kernel Bootstrap    ← 输出到：显示器、串口、环形缓冲区
00:00.001 [INFO] Stage 1: Bootloader        ← 同上
00:00.002 [DEBUG] Multiboot magic: 0x...    ← 同上
...
00:05.000 [INFO] Init process loaded (PID 1)
00:05.001 [INFO] Starting scheduler

[logger::mark_init_started() 被调用]

[用户空间运行]
00:05.100 [INFO] User process started       ← 仅输出到环形缓冲区
00:05.200 [DEBUG] Process created           ← 仅输出到环形缓冲区
...
[如果发生 PANIC]
00:10.000 [PANIC] CPU exception             ← 输出到：显示器、串口、环形缓冲区
```

## 代码修改总结

### 修改文件

1. **src/logger.rs**
   - 添加 `RINGBUF_SIZE` 常量
   - 添加 `RingBuffer` 结构体
   - 添加 `RINGBUF` 全局变量
   - 添加 `INIT_STARTED` 原子标志
   - 修改 `log()` 函数逻辑
   - 添加三个新的公开 API

2. **src/lib.rs**
   - 在启动调度器前调用 `logger::mark_init_started()`

### 改动范围

- 核心日志系统：极小的改动
- 现有代码兼容性：100%（无需改动其他代码）
- 新增功能：环形缓冲区日志保存

## 性能影响

### 积极影响
- **减少 I/O**：Init 启动后不再输出到串口（减少延迟）
- **显示器加载减轻**：减少 VGA 缓冲区的写入

### 中性影响
- **环形缓冲区开销**：每条日志增加 ~10-20 字节写入，影响可忽略
- **锁开销**：Mutex 使用极少（仅在日志输出时），争用极低

### 性能优化空间
- 可以在将来实现无锁环形缓冲区
- 可以在将来实现异步日志写入

## 验证方法

### 启动日志验证
1. 编译内核：`cargo build --release`
2. 构建系统：`./scripts/build-all.sh`
3. 运行 QEMU：`./scripts/run-qemu.sh`
4. 观察启动日志：应该看到所有日志都在控制台显示

### 运行时日志验证
1. Init 启动后，内核日志应该停止出现在显示器/串口
2. 可以通过 syscall 接口查询环形缓冲区的内容
3. 如果发生 PANIC，应该再次看到日志输出

## 注意事项

1. **初始化顺序**：`logger::init()` 必须在 `mark_init_started()` 之前被调用
2. **单次调用**：`mark_init_started()` 应该在整个系统生命周期中只调用一次
3. **PANIC 安全**：PANIC 处理器必须独立于启动状态，始终输出到控制台
4. **缓冲区大小**：64KB 的缓冲区可以存储约 1000+ 行日志（平均 60 字节/行）
