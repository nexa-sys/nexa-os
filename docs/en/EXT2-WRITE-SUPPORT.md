# EXT2 文件系统写支持实现指南

## 概述

本文档描述了 NexaOS ext2 文件系统的写支持实现。这个实现提供了基础的文件写入功能，使得用户程序可以向 ext2 卷中的现有文件写入数据。

## 架构设计

### 核心组件

1. **Ext2Filesystem 结构体** - 读写管理的主要接口
   - 位置: `src/fs/ext2.rs`
   - 特点: 使用全局状态管理写模式，避免可变引用问题

2. **Ext2WriteState 全局状态**
   - 位置: `src/fs/ext2.rs` 第 47-51 行
   - 功能: 使用 `spin::Mutex` 跟踪全局写模式状态
   - 初始化: 通过 `spin::Once` 进行单次初始化

3. **Ext2WriteBuffer 全局缓冲区**
   - 大小: 16 MiB (`EXT2_MAX_WRITE_BUFFER`)
   - 用途: 用于写操作的临时数据存储
   - 位置: 内核缓存段 (`.kernel_cache`)

### 关键函数

#### 1. `is_writable()` - 检查写模式状态
```rust
fn is_writable() -> bool
```
- 检查全局 `EXT2_WRITE_STATE` 的状态
- 返回是否启用了写模式

#### 2. `enable_write_mode()` - 启用写支持
```rust
pub fn enable_write_mode()
```
- 初始化全局写状态
- 必须在写操作前调用
- 使用方法: `ext2::Ext2Filesystem::enable_write_mode()`

#### 3. `write_file_at()` - 写入文件数据
```rust
pub fn write_file_at(
    &self, 
    inode_num: u32, 
    offset: usize, 
    data: &[u8]
) -> Result<usize, Ext2Error>
```
- 在指定 inode 和偏移处写入数据
- 返回实际写入的字节数
- 错误处理:
  - `ReadOnly`: 写模式未启用
  - `InvalidInode`: inode 号无效
  - `NoSpaceLeft`: 没有可用空间
  - `InvalidBlockNumber`: 块号不存在

#### 4. `allocate_block()` - 分配新块
```rust
pub fn allocate_block(&self) -> Result<u32, Ext2Error>
```
- 在文件系统中分配新数据块
- 返回新块的编号
- 当前: 返回 `NoSpaceLeft` 错误 (待实现)

#### 5. `allocate_inode()` - 分配新 inode
```rust
pub fn allocate_inode(&self) -> Result<u32, Ext2Error>
```
- 分配新 inode 结构
- 返回新 inode 编号
- 当前: 返回 `NoSpaceLeft` 错误 (待实现)

### FileSystem Trait 扩展

添加了两个新方法到 `FileSystem` trait:

```rust
fn write(&self, path: &str, data: &[u8]) -> Result<usize, &'static str>;
fn create(&self, path: &str) -> Result<(), &'static str>;
```

#### `write()` 实现
- 查找指定路径的文件
- 调用 `write_file_at()` 进行实际写入
- 检查文件系统是否支持写操作

#### `create()` 实现
- 当前: 返回"不支持"错误
- 待实现: 完整的文件创建流程

### 公共 API (fs.rs)

#### `write_file(path, data)` 
```rust
pub fn write_file(path: &str, data: &[u8]) -> Result<usize, &'static str>
```
- 通过挂载点解析路径
- 调用对应文件系统的 `write()` 方法
- 返回写入的字节数

#### `create_file(path)`
```rust
pub fn create_file(path: &str) -> Result<(), &'static str>
```
- 通过挂载点解析路径
- 调用对应文件系统的 `create()` 方法
- 创建新文件

#### `enable_ext2_write()`
```rust
pub fn enable_ext2_write() -> Result<(), &'static str>
```
- 启用全局 ext2 写支持
- 必须在使用写操作前调用

## 使用示例

### 1. 启用 ext2 写支持

```rust
// 在内核初始化代码中
crate::fs::enable_ext2_write()?;
```

### 2. 在用户程序中写入文件

```c
#include <unistd.h>
#include <fcntl.h>

int main() {
    // 打开文件进行写入
    int fd = open("/mnt/ext/test.txt", O_WRONLY);
    if (fd < 0) {
        perror("open");
        return 1;
    }

    // 写入数据
    const char* data = "Hello, ext2!";
    ssize_t written = write(fd, data, 12);
    
    close(fd);
    return 0;
}
```

### 3. 在 syscall 中处理

