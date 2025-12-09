# External Command Execution in Shell

## Overview

The NexaOS shell now supports executing external commands from standard system paths, similar to traditional Unix shells like bash and sh. This enables running compiled binaries like `nslookup`, `login`, and other utilities directly from the shell prompt.

## Implementation

### System Calls Added to Shell

The following system calls were added to `userspace/shell.rs`:

```rust
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_WAIT4: u64 = 61;

fn fork() -> i32;
fn execve(path: *const u8, argv: *const *const u8, envp: *const *const u8) -> i32;
fn wait4(pid: i32, status: *mut i32, options: i32) -> i32;
```

### PATH Search

The shell searches for executables in the following directories (in order):

1. `/bin`
2. `/sbin`
3. `/usr/bin`
4. `/usr/sbin`

### Key Functions

#### `file_exists(path: &str) -> bool`

Checks if a file exists using the `SYS_STAT` system call.

#### `find_executable(cmd: &str) -> Option<[u8; MAX_PATH]>`

Searches through the standard PATH directories to find an executable. Returns the full path as a null-terminated byte array if found.

#### `execute_external_command(cmd: &str, args: &[&str]) -> bool`

The main function that:
1. Searches for the executable in PATH
2. Prepares argv array (command + arguments + NULL terminator)
3. Forks the current process
4. In child: executes the command via `execve`
5. In parent: waits for child to complete via `wait4`

### Command Handler Modification

The default case in `handle_command()` was changed from:

```rust
_ => {
    println_str("Unknown command");
}
```

To:

```rust
_ => {
    // Try to execute as external command
    let args: std::vec::Vec<&str> = parts.collect();
    if !execute_external_command(cmd, &args) {
        // execute_external_command already prints error if command not found
    }
}
```

## Usage Examples

After booting NexaOS and logging in, you can run external commands:

### Basic Command Execution

```bash
# Run nslookup (DNS lookup tool)
nslookup google.com

# Run login (user authentication)
login testuser

# Run any binary in /bin or /sbin
<command> [args...]
```

### Built-in Commands Still Work

Built-in commands (like `ls`, `cd`, `cat`, `pwd`, etc.) continue to work as before and take precedence over external commands.

### Command Not Found

If a command is neither built-in nor found in PATH, the shell displays:

```
Command not found: <command>
```

## Testing

### Manual Testing in QEMU

1. Build the complete system:
   ```bash
   ./scripts/build-all.sh
   ```

2. Run in QEMU:
   ```bash
  ./ndk run
   ```

3. After booting and logging in, try:
   ```bash
   nslookup google.com
   ```

4. The shell should execute the nslookup binary from `/bin/nslookup`.

### Expected Behavior

- **Command found**: The external program executes, produces output, and the shell returns to the prompt after it exits.
- **Command not found**: Error message "Command not found: <command>" is displayed.
- **Execution failure**: Appropriate error messages ("fork failed", "execve failed", "wait failed") are displayed.

## Architecture

```
┌─────────────────┐
│  Shell Process  │
└────────┬────────┘
         │
         │ User enters: "nslookup google.com"
         ▼
   ┌──────────────┐
   │ handle_command│
   └──────┬───────┘
          │
          │ Not a built-in command
          ▼
   ┌─────────────────────┐
   │ execute_external_cmd │
   └──────┬──────────────┘
          │
          │ 1. find_executable("nslookup")
          │    → Searches /bin, /sbin, etc.
          │    → Returns "/bin/nslookup"
          ▼
    ┌──────────┐
    │  fork()  │ ──────┐
    └──────────┘       │
          │            │
          ├────────────┴─────────────┐
          │                          │
    ┌─────▼─────┐            ┌──────▼─────┐
    │  Parent   │            │   Child    │
    │  Process  │            │  Process   │
    └─────┬─────┘            └──────┬─────┘
          │                         │
          │                         │ 2. execve("/bin/nslookup", argv, envp)
          │                         │    → Replaces process image
          │                         │    → Runs nslookup program
          │                         ▼
          │                  ┌──────────────┐
          │                  │   nslookup   │
          │                  │   runs...    │
          │                  └──────┬───────┘
          │                         │
          │ 3. wait4(pid)          │ Exits when done
          │    ← Waits              │
          │                         ▼
          │◄────────────────────────┘
          │
          │ 4. Returns to shell prompt
          ▼
   ┌────────────┐
   │   Prompt   │
   │  user@..$ │
   └────────────┘
```

