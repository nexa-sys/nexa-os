# NexaOS Rootfs 启动流程

本文档详细说明 NexaOS 的完整启动流程，从 Bootloader 到用户空间，遵循标准 Linux 系统的启动模式。

## 启动流程概览

```
┌───────────────────────┐
│ 1. BOOTLOADER STAGE   │
├───────────────────────┤
│ • Loads vmlinuz       │
│ • Loads initramfs     │
│ • Passes kernel args  │
│   (root=/dev/vda1 rw) │
└───────────┬───────────┘
            ▼
┌───────────────────────┐
│ 2. KERNEL INIT        │
├───────────────────────┤
│ • Hardware detection  │
│ • Memory setup        │
│ • Mounts tmpfs @ /    │  <───┐
│ • Unpacks initramfs   │      │
│   into tmpfs          │      │
└───────────┬───────────┘      │
            ▼                  │
┌───────────────────────┐      │
│ 3. INITRAMFS STAGE    │      │
├───────────────────────┤      │
│ • Mounts /proc, /sys  │      │
│ • Starts udev         │      │
│ • Loads kernel modules│      │
│   - block drivers     │      │
│   - filesystem drivers│      │
│ • Waits for root dev  │      │
│   (udevadm settle)    │      │
│ • Runs fsck on root   │      │
│ • Mounts real root @  │      │
│   /sysroot (read-only)│      │
└───────────┬───────────┘      │
            ▼                  │
┌───────────────────────┐      │
│ 4. ROOT SWITCH        │      │
├───────────────────────┤      │
│ • pivot_root /sysroot │      │
│   /sysroot/initrd     │      │
│ • chroot /            │      │
│ • umount /initrd      │──────┘ (Releases initramfs memory)
└───────────┬───────────┘
            ▼
┌───────────────────────┐
│ 5. REAL ROOT STAGE    │
├───────────────────────┤
│ • Remounts / as rw    │
│ • Mounts /usr, /home  │
│ • Starts init process │
│   (/sbin/init)        │
│ • Systemd/sysvinit    │
│   service startup     │
│ • Launches getty      │
└───────────┬───────────┘
            ▼
┌───────────────────────┐
│ 6. USER SPACE         │
├───────────────────────┤
│ • Login prompt        │
│ • User shell          │
│ • Graphical session   │
└───────────────────────┘
```

## 阶段详解

### 阶段 1: Bootloader 阶段

**负责组件**: GRUB (Multiboot2)

**功能**:
- 加载内核镜像 (vmlinuz)
- 加载 initramfs CPIO 归档
- 传递内核命令行参数
- 启用长模式 (64-bit)
- 跳转到内核入口点

**内核命令行参数示例**:
```
root=/dev/vda1 rw rootfstype=ext2 init=/sbin/init loglevel=info
```

**实现位置**: 
- `boot/long_mode.S` - Assembly 引导代码
- `src/main.rs` - Multiboot 入口点

### 阶段 2: Kernel Init 阶段

**负责组件**: `src/lib.rs::kernel_main()`, `src/boot_stages.rs`

**功能**:
1. **硬件初始化**
   - TSC 频率检测
   - CPU 功能检测
   - 浮点单元 (FPU/SSE) 启用

2. **内存管理设置**
   - 解析 Multiboot 内存映射
   - 启用 NX (No-Execute) 位
   - 设置页表和虚拟内存
   - Identity mapping 前 4GB

3. **核心数据结构**
   - GDT (Global Descriptor Table) 初始化
   - IDT (Interrupt Descriptor Table) 设置
   - GS 寄存器基地址设置

4. **Initramfs 解包**
   - 从 Multiboot 模块加载 CPIO 归档
   - 解析 CPIO newc 格式
   - 将文件内容映射到内存
   - 构建文件系统索引

5. **子系统初始化**
   - 认证系统 (`auth::init()`)
   - IPC 机制 (`ipc::init()`)
   - 信号处理 (`signal::init()`)
   - 管道系统 (`pipe::init()`)
   - 进程调度器 (`scheduler::init()`)
   - 文件系统 (`fs::init()`)
   - Init 系统 (`init::init()`)

