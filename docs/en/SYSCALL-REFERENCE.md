# NexaOS Syscall Reference

> **Status**: Complete 38+ syscall reference  
> **Compliance**: POSIX.1-2017, Linux ABI compatible  
> **Last Updated**: 2024

## Table of Contents

1. [Syscall Overview](#syscall-overview)
2. [Process Management](#process-management)
3. [File Operations](#file-operations)
4. [File Descriptor Operations](#file-descriptor-operations)
5. [Directory Operations](#directory-operations)
6. [Process Control](#process-control)
7. [Signal Handling](#signal-handling)
8. [User/Group Operations](#usergroup-operations)
9. [Time Operations](#time-operations)
10. [Memory Operations](#memory-operations)
11. [Pipe/IPC Operations](#pipeipc-operations)
12. [Information Queries](#information-queries)
13. [Error Codes](#error-codes)
14. [Quick Syscall Table](#quick-syscall-table)

---

## Syscall Overview

### Calling Convention (x86_64)

**Fast Syscall** (SYSCALL instruction):
```
RDI = arg1        RAX = syscall number
RSI = arg2        Return value in RAX
RDX = arg3        Error in RAX (negative)
R10 = arg4        RCX modified (user RIP), R11 modified (user RFLAGS)
R8  = arg5
R9  = arg6
```

**Return Values**:
- Success: Return value in RAX (0 or positive)
- Error: Negative errno in RAX (e.g., -ENOENT)

### Error Handling Pattern

```c
// C wrapper from nrlib
long syscall(int number, ...) {
    long result = raw_syscall(number, args...);
    if (result < 0) {
        errno = -result;  // Extract error code
        return -1;
    }
    return result;
}
```

---

## Process Management

### fork() - Create Child Process

```c
pid_t fork(void);
```

**Purpose**: Create a child process (clone of parent)

**Signature**: Syscall #57 (Linux x86_64)

**Arguments**: None

**Return**:
- In Parent: Child PID (> 0)
- In Child: 0
- Error: -ENOMEM, -EAGAIN

**Behavior**:
- Child inherits parent's memory space (copy-on-write in modern systems, full copy here)
- Child has new PID, PPID = parent's PID
- Child inherits file descriptors
- Child inherits signal handlers
- Execution resumes after fork in both parent and child

**Example**:
```c
#include <unistd.h>
int main() {
    pid_t child = fork();
    if (child == 0) {
        // Child process
        printf("I am child\n");
        exit(0);
    } else if (child > 0) {
        // Parent process
        printf("Child PID: %d\n", child);
    } else {
        perror("fork failed");
    }
}
```

---

### execve() - Execute Program

```c
int execve(const char *filename, char *const argv[],
           char *const envp[]);
```

**Purpose**: Replace current process image with new program

**Signature**: Syscall #59 (Linux x86_64)

**Arguments**:
- `filename`: Path to executable (e.g., "/bin/ls")
- `argv`: Argument array (NULL-terminated)
- `envp`: Environment array (NULL-terminated)

**Return**:
- Success: Never returns (process replaced)
- Error: -ENOENT (file not found), -EACCES (permission denied)

**Behavior**:
- Loads ELF binary from disk
- Replaces memory space with new program
- Re-initializes stack with argc, argv, envp
- Transfers control to entry point (_start)

**Example**:
```c
char *args[] = {"ls", "-la", NULL};
char *env[] = {"HOME=/root", NULL};
execve("/bin/ls", args, env);
perror("execve failed");  // Only reached on error
```

---

### exit() - Terminate Process

```c
void exit(int status);
```

**Purpose**: Terminate current process

**Signature**: Syscall #60 (Linux x86_64)

**Arguments**:
- `status`: Exit code (0-255, stored in process table)

**Return**: Never returns

**Behavior**:
- Close all file descriptors
- Send SIGCHLD to parent
- Transition process to "zombie" state
- Kernel waits for parent to reap (call wait/waitpid)

---

### wait() / waitpid() - Wait for Child

```c
pid_t wait(int *status);
pid_t waitpid(pid_t pid, int *status, int options);
```

**Purpose**: Wait for child process termination

**Signature**: Syscall #114 (wait), #61 (waitpid)

**Arguments** (waitpid):
- `pid`: Child PID (-1 = any child)
- `status`: Pointer to receive exit status
- `options`: WNOHANG (non-blocking), WUNTRACED (stopped), etc.

**Return**:
- Child PID on success
- 0 if WNOHANG and no child ready
- -1 on error

**Status Decoding**:
```c
if (WIFEXITED(status)) {
    int code = WEXITSTATUS(status);  // 0-255
}
if (WIFSIGNALED(status)) {
    int sig = WTERMSIG(status);      // Signal number
}
```

---

## File Operations

### open() - Open File

```c
int open(const char *pathname, int flags, mode_t mode);
```

**Purpose**: Open file and return file descriptor

**Signature**: Syscall #2 (Linux x86_64)

**Arguments**:
- `pathname`: File path (absolute or relative)
- `flags`: O_RDONLY, O_WRONLY, O_RDWR, O_CREAT, O_APPEND, O_TRUNC
- `mode`: Permissions if O_CREAT (e.g., 0644)

**Return**:
- File descriptor (3+, 0=stdin, 1=stdout, 2=stderr)
- -ENOENT (file not found)
- -EACCES (permission denied)
- -EISDIR (is directory)

**Flags Table**:
| Flag | Meaning |
|------|---------|
| O_RDONLY (0) | Read-only |
| O_WRONLY (1) | Write-only |
| O_RDWR (2) | Read and write |
| O_CREAT (0x40) | Create if not exists |
| O_EXCL (0x80) | Fail if exists |
| O_TRUNC (0x200) | Truncate to 0 |
| O_APPEND (0x400) | Append to end |
| O_NONBLOCK (0x800) | Non-blocking |

---

### read() - Read from File

```c
ssize_t read(int fd, void *buf, size_t count);
```

**Purpose**: Read bytes from file descriptor

**Signature**: Syscall #0 (Linux x86_64)

**Arguments**:
- `fd`: File descriptor (0-255)
- `buf`: Buffer for data
- `count`: Number of bytes to read

**Return**:
- Bytes read (0 = EOF)
- -EINVAL, -EBADF, -EIO, etc.

**Behavior**:
- Blocks until data available or EOF
- Updates file position
- Returns up to `count` bytes

---

### write() - Write to File

```c
ssize_t write(int fd, const void *buf, size_t count);
```

**Purpose**: Write bytes to file descriptor

**Signature**: Syscall #1 (Linux x86_64)

**Arguments**:
- `fd`: File descriptor (0-255)
- `buf`: Data to write
- `count`: Number of bytes

**Return**:
- Bytes written
- -EBADF, -EINVAL, -ENOSPC, etc.

---

### close() - Close File Descriptor

```c
int close(int fd);
```

**Purpose**: Close file and release descriptor

**Signature**: Syscall #3 (Linux x86_64)

**Return**: 0 on success, -EBADF on error

**Behavior**:
- Invalidates file descriptor
- Decrements reference count
- Flushes any buffered data

---

### lseek() - Seek in File

```c
off_t lseek(int fd, off_t offset, int whence);
```

**Purpose**: Change file position

**Signature**: Syscall #8 (Linux x86_64)

**Arguments**:
- `offset`: Byte offset
- `whence`: SEEK_SET (0), SEEK_CUR (1), SEEK_END (2)

**Return**: New file position, -EINVAL on error

---

### stat() / fstat() - Get File Information

```c
int stat(const char *pathname, struct stat *statbuf);
int fstat(int fd, struct stat *statbuf);
```

**Purpose**: Get file metadata

**Signature**: Syscall #4 (stat), #5 (fstat)

**Returns in statbuf**:
```c
struct stat {
    ino_t st_ino;        // Inode number
    mode_t st_mode;      // File type & permissions
    nlink_t st_nlink;    // Hard link count
    uid_t st_uid;        // Owner UID
    gid_t st_gid;        // Owner GID
    off_t st_size;       // File size in bytes
    time_t st_atime;     // Last access
    time_t st_mtime;     // Last modify
    time_t st_ctime;     // Inode change
    blksize_t st_blksize;// I/O block size
    blkcnt_t st_blocks;  // Blocks allocated
};
```

---

## File Descriptor Operations

### dup() / dup2() - Duplicate File Descriptor

```c
int dup(int oldfd);
int dup2(int oldfd, int newfd);
```

**Purpose**: Create copy of file descriptor

**Signature**: Syscall #32 (dup), #33 (dup2)

**dup()**: Returns first available fd
**dup2()**: Uses specific fd (closes existing)

**Example**:
```c
// Redirect stdout to file
int fd = open("output.txt", O_WRONLY | O_CREAT, 0644);
dup2(fd, 1);  // 1 = stdout
printf("This goes to file\n");
```

---

### fcntl() - File Control

```c
int fcntl(int fd, int cmd, ... /* arg */);
```

**Purpose**: Manipulate file descriptor properties

**Signature**: Syscall #72 (Linux x86_64)

**Commands**:
- F_GETFD / F_SETFD: Get/set close-on-exec flag
- F_GETFL / F_SETFL: Get/set flags (O_NONBLOCK, O_APPEND)
- F_DUPFD: Duplicate with minimum fd number
- F_SETLK: Set file lock

---

### pipe() - Create Pipe

```c
int pipe(int pipefd[2]);
```

**Purpose**: Create unidirectional pipe for IPC

**Signature**: Syscall #22 (Linux x86_64)

**Returns**: 0 on success, fills pipefd[2]
- pipefd[0]: Read end
- pipefd[1]: Write end

**Example**:
```c
int pfd[2];
pipe(pfd);

if (fork() == 0) {
    // Child
    close(pfd[0]);  // Close read end
    write(pfd[1], "Hello", 5);
} else {
    // Parent
    close(pfd[1]);  // Close write end
    char buf[10];
    read(pfd[0], buf, 10);
    printf("Got: %s\n", buf);
}
```

---

## Directory Operations

### chdir() - Change Directory

```c
int chdir(const char *path);
```

**Purpose**: Change current working directory

**Signature**: Syscall #80 (Linux x86_64)

**Behavior**:
- Updates process's "current directory"
- Affects relative path resolution
- Affects glob patterns

---

### getcwd() - Get Current Directory

```c
char *getcwd(char *buf, size_t size);
```

**Purpose**: Get current working directory path

**Signature**: Syscall #79 (Linux x86_64)

**Returns**:
- Pointer to buf on success
- NULL on error

---

### mkdir() - Create Directory

```c
int mkdir(const char *pathname, mode_t mode);
```

**Purpose**: Create new directory

**Signature**: Syscall #83 (Linux x86_64)

**Return**: 0 on success, -EEXIST, -ENOENT

---

### rmdir() - Remove Directory

```c
int rmdir(const char *pathname);
```

**Purpose**: Remove empty directory

**Signature**: Syscall #84 (Linux x86_64)

**Return**: 0 on success, -ENOTEMPTY, -ENOENT

---

### readdir() - Read Directory

```c
struct dirent *readdir(DIR *dir);
```

**Purpose**: Read directory entries (via getdents syscall)

**Syscall**: #78 (getdents, used internally)

**Returns dirent**:
```c
struct dirent {
    ino_t d_ino;        // Inode number
    off_t d_off;        // Offset in directory
    unsigned short d_reclen;  // Record length
    unsigned char d_type;     // File type
    char d_name[256];   // Null-terminated name
};
```

---

### unlink() - Delete File

```c
int unlink(const char *pathname);
```

**Purpose**: Remove file (decrease link count)

**Signature**: Syscall #87 (Linux x86_64)

**Return**: 0 on success, -ENOENT, -EACCES

**Behavior**:
- Decreases hard link count
- File deleted when count reaches 0
- If open, file remains accessible until closed

---

## Process Control

### getpid() / getppid() - Get Process IDs

```c
pid_t getpid(void);    // Get own PID
pid_t getppid(void);   // Get parent PID
```

**Signature**: Syscall #39 (getpid), #110 (getppid)

**Returns**: Process ID (>0)

---

### setpgid() / getpgid() - Process Group

```c
int setpgid(pid_t pid, pid_t pgid);
pid_t getpgid(pid_t pid);
```

**Purpose**: Set/get process group for job control

**Signature**: Syscall #109 (setpgid), #111 (getpgid)

---

### pause() - Wait for Signal

```c
int pause(void);
```

**Purpose**: Suspend process until signal caught

**Signature**: Syscall #34 (Linux x86_64)

**Behavior**:
- Returns when signal delivered
- Signal handler may terminate or continue
- Returns -EINTR

---

## Signal Handling

### signal() - Set Signal Handler

```c
typedef void (*sighandler_t)(int);
sighandler_t signal(int signum, sighandler_t handler);
```

**Purpose**: Register signal handler

**Signature**: Syscall #48 (Linux x86_64)

**Handlers**:
- SIG_DFL: Default action
- SIG_IGN: Ignore signal
- Function pointer: Custom handler

**Common Signals**:
| Signal | Default | Meaning |
|--------|---------|---------|
| SIGHUP (1) | Terminate | Hangup detected |
| SIGINT (2) | Terminate | Interrupt (Ctrl-C) |
| SIGQUIT (3) | Core dump | Quit (Ctrl-\) |
| SIGABRT (6) | Core dump | Abort |
| SIGKILL (9) | Terminate | Kill (cannot catch) |
| SIGSEGV (11) | Core dump | Segmentation fault |
| SIGTERM (15) | Terminate | Termination signal |
| SIGCHLD (17) | Ignore | Child process stopped |
| SIGSTOP (19) | Stop | Stop (cannot catch) |
| SIGCONT (18) | Continue | Continue if stopped |

---

### kill() - Send Signal

```c
int kill(pid_t pid, int sig);
```

**Purpose**: Send signal to process

**Signature**: Syscall #62 (Linux x86_64)

**Arguments**:
- `pid`: Target PID (or special values)
- `sig`: Signal number (0-64)

**Special PIDs**:
- pid > 0: Send to process
- pid = 0: Send to all processes in process group
- pid = -1: Send to all processes
- pid < -1: Send to process group (-pid)

---

### sigaction() - Advanced Signal Handling

```c
int sigaction(int signum, const struct sigaction *act,
              struct sigaction *oldact);
```

**Purpose**: Set signal handler with flags

**Signature**: Syscall #13 (Linux x86_64)

**Structure**:
```c
struct sigaction {
    void (*sa_handler)(int);     // Handler function
    void (*sa_sigaction)(int, siginfo_t *, void *);  // Advanced handler
    sigset_t sa_mask;            // Signals to block during handler
    int sa_flags;                // Flags (SA_RESTART, SA_NODEFER, etc)
};
```

---

### sigprocmask() - Block Signals

```c
int sigprocmask(int how, const sigset_t *set,
                sigset_t *oldset);
```

**Purpose**: Block or unblock signals

**Signature**: Syscall #14 (Linux x86_64)

**How Values**:
- SIG_BLOCK: Add to blocked set
- SIG_UNBLOCK: Remove from blocked set
- SIG_SETMASK: Replace blocked set

---

## User/Group Operations

### getuid() / geteuid() - Get User ID

```c
uid_t getuid(void);     // Real UID
uid_t geteuid(void);    // Effective UID
```

**Signature**: Syscall #102 (getuid), #107 (geteuid)

---

### getgid() / getegid() - Get Group ID

```c
gid_t getgid(void);     // Real GID
gid_t getegid(void);    // Effective GID
```

**Signature**: Syscall #104 (getgid), #108 (getegid)

---

### setuid() / setgid() - Set User/Group ID

```c
int setuid(uid_t uid);
int setgid(gid_t gid);
```

**Signature**: Syscall #105 (setuid), #106 (setgid)

**Behavior**: Only root can change to other user

---

### getgroups() / setgroups() - Supplementary Groups

```c
int getgroups(int size, gid_t list[]);
int setgroups(size_t size, const gid_t *list);
```

**Signature**: Syscall #115 (getgroups), #116 (setgroups)

---

## Time Operations

### time() - Get Current Time

```c
time_t time(time_t *tloc);
```

**Purpose**: Get current time since epoch (1970-01-01)

**Signature**: Syscall #201 (Linux x86_64)

**Returns**: Seconds since epoch

---

### gettimeofday() - Get Time with Microseconds

```c
int gettimeofday(struct timeval *tv, struct timezone *tz);
```

**Purpose**: Get current time with microsecond precision

**Signature**: Syscall #96 (Linux x86_64)

**Structure**:
```c
struct timeval {
    time_t tv_sec;       // Seconds
    suseconds_t tv_usec; // Microseconds (0-999999)
};
```

---

## Memory Operations

### mmap() - Map Memory

```c
void *mmap(void *addr, size_t length, int prot, int flags,
           int fd, off_t offset);
```

**Purpose**: Map file or allocate memory

**Signature**: Syscall #9 (Linux x86_64)

**Protections**:
- PROT_NONE: No access
- PROT_READ: Read allowed
- PROT_WRITE: Write allowed
- PROT_EXEC: Execute allowed

**Flags**:
- MAP_SHARED: Changes visible to other processes
- MAP_PRIVATE: Private copy
- MAP_ANON: No file backing
- MAP_FIXED: Use exact address

---

### munmap() - Unmap Memory

```c
int munmap(void *addr, size_t length);
```

**Purpose**: Unmap memory region

**Signature**: Syscall #11 (Linux x86_64)

---

### brk() - Set Heap End

```c
int brk(void *addr);
void *sbrk(intptr_t increment);
```

**Purpose**: Adjust heap size

**Signature**: Syscall #12 (Linux x86_64)

**Behavior**:
- Allocates memory on heap (contiguous)
- Used by malloc() internally

---

## Pipe/IPC Operations

### dup() / dup2() - See File Descriptor Operations

### fcntl() - See File Descriptor Operations

### poll() - Wait for I/O Events

```c
int poll(struct pollfd *fds, nfds_t nfds, int timeout);
```

**Purpose**: Wait for file descriptor readiness

**Signature**: Syscall #7 (Linux x86_64)

**Structure**:
```c
struct pollfd {
    int fd;        // File descriptor (-1 to skip)
    short events;  // Requested events (POLLIN, POLLOUT, etc)
    short revents; // Returned events
};
```

---

## Information Queries

### uname() - Get System Information

```c
int uname(struct utsname *buf);
```

**Purpose**: Get kernel and system information

**Signature**: Syscall #63 (Linux x86_64)

**Structure**:
```c
struct utsname {
    char sysname[65];   // "NexaOS"
    char nodename[65];  // Hostname
    char release[65];   // Kernel version
    char version[65];   // Build version
    char machine[65];   // Hardware (e.g., "x86_64")
};
```

---

## Error Codes

### POSIX Error Numbers

| Error | Errno | Meaning |
|-------|-------|---------|
| EPERM | 1 | Operation not permitted |
| ENOENT | 2 | No such file or directory |
| ESRCH | 3 | No such process |
| EINTR | 4 | Interrupted system call |
| EIO | 5 | I/O error |
| ENXIO | 6 | No such device or address |
| E2BIG | 7 | Argument list too long |
| ENOEXEC | 8 | Exec format error |
| EBADF | 9 | Bad file descriptor |
| ECHILD | 10 | No child processes |
| EAGAIN | 11 | Resource temporarily unavailable |
| ENOMEM | 12 | Out of memory |
| EACCES | 13 | Permission denied |
| EFAULT | 14 | Bad address |
| ENOTBLK | 15 | Block device required |
| EBUSY | 16 | Device or resource busy |
| EEXIST | 17 | File exists |
| EXDEV | 18 | Invalid cross-device link |
| ENODEV | 19 | No such device |
| ENOTDIR | 20 | Not a directory |
| EISDIR | 21 | Is a directory |
| EINVAL | 22 | Invalid argument |
| ENFILE | 23 | Too many open files in system |
| EMFILE | 24 | Too many open files |
| ENOTTY | 25 | Inappropriate ioctl for device |
| ETXTBSY | 26 | Text file busy |
| EFBIG | 27 | File too large |
| ENOSPC | 28 | No space left on device |
| ESPIPE | 29 | Illegal seek |
| EROFS | 30 | Read-only file system |
| EMLINK | 31 | Too many links |
| EPIPE | 32 | Broken pipe |

---

## Quick Syscall Table

| # | Syscall | Description | Category |
|---|---------|-------------|----------|
| 0 | read | Read from FD | File I/O |
| 1 | write | Write to FD | File I/O |
| 2 | open | Open file | File I/O |
| 3 | close | Close FD | File I/O |
| 4 | stat | Get file stats | File Info |
| 5 | fstat | Get FD stats | File Info |
| 8 | lseek | Seek in file | File I/O |
| 9 | mmap | Map memory | Memory |
| 11 | munmap | Unmap memory | Memory |
| 12 | brk | Set heap end | Memory |
| 13 | sigaction | Set signal handler | Signal |
| 14 | sigprocmask | Block signals | Signal |
| 22 | pipe | Create pipe | IPC |
| 32 | dup | Dup FD | File Desc |
| 33 | dup2 | Dup to FD | File Desc |
| 34 | pause | Wait for signal | Process |
| 39 | getpid | Get PID | Process |
| 48 | signal | Set handler (old) | Signal |
| 57 | fork | Create child | Process |
| 59 | execve | Execute program | Process |
| 60 | exit | Terminate | Process |
| 61 | wait4 | Wait for child | Process |
| 62 | kill | Send signal | Signal |
| 63 | uname | System info | Info |
| 72 | fcntl | File control | File Desc |
| 78 | getdents | Read directory | Directory |
| 79 | getcwd | Get cwd | Directory |
| 80 | chdir | Change directory | Directory |
| 83 | mkdir | Create directory | Directory |
| 84 | rmdir | Remove directory | Directory |
| 87 | unlink | Delete file | File |
| 96 | gettimeofday | Get time | Time |
| 102 | getuid | Get UID | User |
| 104 | getgid | Get GID | Group |
| 105 | setuid | Set UID | User |
| 106 | setgid | Set GID | Group |
| 107 | geteuid | Get effective UID | User |
| 108 | getegid | Get effective GID | Group |
| 109 | setpgid | Set process group | Process |
| 110 | getppid | Get parent PID | Process |
| 111 | getpgid | Get process group | Process |
| 114 | wait | Wait for child | Process |
| 115 | getgroups | Get groups | Group |
| 116 | setgroups | Set groups | Group |
| 201 | time | Get time (sec) | Time |

---

## Common Syscall Patterns

### Read File
```c
int fd = open("file.txt", O_RDONLY);
char buf[1024];
ssize_t n = read(fd, buf, sizeof(buf));
close(fd);
```

### Write File
```c
int fd = open("output.txt", O_WRONLY | O_CREAT, 0644);
write(fd, "Hello\n", 6);
close(fd);
```

### Pipe to Child
```c
int pfd[2];
pipe(pfd);
pid_t child = fork();
if (child == 0) {
    close(pfd[0]);
    dup2(pfd[1], STDOUT_FILENO);
    execve("/bin/ls", ...);
} else {
    close(pfd[1]);
    char buf[4096];
    while (read(pfd[0], buf, sizeof(buf)) > 0) {
        // Process output
    }
}
```

### Signal Handling
```c
void handler(int sig) {
    printf("Got signal %d\n", sig);
}

signal(SIGINT, handler);
pause();  // Wait for signal
```

---

## Related Documentation

- [Architecture](./ARCHITECTURE.md) - System design
- [Quick Reference](./QUICK-REFERENCE.md) - Commands and tools
- [Build System](./BUILD-SYSTEM.md) - Compilation guide
- [System Overview](./SYSTEM-OVERVIEW.md) - Component descriptions
- [Chinese Version](../zh/) - Chinese documentation

---

## FAQ

**Q: What's the difference between fork() and execve()?**
A: fork() creates a child process (copy of parent), execve() replaces the current process with a new program. Combined: fork() + execve() = spawn new program.

**Q: How do I handle errors from syscalls?**
A: Check for negative return values. If negative, the absolute value is the errno. Use strerror(errno) to get error message.

**Q: Can I use signal handlers safely?**
A: Only use async-signal-safe functions. Most library functions are NOT safe (e.g., malloc, printf). Use sigaction() with SA_RESTART to safely restart interrupted syscalls.

**Q: What happens if I close a pipe with pending data?**
A: If the write end is closed and data remains, readers get EOF after data is consumed. If read end is closed, writers get SIGPIPE.

**Q: How do I implement concurrency?**
A: Use fork() to create child processes, pipes/signals for communication, wait()/waitpid() to collect status. Alternatively, use threads (via pthread library in nrlib).

---

**Last Updated**: 2024-01-15  
**Maintainer**: NexaOS Development Team  
**Compliance**: POSIX.1-2017, Linux x86_64 ABI  
**License**: Same as NexaOS kernel
