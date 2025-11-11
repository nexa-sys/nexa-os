# NexaOS System Call Reference

> **Last Updated**: 2025年11月12日  
> **Total Syscalls**: 38+  
> **POSIX Compliance**: Core syscalls implemented

## Overview

NexaOS implements a comprehensive set of system calls covering process management, file I/O, IPC, signals, authentication, and system control. The syscall interface follows POSIX conventions where applicable, with NexaOS-specific extensions for advanced features.

## Syscall Mechanism

### x86_64 Fast Syscall

**Instruction**: `syscall` (AMD64 fast system call)

**Calling Convention**:
```
Registers:
  RAX: System call number
  RDI: Argument 1
  RSI: Argument 2
  RDX: Argument 3
  R10: Argument 4
  R8:  Argument 5
  R9:  Argument 6

Return:
  RAX: Return value (≥ 0 on success, -errno on error)
```

**Context Saving**:
- User context saved to GS-relative area (GS_DATA[16])
- Kernel stack switched automatically
- Interrupts enabled in kernel mode
- Return via `sysretq`

## System Call Table

### File I/O (8 syscalls)

#### `read` (0)
Read bytes from file descriptor.

**Signature**: `ssize_t read(int fd, void *buf, size_t count)`

**Parameters**:
- `fd`: File descriptor (0=stdin, 1=stdout, 2=stderr, ≥3=user files)
- `buf`: Buffer to read into
- `count`: Maximum bytes to read

**Returns**:
- Number of bytes read (≥0)
- 0 on EOF
- -EBADF if fd invalid
- -EINVAL if buf is NULL

**POSIX**: ✅ Yes

**Example**:
```c
char buf[256];
ssize_t n = read(0, buf, sizeof(buf));  // Read from stdin
```

---

#### `write` (1)
Write bytes to file descriptor.

**Signature**: `ssize_t write(int fd, const void *buf, size_t count)`

**Parameters**:
- `fd`: File descriptor
- `buf`: Buffer to write from
- `count`: Number of bytes to write

**Returns**:
- Number of bytes written (≥0)
- -EBADF if fd invalid
- -EINVAL if buf is NULL

**POSIX**: ✅ Yes

**Example**:
```c
const char *msg = "Hello, world!\n";
write(1, msg, 14);  // Write to stdout
```

---

#### `open` (2)
Open or create a file.

**Signature**: `int open(const char *path, int flags, mode_t mode)`

**Parameters**:
- `path`: File path (null-terminated string)
- `flags`: Open flags (O_RDONLY, O_WRONLY, O_RDWR, O_CREAT, O_NONBLOCK)
- `mode`: File permissions (if O_CREAT)

**Returns**:
- File descriptor (≥3)
- -ENOENT if file not found
- -EINVAL if path is NULL
- -EMFILE if too many open files

**POSIX**: ✅ Yes

**Flags**:
```c
#define O_RDONLY    0x0000
#define O_WRONLY    0x0001
#define O_RDWR      0x0002
#define O_CREAT     0x0100
#define O_NONBLOCK  0x0800
```

**Example**:
```c
int fd = open("/etc/motd", O_RDONLY, 0);
if (fd < 0) {
    perror("open");
}
```

---

#### `close` (3)
Close file descriptor.

**Signature**: `int close(int fd)`

**Parameters**:
- `fd`: File descriptor to close

**Returns**:
- 0 on success
- -EBADF if fd invalid

**POSIX**: ✅ Yes

**Example**:
```c
close(fd);
```

---

#### `stat` (4)
Get file status.

**Signature**: `int stat(const char *path, struct stat *buf)`

**Parameters**:
- `path`: File path
- `buf`: Buffer to fill with file metadata

**Returns**:
- 0 on success
- -ENOENT if file not found
- -EINVAL if path or buf is NULL

**POSIX**: ✅ Yes

**struct stat**:
```c
struct stat {
    uint64_t st_dev;      // Device ID
    uint64_t st_ino;      // Inode number
    uint32_t st_mode;     // File type and permissions
    uint32_t st_nlink;    // Number of hard links
    uint32_t st_uid;      // User ID
    uint32_t st_gid;      // Group ID
    uint64_t st_rdev;     // Device ID (if special file)
    uint64_t st_size;     // File size in bytes
    uint64_t st_blksize;  // Block size
    uint64_t st_blocks;   // Number of blocks
    uint64_t st_atime;    // Last access time
    uint64_t st_mtime;    // Last modification time
    uint64_t st_ctime;    // Last status change time
};
```