**启动配置解析**:
```rust
// 从内核命令行解析配置
boot_stages::parse_boot_config(cmdline);
```

支持的参数:
- `root=` - 根设备路径
- `rootfstype=` - 根文件系统类型
- `rootflags=` - 挂载选项 (rw/ro)
- `init=` - Init 程序路径
- `emergency` / `single` / `1` - 紧急模式

### 阶段 3: Initramfs 阶段

**负责组件**: `src/boot_stages.rs::initramfs_stage()`

**功能**:

1. **挂载虚拟文件系统**
   ```rust
   mount_proc()  // 挂载 /proc
   mount_sys()   // 挂载 /sys
   mount_dev()   // 挂载 /dev
   ```

   创建的目录结构:
   ```
   /proc/              - 进程信息伪文件系统
   /sys/               - 系统和设备信息
     /sys/block/       - 块设备信息
     /sys/class/       - 设备类
     /sys/devices/     - 设备层次结构
     /sys/kernel/      - 内核参数
   /dev/               - 设备节点
     /dev/null
     /dev/zero
     /dev/console
   ```

2. **设备检测和等待**
   ```rust
   wait_for_root_device(root_dev)
   ```
   
   在真实系统中，这会:
   - 启动 udev 守护进程
   - 加载内核模块 (块设备驱动、文件系统驱动)
   - 等待根设备出现 (`udevadm settle`)
   - 验证设备可访问性

3. **文件系统检查**
   - 运行 fsck 检查根设备
   - 记录任何错误或警告

4. **挂载真实根**
   ```rust
   mount_real_root()
   ```
   
   - 在 `/sysroot` 创建挂载点
   - 以只读模式挂载根设备
   - 验证挂载成功

### 阶段 4: Root Switch 阶段

**负责组件**: `src/boot_stages.rs::pivot_to_real_root()`

**功能**:

1. **Pivot Root 操作**
   ```c
   // 伪代码，展示标准 Linux pivot_root 流程
   pivot_root("/sysroot", "/sysroot/initrd");
   chdir("/");
   ```

   这个操作:
   - 将 `/sysroot` 设为新的根文件系统
   - 将旧根移动到 `/sysroot/initrd`
   - 允许后续卸载 initramfs

2. **移动挂载点**
   ```bash
   # 将虚拟文件系统移到新根
   mount --move /proc /sysroot/proc
   mount --move /sys /sysroot/sys
   mount --move /dev /sysroot/dev
   ```

3. **清理 Initramfs**
   ```bash
   # 切换到新根后
   umount /initrd
   ```
   
   释放 initramfs 占用的内存

### 阶段 5: Real Root 阶段

**负责组件**: `src/boot_stages.rs::start_real_root_init()`

**功能**:

1. **重新挂载根为读写**
   ```rust
   if config.root_options == Some("rw") {
       remount_root_rw();
   }
   ```

2. **挂载其他文件系统**
   - 解析 `/etc/fstab`
   - 挂载 `/usr`, `/home`, `/var` 等
   - 验证关键文件系统可用

3. **启动 Init 进程**
   
   Init 搜索路径 (优先级顺序):
   ```rust
   static INIT_PATHS: &[&str] = &[
       "/sbin/ni",      // Nexa Init (primary)
       "/sbin/init",    // Traditional init
       "/etc/init",     // Alternative location
       "/bin/init",     // Fallback
       "/bin/sh",       // Emergency shell
   ];
   ```

4. **服务启动**
   - 读取 `/etc/inittab` 或 systemd units
   - 按依赖顺序启动服务
   - 启动 getty (登录提示符)

### 阶段 6: User Space 阶段

**负责组件**: Init 进程 (`/sbin/init` 或 `/bin/sh`)

**功能**:

1. **登录管理**
   - Getty 启动登录提示符
   - 用户认证
   - 启动用户 shell

2. **服务管理**
   - 守护进程监控
   - 服务重启 (respawn)
   - 运行级别切换

3. **用户会话**
   - Shell 环境设置
   - 执行用户命令
   - 图形会话 (如果配置)

## 关键原则

### 1. Initramfs 是临时的
- 仅存在于准备真实根挂载的阶段
- 包含必要的工具和驱动
- 在 pivot_root 后释放内存