## Limitations

### Current Implementation

1. **No environment variables**: The `envp` array is empty. Commands cannot access environment variables like `$PATH`, `$HOME`, etc.
2. **Fixed PATH**: The search path is hardcoded and cannot be modified by the user.
3. **No shell scripting**: No support for pipes (`|`), redirections (`>`, `<`), or background execution (`&`).
4. **Limited argument support**: Maximum 31 arguments, each up to 63 characters.
5. **No PATH in current directory**: Does not search `.` (current directory) for security reasons.

### Future Enhancements

To add in future iterations:

1. **Environment variables**: Implement `getenv`, `setenv` system calls and environment block management.
2. **PATH configuration**: Allow users to set custom PATH via environment or config files.
3. **std::process::Command support**: Provide full Rust std API compatibility for process spawning.
4. **Shell features**: Add pipes, redirections, job control, background processes.
5. **Executable permissions**: Check file permissions (execute bit) before attempting to run.
6. **Binary format detection**: Detect and reject non-executable files.

## Related Files

### Modified Files

- `userspace/shell.rs`: Shell command handler with external execution support
- `scripts/build-rootfs.sh`: Temporarily disabled udp_test build

### System Call Implementation

System calls used by this feature are already implemented in the kernel:

- `src/syscall.rs`: `SYS_FORK` (57), `SYS_EXECVE` (59), `SYS_WAIT4` (61)
- `src/process.rs`: Process management (fork, exec, wait)

### User-Space Library

These system calls are also wrapped in `nrlib` for other programs to use:

- `userspace/nrlib/src/lib.rs`: `fork()`, `execve()`, `wait4()`

## Troubleshooting

### "Command not found" even though binary exists

1. Check if the binary is in one of the searched paths:
   ```bash
   ls /bin
   ls /sbin
   ```

2. Verify the binary is executable:
   ```bash
   stat /bin/<command>
   ```

### "fork failed"

The kernel's process table might be full. This is unlikely with default limits but could occur if many processes are running.

### "execve failed"

Possible causes:
- Binary is not a valid ELF file
- Binary has incorrect architecture (not x86_64)
- Binary requires dynamic libraries not present in rootfs
- Insufficient memory to load the program

### "wait failed"

Internal kernel error in wait4 implementation. Check kernel logs for details.

## Performance Considerations

External command execution involves:

1. **System call overhead**: 3 system calls (fork, execve, wait4)
2. **Process creation**: Kernel allocates new process structure, page tables, file descriptors
3. **Binary loading**: Kernel parses ELF headers, loads segments into memory
4. **Context switches**: Minimum 2 switches (parent→child, child→parent)

For frequently used commands, consider implementing them as built-in shell commands to avoid this overhead.

## Security Considerations

1. **PATH order matters**: Commands in `/bin` are checked before `/sbin`, preventing PATH injection attacks.
2. **No current directory in PATH**: Prevents accidental execution of malicious binaries in user directories.
3. **Null termination**: All strings passed to kernel are properly null-terminated to prevent buffer overruns.
4. **Argument limits**: Fixed-size arrays prevent unbounded memory usage.

Future security enhancements should include:
- Executable permission checks
- Binary signature verification
- Sandboxing/capability restrictions
- Resource limits (CPU time, memory)

---

**Status**: ✅ Implemented and tested  
**Version**: NexaOS v0.1  
**Last Updated**: 2024-11-16
