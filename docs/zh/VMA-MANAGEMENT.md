# VMA (Virtual Memory Area) 管理系统

## 概述

NexaOS的VMA管理系统提供了生产级别的虚拟内存区域管理，类似于Linux内核的`mm/mmap.c`实现。它使用增强红黑树（区间树）来实现O(log n)的查找性能。

## 架构

```
AddressSpace (每进程)
├── VMAManager (区间树)
│   ├── VMA [0x1000000 - 0x1100000] code, r-x
│   ├── VMA [0x1100000 - 0x1200000] data, rw-
│   ├── VMA [0x1200000 - 0x1400000] heap, rw-
│   └── VMA [0x1400000 - 0x1600000] stack, rw-
└── Page Table (CR3)
```

## 核心组件

### 1. VMA结构 (`src/mm/vma.rs`)

```rust
pub struct VMA {
    pub start: u64,           // 起始地址（页对齐）
    pub end: u64,             // 结束地址（不包含，页对齐）
    pub perm: VMAPermissions, // 访问权限
    pub flags: VMAFlags,      // VMA标志
    pub backing: VMABacking,  // 后备存储类型
    pub generation: u64,      // COW代数计数
    pub cow_parent: i32,      // COW父VMA索引
    pub refcount: u32,        // 引用计数
}
```

### 2. VMA权限 (`VMAPermissions`)

| 标志 | 值 | 描述 |
|------|-----|------|
| NONE | 0 | 无权限 |
| READ | 1 | 可读 |
| WRITE | 2 | 可写 |
| EXEC | 4 | 可执行 |

### 3. VMA标志 (`VMAFlags`)

| 标志 | 描述 |
|------|------|
| SHARED | MAP_SHARED映射 |
| PRIVATE | MAP_PRIVATE映射 |
| ANONYMOUS | 匿名映射 |
| FIXED | MAP_FIXED |
| GROWSDOWN | 栈向下增长 |
| STACK | 主线程栈 |
| HEAP | 堆区域 |
| DEMAND | 需求分页 |
| COW | 写时复制待处理 |

### 4. VMA后备类型 (`VMABacking`)

```rust
pub enum VMABacking {
    Anonymous,                    // 匿名映射（零填充）
    File { inode: u64, offset: u64 }, // 文件映射
    Device { phys_addr: u64 },    // 设备内存映射
    SharedMemory { shmid: u64 },  // 共享内存段
}
```

## VMA管理器 (`VMAManager`)

使用增强红黑树实现的区间树，支持：

- **O(log n)** 按地址查找VMA
- **O(log n)** 范围查询（重叠VMA）
- **O(log n)** 插入新VMA
- **O(log n)** 删除VMA
- 自动VMA合并（相邻兼容区域）
- VMA分割（部分munmap/mprotect）

### 关键方法

```rust
impl VMAManager {
    pub fn find(&self, addr: u64) -> Option<&VMA>;
    pub fn find_overlapping(&self, start: u64, end: u64, buffer: &mut [i32]) -> usize;
    pub fn insert(&mut self, vma: VMA) -> Option<i32>;
    pub fn remove(&mut self, start: u64) -> Option<VMA>;
    pub fn find_free_region(&self, min: u64, max: u64, size: u64) -> Option<u64>;
    pub fn try_merge(&mut self, start: u64) -> bool;
}
```

## 地址空间 (`AddressSpace`)

每个进程的完整地址空间描述：

```rust
pub struct AddressSpace {
    pub vmas: VMAManager,       // VMA管理器
    pub cr3: u64,               // 页表根
    pub pid: u64,               // 拥有者PID
    pub heap_start: u64,        // 堆起始
    pub heap_end: u64,          // 当前堆结束
    pub stack_start: u64,       // 栈起始
    pub stack_end: u64,         // 栈结束
    pub mmap_base: u64,         // mmap分配基址
    pub mmap_current: u64,      // 当前mmap指针
    pub rss_pages: u64,         // 常驻集大小
    pub vm_pages: u64,          // 虚拟大小
}
```

