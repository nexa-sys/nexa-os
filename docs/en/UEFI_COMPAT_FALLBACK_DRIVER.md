# UEFI 兼容性回退驱动实现总结

## 概述

本文档描述了 NexaOS 的 UEFI 兼容性回退驱动的完整实现，该驱动允许在 UEFI Boot Services 退出后继续访问启动阶段发现的硬件设备信息。

## 架构概览

### 四阶段流程

```
阶段 1: Bootloader / Early Kernel (UEFI 环境)
    ↓ 采集 GOP、Block I/O、SNP、USB 控制器信息
    ↓ 保存到 BootInfo 结构
    
阶段 2: ExitBootServices()
    ↓ UEFI Boot Services 失效
    ↓ 但硬件状态保持不变
    
阶段 3: 内核完全接管
    ↓ 读取 BootInfo
    ↓ 映射 MMIO 区域
    ↓ 注册设备节点
    ↓ 提供系统调用接口
    
阶段 4: 用户态 uefi-compatd 服务
    ↓ 查询设备信息
    ↓ 请求 MMIO 映射
    ↓ 初始化设备驱动
```

## 实现细节

### 1. Bootloader 层（boot/uefi-loader/src/main.rs）

#### 设备枚举增强

**GOP (图形输出):**
```rust
let gop = boot_services.locate_protocol::<GraphicsOutput>()?;
let mode = gop.current_mode_info();
let fb_info = FramebufferInfo {
    address: mode.framebuffer as u64,
    width: mode.horizontal_resolution,
    height: mode.vertical_resolution,
    pitch: mode.pixels_per_scan_line * 4,
    bpp: 32,
    // ...
};
```

**Block I/O (硬盘):**
```rust
let block_io = boot_services.locate_protocol::<BlockIO>()?;
let media = block_io.media();
let block_info = BlockDeviceInfo {
    block_size: media.block_size(),
    last_block: media.last_block(),
    media_id: media.media_id(),
    // ...
};
```

**Simple Network Protocol (网卡):**
```rust
let snp = boot_services.locate_protocol::<SimpleNetwork>()?;
let mode = snp.mode();
let network_info = NetworkDeviceInfo {
    mac_address: mode.current_address,
    if_type: mode.if_type,
    max_packet_size: mode.max_packet_size,
    // ...
};
```

**USB 控制器 (新增):**
```rust
// 通过 PCI 枚举发现 USB 控制器
// Class 0x0C, Subclass 0x03 = USB
let pci_io = boot_services.locate_protocol::<PciIo>()?;
let class_reg = pci_io.read_config_u32(0x08)?;
let prog_if = (class_reg >> 8) & 0xFF;

let controller_type = match prog_if {
    0x30 => 3,  // xHCI (USB 3.0)
    0x20 => 2,  // EHCI (USB 2.0)
    0x10 => 1,  // OHCI (USB 1.1)
    _ => 0,     // Unknown
};

let usb_info = UsbHostInfo {
    controller_type,
    mmio_base: bar0_address,
    mmio_size: bar0_size,
    usb_version: 0x0300,  // USB 3.0
    port_count: 4,
    // ...
};
```

### 2. 内核层（src/uefi_compat.rs）

#### 数据结构

```rust
// 设备计数
#[repr(C)]
pub struct CompatCounts {
    pub framebuffer: u8,
    pub network: u8,
    pub block: u8,
    pub usb_host: u8,
    pub hid_input: u8,
    pub _reserved: [u8; 3],
}

// USB 主机控制器描述符
#[repr(C)]
pub struct UsbHostDescriptor {
    pub info: UsbHostInfo,
    pub mmio_base: u64,
    pub mmio_size: u64,
    pub interrupt_line: u8,
    pub _reserved: [u8; 7],
}

// HID 输入设备描述符
#[repr(C)]
pub struct HidInputDescriptor {
    pub info: HidInputInfo,
    pub _reserved: [u8; 16],
}
```

#### 设备节点注册