**File Types** (in st_mode):
```c
#define S_IFMT   0170000  // Bit mask for file type
#define S_IFREG  0100000  // Regular file
#define S_IFDIR  0040000  // Directory
#define S_IFCHR  0020000  // Character device
#define S_IFBLK  0060000  // Block device
#define S_IFIFO  0010000  // FIFO/pipe
#define S_IFLNK  0120000  // Symbolic link
```

---

#### `fstat` (5)
Get file status by descriptor.

**Signature**: `int fstat(int fd, struct stat *buf)`

**Parameters**:
- `fd`: File descriptor
- `buf`: Buffer to fill

**Returns**:
- 0 on success
- -EBADF if fd invalid
- -EINVAL if buf is NULL

**POSIX**: ✅ Yes

---

#### `lseek` (8)
Reposition read/write file offset.

**Signature**: `off_t lseek(int fd, off_t offset, int whence)`

**Parameters**:
- `fd`: File descriptor
- `offset`: Offset in bytes
- `whence`: Reference point (SEEK_SET, SEEK_CUR, SEEK_END)

**Returns**:
- New offset on success
- -EBADF if fd invalid
- -EINVAL if whence is invalid

**POSIX**: ✅ Yes

**whence values**:
```c
#define SEEK_SET  0  // Beginning of file
#define SEEK_CUR  1  // Current position
#define SEEK_END  2  // End of file
```

---

#### `fcntl` (72)
File control operations.

**Signature**: `int fcntl(int fd, int cmd, ...)`

**Parameters**:
- `fd`: File descriptor
- `cmd`: Command (F_DUPFD, F_GETFL, F_SETFL)
- `...`: Command-specific arguments

**Commands**:
```c
#define F_DUPFD  0  // Duplicate file descriptor
#define F_GETFL  3  // Get file status flags
#define F_SETFL  4  // Set file status flags
```

**Returns**:
- Command-specific value on success
- -EBADF if fd invalid
- -EINVAL if cmd is invalid

**POSIX**: ✅ Yes

---

### Process Control (9 syscalls)

#### `fork` (57)
Create a child process.

**Signature**: `pid_t fork(void)`

**Parameters**: None

**Returns**:
- 0 in child process
- Child PID in parent process
- -EAGAIN if process table full

**POSIX**: ✅ Yes

**Behavior**:
- Duplicates current process
- Child inherits: memory, open files, signal handlers
- Child has new PID, PPID set to parent
- Copy-on-write semantics (planned, currently full copy)

**Example**:
```c
pid_t pid = fork();
if (pid == 0) {
    // Child process
    execve("/bin/sh", argv, envp);
} else if (pid > 0) {
    // Parent process
    wait4(pid, &status, 0, NULL);
}
```

---

#### `execve` (59)
Execute a program.

**Signature**: `int execve(const char *path, char *const argv[], char *const envp[])`

**Parameters**:
- `path`: Path to executable (ELF binary)
- `argv`: Argument vector (NULL-terminated)
- `envp`: Environment vector (NULL-terminated)

**Returns**:
- Does not return on success
- -ENOENT if file not found
- -EINVAL if path is NULL

**POSIX**: ✅ Yes

**Behavior**:
- Replaces current process image with new program
- Preserves PID and PPID
- Closes files with close-on-exec flag
- Resets signal handlers to default

**Example**:
```c
char *argv[] = {"/bin/sh", "-c", "echo hello", NULL};
char *envp[] = {"PATH=/bin", NULL};
execve("/bin/sh", argv, envp);
// Never reached if successful
```

---

#### `exit` (60)
Terminate current process.

**Signature**: `void exit(int status)`

**Parameters**:
- `status`: Exit status code (0-255)

**Returns**: Never returns

**POSIX**: ✅ Yes

