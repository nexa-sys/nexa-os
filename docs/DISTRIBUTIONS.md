# NexaOS Distributions

NexaOS 支持多个发行版/版本，每个版本针对不同的用例进行优化。

## 可用发行版

| 发行版 | 代号 | 描述 | 目标用户 |
|--------|------|------|----------|
| **Desktop** | Aurora | 完整桌面环境，带图形界面 | 开发者、终端用户、工作站 |
| **Server** | Bastion | 无头服务器，优化稳定性和性能 | 系统管理员、数据中心、云 |
| **EXVM** | Horizon | 企业虚拟化平台（类似 ESXi/Proxmox） | 虚拟化管理员、私有云 |
| **VCEN** | Nexus | 虚拟化控制中心，管理多个 EXVM 主机 | 云管理员、MSP、企业 |
| **K8S** | Nautilus | 容器优化 OS，用于 Kubernetes 节点 | DevOps、平台工程师 |

## 使用方法

### 列出发行版

```bash
./ndk dist list
```

### 查看发行版详情

```bash
./ndk dist info <distribution>

# 示例
./ndk dist info exvm
./ndk dist info desktop
```

### 构建特定发行版

```bash
./ndk dist build <distribution> [options]

# 示例
./ndk dist build exvm           # 构建 EXVM
./ndk dist build desktop -r     # 构建 Desktop (release 模式)
./ndk dist build server --iso-only  # 仅生成 ISO
```

### 使用 build 命令构建

```bash
./ndk build full --dist <distribution>

# 示例
./ndk build full --dist exvm
```

## 发行版详解

### NexaOS Desktop (Aurora)

完整的桌面操作系统，包含：
- 图形化 Compositor
- 文件管理器
- 终端模拟器
- 系统设置
- 完整的 USB/蓝牙/音频支持

**内核特性**：smp, graphics, audio, usb, bluetooth, acpi, power_management

**服务**：ni, networkd, dbus, compositorcd, powerd

### NexaOS Server (Bastion)

无头服务器版本，专注于：
- 服务器稳定性
- 网络性能
- SSH 远程管理
- 系统监控

**内核特性**：smp, numa, large_pages, network_offload, acpi

**服务**：ni, networkd, sshd, cron, syslogd

### NexaOS EXVM (Horizon)

企业级虚拟化平台，类似于 VMware ESXi 或 Proxmox VE：
- 完整的 KVM/QEMU 支持
- VT-x/AMD-V 硬件虚拟化
- IOMMU 设备直通
- SR-IOV 网络虚拟化
- Web 管理界面 (NVM)
- REST API

**内核特性**：smp, vt_x, amd_v, iommu, large_pages, numa, sr_iov

**服务**：ni, networkd, nvm-server

**注意**：这是一个专用虚拟化管理程序，不是通用操作系统。

### NexaOS VCEN (Nexus)

虚拟化控制中心，用于：
- 管理多个 EXVM 主机
- 集群调度
- 高可用性 (HA)
- 资源池管理
- 企业功能：RBAC、审计、合规

**内核特性**：smp, numa, network_offload

**服务**：ni, networkd, vcen-server, postgresqld, redis

### NexaOS K8S (Nautilus)

Kubernetes 优化版本：
- 最小化系统占用
- 容器运行时优先
- 网络优化（CNI 兼容）
- 只读根文件系统
- 快速启动

**内核特性**：smp, namespaces, cgroups, network_namespaces, seccomp, large_pages

**服务**：ni, networkd, containerd, kubeletd

## 配置文件

发行版配置位于 `config/distributions.yaml`。

每个发行版定义包括：
- `name`: 发行版名称
- `codename`: 代号
- `description`: 描述
- `target_users`: 目标用户
- `kernel.features`: 启用的内核特性
- `modules`: 包含的内核模块
- `programs`: 包含的用户空间程序
- `services`: 启用的服务
- `rootfs`: 根文件系统配置
- `iso`: ISO 生成设置

## 环境变量

构建时会设置以下环境变量：

| 变量 | 描述 |
|------|------|
| `NEXAOS_DIST` | 发行版 ID (desktop, server, exvm, vcen, k8s) |
| `NEXAOS_DIST_NAME` | 发行版全名 |

这些变量可在构建脚本中使用来条件性地包含组件。

## 输出路径

每个发行版的输出位于不同路径：

- Desktop: `build/dist/desktop/NexaOS-Desktop-{version}-{arch}.iso`
- Server: `build/dist/server/NexaOS-Server-{version}-{arch}.iso`
- EXVM: `build/dist/exvm/NexaOS-EXVM-{version}-{arch}.iso`
- VCEN: `build/dist/vcen/NexaOS-VCEN-{version}-{arch}.iso`
- K8S: `build/dist/k8s/NexaOS-K8S-{version}-{arch}.iso`

## 扩展发行版

要添加新发行版：

1. 编辑 `config/distributions.yaml`
2. 添加新的发行版定义
3. 运行 `./ndk dist list` 验证

```yaml
distributions:
  my_edition:
    name: "NexaOS My Edition"
    codename: "Custom"
    description: "My custom NexaOS distribution"
    target_users: ["my_users"]
    
    kernel:
      features:
        - smp
        - ...
        
    modules:
      filesystem: [ext2, ext4]
      ...
      
    programs:
      core: all
      ...
      
    services:
      - ni
      - ...
```
