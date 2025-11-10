# Fork() RIP 修复总结

## 问题描述

系统启动时，init (PID 1) 调用 fork() 创建 getty (PID 2)，然后调用 wait4() 等待。但系统进入紧急模式并显示"Scheduler returned to kernel_main!"的错误。

## 根本原因

### 第一层：wait4() 导致调度器崩溃（已识别但不是最关键）
- wait4() 将父进程设置为 Sleeping 状态，导致调度器没有 Ready 进程可运行

### 第二层：fork() 的 RIP 处理不正确（**CRITICAL - 现已修复**）
- 子进程的 RIP 没有正确设置为 fork() 的返回地址
- 子进程从父进程的当前 RIP 继续执行，而不是从 fork() 返回点开始
- 这导致子进程无法正确执行其代码路径

## 修复方案

### 修复 1: 获取 syscall 返回地址（中断处理程序）

**文件**: `src/interrupts.rs` (int 0x81 处理程序)

**问题**: 用户空间程序使用 `int 0x81` 而不是 `syscall` 指令。当 CPU 处理中断时，它自动推送返回地址到中断堆栈：
```
[RSP + 0]: RIP (返回地址)
[RSP + 8]: CS
[RSP + 16]: RFLAGS
[RSP + 24]: RSP (用户 RSP)
[RSP + 32]: SS (用户 SS)
```

**解决方案**: 在推送其他寄存器之前，先将 RIP 保存到 R10：

```asm
syscall_interrupt_handler:
    mov r10, [rsp + 0]      # r10 = user RIP (返回地址)
    push rcx
    push rdx
    # ... 推送其他寄存器 ...
    
    # 准备参数
    mov r8, r10             # r8 = syscall_return_addr (第5参数)
    # ... 其他参数 ...
    call syscall_dispatch
```

### 修复 2: 修改 syscall_dispatch 签名

**文件**: `src/syscall.rs`

**修改**:
- 旧: `pub extern "C" fn syscall_dispatch(nr: u64, arg1: u64, arg2: u64, arg3: u64) -> u64`
- 新: `pub extern "C" fn syscall_dispatch(nr: u64, arg1: u64, arg2: u64, arg3: u64, syscall_return_addr: u64) -> u64`

### 修复 3: 修改 fork() 签名和实现

**文件**: `src/syscall.rs`

**修改**:
```rust
fn syscall_fork(syscall_return_addr: u64) -> u64 {
    // ...
    // 设置子进程的 RIP 为 fork() 返回地址，而不是复制父进程的 RIP
    child_process.context.rip = syscall_return_addr;
    // ...
}
```

这确保了子进程在被调度时，会从 fork() 调用之后的下一条指令开始执行。

### 修复 4: 调整汇编参数传递

**文件**: `src/interrupts.rs` 和 `src/syscall.rs`

确保返回地址正确地通过 x86_64 System V ABI 传递：
- 参数 5 通过 R8 传递（System V x86_64 ABI）

## 测试结果

启动日志显示：

```
[DEBUG] syscall_fork: syscall_return_addr = 0x4087ec    # ✓ 正确的用户地址
[INFO ] fork() called from PID 1
[DEBUG] Child RIP set to 0x4087ec, Child RAX = 0        # ✓ 子进程 RIP 正确
[INFO ] fork() created child PID 2 from parent PID 1
[ni] start_service: parent continuing, child PID=2
[INFO ] wait4() from PID 1 waiting for pid -1
[INFO ] wait4() yielding CPU at check 100
[ni] start_service: child process (PID 0 from fork), exiting to avoid loop  # ✓ 子进程执行了正确代码路径！
[INFO ] Process 2 exiting with code: 0                  # ✓ 子进程成功退出
```

## 关键改进

| 指标 | 修复前 | 修复后 |
|------|--------|--------|
| 子进程返回地址 | 0x4 (错误) | 0x4087ec (正确) |
| 子进程代码路径 | 错误执行 | 正确执行子分支 |
| 子进程状态 | 持续轮询，永不退出 | 正确退出 (exit code 0) |
| fork() 语义 | 破损 | **FIXED** ✓ |

## 已知限制和后续工作

1. **内存隔离** (未实现)
   - fork() 当前仍共享父进程的内存
   - 需要为每个进程分配独立的页表和物理内存
   - 参考: `src/process.rs` 中已添加的 `cr3` 字段

2. **调度器优化** (部分完成)
   - wait4() 当前使用忙轮询，效率低下
   - 真正的生产实现需要进程阻塞和事件唤醒机制

3. **getty 启动失败** (下一阶段)
   - 系统仍然无法启动 getty 进程
   - 子进程在 exec() 之前立即退出
   - 可能需要 exec() 系统调用修复

## 架构改进

### x86_64 中断处理链

```
用户程序 (Ring 3)
    ↓ int 0x81
    ↓
syscall_interrupt_handler (汇编)
    ↓ 保存 RIP 到 R10
    ↓ 推送寄存器
    ↓ 对齐堆栈
    ↓ 准备参数 (R8 = RIP)
    ↓
syscall_dispatch (Rust)
    ↓ 根据 SYS_FORK 分发
    ↓
syscall_fork (Rust)
    ↓ 使用返回地址初始化子进程 RIP
    ↓
子进程加入调度器
    ↓ 调度时从正确的 RIP 开始执行
```

## 代码审计检查清单

- [x] int 0x81 处理程序正确保存返回地址到 R10
- [x] R10 正确传递为第5参数（R8）到 syscall_dispatch
- [x] fork() 接收并使用返回地址初始化子进程 RIP
- [x] 子进程的 RAX 仍设置为 0（fork() 返回值）
- [x] 调度器能正确切换到子进程
- [x] 子进程从返回地址继续执行

## 感谢和致谢

这个修复需要深入理解：
1. x86_64 中断处理机制
2. System V x86_64 ABI 参数传递约定
3. 进程上下文管理和 context switch 语义
4. 用户空间/内核空间边界

修复过程中发现的关键洞察：
- 中断返回地址位置不同于 syscall (RCX) 返回地址
- 需要在推送寄存器之前保存返回地址
- 堆栈布局计算必须精确