**Behavior**:
- Process becomes zombie (state = Zombie)
- Exit status saved for parent's wait4()
- All file descriptors closed
- Children reparented to init (PID 1)
- SIGCHLD sent to parent

---

#### `wait4` (61)
Wait for process state change.

**Signature**: `pid_t wait4(pid_t pid, int *status, int options, struct rusage *rusage)`

**Parameters**:
- `pid`: Process to wait for (-1=any child, >0=specific PID)
- `status`: Exit status returned here (can be NULL)
- `options`: Wait options (WNOHANG, WUNTRACED)
- `rusage`: Resource usage (currently unused)

**Returns**:
- PID of child that changed state
- 0 if WNOHANG and no child ready
- -ECHILD if no children

**POSIX**: ✅ Yes (wait4 is BSD/Linux extension)

**Options**:
```c
#define WNOHANG   1  // Don't block if no child ready
#define WUNTRACED 2  // Report stopped children
```

**Status Macros**:
```c
#define WIFEXITED(status)   (((status) & 0x7F) == 0)
#define WEXITSTATUS(status) (((status) & 0xFF00) >> 8)
#define WIFSIGNALED(status) (((status) & 0x7F) != 0)
#define WTERMSIG(status)    ((status) & 0x7F)
```

---

#### `getpid` (39)
Get process ID.

**Signature**: `pid_t getpid(void)`

**Returns**: Current process PID (always ≥1)

**POSIX**: ✅ Yes

---

#### `getppid` (110)
Get parent process ID.

**Signature**: `pid_t getppid(void)`

**Returns**: Parent process PID (0 if orphan)

**POSIX**: ✅ Yes

---

#### `kill` (62)
Send signal to process.

**Signature**: `int kill(pid_t pid, int sig)`

**Parameters**:
- `pid`: Target process (>0=specific PID, 0=process group, -1=all)
- `sig`: Signal number (SIGTERM, SIGKILL, etc.)

**Returns**:
- 0 on success
- -ESRCH if process not found
- -EINVAL if sig is invalid

**POSIX**: ✅ Yes

---

#### `sched_yield` (24)
Yield CPU to another process.

**Signature**: `int sched_yield(void)`

**Returns**: 0 (always succeeds)

**POSIX**: ✅ Yes

**Behavior**:
- Current process moved to end of ready queue
- Scheduler selects next ready process
- Voluntary context switch

---

### IPC (5 syscalls)

#### `pipe` (22)
Create pipe for IPC.

**Signature**: `int pipe(int pipefd[2])`

**Parameters**:
- `pipefd`: Array to receive [read_fd, write_fd]

**Returns**:
- 0 on success
- -EMFILE if too many open files
- -ENFILE if system pipe limit reached

**POSIX**: ✅ Yes

**Behavior**:
- Creates unidirectional data channel
- `pipefd[0]`: Read end
- `pipefd[1]`: Write end
- 4 KB buffer per pipe
- Blocking I/O by default

**Example**:
```c
int pipefd[2];
pipe(pipefd);
if (fork() == 0) {
    close(pipefd[1]);  // Child reads
    read(pipefd[0], buf, sizeof(buf));
} else {
    close(pipefd[0]);  // Parent writes
    write(pipefd[1], "data", 4);
}
```

---

#### `dup` (32)
Duplicate file descriptor.

**Signature**: `int dup(int oldfd)`

**Parameters**:
- `oldfd`: File descriptor to duplicate

**Returns**:
- New file descriptor (lowest available)
- -EBADF if oldfd invalid
- -EMFILE if too many open files

**POSIX**: ✅ Yes

---

#### `dup2` (33)
Duplicate file descriptor to specific number.

**Signature**: `int dup2(int oldfd, int newfd)`

**Parameters**:
- `oldfd`: Source file descriptor
- `newfd`: Target file descriptor number

**Returns**:
- `newfd` on success
- -EBADF if oldfd invalid

**POSIX**: ✅ Yes

**Behavior**:
- If `newfd` is open, it's closed first
- If `oldfd == newfd`, does nothing

**Example**:
```c
dup2(pipefd[1], 1);  // Redirect stdout to pipe
```

---

#### `ipc_create` (210)
Create IPC message channel.

**Signature**: `int ipc_create(void)`