```rust
// src/syscall.rs 中的处理
fn syscall_write(fd: u64, buf: u64, count: u64) -> u64 {
    // 对于 ext2 挂载点的文件:
    if let Some((fs, path)) = resolve_file_from_fd(fd) {
        match fs.write(path, data) {
            Ok(n) => n as u64,
            Err(_) => u64::MAX, // errno = EIO
        }
    }
}
```

## 实现细节

### 1. 块分配策略

当前实现仅支持写入现有块。完整实现需要:

1. **块位图扫描** - 查找第一个空闲块
2. **位图更新** - 标记块为已使用
3. **超级块更新** - 更新自由块计数
4. **间接块处理** - 对于超过 12 个直接块的文件

### 2. Inode 分配策略

类似块分配，需要:

1. **Inode 位图扫描**
2. **Inode 初始化**
3. **目录项创建**
4. **父目录更新**

### 3. 数据一致性

考虑事项:

- **原子性**: 多个块的写入应该原子化
- **同步**: 关键元数据必须立即同步
- **日志**: 考虑添加日志记录以便恢复

## 错误处理

### Ext2Error 类型

```rust
pub enum Ext2Error {
    BadMagic,                  // 无效的 ext2 签名
    ImageTooSmall,            // 镜像数据不完整
    UnsupportedInodeSize,     // Inode 大小不支持
    InvalidGroupDescriptor,   // 组描述符错误
    InodeOutOfBounds,         // Inode 号超出范围
    NoSpaceLeft,              // 文件系统空间不足
    ReadOnly,                 // 文件系统为只读
    InvalidInode,             // Inode 结构无效
    InvalidBlockNumber,       // 块号无效
}
```

## 限制和已知问题

1. **块分配未实现** - `allocate_block()` 总是返回错误
2. **Inode 分配未实现** - `allocate_inode()` 总是返回错误
3. **文件创建不支持** - `create()` 方法返回错误
4. **不支持目录创建** - `mkdir` 等操作无效
5. **不支持符号链接** - 只能写入常规文件
6. **全局写缓冲区** - 16 MiB 限制可能对大文件不够

## 性能考虑

1. **缓冲策略** - 使用全局缓冲区避免栈溢出
2. **锁竞争** - 所有写操作通过单一互斥锁
3. **块缓存** - 考虑添加块缓存以提高性能

## 将来改进方向

1. **块分配器实现**
   ```rust
   fn allocate_block_real(&self) -> Result<u32, Ext2Error>
   ```

2. **Inode 分配器实现**
   ```rust
   fn allocate_inode_real(&self) -> Result<u32, Ext2Error>
   ```

3. **目录项管理**
   ```rust
   fn add_dir_entry(&self, parent_inode: u32, name: &str, target_inode: u32) -> Result<(), Ext2Error>
   ```

4. **文件创建支持**
   ```rust
   fn create_file(&self, path: &str) -> Result<u32, Ext2Error>
   ```

5. **块缓存层**
   - LRU 块缓存
   - 减少重复 I/O

6. **日志记录**
   - 元数据日志
   - 恢复支持

## 调试技巧

### 启用详细日志

在 `src/logger.rs` 中设置日志级别:

```rust
crate::kdebug!("Writing {} bytes at offset {}", data.len(), offset);
```

### 检查文件系统状态

```rust
pub fn fs_stats() {
    if let Some(fs) = ext2::global() {
        crate::kinfo!("EXT2 block_size: {}", fs.block_size);
        crate::kinfo!("EXT2 total_groups: {}", fs.total_groups);
    }
}
```

## 参考资源

- EXT2 文件系统规范: https://www.kernel.org/doc/html/latest/filesystems/ext2.html
- NexaOS 文件系统文档: `docs/en/SYSTEM-OVERVIEW.md`
- Syscall 参考: `docs/en/SYSCALL-REFERENCE.md`

## 相关源文件

- `src/fs/ext2.rs` - 核心实现
- `src/fs.rs` - 公共 API
- `src/syscall.rs` - Syscall 处理
- `build/rootfs/` - EXT2 镜像挂载点

## 测试

为了测试 ext2 写支持，可以:

1. 在 init 程序中调用 `enable_ext2_write()`
2. 编写简单的用户程序进行读写测试
3. 使用 `e2fsck` 验证文件系统完整性

```bash
# 编译测试程序
gcc -o test_write test_write.c

# 运行内核并测试
./scripts/run-qemu.sh
# 在内核中执行测试
/test_write
```

---

**最后更新**: 2025年11月16日  
**维护者**: NexaOS 开发团队  
**状态**: 基础实现完成，高级功能待开发
