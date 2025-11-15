# UEFI 兼容性回退驱动 - 快速参考

## 概述

NexaOS 的 UEFI 兼容性回退驱动允许在 UEFI Boot Services 退出后继续访问硬件设备信息。

## 四阶段流程

```
Bootloader (UEFI) → ExitBootServices() → 内核接管 → 用户态服务
     ↓                      ↓                 ↓              ↓
  采集设备信息          Boot Services失效    映射MMIO     访问设备
  保存到BootInfo         硬件状态保持        注册设备节点  初始化驱动
```

## 关键组件

### 1. Bootloader (boot/uefi-loader/src/main.rs)

采集设备信息：
- **GOP**: 图形输出（Framebuffer）
- **Block I/O**: 硬盘控制器
- **Simple Network**: 网卡
- **USB Controllers**: xHCI/EHCI/OHCI（通过 PCI 枚举）

### 2. 内核 (src/uefi_compat.rs)

提供功能：
- 读取 BootInfo 中的设备信息
- 映射设备 MMIO 区域
- 注册设备节点 (/dev/net*, /dev/usb*, /dev/fb0 等)
- 暴露系统调用接口

### 3. 系统调用 (src/syscall.rs)

```rust
SYS_UEFI_GET_COUNTS      (240) // 获取设备计数
SYS_UEFI_GET_FB_INFO     (241) // 获取 Framebuffer 信息
SYS_UEFI_GET_NET_INFO    (242) // 获取网卡信息
SYS_UEFI_GET_BLOCK_INFO  (243) // 获取块设备信息
SYS_UEFI_MAP_NET_MMIO    (244) // 映射网卡 MMIO
SYS_UEFI_GET_USB_INFO    (245) // 获取 USB 控制器信息
SYS_UEFI_GET_HID_INFO    (246) // 获取 HID 设备信息
SYS_UEFI_MAP_USB_MMIO    (247) // 映射 USB MMIO
```

### 4. 用户态服务 (userspace/uefi_compatd.rs)

启动时自动运行的服务：
- 查询所有设备信息
- 映射必要的 MMIO 区域
- 打印设备发现日志
- 为高级驱动提供设备信息

## 设备节点

系统启动后自动创建：

```
/dev/fb0        # Framebuffer (图形输出)
/dev/net0-7     # 网络设备
/dev/block0-7   # 块设备
/dev/usb0-7     # USB 主机控制器
/dev/hid0-7     # HID 输入设备
```

## 使用示例

### 查询设备计数

```rust
use nrlib::{uefi_get_counts, UefiCompatCounts};

let mut counts = UefiCompatCounts::default();
if uefi_get_counts(&mut counts) == 0 {
    println!("Framebuffer: {}", counts.framebuffer);
    println!("Network: {}", counts.network);
    println!("Block: {}", counts.block);
    println!("USB Host: {}", counts.usb_host);
    println!("HID Input: {}", counts.hid_input);
}
```

### 访问 Framebuffer

```rust
use nrlib::{uefi_get_framebuffer, FramebufferInfo};

let mut fb_info = FramebufferInfo::default();
if uefi_get_framebuffer(&mut fb_info) == 0 {
    println!("FB @ {:#x}, {}x{}", 
             fb_info.address, fb_info.width, fb_info.height);
}
```

### 查询 USB 控制器

```rust
use nrlib::{uefi_get_usb_host, UefiUsbHostDescriptor};

let mut usb = UefiUsbHostDescriptor::default();
if uefi_get_usb_host(0, &mut usb) == 0 {
    let type_name = match usb.info.controller_type {
        3 => "xHCI",
        2 => "EHCI",
        1 => "OHCI",
        _ => "Unknown",
    };
    println!("USB0: {} at {:#x}", type_name, usb.mmio_base);
}
```

### 映射 USB MMIO

```rust
use nrlib::uefi_map_usb_mmio;
use core::ptr;

let mmio_ptr = uefi_map_usb_mmio(0);
if !mmio_ptr.is_null() {
    // 读取 xHCI 寄存器
    let cap_length = unsafe { 
        ptr::read_volatile(mmio_ptr as *const u8) 
    };
    println!("xHCI Capability Length: {}", cap_length);
}
```

## 数据结构

### UsbHostInfo

```rust
pub struct UsbHostInfo {
    pub pci_segment: u16,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    pub controller_type: u8,  // 1=OHCI, 2=EHCI, 3=xHCI
    pub port_count: u8,
    pub usb_version: u16,     // 0x0300 = USB 3.0
    pub mmio_base: u64,
    pub mmio_size: u64,
    pub interrupt_line: u8,
}
```

### HidInputInfo

```rust
pub struct HidInputInfo {
    pub device_type: u8,      // 1=keyboard, 2=mouse, 3=combined
    pub protocol: u8,         // 1=keyboard, 2=mouse
    pub is_usb: u8,           // 0=PS/2, 1=USB
    pub usb_host_bus: u8,
    pub usb_host_device: u8,
    pub usb_host_function: u8,
    pub usb_device_addr: u8,
    pub usb_endpoint: u8,
    pub vendor_id: u16,
    pub product_id: u16,
}
```

## 测试

```bash
# 构建并运行
./scripts/build-all.sh
./scripts/run-qemu.sh

# 预期日志输出
[INFO] Registered /dev/fb0 for framebuffer access
[INFO] Registered /dev/usb0 (USB xHCI, ...)
[INFO] Registered service: uefi-compatd -> /sbin/uefi-compatd
[uefi-compatd] framebuffer=1, network=1, block=1, usb_host=1, hid_input=0
[uefi-compatd] usb0 xHCI USB3.0 ports=4 mmio=0xfebc0000+0x10000
[uefi-compatd] initialisation complete
```

## 限制

1. **硬件依赖**: 依赖 UEFI 固件正确配置硬件
2. **USB 状态**: 不保存运行时状态，用户态需重新初始化
3. **PS/2 键鼠**: 通过 Legacy USB Support 模拟，不走 USB 协议
4. **Framebuffer**: 分辨率由 UEFI GOP 决定，不支持模式切换

## 后续改进

- [ ] 完整的 xHCI 驱动实现
- [ ] USB HID 协议解析
- [ ] GPU 驱动和 DRM/KMS 支持
- [ ] IOMMU 隔离和设备安全

## 参考文档

- [完整实现文档](./UEFI_COMPAT_FALLBACK_DRIVER.md)
- [UEFI 网络和 TCP 支持](./UEFI_COMPAT_NETWORK_TCP.md)
- [系统调用参考](./SYSCALL-REFERENCE.md)

## 相关文件

```
boot/boot-info/src/lib.rs           # BootInfo 数据结构
boot/uefi-loader/src/main.rs        # UEFI Bootloader 设备枚举
src/bootinfo.rs                      # BootInfo 访问接口
src/uefi_compat.rs                   # UEFI 兼容层内核实现
src/syscall.rs                       # 系统调用实现
userspace/nrlib/src/lib.rs           # 用户态系统调用包装
userspace/uefi_compatd.rs            # UEFI 兼容服务守护进程
```