**Returns**:
- Channel ID (0-31)
- -EAGAIN if no channels available

**POSIX**: ❌ NexaOS extension

**Behavior**:
- Allocates message channel from pool
- 32 messages per channel, 256 bytes each
- Blocking send/recv

---

#### `ipc_send` (211)
Send message to channel.

**Signature**: `int ipc_send(int chan, const void *msg, size_t len)`

**Parameters**:
- `chan`: Channel ID
- `msg`: Message buffer
- `len`: Message length (max 256 bytes)

**Returns**:
- 0 on success
- -EINVAL if chan invalid or len > 256
- -EAGAIN if channel full (blocks)

**POSIX**: ❌ NexaOS extension

---

#### `ipc_recv` (212)
Receive message from channel.

**Signature**: `int ipc_recv(int chan, void *msg, size_t len)`

**Parameters**:
- `chan`: Channel ID
- `msg`: Buffer to receive message
- `len`: Buffer size

**Returns**:
- Number of bytes received
- -EINVAL if chan invalid
- 0 if channel empty (blocks)

**POSIX**: ❌ NexaOS extension

---

### Signals (2 syscalls)

#### `sigaction` (13)
Set signal handler.

**Signature**: `int sigaction(int sig, const struct sigaction *act, struct sigaction *oldact)`

**Parameters**:
- `sig`: Signal number (SIGINT, SIGTERM, etc.)
- `act`: New action (can be NULL)
- `oldact`: Previous action returned here (can be NULL)

**Returns**:
- 0 on success
- -EINVAL if sig is invalid

**POSIX**: ✅ Yes

**struct sigaction**:
```c
struct sigaction {
    void (*sa_handler)(int);       // Signal handler function
    sigset_t sa_mask;              // Signals to block during handler
    int sa_flags;                  // Flags (SA_RESTART, etc.)
};
```

**Special Handlers**:
```c
#define SIG_DFL  ((void (*)(int)) 0)  // Default action
#define SIG_IGN  ((void (*)(int)) 1)  // Ignore signal
```

---

#### `sigprocmask` (14)
Set signal mask.

**Signature**: `int sigprocmask(int how, const sigset_t *set, sigset_t *oldset)`

**Parameters**:
- `how`: Operation (SIG_BLOCK, SIG_UNBLOCK, SIG_SETMASK)
- `set`: Signal set to apply (can be NULL)
- `oldset`: Previous mask returned here (can be NULL)

**Returns**:
- 0 on success
- -EINVAL if how is invalid

**POSIX**: ✅ Yes

**Operations**:
```c
#define SIG_BLOCK    0  // Add signals to mask
#define SIG_UNBLOCK  1  // Remove signals from mask
#define SIG_SETMASK  2  // Replace entire mask
```

---

### Filesystem Management (4 syscalls)

#### `mount` (165)
Mount filesystem.

**Signature**: `int mount(const char *source, const char *target, const char *fstype, unsigned long flags, const void *data)`

**Parameters**:
- `source`: Device or filesystem source
- `target`: Mount point directory
- `fstype`: Filesystem type ("ext2", "proc", "sysfs", etc.)
- `flags`: Mount flags (MS_RDONLY, MS_NOEXEC, etc.)
- `data`: Filesystem-specific options

**Returns**:
- 0 on success
- -ENOENT if target doesn't exist
- -EINVAL if fstype is invalid

**POSIX**: ✅ Yes (Linux-specific)

**Example**:
```c
mount("/dev/vda1", "/", "ext2", 0, NULL);
mount("proc", "/proc", "proc", 0, NULL);
```

---

#### `umount` (166)
Unmount filesystem.

**Signature**: `int umount(const char *target)`

**Parameters**:
- `target`: Mount point to unmount

**Returns**:
- 0 on success
- -ENOENT if target not mounted
- -EBUSY if filesystem is busy

**POSIX**: ✅ Yes (Linux-specific)

---

#### `pivot_root` (155)
Change root filesystem.

**Signature**: `int pivot_root(const char *new_root, const char *put_old)`

**Parameters**:
- `new_root`: New root directory
- `put_old`: Where to move old root

**Returns**:
- 0 on success
- -EINVAL if new_root or put_old is invalid