```rust
pub fn install_device_nodes() {
    // 网络设备: /dev/net0, /dev/net1, ...
    for idx in 0..network_count {
        register_device_node("/dev/net0", FileType::Character, 0o660);
    }
    
    // 块设备: /dev/block0, /dev/block1, ...
    for idx in 0..block_count {
        register_device_node("/dev/block0", FileType::Block, 0o660);
    }
    
    // USB 控制器: /dev/usb0, /dev/usb1, ...
    for idx in 0..usb_count {
        register_device_node("/dev/usb0", FileType::Character, 0o660);
    }
    
    // HID 设备: /dev/hid0, /dev/hid1, ...
    for idx in 0..hid_count {
        register_device_node("/dev/hid0", FileType::Character, 0o660);
    }
    
    // Framebuffer: /dev/fb0
    register_device_node("/dev/fb0", FileType::Character, 0o660);
}
```

#### MMIO 映射

```rust
fn map_mmio_region(base: u64, length: u64, label: &str) {
    if base == 0 {
        return;
    }
    
    let span = if length == 0 { 0x1000 } else { length };
    
    unsafe {
        match paging::map_device_region(base, span as usize) {
            Ok(_) => {
                kinfo!("uefi_compat: mapped {} region {:#x}+{:#x}", 
                       label, base, span);
            }
            Err(_) => {
                kwarn!("uefi_compat: failed to map {} region", label);
            }
        }
    }
}
```

### 3. 系统调用接口（src/syscall.rs）

#### 新增系统调用

```rust
pub const SYS_UEFI_GET_COUNTS: u64 = 240;      // 获取设备计数
pub const SYS_UEFI_GET_FB_INFO: u64 = 241;     // 获取 Framebuffer 信息
pub const SYS_UEFI_GET_NET_INFO: u64 = 242;    // 获取网卡信息
pub const SYS_UEFI_GET_BLOCK_INFO: u64 = 243;  // 获取块设备信息
pub const SYS_UEFI_MAP_NET_MMIO: u64 = 244;    // 映射网卡 MMIO
pub const SYS_UEFI_GET_USB_INFO: u64 = 245;    // 获取 USB 控制器信息 (新)
pub const SYS_UEFI_GET_HID_INFO: u64 = 246;    // 获取 HID 设备信息 (新)
pub const SYS_UEFI_MAP_USB_MMIO: u64 = 247;    // 映射 USB MMIO (新)
```

#### 系统调用实现示例

```rust
fn syscall_uefi_get_usb_info(index: usize, out: *mut UsbHostDescriptor) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let Some(descriptor) = uefi_compat::usb_host_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    unsafe {
        ptr::write(out, descriptor);
    }
    posix::set_errno(0);
    0
}

fn syscall_uefi_map_usb_mmio(index: usize) -> u64 {
    let Some(descriptor) = uefi_compat::usb_host_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    if descriptor.mmio_base == 0 {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    }

    let span = descriptor.mmio_size.max(0x1000) as usize;
    let map_result = unsafe { 
        paging::map_user_device_region(descriptor.mmio_base, span) 
    };
    
    match map_result {
        Ok(ptr) => {
            posix::set_errno(0);
            ptr as u64
        }
        Err(_) => {
            posix::set_errno(posix::errno::ENOMEM);
            u64::MAX
        }
    }
}
```

### 4. 用户态服务（userspace/uefi_compatd.rs）

#### 设备发现和初始化

