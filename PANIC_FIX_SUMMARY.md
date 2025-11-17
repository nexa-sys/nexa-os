# Shell退出后Getty重启时的Panic修复总结

## 问题描述
当用户在shell中执行`exit`命令后，系统尝试重新启动getty服务时触发General Protection Fault。

## 已修复的Bug

### Bug 1: execve错误地重置`has_entered_user`标志
**文件**: `src/syscall.rs` 第1781行  
**问题**: execve将`has_entered_user`设置为false，导致调度器将已经在用户态运行的进程当作首次运行  
**修复**: 移除了`entry.process.has_entered_user = false;`这行代码，因为execve通过`sysretq`直接返回用户态，进程实际上已经entered user mode

## 仍存在的问题

### 问题: CR3页表损坏
**症状**: 
- PID 2 (getty)的CR3=0x8003000在第一次使用时正常
- 第二次尝试激活同一个CR3时触发GP fault
- PML4 entry[0]内容看起来有效(0x8004027)，但深层页表结构可能已损坏

**可能的原因**:
1. 某个进程的野指针写入破坏了页表区域(0x08000000-0x08100000)
2. 页表创建时使用了错误的物理地址映射
3. TLB刷新时机不当导致页表缓存不一致

**调试发现**:
- 页表区域(0x08000000起)与用户内存区域(0x10000000起)没有重叠
- `free_process_address_space`是空实现，不会释放页表
- execve清零内存不会影响页表区域

## 建议的进一步调试步骤

1. 在fork和execve时保存CR3页表的校验和，在使用前验证
2. 添加页表区域的内存保护，检测非法写入
3. 详细记录所有对0x08000000-0x08100000区域的内存访问
4. 检查是否有进程使用了错误的物理地址进行DMA或其他操作

## 临时解决方案（未实施）

如果根本原因难以追踪，可以考虑：
- 为每个进程实例分配新的页表，而不是在execve时重用
- 在调度前重新验证并可能重建损坏的页表
- 使用影子页表技术保护关键页表结构

## 相关文件

- `src/syscall.rs` - fork和execve实现
- `src/paging.rs` - 页表管理和CR3激活
- `src/scheduler.rs` - 进程调度逻辑
- `src/process.rs` - 进程创建和ELF加载

## 测试方法

```bash
./scripts/build-all.sh
./scripts/test-shell-exit.sh
```

预期行为：shell退出后，getty应该成功重启并显示登录提示
实际行为：触发GP fault并打印寄存器dump
