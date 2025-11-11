# NexaOS Quick Reference Card

> **Version**: 1.0 Production  
> **Platform**: x86_64  
> **Type**: Hybrid Kernel OS

## Essential Commands

### Build & Run
```bash
./scripts/build-all.sh    # Build complete system
./scripts/run-qemu.sh     # Run in QEMU
cargo build --release     # Kernel only
```

### Build Components
```bash
./scripts/build-userspace.sh   # Initramfs (~40 KB)
./scripts/build-rootfs.sh      # Ext2 root (50 MB)
./scripts/build-iso.sh         # Bootable ISO
```

## System Call Quick Reference

### Most Used (Top 15)

| # | Syscall | Signature | Use Case |
|---|---------|-----------|----------|
| 0 | read | `read(fd, buf, count)` | Read from file/stdin |
| 1 | write | `write(fd, buf, count)` | Write to file/stdout |
| 2 | open | `open(path, flags, mode)` | Open file |
| 3 | close | `close(fd)` | Close file descriptor |
| 39 | getpid | `getpid()` | Get process ID |
| 57 | fork | `fork()` | Create child process |
| 59 | execve | `execve(path, argv, envp)` | Execute program |
| 60 | exit | `exit(status)` | Terminate process |
| 61 | wait4 | `wait4(pid, status, opts, ...)` | Wait for child |
| 22 | pipe | `pipe(pipefd[2])` | Create pipe |
| 4 | stat | `stat(path, buf)` | Get file info |
| 62 | kill | `kill(pid, sig)` | Send signal |
| 110 | getppid | `getppid()` | Get parent PID |
| 32 | dup | `dup(oldfd)` | Duplicate FD |
| 33 | dup2 | `dup2(oldfd, newfd)` | Dup to specific FD |

### Error Handling
```c
// Syscall returns -errno on error
if (syscall(...) < 0) {
    int err = errno;  // From TLS in nrlib
    // ENOENT, EINVAL, EBADF, EAGAIN, etc.
}
```

## Shell Commands (19 total)

### File Operations
```bash
ls [-a] [-l] [path]    # List files
cat <file>             # Display file
stat <file>            # File metadata
pwd                    # Current directory
cd [path]              # Change directory
mkdir <path>           # Create directory
```

### System Info
```bash
help                   # Show help
uname [-a]             # System info
echo [text]            # Print text
whoami                 # Current user
ps                     # Process list
```

### User Management
```bash
users                  # List users
login <user>           # Log in
logout                 # Log out
adduser [-a] <user>    # Add user (-a = admin)
```

### IPC
```bash
ipc-create             # Create channel
ipc-send <ch> <msg>    # Send message
ipc-recv <ch>          # Receive message
```

### Shortcuts
```
Tab         - Command/path completion
Ctrl-C      - Cancel line
Ctrl-D      - Exit shell (empty line)
Ctrl-U      - Clear line
Ctrl-W      - Delete word
Ctrl-L      - Clear screen
Backspace   - Delete char
```

## Boot Stages

```
1. Bootloader       GRUB loads kernel + initramfs       ~100ms
2. Kernel Init      Hardware setup, memory, GDT/IDT    ~200ms
3. Initramfs        Unpack CPIO, mount virtfs           ~50ms
4. Root Switch      Mount ext2, pivot_root             ~100ms
5. Real Root        Start init (PID 1)                 ~150ms
6. User Space       Getty → Login → Shell              ongoing
─────────────────────────────────────────────────────────────
Total boot time: ~600ms (QEMU with KVM)
```

## Memory Layout (Per Process)

```
0x1000000   ┌─────────────────────┐  Top of user space
            │  Dynamic Linker     │  6 MB (ld-linux.so)
0xA00000    ├─────────────────────┤  INTERP_BASE
            │  Stack (↓)          │  2 MB, grows down
0x800000    ├─────────────────────┤  STACK_BASE
            │  Heap (↑)           │  2 MB, grows up
0x600000    ├─────────────────────┤  HEAP_BASE
            │  .data, .bss        │  Initialized/uninitialized data
            ├─────────────────────┤
            │  .text              │  Code segment
0x400000    ├─────────────────────┤  USER_VIRT_BASE
            │  Guard page         │  NULL dereference protection
0x000000    └─────────────────────┘
```

## File System Structure

