# PID Radix Tree Implementation

## 概述

NexaOS 使用基数树（Radix Tree）实现高效的 PID 管理。相比简单的线性数组遍历，基数树提供 O(log N) 时间复杂度的 PID 分配、查找和释放操作，并支持 PID 回收。

## 设计

### 数据结构

基数树使用 64 路分支因子（每层 6 位），对于最大支持的 2^18 (262144) 个 PID，只需要最多 3 层。

```
Level 0: bits 17-12 (root)
Level 1: bits 11-6
Level 2: bits 5-0 (leaf)
```

### 组件

1. **PID 分配器 (PidAllocator)**
   - 使用位图跟踪已分配的 PID
   - 64 个 u64 = 4096 个 PID 的跟踪能力
   - 支持 PID 回收和重用
   - 使用 `next_hint` 加速查找空闲 PID

2. **基数树 (PidRadixTree)**
   - 存储 PID 到进程表索引的映射
   - 最多 256 个节点的静态池
   - O(log N) 查找、插入、删除

### 内存布局

```
MAX_PID:         262143 (2^18 - 1)
MIN_PID:         1 (PID 0 reserved for kernel)
RADIX_BITS:      6 bits per level
RADIX_CHILDREN:  64 children per node
RADIX_LEVELS:    3 levels
MAX_RADIX_NODES: 256 nodes
```

## API

### 核心函数

```rust
// 分配新 PID
pub fn allocate_pid() -> u64;

// 释放 PID（进程退出时调用）
pub fn free_pid(pid: u64);

// 注册 PID 到进程表索引的映射
pub fn register_pid_mapping(pid: u64, process_table_idx: u16) -> bool;

// 查找 PID 对应的进程表索引
pub fn lookup_pid(pid: u64) -> Option<u16>;

// 移除 PID 映射
pub fn unregister_pid_mapping(pid: u64) -> Option<u16>;

// 检查 PID 是否已分配
pub fn is_pid_allocated(pid: u64) -> bool;

// 获取已分配 PID 数量
pub fn allocated_pid_count() -> u64;

// 分配特定 PID（用于 init 进程等特殊情况）
pub fn allocate_specific_pid(pid: u64) -> bool;

// 获取统计信息
pub fn get_pid_stats() -> (u64, usize);  // (allocated_pids, radix_nodes)
```

## 使用示例

### 创建新进程

```rust
// 1. 分配 PID
let pid = crate::process::allocate_pid();

// 2. 在进程表中找到空闲槽位
let slot_idx = find_empty_slot();

// 3. 注册映射
crate::process::register_pid_mapping(pid, slot_idx as u16);

// 4. 初始化进程条目
table[slot_idx] = Some(ProcessEntry { ... });
```

### 查找进程

```rust
// 快速 O(log N) 查找
if let Some(idx) = crate::process::lookup_pid(target_pid) {
    let idx = idx as usize;
    if let Some(entry) = &table[idx] {
        // 使用 entry
    }
}
```

### 进程退出

```rust
// 1. 从进程表移除
table[idx] = None;

// 2. 释放 PID（同时移除基数树映射）
crate::process::free_pid(pid);
```

## 性能对比

| 操作 | 线性数组 (旧) | 基数树 (新) |
|------|---------------|-------------|
| PID 分配 | O(1) | O(1) 摊销 |
| PID 查找 | O(N) | O(log N) |
| PID 释放 | O(N) | O(log N) |
| 进程状态更新 | O(N) | O(log N) |
| 内存开销 | 无额外开销 | ~12 KB |

## 实现细节

### PID 分配

```rust
fn allocate_next(&mut self) -> Option<u64> {
    // 从 hint 位置开始搜索
    for word_idx in start_word..self.bitmap.len() {
        let word = self.bitmap[word_idx];
        if word == u64::MAX { continue; }  // 全部占用
        
        // 找到第一个空闲位
        let first_zero = (!word).trailing_zeros();
        let pid = word_idx * 64 + first_zero;
        
        if self.mark_allocated(pid) {
            self.next_hint = pid + 1;
            return Some(pid);
        }
    }
    // 回绕搜索...
}
```

### 基数树查找

```rust
fn lookup(&self, pid: u64) -> Option<u16> {
    let mut node_idx = 0;  // 从根节点开始
    
    for level in 0..RADIX_LEVELS {
        let radix_idx = Self::radix_index(pid, level);
        let child_idx = self.nodes[node_idx].children[radix_idx];
        
        if child_idx == 0 { return None; }
        node_idx = child_idx as usize;
    }
    
    let process_idx = self.nodes[node_idx].process_idx;
    if process_idx == u16::MAX { None } else { Some(process_idx) }
}
```

## 兼容性

为了保持向后兼容，所有使用 PID 查找的函数都包含回退机制：

1. 首先尝试基数树 O(log N) 查找
2. 如果查找失败或结果过期，回退到线性扫描
3. 这确保即使基数树状态不一致，系统仍能正常工作

## 未来改进

1. **更大的 PID 空间**: 增加位图大小以支持更多 PID
2. **节点回收**: 实现基数树节点的回收以减少内存使用
3. **无锁操作**: 使用原子操作减少锁竞争
4. **统计收集**: 添加更详细的性能统计