**POSIX**: ✅ Yes (Linux-specific)

**Use Case**: Switching from initramfs to real root

---

#### `chroot` (161)
Change root directory.

**Signature**: `int chroot(const char *path)`

**Parameters**:
- `path`: New root directory

**Returns**:
- 0 on success
- -ENOENT if path doesn't exist

**POSIX**: ✅ Yes

---

### Init System (3 syscalls)

#### `reboot` (169)
Reboot or halt system.

**Signature**: `int reboot(int cmd)`

**Parameters**:
- `cmd`: Reboot command (RB_AUTOBOOT, RB_HALT_SYSTEM, etc.)

**Returns**: Does not return

**POSIX**: ✅ Yes (Linux-specific)

**Commands**:
```c
#define RB_AUTOBOOT    0x01234567  // Reboot system
#define RB_HALT_SYSTEM 0xCDEF0123  // Halt system
#define RB_POWER_OFF   0x4321FEDC  // Power off
```

---

#### `shutdown` (230)
Shutdown system.

**Signature**: `int shutdown(int mode)`

**Parameters**:
- `mode`: Shutdown mode (0=halt, 1=reboot, 2=poweroff)

**Returns**: Does not return

**POSIX**: ❌ NexaOS extension

---

#### `runlevel` (231)
Get or set system runlevel.

**Signature**: `int runlevel(int level)`

**Parameters**:
- `level`: New runlevel (0-6, -1=query current)

**Returns**:
- Current runlevel if level=-1
- 0 on success if setting runlevel
- -EINVAL if level is invalid

**POSIX**: ❌ NexaOS extension

**Runlevels**:
```
0: Halt
1: Single-user
2: Multi-user (no network)
3: Multi-user (default)
4: Unused
5: Graphical (future)
6: Reboot
```

---

### Authentication (5 syscalls)

#### `user_add` (220)
Add user to system.

**Signature**: `int user_add(const char *username, const char *password, uint64_t flags)`

**Parameters**:
- `username`: Username (max 32 chars)
- `password`: Password (plaintext, will be hashed)
- `flags`: User flags (0x1=admin)

**Returns**:
- 0 on success
- -EINVAL if username is NULL or exists
- -EPERM if not root

**POSIX**: ❌ NexaOS extension

---

#### `user_login` (221)
Authenticate user.

**Signature**: `int user_login(const char *username, const char *password)`

**Parameters**:
- `username`: Username
- `password`: Password

**Returns**:
- UID on success (≥0)
- -EINVAL if username/password is NULL
- -EACCES if authentication failed

**POSIX**: ❌ NexaOS extension

---

#### `user_info` (222)
Get current user information.

**Signature**: `int user_info(struct user_info *buf)`

**Parameters**:
- `buf`: Buffer to fill with user info

**Returns**:
- 0 on success
- -EINVAL if buf is NULL
- -ENOENT if not logged in

**POSIX**: ❌ NexaOS extension

---

#### `user_list` (223)
List all users.

**Signature**: `int user_list(void)`

**Returns**: Number of users in system

**POSIX**: ❌ NexaOS extension

---

#### `user_logout` (224)
Log out current user.

**Signature**: `int user_logout(void)`

**Returns**:
- 0 on success
- -ENOENT if not logged in

**POSIX**: ❌ NexaOS extension

---

### Utilities (2 syscalls)

#### `list_files` (200)
List files in directory.

**Signature**: `int list_files(const char *path, uint64_t flags)`

**Parameters**:
- `path`: Directory path
- `flags`: List flags (0x1=include hidden)

**Returns**:
- Number of files listed
- -ENOENT if path not found
- -ENOTDIR if path is not a directory

**POSIX**: ❌ NexaOS extension

---

#### `geterrno` (201)
Get last error number.

**Signature**: `int geterrno(void)`

**Returns**: Last errno value for current process

**POSIX**: ❌ NexaOS extension

**Note**: Standard errno is stored in TLS (thread-local storage) in nrlib

---

## Error Codes

### POSIX Error Numbers