```
/
├── bin/          User binaries (sh, login)
├── sbin/         System binaries (ni, getty, init→ni)
├── etc/          Configuration (inittab, motd)
├── lib64/        Shared libraries (ld-linux.so)
├── dev/          Device nodes (runtime)
├── proc/         Process info (virtual)
├── sys/          System info (virtual)
├── tmp/          Temporary files
├── var/          Variable data (logs, run)
├── home/         User home directories
└── root/         Root user home
```

## Key Files

### Configuration
- `/etc/inittab` - Init system services
- `/etc/ni/ni.conf` - Init system config
- `/etc/motd` - Message of the day

### Boot
- `boot/long_mode.S` - Assembly bootstrap
- `linker.ld` - Kernel linker script
- `x86_64-nexaos.json` - Custom target

### Build Artifacts
- `target/x86_64-nexaos/release/nexa-os` - Kernel ELF
- `build/initramfs.cpio` - Early boot FS
- `build/rootfs.ext2` - Root filesystem
- `dist/nexaos.iso` - Bootable ISO

## Debugging

### Kernel Logs (Serial Console)
```bash
# In QEMU, serial output goes to stdio
[timestamp] [LEVEL] message

# Log levels: FATAL, ERROR, WARN, INFO, DEBUG
# Set via kernel cmdline: loglevel=debug
```

### Boot Parameters
```
root=/dev/vda1          # Root device
rootfstype=ext2         # FS type
loglevel=debug          # Log level
init=/path/to/program   # Custom init
```

### GDB Debugging
```bash
# Terminal 1: QEMU with GDB stub
qemu-system-x86_64 -s -S -kernel nexa-os ...

# Terminal 2: GDB
gdb target/x86_64-nexaos/release/nexa-os
(gdb) target remote :1234
(gdb) break kernel_main
(gdb) continue
```

## Architecture Facts

| Component | Details |
|-----------|---------|
| **CPU** | x86_64 long mode (64-bit) |
| **Pages** | 4 KB, 4-level paging |
| **Processes** | Max 32 concurrent |
| **Files** | 16 FDs per process, 64 global |
| **Scheduler** | Round-robin, 10ms time slice |
| **Pipes** | 4 KB buffer, 16 max |
| **IPC** | 32 channels, 32 msgs/channel, 256 bytes/msg |
| **Signals** | Full POSIX (32 signals) |
| **Boot** | Multiboot2 + GRUB |

## Error Codes (Common)

```c
#define EPERM   1    // Operation not permitted
#define ENOENT  2    // No such file or directory
#define ESRCH   3    // No such process
#define EINTR   4    // Interrupted system call
#define EBADF   9    // Bad file number
#define ECHILD  10   // No child processes
#define EAGAIN  11   // Try again
#define ENOMEM  12   // Out of memory
#define EACCES  13   // Permission denied
#define EINVAL  22   // Invalid argument
#define EMFILE  24   // Too many open files
```

## Performance

| Metric | Value |
|--------|-------|
| Boot time | ~600ms (QEMU) |
| Context switch | ~5 μs |
| Syscall (fast) | ~500 ns |
| Fork | ~50 μs |
| Kernel size | ~2 MB |
| Per-process overhead | ~100 KB |

## Development

### Prerequisites
```bash
rustup override set nightly
rustup component add rust-src llvm-tools-preview
sudo apt install build-essential lld grub-pc-bin \
                 xorriso qemu-system-x86 e2fsprogs
```

### Workflow
```bash
# Edit code
vim src/syscall.rs

# Build & test
./scripts/build-all.sh
./scripts/run-qemu.sh

# Verify
# - Check serial output for errors
# - Test in shell
# - Verify syscalls work
```

## Documentation

- `docs/SYSTEM-OVERVIEW.md` - Complete system guide
- `docs/ARCHITECTURE.md` - Technical architecture
- `docs/BUILD-SYSTEM.md` - Build process
- `docs/SYSCALL-REFERENCE.md` - All 38+ syscalls
- `docs/zh/getting-started.md` - Setup guide (中文)

## Links

- GitHub: https://github.com/nexa-sys/nexa-os
- Issues: https://github.com/nexa-sys/nexa-os/issues
- Docs: https://github.com/nexa-sys/nexa-os/tree/main/docs

---

**Quick Help**: Run `help` in shell for command list, or see `docs/` for full documentation.