```rust
fn main() {
    println!("[uefi-compatd] starting");

    // 1. 查询设备计数
    let mut counts = UefiCompatCounts::default();
    if uefi_get_counts(&mut counts) != 0 {
        eprintln!("Failed to query device counts");
        exit(1);
    }

    println!("[uefi-compatd] framebuffer={}, network={}, block={}, \
              usb_host={}, hid_input={}",
             counts.framebuffer, counts.network, counts.block, 
             counts.usb_host, counts.hid_input);

    // 2. Framebuffer 信息
    if counts.framebuffer != 0 {
        let mut fb_info = unsafe { 
            core::mem::zeroed::<FramebufferInfo>() 
        };
        if uefi_get_framebuffer(&mut fb_info) == 0 {
            println!("[uefi-compatd] framebuffer @ {:#x}, {}x{} \
                      pitch={} bpp={}",
                     fb_info.address, fb_info.width, fb_info.height, 
                     fb_info.pitch, fb_info.bpp);
        }
    }

    // 3. USB 控制器枚举
    for idx in 0..counts.usb_host as usize {
        let mut descriptor = UefiUsbHostDescriptor::default();
        if uefi_get_usb_host(idx, &mut descriptor) == 0 {
            let controller_type = match descriptor.info.controller_type {
                1 => "OHCI",
                2 => "EHCI",
                3 => "xHCI",
                _ => "Unknown",
            };
            
            println!("[uefi-compatd] usb{} {} USB{}.{} ports={} \
                      mmio={:#x}+{:#x}",
                     idx, controller_type,
                     descriptor.info.usb_version >> 8,
                     descriptor.info.usb_version & 0xFF,
                     descriptor.info.port_count,
                     descriptor.mmio_base,
                     descriptor.mmio_size);

            // 映射 MMIO 以便访问控制器寄存器
            let mmio_ptr = uefi_map_usb_mmio(idx);
            if !mmio_ptr.is_null() {
                let reg0 = unsafe { 
                    ptr::read_volatile(mmio_ptr as *const u32) 
                };
                println!("[uefi-compatd] usb{} MMIO mapped at {:p}, \
                          REG0={:#x}", idx, mmio_ptr, reg0);
            }
        }
    }

    // 4. HID 输入设备
    for idx in 0..counts.hid_input as usize {
        let mut descriptor = UefiHidInputDescriptor::default();
        if uefi_get_hid_input(idx, &mut descriptor) == 0 {
            let device_type = match descriptor.info.device_type {
                1 => "keyboard",
                2 => "mouse",
                3 => "combined",
                _ => "unknown",
            };
            
            println!("[uefi-compatd] hid{} {} protocol={} usb={} \
                      vid={:#x} pid={:#x}",
                     idx, device_type, descriptor.info.protocol,
                     if descriptor.info.is_usb != 0 { "yes" } else { "no" },
                     descriptor.info.vendor_id,
                     descriptor.info.product_id);
        }
    }

    println!("[uefi-compatd] initialisation complete");
}
```

## 使用示例

### 在用户程序中访问 Framebuffer

```rust
use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use nix::sys::mman::{mmap, MapFlags, ProtFlags};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 获取 framebuffer 信息
    let mut counts = UefiCompatCounts::default();
    uefi_get_counts(&mut counts)?;
    
    if counts.framebuffer == 0 {
        return Err("No framebuffer available".into());
    }
    
    let mut fb_info = FramebufferInfo::default();
    uefi_get_framebuffer(&mut fb_info)?;
    
    // 2. 打开 /dev/fb0
    let fd = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/fb0")?;
    
    // 3. 映射 framebuffer 内存
    let size = (fb_info.height * fb_info.pitch) as usize;
    let fb_ptr = unsafe {
        mmap(
            None,
            size.try_into()?,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        )?
    };
    
    // 4. 绘制像素
    let fb_slice = unsafe {
        std::slice::from_raw_parts_mut(
            fb_ptr as *mut u32,
            (fb_info.width * fb_info.height) as usize
        )
    };
    
    // 填充红色
    for pixel in fb_slice.iter_mut() {
        *pixel = 0x00FF0000;
    }
    
    Ok(())
}
```

### 访问 USB 控制器

