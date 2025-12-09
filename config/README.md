# NexaOS Build Configuration

This directory contains all build configuration files for NexaOS.

## Configuration Files

| File | Description |
|------|-------------|
| `build.yaml` | Main build configuration and settings |
| `modules.yaml` | Kernel modules configuration (drivers, filesystems, etc.) |
| `programs.yaml` | Userspace programs configuration |
| `libraries.yaml` | Userspace shared libraries configuration |
| `features.yaml` | Compile-time feature flags for conditional compilation |
| `qemu.yaml` | QEMU emulator configuration |

## QEMU Configuration (qemu.yaml)

The `qemu.yaml` file configures the QEMU emulator settings. The build system
uses this to dynamically generate `build/run-qemu.sh`.

### Key Settings

- **Machine**: Architecture, memory, CPU cores
- **Boot**: UEFI or legacy BIOS mode
- **Display**: VGA type and display backend
- **Storage**: ISO, rootfs, and swap device configuration
- **Network**: Network mode (auto/user/bridge/tap)
- **Debug**: GDB server settings

### QEMU Profiles

| Profile | Description |
|---------|-------------|
| `default` | Standard development setup |
| `minimal` | Fast boot, reduced features |
| `debug` | GDB server enabled, pause on start |
| `headless` | No display, serial console only |
| `full` | All features, more resources |

### Managing QEMU Configuration

```bash
./ndk qemu config           # Show current configuration
./ndk qemu profiles         # List available profiles
./ndk qemu generate         # Regenerate run-qemu.sh
./ndk qemu generate -p debug  # Generate with debug profile
```

## Feature Flags (features.yaml)

The `features.yaml` file controls compile-time feature flags for the kernel.
This allows you to enable or disable specific kernel features like network protocols,
filesystem support, and debug options.

### Network Protocol Features

| Feature | Description | Dependencies |
|---------|-------------|--------------|
| `ipv4` | IPv4 protocol support | ethernet |
| `udp` | UDP/IP protocol | ipv4 |
| `tcp` | TCP/IP protocol (in development) | ipv4 |
| `arp` | ARP protocol | ethernet |
| `ethernet` | Ethernet frame support | - |
| `dns` | DNS resolver | udp |
| `dhcp` | DHCP client | udp |
| `netlink` | Netlink socket support | - |

### Enabling/Disabling Features

Edit `features.yaml` to enable or disable specific features:

```yaml
network:
  tcp:
    enabled: false   # Disable TCP support
  udp:
    enabled: true    # Enable UDP support
```

Or use environment variables at build time:

```bash
FEATURE_TCP=false ./ndk kernel    # Disable TCP
FEATURE_UDP=true ./ndk kernel     # Enable UDP
```

### Feature CLI Commands

```bash
./ndk features list           # List all features
./ndk features enable tcp     # Enable TCP
./ndk features disable debug  # Disable debug features
./ndk features presets        # List available presets
./ndk features apply minimal  # Apply a preset
```

### Feature Presets

The configuration supports presets for quick configuration:

- `full_network`: All protocols enabled (default)
- `minimal_network`: UDP only (for basic services)
- `no_network`: No network support
- `development`: All debug features enabled
- `production`: Security features, no debug

## Module Categories

Kernel modules are organized by type:
- **filesystem**: File system drivers (ext2, ext3, ext4)
- **block**: Block device drivers (virtio_blk)
- **memory**: Memory management (swap)
- **network**: Network drivers (e1000, virtio_net)

## Enabling/Disabling Modules

Edit `modules.yaml` to enable or disable specific modules:

```yaml
filesystem:
  ext2:
    enabled: true    # Enable ext2 support
  ext3:
    enabled: false   # Disable ext3 support
```

## Build Profiles

The `build.yaml` file supports multiple profiles:
- `default`: Standard build with common modules
- `minimal`: Minimal boot with essential modules only
- `full`: All modules enabled

## Usage

Build scripts automatically read these configuration files:

```bash
./ndk full                        # Full build with default profile
BUILD_PROFILE=minimal ./ndk full  # Use minimal profile
./ndk dev                         # Build and run in QEMU
./ndk run                         # Run in QEMU (requires built system)
./ndk run --debug                 # Run with GDB server
```
