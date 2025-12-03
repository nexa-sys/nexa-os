# NexaOS Quick Reference Card

> **Version**: 1.0 Production  
> **Platform**: x86_64  
> **Type**: Hybrid Kernel OS (POSIX.1-2017 compliant)  
> **Updated**: 2025-12-04

---

## Table of Contents

1. [Essential Commands](#essential-commands)
2. [Build Process](#build-process)
3. [System Call Reference](#system-call-reference)
4. [Shell Commands](#shell-commands)
5. [Keyboard Shortcuts](#keyboard-shortcuts)
6. [Common Tasks](#common-tasks)
7. [Quick Debugging](#quick-debugging)

---

## Essential Commands

### Build & Run Complete System
```bash
./scripts/build.sh all      # Complete build (kernel + rootfs + ISO)
./scripts/run-qemu.sh       # Run in QEMU with serial output
```

### Build Individual Components
```bash
./scripts/build.sh kernel       # Kernel only
./scripts/build.sh userspace    # nrlib + userspace binaries
./scripts/build.sh modules      # Kernel modules
./scripts/build.sh initramfs    # Initial RAM filesystem
./scripts/build.sh rootfs       # ext2 root filesystem
./scripts/build.sh iso          # Bootable ISO

# Combine steps:
./scripts/build.sh kernel iso               # Just kernel and ISO
./scripts/build.sh userspace rootfs iso     # Userspace chain
```

### Environment Variables
```bash
BUILD_TYPE=release ./scripts/build.sh all   # Release build (faster, smaller)
BUILD_TYPE=debug ./scripts/build.sh all     # Debug build (default, stable)
LOG_LEVEL=info ./scripts/build.sh kernel    # Set kernel log level
```

### Monitor & Debug
```bash
tail -f /tmp/qemu-serial.log    # Monitor kernel logs (in another terminal)
./scripts/run-qemu.sh -gdb      # Run QEMU with GDB support
```

---

## Build Process

### Complete Build Order
1. Compile kernel: `./scripts/build.sh kernel`
2. Build nrlib: (included in userspace)
3. Build userspace: `./scripts/build.sh userspace`
4. Build modules: `./scripts/build.sh modules`
5. Create initramfs: `./scripts/build.sh initramfs`
6. Create rootfs: `./scripts/build.sh rootfs`
7. Package ISO: `./scripts/build.sh iso`

**Or all at once**: `./scripts/build.sh all`

### Build Output Artifacts
- **Kernel**: `target/x86_64-nexaos/debug/nexa-os` (main binary, debug default)
- **Initramfs**: `build/initramfs.cpio` (bootstrap environment)
- **Root FS**: `build/rootfs.ext2` (full system filesystem)
- **ISO**: `target/iso/nexaos.iso` (bootable image)

---

## System Call Reference

### Most Used System Calls (Top 20)

| # | Syscall | Signature | Returns | Error |
|---|---------|-----------|---------|-------|
| 0 | `read` | `read(fd, buf, count)` | bytes read | EBADF, EFAULT |
| 1 | `write` | `write(fd, buf, count)` | bytes written | EBADF, EFAULT |
| 2 | `open` | `open(path, flags, mode)` | fd | ENOENT, EACCES |
| 3 | `close` | `close(fd)` | 0 | EBADF |
| 4 | `stat` | `stat(path, buf)` | 0 | ENOENT, EACCES |
| 5 | `fstat` | `fstat(fd, buf)` | 0 | EBADF |
| 32 | `dup` | `dup(oldfd)` | newfd | EBADF, EMFILE |
| 33 | `dup2` | `dup2(oldfd, newfd)` | newfd | EBADF |
| 39 | `getpid` | `getpid()` | pid | - |
| 40 | `sendto` | `sendto(sock, buf, len, flags, addr, addrlen)` | bytes | EBADF |
| 41 | `recvfrom` | `recvfrom(sock, buf, len, flags, addr, addrlen)` | bytes | EBADF |
| 57 | `fork` | `fork()` | child_pid (parent), 0 (child) | EAGAIN, ENOMEM |
| 59 | `execve` | `execve(path, argv, envp)` | - (never returns) | ENOENT, EACCES |
| 60 | `exit` | `exit(status)` | - (never returns) | - |
| 61 | `wait4` | `wait4(pid, status, opts, rusage)` | pid | ECHILD, EINTR |
| 62 | `kill` | `kill(pid, sig)` | 0 | ESRCH, EPERM |
| 110 | `getppid` | `getppid()` | parent_pid | - |
| 22 | `pipe` | `pipe(pipefd[2])` | 0 | EMFILE, ENFILE |
| 14 | `ioctl` | `ioctl(fd, cmd, arg)` | varies | EBADF, EINVAL |
| 71 | `fcntl` | `fcntl(fd, cmd, arg)` | varies | EBADF, EINVAL |

### Error Codes

```c
// Check for errors after syscall
if (result < 0) {
    int err = errno;  // TLS variable from nrlib
    switch (err) {
        case ENOENT:  // File not found
        case EACCES:  // Permission denied
        case EBADF:   // Bad file descriptor
        case EFAULT:  // Bad address
        case EINVAL:  // Invalid argument
        case EAGAIN:  // Resource temporarily unavailable
        case ENOMEM:  // Out of memory
        case EPERM:   // Operation not permitted
        // ... handle errors
    }
}
```

### Socket System Calls

| Syscall | Signature | Purpose |
|---------|-----------|---------|
| `socket` | `socket(domain, type, protocol)` | Create socket |
| `bind` | `bind(sock, addr, addrlen)` | Bind to address |
| `listen` | `listen(sock, backlog)` | Listen for connections |
| `accept` | `accept(sock, addr, addrlen)` | Accept connection |
| `connect` | `connect(sock, addr, addrlen)` | Connect to server |
| `send` | `send(sock, buf, len, flags)` | Send data |
| `recv` | `recv(sock, buf, len, flags)` | Receive data |

---

## Shell Commands

### File Operations (9 commands)

```bash
ls [-a] [-l] [path]        # List files (-a=all, -l=long format)
cat <file>                 # Display file contents
stat <file>                # Show file metadata
pwd                        # Print working directory
cd [path]                  # Change directory
mkdir <path>               # Create directory
touch <file>               # Create empty file or update timestamp
rm <file>                  # Remove file
cp <src> <dst>             # Copy file
```

### System Information

```bash
help                       # Show available commands
uname [-a]                 # Print system information
echo [text]                # Print text
whoami                     # Current user name
ps                         # List running processes
date                       # Show current date/time
uptime                     # System uptime
```

### Process Management

```bash
ps                         # List processes
kill <pid>                 # Send signal to process
bg                         # Run job in background
fg                         # Bring job to foreground
```

### User Management

```bash
users                      # List logged-in users
login <user>               # Log in as user
logout                     # Log out
adduser [-a] <user>        # Add user (-a for admin)
```

### Network Tools

```bash
ifconfig                   # Network interface configuration
nslookup <domain>          # DNS lookup
ping <host>                # Test connectivity
netstat                    # Network statistics
```

### Advanced

```bash
ipc-create [name]          # Create IPC channel
ipc-send <ch> <msg>        # Send message over IPC
ipc-recv <ch>              # Receive message from IPC
```

---

## Keyboard Shortcuts

### Line Editing

| Key | Action |
|-----|--------|
| Tab | Command/path completion |
| Ctrl-C | Cancel current line |
| Ctrl-D | Exit shell (on empty line) |
| Ctrl-U | Clear entire line |
| Ctrl-W | Delete previous word |
| Ctrl-L | Clear screen |
| Backspace | Delete previous character |
| Delete | Delete character at cursor |
| Left/Right | Move cursor |
| Home | Go to line start |
| End | Go to line end |

### Command History

| Key | Action |
|-----|--------|
| Up Arrow | Previous command |
| Down Arrow | Next command |

---

## Common Tasks

### Compile a User Program

```bash
# Write program in userspace/myapp.rs
# Build:
./scripts/build-userspace.sh

# It gets copied to rootfs during build
# Run in NexaOS shell:
./myapp [args]
```

### Add a New Syscall

1. Define syscall number in `src/syscalls/mod.rs`
2. Implement handler in `src/syscalls/[category].rs`
3. Add wrapper in `userspace/nrlib/src/libc_compat.rs` (optional)
4. Rebuild: `cargo build --release --target x86_64-nexaos.json`

### Debug I/O Hangs

1. Enable debug logging: Add `-loglevel=debug` to GRUB boot parameters
2. Monitor serial output: `tail -f /tmp/qemu-serial.log`
3. Check for deadlocks: Look for repeated messages
4. Run with debug build: `./scripts/build-rootfs-debug.sh`

### Test a Shell Command

1. Run system: `./scripts/run-qemu.sh`
2. In shell, type: `your-command [args]`
3. Check exit code: `echo $?`
4. Debug: Use `ps` to see processes, `kill` to terminate

---

## Quick Debugging

### Kernel Panic

**Symptom**: System crashes with error message

**Steps**:
1. Enable debug: Boot with `loglevel=debug`
2. Watch serial: `tail -f /tmp/qemu-serial.log`
3. Look for: Function name, line number, error code
4. Search docs: Use error name (e.g., "General Protection Fault")

### System Hangs

**Symptom**: System stops responding

**Steps**:
1. Check logs: `tail -f /tmp/qemu-serial.log`
2. Press Ctrl-A then X to exit QEMU
3. Look for last syscall or interrupt
4. Try debug build: `./scripts/build-rootfs-debug.sh`

### Syscall Fails

**Symptom**: Program returns -errno

**Steps**:
1. Check errno value (e.g., -2 = ENOENT)
2. Look up in [System Call Reference](#system-call-reference)
3. Verify arguments are valid
4. Check file permissions with `stat`

### Shell Command Hangs

**Symptom**: Command doesn't return

**Steps**:
1. Press Ctrl-C to interrupt
2. Check if process is stuck: `ps`
3. Kill process: `kill <pid>`
4. Check for recursive I/O: Enable debug logging

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `PATH` | Command search paths |
| `HOME` | User home directory |
| `USER` | Current username |
| `PWD` | Current working directory |
| `OLDPWD` | Previous working directory |
| `SHELL` | Shell executable |
| `TERM` | Terminal type |

---

## File Permissions & Access

```bash
stat <file>          # Show permissions (owner/group/mode)
chmod <mode> <file>  # Change file mode
```

**Permission bits**: rwx = read/write/execute for owner/group/other

---

**More Information**: See [docs/en/README.md](README.md) for complete documentation  
**Syscall Details**: [SYSCALL-REFERENCE.md](SYSCALL-REFERENCE.md)  
**Build Guide**: [BUILD-SYSTEM.md](BUILD-SYSTEM.md)