## 系统调用实现 (`src/syscalls/memory_vma.rs`)

### mmap_vma

```rust
pub fn mmap_vma(
    addr: u64,    // 提示地址
    length: u64,  // 映射长度
    prot: u64,    // 保护标志
    flags: u64,   // 映射标志
    fd: i64,      // 文件描述符
    offset: u64   // 文件偏移
) -> u64;
```

特性：
- 支持MAP_FIXED精确地址映射
- 支持地址提示
- 自动寻找空闲区域
- VMA合并优化
- 需求分页支持

### munmap_vma

```rust
pub fn munmap_vma(addr: u64, length: u64) -> u64;
```

特性：
- 部分解除映射（VMA分割）
- 完整VMA删除
- 跨多个VMA的解除映射

### mprotect_vma

```rust
pub fn mprotect_vma(addr: u64, length: u64, prot: u64) -> u64;
```

### brk_vma

```rust
pub fn brk_vma(addr: u64) -> u64;
```

## 统计跟踪

```rust
pub struct VMAStats {
    pub mmap_count: u64,       // mmap调用次数
    pub munmap_count: u64,     // munmap调用次数
    pub mprotect_count: u64,   // mprotect调用次数
    pub merge_count: u64,      // VMA合并次数
    pub split_count: u64,      // VMA分割次数
    pub mapped_bytes: u64,     // 总映射字节数
    pub peak_vma_count: u64,   // VMA峰值数量
    pub page_faults: u64,      // 页面错误数
    pub cow_faults: u64,       // COW错误数
}
```

## 内存布局常量

| 常量 | 值 | 描述 |
|------|-----|------|
| USER_VIRT_BASE | 0x1000000 | 用户空间基址 (16MB) |
| HEAP_BASE | 0x1200000 | 堆起始 |
| STACK_BASE | 0x1400000 | 栈起始 |
| STACK_SIZE | 0x200000 | 栈大小 (2MB) |
| INTERP_BASE | 0x1600000 | 动态链接器基址 |

## 使用示例

### 初始化进程地址空间

```rust
use crate::syscalls::memory_vma;

// 为新进程初始化地址空间
memory_vma::init_process_address_space(pid, cr3)?;
```

### fork时复制地址空间

```rust
memory_vma::copy_address_space_for_fork(parent_pid, child_pid)?;
```

### 清理地址空间

```rust
memory_vma::free_process_address_space(pid);
```

## 调试

### 打印内存映射

```rust
memory_vma::print_current_maps();
```

输出类似 `/proc/self/maps`:
```
0000000001000000-0000000001100000 r-xp 00000000 00:00 0 [text]
0000000001200000-0000000001300000 rw-p 00000000 00:00 0 [heap]
0000000001400000-0000000001600000 rw-p 00000000 00:00 0 [stack]
```

### 获取VMA统计

```rust
if let Some(stats) = memory_vma::get_vma_stats() {
    kinfo!("mmap calls: {}", stats.mmap_count);
    kinfo!("mapped bytes: {}", stats.mapped_bytes);
}
```

## 未来改进

1. **完整COW支持**: 实现fork时的写时复制页面处理
2. **与页表集成**: 将VMA权限变化同步到页表
3. **mremap支持**: 实现内存区域重映射
4. **MADV_*支持**: 实现madvise内存建议
5. **huge pages**: 支持大页映射
6. **NUMA感知**: 按NUMA节点优化内存分配
7. **/proc/[pid]/smaps**: 详细内存统计

## 文件列表

| 文件 | 描述 |
|------|------|
| `src/mm/vma.rs` | VMA核心数据结构和管理器 |
| `src/mm/mod.rs` | 内存管理模块导出 |
| `src/syscalls/memory_vma.rs` | 基于VMA的系统调用实现 |
| `src/syscalls/mod.rs` | 系统调用模块（添加memory_vma） |
| `src/fs/procfs.rs` | /proc/[pid]/maps生成 |