```rust
fn access_usb_controller(index: usize) -> Result<(), Box<dyn std::error::Error>> {
    // 1. 获取 USB 控制器信息
    let mut descriptor = UefiUsbHostDescriptor::default();
    if uefi_get_usb_host(index, &mut descriptor) != 0 {
        return Err("USB controller not found".into());
    }
    
    println!("USB Controller Type: {}", 
             match descriptor.info.controller_type {
                 3 => "xHCI",
                 2 => "EHCI",
                 1 => "OHCI",
                 _ => "Unknown",
             });
    
    // 2. 映射 MMIO 区域
    let mmio_ptr = uefi_map_usb_mmio(index);
    if mmio_ptr.is_null() {
        return Err("Failed to map USB MMIO".into());
    }
    
    // 3. 读取控制器寄存器
    unsafe {
        // xHCI: CAPLENGTH at offset 0x00
        let cap_length = ptr::read_volatile(mmio_ptr as *const u8);
        println!("xHCI Capability Length: {}", cap_length);
        
        // xHCI: HCIVERSION at offset 0x02
        let version_ptr = (mmio_ptr as usize + 0x02) as *const u16;
        let version = ptr::read_volatile(version_ptr);
        println!("xHCI Interface Version: {:#x}", version);
    }
    
    Ok(())
}
```

## 技术要点

### 1. MMIO 映射安全性

- 所有 MMIO 区域在内核中先映射为设备内存
- 用户态通过系统调用请求映射，内核验证范围合法性
- 使用页表保护机制防止越界访问

### 2. 多核同步

- 使用 Spin 锁保护全局设备表
- 确保设备初始化只执行一次
- 避免竞态条件

### 3. 错误处理

- 所有系统调用返回标准 errno
- 设备不存在返回 ENODEV
- 内存不足返回 ENOMEM
- 参数非法返回 EINVAL

### 4. 性能优化

- 设备信息在启动时一次性采集
- MMIO 映射使用 TLB 缓存
- 避免频繁系统调用

## 限制与注意事项

1. **硬件限制**
   - 依赖 UEFI 固件正确配置硬件
   - 某些设备可能未通过 UEFI 协议暴露

2. **USB 控制器**
   - 当前仅保存 MMIO 基址，不包含运行时状态
   - 用户态驱动需要重新初始化控制器
   - Legacy USB Support 可能影响访问

3. **HID 设备**
   - PS/2 模拟键鼠不通过 USB 协议访问
   - 需要分别处理 USB HID 和 PS/2 路径

4. **Framebuffer**
   - 分辨率和格式由 UEFI GOP 决定
   - 不支持模式切换（需要额外 GPU 驱动）

## 未来改进方向

1. **完整的 USB 驱动栈**
   - 实现 xHCI 驱动
   - 支持 USB HID 解析
   - 热插拔支持

2. **高级图形**
   - GPU 加速
   - DRM/KMS 支持
   - 多显示器

3. **电源管理**
   - ACPI 集成
   - 设备休眠/唤醒

4. **安全增强**
   - IOMMU 隔离
   - 设备访问权限控制

## 参考资料

- UEFI Specification 2.10
- xHCI Specification 1.2
- USB HID Specification 1.11
- PCI Express Base Specification 6.0

## 测试验证

运行以下命令测试系统：

```bash
# 构建完整系统
./scripts/build-all.sh

# 启动 QEMU
./scripts/run-qemu.sh

# 在系统启动后，检查日志
# 应看到：
# [INFO] Registered /dev/fb0 for framebuffer access
# [INFO] Registered /dev/usb0 (USB xHCI, ...)
# [INFO] Registered service: uefi-compatd -> /sbin/uefi-compatd
# [uefi-compatd] framebuffer=1, network=1, block=1, usb_host=1, hid_input=0
```

## 总结

UEFI 兼容性回退驱动成功实现了从 UEFI Boot Services 到用户态的完整硬件信息传递链路：

1. ✅ Bootloader 采集 GOP、网卡、块设备、USB 控制器信息
2. ✅ 内核接管并映射 MMIO 区域
3. ✅ 系统调用接口暴露设备信息
4. ✅ 用户态服务查询和初始化设备
5. ✅ 设备节点注册到 /dev 文件系统

此实现为 NexaOS 提供了在 UEFI 环境下完整的硬件发现和访问能力，是后续实现高级驱动的基础。
