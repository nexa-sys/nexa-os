# NexaOS Build Configuration

This directory contains all build configuration files for NexaOS.

## Configuration Files

| File | Description |
|------|-------------|
| `build.yaml` | Main build configuration and settings |
| `modules.yaml` | Kernel modules configuration (drivers, filesystems, etc.) |
| `programs.yaml` | Userspace programs configuration |
| `libraries.yaml` | Userspace shared libraries configuration |

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
./scripts/build.sh all              # Uses default profile
BUILD_PROFILE=minimal ./scripts/build.sh all  # Use minimal profile
```
