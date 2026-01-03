//! NexaOS NVM Hypervisor Platform Test Suite
//!
//! 测试 NVM 虚拟机平台组件：
//! - cpu: x86-64 CPU 模拟
//! - hypervisor: VT-x/AMD-V 虚拟化
//! - vm: 虚拟机管理
//! - memory: 内存管理
//! - devices: 设备模拟

pub mod cpu;
pub mod hypervisor;
pub mod vm;
pub mod memory;
pub mod devices;