```c
#define EPERM   1   // Operation not permitted
#define ENOENT  2   // No such file or directory
#define ESRCH   3   // No such process
#define EINTR   4   // Interrupted system call
#define EIO     5   // I/O error
#define ENXIO   6   // No such device or address
#define E2BIG   7   // Argument list too long
#define ENOEXEC 8   // Exec format error
#define EBADF   9   // Bad file number
#define ECHILD  10  // No child processes
#define EAGAIN  11  // Try again (EWOULDBLOCK)
#define ENOMEM  12  // Out of memory
#define EACCES  13  // Permission denied
#define EFAULT  14  // Bad address
#define ENOTBLK 15  // Block device required
#define EBUSY   16  // Device or resource busy
#define EEXIST  17  // File exists
#define EXDEV   18  // Cross-device link
#define ENODEV  19  // No such device
#define ENOTDIR 20  // Not a directory
#define EISDIR  21  // Is a directory
#define EINVAL  22  // Invalid argument
#define ENFILE  23  // File table overflow
#define EMFILE  24  // Too many open files
#define ENOTTY  25  // Not a typewriter
#define ETXTBSY 26  // Text file busy
#define EFBIG   27  // File too large
#define ENOSPC  28  // No space left on device
#define ESPIPE  29  // Illegal seek
#define EROFS   30  // Read-only file system
#define EMLINK  31  // Too many links
#define EPIPE   32  // Broken pipe
```

## Userspace Syscall Wrappers

### nrlib (C ABI for Rust std)

Located in `userspace/nrlib/src/lib.rs`

**Example wrappers**:
```rust
#[no_mangle]
pub extern "C" fn read(fd: i32, buf: *mut u8, count: usize) -> isize {
    let ret = syscall3(SYS_READ, fd as u64, buf as u64, count as u64);
    if ret == u64::MAX {
        refresh_errno_from_kernel();
        return -1;
    }
    ret as isize
}

#[no_mangle]
pub extern "C" fn fork() -> pid_t {
    let ret = syscall0(SYS_FORK);
    if ret == u64::MAX {
        refresh_errno_from_kernel();
        return -1;
    }
    ret as pid_t
}
```

### Direct Syscall in Assembly

**Example**:
```rust
fn syscall3(n: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    let ret: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") n,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            lateout("rax") ret,
            options(nostack)
        );
    }
    ret
}
```

## Syscall Implementation (Kernel Side)

Located in `src/syscall.rs`

**Dispatch Function**:
```rust
#[no_mangle]
pub extern "C" fn syscall_dispatch(
    nr: u64,      // RAX: syscall number
    arg1: u64,    // RDI
    arg2: u64,    // RSI
    arg3: u64,    // RDX
    arg4: u64,    // R10
    arg5: u64,    // R8
    arg6: u64,    // R9
) -> u64 {
    match nr {
        SYS_READ => sys_read(arg1, arg2, arg3),
        SYS_WRITE => sys_write(arg1, arg2, arg3),
        SYS_OPEN => sys_open(arg1, arg2, arg3),
        SYS_FORK => sys_fork(),
        // ... more syscalls
        _ => {
            kerror!("Unknown syscall: {}", nr);
            u64::MAX  // -1 (error)
        }
    }
}
```

## Future Syscalls (Planned)

### Memory Management
- `mmap` (9): Map files or devices into memory
- `munmap` (11): Unmap memory region
- `mprotect` (10): Set memory protection
- `brk` (12): Change data segment size
- `sbrk`: Adjust heap size

### Threading
- `clone` (56): Create thread/process
- `futex` (202): Fast userspace mutex
- `set_tid_address` (218): Set thread ID pointer
- `exit_group` (231): Exit all threads

### Networking
- `socket` (41): Create socket
- `bind` (49): Bind socket to address
- `listen` (50): Listen for connections
- `accept` (43): Accept connection
- `connect` (42): Connect to address
- `send` (44): Send data
- `recv` (45): Receive data

### Advanced I/O
- `select` (23): Monitor file descriptors
- `poll` (7): Wait for events
- `epoll_create` (213): Create epoll instance
- `ioctl` (16): Device control

---

**Documentation Status**: ✅ Complete  
**Implementation Status**: ✅ 38+ syscalls functional  
**POSIX Compliance**: ⚙️ Core syscalls, expanding
