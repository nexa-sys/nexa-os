# NexaOS 测试套件

这是 NexaOS 内核的独立测试套件。测试在标准 Rust 环境 (`std`) 中运行，不是在裸机上。

## 设计理念

- **测试代码与内核代码分离** - 不需要在内核代码中使用 `#[cfg(test)]`
- **测试纯逻辑** - 测试不依赖硬件的算法和数据结构
- **Mock 组件** - 使用 mock 对象模拟硬件相关组件

## 目录结构

```
tests/
├── Cargo.toml              # 独立的 Cargo 配置
├── .cargo/
│   └── config.toml         # 覆盖父目录配置，使用 host target
└── src/
    ├── lib.rs              # 测试入口
    ├── posix.rs            # POSIX 类型测试
    ├── algorithms/         # 算法测试
    │   ├── bitmap.rs       # 位图分配器
    │   ├── ring_buffer.rs  # 环形缓冲区
    │   └── checksum.rs     # 校验和算法
    ├── data_structures/    # 数据结构测试
    │   ├── fixed_vec.rs    # 固定大小向量
    │   └── path.rs         # 路径操作
    └── mock/               # Mock 组件
        ├── memory.rs       # 内存分配器 mock
        └── scheduler.rs    # 调度器 mock
```

## 运行测试

### 使用 NDK (推荐)

```bash
# 运行所有测试
./ndk test

# 运行匹配模式的测试
./ndk test --filter bitmap

# 详细输出
./ndk test --verbose

# release 模式
./ndk test --release
```

### 直接使用 Cargo

```bash
cd tests
cargo test

# 运行特定测试
cargo test bitmap

# 详细输出
cargo test -- --nocapture
```

## 添加新测试

### 1. 纯算法测试

在 `src/algorithms/` 中添加新模块：

```rust
// src/algorithms/my_algorithm.rs

pub fn my_function(input: u32) -> u32 {
    input * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_function() {
        assert_eq!(my_function(21), 42);
    }
}
```

然后在 `src/algorithms.rs` 中导出：

```rust
pub mod my_algorithm;
```

### 2. Mock 组件测试

对于需要模拟硬件的组件，在 `src/mock/` 中添加 mock 实现：

```rust
// src/mock/my_device.rs

pub struct MockDevice {
    // 模拟设备状态
}

impl MockDevice {
    pub fn new() -> Self { ... }
    pub fn read(&self) -> u8 { ... }
    pub fn write(&mut self, val: u8) { ... }
}
```

## 与内核代码同步

当内核中的数据结构或算法变化时，需要手动同步测试代码。这是有意为之，以保持测试的独立性和简洁性。

如果需要测试更复杂的内核逻辑，考虑：

1. 将内核中的纯逻辑提取到 `#![no_std]` 兼容的独立 crate
2. 在测试套件中依赖该 crate
3. 内核也依赖该 crate

## 测试分类

| 模块 | 描述 | 测试数量 |
|------|------|----------|
| `posix` | POSIX 错误码、文件类型 | 4 |
| `algorithms::bitmap` | 位图分配 | 5 |
| `algorithms::ring_buffer` | 环形缓冲区 | 6 |
| `algorithms::checksum` | 网络校验和 | 6 |
| `data_structures::fixed_vec` | 固定大小向量 | 7 |
| `data_structures::path` | 路径操作 | 8 |
| `mock::memory` | 内存分配器 mock | 3 |
| `mock::scheduler` | 调度器 mock | 5 |

总计: **44 个测试**
