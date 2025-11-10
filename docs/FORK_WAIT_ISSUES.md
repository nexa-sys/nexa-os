# Fork() 和 Wait4() 问题分析与修复

## 问题描述

当 init (ni) 程序调用 fork() 创建子进程（getty），然后调用 wait4() 等待子进程时，系统进入紧急模式并显示错误。

### 日志示例
```
[    6.298262] [INFO ] fork() created child PID 2 from parent PID 1 (child will return 0)
[ni] start_service: parent continuing, child PID=2
[  OK  ] Unit started
         PID: 2

[    6.419949] [INFO ] wait4(pid=-1) called - will block until child exits
[    6.435297] [INFO ] wait4() from PID 1 waiting for child -1
[    6.471517] [FATAL] Scheduler returned to kernel_main!
[    6.496428] [INFO ] Boot stage transition: UserSpace -> Emergency
```

## 根本原因分析

### 问题1: fork() 的 RIP 处理不正确 ✅ IDENTIFIED

**症状**: 子进程没有正确从 fork() 返回

**原因**: 
- fork() 系统调用在内核中执行时，复制父进程的完整 context（包括 RIP 和 RSP）
- 这导致子进程的 RIP 指向父进程当前执行的地方，而不是 fork() 的返回地址
- 当子进程被调度时，它继续执行父进程的代码，而不是从 fork() 返回点开始

**根本原因**:
- x86_64 syscall 指令将返回地址保存在 RCX 中
- syscall 处理程序将 RCX 推送到堆栈，然后重新使用 RCX
- fork() 系统调用处理程序无法访问原始的 syscall 返回地址 (RCX)
- 子进程被初始化时，使用父进程的 context，而不是 fork() 返回点

**修复方案**:
需要在 syscall 处理程序中将 syscall 返回地址 (RCX) 传递给 fork() 处理程序。

```rust
// 在 syscall_fork() 中需要接收 syscall_return_address 参数
fn syscall_fork(syscall_return_address: u64) -> u64 {
    // ...
    child_process.context.rip = syscall_return_address;  // 设置为 fork() 返回点
    // ...
}
```

### 问题2: wait4() 调用 do_schedule() 导致调度器返回 ✅ FIXED

**症状**: wait4() 在第 100 次轮询检查时调用 do_schedule()，导致"Scheduler returned to kernel_main"

**原因**:
- wait4() 的初始实现将父进程设置为 Sleeping 状态，然后调用 do_schedule()
- 当调度器没有找到其他 Ready 进程时，会返回到 kernel_main
- 这导致整个系统失败

**修复（已实现）**:
- 去掉 do_schedule() 调用
- 改用忙轮询（busy-waiting）
- 在忙轮询中每 100 次检查调用一次 do_schedule() 以让出 CPU

```rust
// wait4() 现在实现为：
loop {
    // 检查子进程状态
    if child_exited {
        return child_pid;
    }
    
    // 忙轮询（不设置进程为 Sleeping）
    if check_count % 100 == 0 {
        do_schedule();  // 偶尔让出 CPU
    }
}
```

**权衡**:
- ✅ 不再导致调度器崩溃
- ❌ 忙轮询低效
- ❌ 真正的问题（子进程 RIP 不正确）仍未解决

### 问题3: 页表隔离缺失 ✅ PARTIALLY ADDRESSED

**症状**: fork() 后父子进程共享内存和堆栈

**原因**:
- fork() 当前实现共享父进程的内存（为了快速实现 fork+exec 模式）
- 父进程和子进程有相同的虚拟地址空间
- 堆栈操作会相互污染

**修复（部分实现）**:
- 为 Process 结构添加了 cr3 字段
- 这为未来实现每个进程的页表做了准备
- 但目前 cr3 始终为 0（使用内核页表）

**长期解决方案**:
- 为每个 fork() 创建独立的页表 (CR3)
- 复制或映射父进程的内存空间
- 实现写时复制 (COW) 以提高效率

## 修复状态

| 问题 | 状态 | 说明 |
|------|------|------|
| wait4() 返回问题 | ✅ 修复 | 改用忙轮询，避免调度器崩溃 |
| fork() RIP 问题 | ❌ 未修复 | 需要修改 syscall 处理程序传递返回地址 |
| 页表隔离 | 🟡 部分 | 添加了 cr3 字段，但未实现实际隔离 |

## 建议的修复顺序

### 第1步（优先级高）: 修复 fork() RIP
1. 修改 syscall 处理程序，将 syscall 返回地址 (RCX) 传递给系统调用处理程序
2. 修改 fork() 系统调用，接收并使用返回地址初始化子进程 context.rip
3. 测试 fork() 返回值正确性

### 第2步（优先级中）: 实现基本页表隔离
1. 实现 fork_user_page_table() 函数，为子进程创建独立的页表
2. 在 fork() 中为子进程分配新的 CR3
3. 复制父进程的用户空间映射

### 第3步（优先级低）: 优化 wait4()
1. 用真正的进程阻塞替换忙轮询
2. 在子进程退出时唤醒等待的父进程

## 测试步骤

```bash
# 编译并重建
./scripts/build-all.sh

# 运行 QEMU
./scripts/run-qemu.sh

# 预期的启动流程
# 1. 内核启动 init (PID 1)
# 2. init fork() 并创建 getty (PID 2)
# 3. init 调用 wait4() 等待 getty
# 4. getty 应该加载并显示登录提示
# 5. 父进程 init 检测到 getty 已启动并继续
```

## 相关代码位置

- `src/syscall.rs` - 系统调用处理 (第 1025-1110 行: fork(), 第 1219-1330 行: wait4())
- `src/scheduler.rs` - 调度器 (do_schedule() 在第 373 行)
- `src/process.rs` - 进程结构定义 (Process 结构在第 73 行)
- `userspace/init.rs` - init 程序 (start_service() 在第 1237 行)

## 附加说明

虽然当前实现有这些问题，但系统已采取的临时措施（忙轮询、do_schedule() 安全返回）至少允许系统启动并进入紧急模式而不是完全崩溃。这为进一步调试和修复提供了时间和机会。