### 2. 真实根是持久的
- 位于物理存储 (SSD/HDD)
- 包含完整的操作系统
- 用户数据和配置

### 3. 内存安全
- Initramfs 内存在 pivot_root 后回收
- 减少内存占用
- 提高系统性能

### 4. 故障回退安全
- 每个阶段都有错误处理
- 失败时进入紧急 shell
- 允许手动恢复

## 故障处理

### 紧急模式

当启动失败时，系统进入紧急模式:

```rust
boot_stages::enter_emergency_mode(reason)
```

**紧急模式功能**:
1. 显示失败原因
2. 提供诊断信息
3. 启动紧急 shell
4. 允许手动修复

**示例输出**:
```
==========================================================
EMERGENCY MODE: System cannot complete boot
Reason: Root device /dev/vda1 not found
==========================================================

The system encountered a critical error during boot.
You may attempt manual recovery or inspect the system.

Available actions:
  - Inspect /sys/block for available block devices
  - Check kernel log for error messages
  - Type 'exit' to attempt boot continuation

nexa-os emergency shell>
```

### 紧急模式操作

在紧急 shell 中，您可以:

1. **检查可用设备**
   ```bash
   ls /sys/block
   cat /sys/block/*/dev
   ```

2. **加载缺失的驱动**
   ```bash
   modprobe virtio_blk
   modprobe ext4
   ```

3. **手动挂载设备**
   ```bash
   mount /dev/vda1 /sysroot
   ```

4. **运行文件系统检查**
   ```bash
   fsck /dev/vda1
   ```

5. **继续启动**
   ```bash
   exit
   ```

## 实现状态

### 已实现 ✅
- Bootloader 集成 (GRUB/Multiboot2)
- Kernel Init 阶段完整实现
- Initramfs 解包和文件系统
- 虚拟文件系统创建 (/proc, /sys, /dev)
- 启动配置解析
- Init 进程启动
- 紧急模式框架

### 部分实现 ⚙️
- 设备等待 (简化版本)
- Root 设备检测 (模拟)
- Pivot root (框架，未完全实现)

### 待实现 ❌
- 实际 udev 功能
- 内核模块加载
- 文件系统检查 (fsck)
- 真实块设备驱动
- Pivot root 系统调用
- Root 重新挂载

## 配置示例

### 内核命令行

**基本启动** (使用 initramfs):
```
loglevel=info
```

**从块设备启动**:
```
root=/dev/vda1 rootfstype=ext2 rw loglevel=info
```

**紧急模式**:
```
emergency loglevel=debug
```

**自定义 init**:
```
init=/bin/sh root=/dev/vda1 rw
```

### GRUB 配置

`etc/grub.cfg`:
```grub
menuentry "NexaOS" {
    multiboot2 /boot/nexa-os.elf root=/dev/vda1 rw
    module2 /boot/initramfs.cpio
    boot
}
```

## 开发者注意事项

### 添加新的启动阶段

1. 在 `BootStage` 枚举中添加新阶段
2. 实现阶段函数
3. 在 `kernel_main()` 中调用
4. 添加错误处理

### 调试启动问题

1. **增加日志级别**
   ```
   loglevel=debug
   ```

2. **检查串口输出**
   ```bash
  ./ndk run | tee boot.log
   ```

3. **使用 GDB 调试**
   ```bash
   qemu-system-x86_64 -s -S ...
   gdb target/x86_64-nexaos/release/nexa-os
   (gdb) target remote :1234
   (gdb) break kernel_main
   (gdb) continue
   ```

### 性能考虑

- Initramfs 大小应保持最小 (< 10MB 理想)
- Pivot root 应尽快执行以释放内存
- 延迟加载非关键模块
- 并行启动服务 (未来实现)

## 参考资料

- [Linux Boot Process](https://www.kernel.org/doc/html/latest/admin-guide/initrd.html)
- [pivot_root(2) man page](https://man7.org/linux/man-pages/man2/pivot_root.2.html)
- [initramfs buffer format](https://www.kernel.org/doc/html/latest/filesystems/ramfs-rootfs-initramfs.html)
- [GRUB Multiboot2 Specification](https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html)
