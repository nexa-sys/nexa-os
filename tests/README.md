# NexaOS 测试套件

这是 NexaOS 内核的独立测试套件。测试在标准 Rust 环境 (`std`) 中运行，不是在裸机上。

## 设计理念

- **测试代码与内核代码分离** - 不需要在内核代码中使用 `#[cfg(test)]`
- **按子系统组织测试** - 每个内核子系统有独立的测试目录
- **Mock 组件** - 使用 `mock/` 模块模拟硬件（CPU、中断控制器、设备等）
- **集成测试** - `integration/` 目录测试多子系统协作

## 目录结构

```
tests/
├── Cargo.toml              # 独立的 Cargo 配置
├── .cargo/
│   └── config.toml         # 覆盖父目录配置，使用 host target
├── README.md               # 本文档
└── src/
    ├── lib.rs              # 测试入口 + 内核源码导入
    │
    ├── fs/                 # 文件系统测试
    │   ├── mod.rs
    │   ├── fstab.rs        # fstab 解析
    │   └── comprehensive.rs # 文件描述符、inode 综合测试
    │
    ├── mm/                 # 内存管理测试
    │   ├── mod.rs
    │   ├── allocator.rs    # Buddy 分配器
    │   ├── comprehensive.rs # 虚拟地址、分页、内存布局
    │   └── safety.rs       # layout_of、layout_array 安全工具
    │
    ├── net/                # 网络协议栈测试
    │   ├── mod.rs
    │   ├── ethernet.rs     # 以太网帧
    │   ├── ipv4.rs         # IPv4 地址和数据包
    │   ├── arp.rs          # ARP 协议
    │   ├── udp.rs          # UDP 数据报
    │   ├── udp_helper.rs   # UDP 辅助函数
    │   └── comprehensive.rs # 综合协议栈测试
    │
    ├── ipc/                # 进程间通信测试
    │   ├── mod.rs
    │   ├── signal.rs       # 信号处理
    │   └── comprehensive.rs # 信号、管道综合测试
    │
    ├── process/            # 进程管理测试
    │   ├── mod.rs
    │   ├── types.rs        # ProcessState、Context 基础类型
    │   ├── context.rs      # 进程上下文
    │   ├── state.rs        # 状态转换
    │   ├── thread.rs       # 线程管理
    │   └── comprehensive.rs # PID 分配、生命周期综合测试
    │
    ├── scheduler/          # 调度器测试
    │   ├── mod.rs
    │   ├── types.rs        # CpuMask、SchedPolicy 基础类型
    │   ├── basic.rs        # 基础调度测试
    │   ├── eevdf.rs        # EEVDF 算法
    │   ├── eevdf_vruntime.rs # vruntime 计算
    │   ├── percpu.rs       # Per-CPU 队列
    │   ├── smp.rs          # SMP 调度
    │   ├── smp_comprehensive.rs # SMP 综合测试
    │   └── stress.rs       # 压力测试
    │
    ├── kmod/               # 内核模块测试
    │   ├── mod.rs
    │   ├── crypto.rs       # 加密算法
    │   ├── pkcs7.rs        # PKCS#7 签名
    │   └── nkm.rs          # NKM 模块格式
    │
    ├── integration/        # 集成测试
    │   ├── mod.rs
    │   ├── boot.rs         # 启动流程
    │   ├── devices.rs      # 设备初始化
    │   ├── interrupt.rs    # 中断处理
    │   ├── memory.rs       # 内存子系统
    │   ├── smp.rs          # SMP 启动
    │   └── scheduler_smp.rs # SMP 调度集成
    │
    ├── mock/               # 硬件模拟层
    │   ├── mod.rs
    │   ├── cpu.rs          # CPU 模拟
    │   ├── memory.rs       # 内存模拟
    │   ├── hal.rs          # 硬件抽象层
    │   ├── pci.rs          # PCI 总线
    │   ├── vm.rs           # 虚拟机环境
    │   └── devices/        # 设备模拟
    │       ├── lapic.rs    # Local APIC
    │       ├── ioapic.rs   # I/O APIC
    │       ├── pic.rs      # 8259 PIC
    │       ├── pit.rs      # 8254 PIT
    │       ├── rtc.rs      # RTC
    │       └── uart.rs     # 串口
    │
    ├── interrupts.rs       # 中断处理单元测试
    ├── syscalls.rs         # 系统调用接口测试
    └── udrv.rs             # 用户态驱动框架测试
```

## 运行测试

### 使用 NDK (推荐)

```bash
# 运行所有测试
./ndk test

# 运行匹配模式的测试
./ndk test --filter scheduler     # 调度器相关
./ndk test --filter process       # 进程管理相关
./ndk test --filter comprehensive # 所有综合测试
./ndk test --filter eevdf         # EEVDF 算法测试

# 详细输出
./ndk test --verbose

# 生成覆盖率报告
./ndk coverage html
```

### 直接使用 Cargo

```bash
cd tests
cargo test

# 运行特定模块测试
cargo test fs::                   # 文件系统测试
cargo test net::                  # 网络测试
cargo test scheduler::            # 调度器测试

# 详细输出
cargo test -- --nocapture
```

## 添加新测试

### 1. 在已有子系统中添加测试

在对应目录添加新文件，然后在 `mod.rs` 中导入：

```rust
// 例如: src/fs/vfs.rs
#[cfg(test)]
mod tests {
    #[test]
    fn test_vfs_mount() {
        // 测试代码
    }
}
```

然后在 `src/fs/mod.rs` 中添加：

```rust
mod vfs;
```

### 2. 添加新的子系统测试

1. 创建目录 `src/new_subsystem/`
2. 创建 `src/new_subsystem/mod.rs`
3. 在 `src/lib.rs` 中添加模块声明

### 3. 使用 Mock 硬件

```rust
use crate::mock::cpu::MockCpu;
use crate::mock::devices::MockLapic;

#[test]
fn test_with_mock_hardware() {
    let cpu = MockCpu::new();
    let lapic = MockLapic::new();
    // 使用模拟硬件进行测试
}
```

## 测试命名约定

- 文件名: `snake_case.rs`
- 测试函数: `功能描述>()`
- 综合测试文件: `comprehensive.rs`
- 类型定义测试: `types.rs`
