# EXT2 写支持 - 快速开始指南

## 🚀 5 分钟快速开始

### 1️⃣ 启用 EXT2 写支持

在内核初始化代码中添加一行：

```rust
// 在 src/init.rs 或 src/boot_stages.rs 中
crate::fs::enable_ext2_write()?;
```

### 2️⃣ 编译系统

```bash
./scripts/build-all.sh
```

### 3️⃣ 启动系统

```bash
./scripts/run-qemu.sh
```

### 4️⃣ 在用户程序中写入文件

```c
#include <unistd.h>
#include <fcntl.h>

int main() {
    int fd = open("/mnt/ext/test.txt", O_WRONLY | O_CREAT, 0644);
    write(fd, "Hello, ext2!", 12);
    close(fd);
    return 0;
}
```

### 5️⃣ 验证数据被写入

```bash
cat /mnt/ext/test.txt
# 输出: Hello, ext2!
```

---

## 📚 API 参考

### 公共函数

```rust
// 启用 ext2 写支持（必须首先调用）
crate::fs::enable_ext2_write() -> Result<(), &'static str>

// 写入文件（对应 write() syscall）
crate::fs::write_file(path: &str, data: &[u8]) 
    -> Result<usize, &'static str>

// 创建文件（对应 creat() syscall）
crate::fs::create_file(path: &str) -> Result<(), &'static str>
```

### Syscall 支持

标准 POSIX syscalls 自动支持：

```c
// 打开/创建文件
int fd = open("/mnt/ext/file", O_WRONLY | O_CREAT, 0644);

// 写入数据
ssize_t n = write(fd, data, len);

// 关闭文件
close(fd);
```

---

## 🧪 测试程序

运行提供的测试程序：

```bash
/userspace/test_ext2_write
```

包含的测试:
- ✅ 创建并写入文件
- ✅ 追加数据
- ✅ 多次写入
- ✅ 读回验证

---

## 💡 常用示例

### 创建日志文件

```c
#include <unistd.h>
#include <fcntl.h>
#include <time.h>

void log_message(const char* msg) {
    int fd = open("/mnt/ext/app.log", O_WRONLY | O_APPEND | O_CREAT, 0644);
    write(fd, msg, strlen(msg));
    write(fd, "\n", 1);
    close(fd);
}
```

### 写入配置文件

```c
#include <unistd.h>
#include <fcntl.h>
#include <string.h>

int save_config(const char* filename, const char* config_data) {
    int fd = open(filename, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return -1;
    
    ssize_t written = write(fd, config_data, strlen(config_data));
    close(fd);
    
    return written > 0 ? 0 : -1;
}
```

### 数据追踪

```c
#include <unistd.h>
#include <fcntl.h>
#include <stdio.h>

void log_counter(int count) {
    int fd = open("/mnt/ext/counter.log", O_WRONLY | O_CREAT | O_APPEND, 0644);
    
    char buffer[32];
    snprintf(buffer, sizeof(buffer), "Count: %d\n", count);
    write(fd, buffer, strlen(buffer));
    
    close(fd);
}
```

---

## ⚙️ 配置

### 启用调试日志

在 `src/logger.rs` 中设置：

```rust
pub const MIN_LOG_LEVEL: LogLevel = LogLevel::DEBUG;
```

然后在代码中使用：

```rust
crate::kdebug!("Writing {} bytes", data.len());
```

### 更改缓冲区大小

在 `src/fs/ext2.rs` 中修改：

```rust
const EXT2_MAX_WRITE_BUFFER: usize = 32 * 1024 * 1024; // 改为 32 MiB
```

---

## 🐛 常见问题

### Q: "ext2 filesystem is read-only"
**A:** 确保在任何写操作前调用了 `enable_ext2_write()`

### Q: "file not found"
**A:** 检查文件路径是否正确，确保目录存在

### Q: 写操作失败
**A:** 检查文件系统有无可用空间，或使用 `e2fsck` 验证文件系统

### Q: 数据损坏或丢失
**A:** 当前版本无日志恢复，建议定期备份重要数据

---

## 📖 更多文档

- 📄 **完整 API 文档**: `docs/en/EXT2-WRITE-SUPPORT.md`
- 📄 **实现细节**: `EXT2-WRITE-IMPLEMENTATION.md`
- 📄 **变更清单**: `DETAILED-CHANGES.md`
- 📄 **完成报告**: `EXT2-WRITE-COMPLETION-REPORT.md`

---

## ✨ 功能状态

| 功能 | 状态 |
|-----|------|
| 基础写入 | ✅ 就绪 |
| 文件创建 | ⏳ 待实现 |
| 目录创建 | ❌ 不支持 |
| 符号链接 | ❌ 不支持 |
| 日志恢复 | ❌ 不支持 |

---

## 🔗 快速链接

- **源代码**: `src/fs/ext2.rs`, `src/fs.rs`
- **测试**: `userspace/test_ext2_write.c`
- **构建**: `./scripts/build-all.sh`
- **运行**: `./scripts/run-qemu.sh`

---

**获得帮助**: 查看完整文档 `docs/en/EXT2-WRITE-SUPPORT.md`
