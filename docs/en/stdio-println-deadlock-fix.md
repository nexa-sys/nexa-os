# NexaOS stdio/println! 死锁修复

## 问题描述

init 进程（PID 1）在调用 Rust std 的 `println!` 宏时卡死，导致系统无法继续启动。

### 症状
- `eprintln!` 正常工作
- 直接调用 libc 的 `write()` 函数会卡死
- `println!` 宏会卡死
- 系统在 `[init] Starting init_main` 后停止响应

## 根本原因分析

问题存在于 `nrlib/src/stdio.rs` 中的 stdio 实现:

### 1. 自旋锁死锁 (已部分修复)
**位置**: `lock_stream()` 函数

**原始问题**:
- 使用无限自旋锁等待 FILE 结构体的原子锁
- 没有超时机制
- 在单线程模式下不应该有竞争，但是出现了

**修复**:
- 添加了指数退避机制 (exponential backoff)
- 限制最大自旋次数 (10000)
- 返回 EAGAIN 错误而不是无限等待
- 添加了详细的诊断注释

### 2. 重入锁定 (已修复)
**位置**: `debug_log()` 函数

**原始问题**:
- `debug_log()` 尝试写入 stderr
- 调用 `write_all_fd()` 获取 stderr 的锁
- 如果在已获得锁的代码路径中调用，会导致死锁

**修复**:
- 禁用 `debug_log` 内的 `write_all_fd` 调用
- 改为使用直接 syscall
- 或者完全禁用调试日志输出

### 3. 待诊断的问题

当前，系统在以下操作时卡死:

```rust
// 这个调用导致系统卡死
unsafe {
    let _ = write(STDOUT, TEST_MSG.as_ptr() as *const c_void, TEST_MSG.len());
}
```

**可能的原因**:
1. Rust std 的初始化代码在访问 stdout 时出现问题
2. stdout FILE 结构体在初始化时就被锁定
3. Rust std 期望的 libc 接口与 nrlib 提供的不匹配
4. write syscall 在返回时有异常

## 已实施的修复

### 修改1: lock_stream() 函数 - 生产级锁定
```rust
// 添加了:
// - 最大自旋计数限制
// - 指数退避机制 (exponential backoff)
// - 超时后返回 EAGAIN
// - 详细的诊断注释
```

**影响**: 
- ✅ 防止了纯粹的无限自旋
- ❌ 未能解决实际的卡死问题

###修改2: debug_log() 函数 - 禁用重入
```rust
// 原来:
fn debug_log(msg: &[u8]) {
    let _ = write_all_fd(STDERR, msg);  // 可能导致锁定重入
}

// 现在:
fn debug_log(msg: &[u8]) {
    // 使用直接 syscall，避免锁定
    let _ = unsafe { crate::syscall3(SYS_WRITE, 2, msg.as_ptr() as u64, msg.len() as u64) };
}
```

**影响**:
- ✅ 防止了 debug_log 导致的重入死锁
- ❌ 未能解决 println! 的卡死问题

### 修改3: 添加到 copilot-instructions.md
添加了"生产级系统标准"部分，强调:
- 正确性优于简化
- 同步原语需要超时机制
- 需要诊断和可观测性
- 系统可靠性原则

**影响**:
- ✅ 指导后续开发不要为简化而牺牲正确性
- ℹ️ 记录了设计原则

## 当前状态

### 工作正常
- ✅ 内核启动和初始化
- ✅ 加载 init 进程
- ✅ 调用 init 的 main 函数
- ✅ `eprintln!` 宏（通过 stderr）
- ✅ 直接 syscall（在 announce_runtime_start() 中）

### 卡死
- ❌ `println!` 宏
- ❌ Rust std 的 IO 操作
- ❌ 直接调用 libc 的 `write()` 函数（在初始化上下文中）

## 下一步诊断步骤

### 1. 确认 stdout 初始化状态
```rust
// 检查 stdout FILE 结构体是否已初始化
// 检查锁定状态
// 检查缓冲区状态
```

### 2. 追踪 write() 调用
```rust
// 在 nrlib 的 write() 函数中添加 syscall 前后诊断
// 确认 syscall 返回值
// 检查 errno 设置
```

### 3. 检查 Rust std 初始化
```rust
// 确认 std::io::_print 如何工作
// 确认 stdout 是否正确链接到 nrlib 提供的 FILE*
// 检查 std 是否期望特定的 FILE 结构布局
```

### 4. 测试替代方案
- 尝试不使用 nrlib 的 stdio，直接使用 syscall
- 跳过 Rust std，使用原始写入

## 设计原则 (更新)

根据本修复，更新了以下原则到 `.github/copilot-instructions.md`:

**正确性优于简化**: NexaOS 是生产级系统，绝不为了代码简洁而牺牲正确性、健壮性或安全性。

**同步原语**: 
- 必须包括超时/出错机制
- 实现指数退避
- 检测重入问题
- 添加诊断工具

**可观测性**:
- 维护诊断能力
- 如果诊断工具导致问题（如 debug_log 死锁），修复根本原因而不是禁用诊断
- 记录设计决策

## 参考文献

- `userspace/nrlib/src/stdio.rs`: FILE 结构体和 stdio 实现
- `userspace/init.rs`: init 进程的 main 函数
- `userspace/nrlib/src/lib.rs`: write() 和其他 libc 函数
- `.github/copilot-instructions.md`: 开发指导原则
