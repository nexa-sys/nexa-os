# External Command Execution - Implementation Status & Next Steps

## Summary

Implemented external command execution in NexaOS shell with PATH search, enabling the shell to run binaries from `/bin`, `/sbin`, `/usr/bin`, and `/usr/sbin` like traditional Unix shells.

## Current Status: 🟡 Partial - Debug Phase

### ✅ Completed

1. **Shell Infrastructure**
   - Added `fork()`, `execve()`, `wait4()` system call wrappers to shell
   - Added `syscall0()` helper function (was missing)
   - Implemented `find_executable()` - searches PATH for commands
   - Implemented `execute_external_command()` - forks, execs, and waits
   - Modified `handle_command()` to try external execution when command not built-in

2. **Debug Logging**
   - Added extensive logging to `syscall_execve()` in kernel
   - Logs path, file size, ELF loading success/failure
   - Logs entry point and stack pointer after ELF load
   - Added error code and errno printing in shell

3. **Documentation**
   - Created detailed English documentation (`EXTERNAL-COMMAND-EXECUTION.md`)
   - Created detailed Chinese documentation (`外部命令执行.md`)
   - Created bug tracking document (`EXECVE-GP-FAULT-BUG.md`)

### 🔴 Blocking Issue: General Protection Fault

When attempting to execute an external command (e.g., `nslookup`):

```
root@nexa:/$ nslookup
execve failed
GP 0000000000000000 ...
RIP=0000000000000000
```

**Problem**: General Protection fault with instruction pointer at address 0, indicating the program is trying to execute code at null address.

### Root Cause Hypotheses

1. **Process Replacement Issue**: `execve` may not be correctly replacing the process image, leaving it in an invalid state

2. **Stack Corruption**: The new process's stack might be corrupted or improperly set up

3. **Entry Point Invalid**: The entry point might be calculated incorrectly or overwritten

4. **Missing argv/envp Handling**: The kernel's `syscall_execve` completely ignores argv and envp (marked with `_` prefix), but this shouldn't cause a GP fault - just broken argument passing

### Next Steps for Debugging

1. **Run with New Debug Logs**
   ```bash
  ./ndk run
   # Try: nslookup
   # Check kernel logs for execve debug output
   ```

2. **Analyze Kernel Logs**
   - Check if `[syscall_execve] Path:` shows correct path
   - Check if ELF loading succeeds
   - Check if entry point and stack values are reasonable
   - Compare successful init load vs failed shell exec

3. **Compare with Init Process**
   - Init process loads successfully via `Process::from_elf()`
   - Shell's execve uses same `Process::from_elf()` but fails
   - Key difference: Init is first process, execve replaces existing process

4. **Potential Fixes**

   **Option A: Fix Process Replacement**
   ```rust
   // In syscall_execve, after creating new_process:
   // Maybe need to preserve file descriptors?
   // Maybe need to flush TLB?
   // Maybe need to update page tables?
   ```

   **Option B: Implement argv/envp**
   ```rust
   // Read argv from user space
   // Pass to build_initial_stack()
   // Properly set up stack with arguments
   ```

   **Option C: Use posix_spawn Instead**
   ```rust
   // Implement posix_spawn as alternative
   // Might have cleaner semantics for process replacement
   ```

## Architecture

### Current Flow

```
Shell: nslookup
  ↓
find_executable("nslookup") → "/bin/nslookup"
  ↓
fork() → pid=X (child)
  ↓ (child process)
execve("/bin/nslookup", ...)
  ↓
[syscall_execve] Called
[syscall_execve] Path: /bin/nslookup
[syscall_execve] Found file, XXXXX bytes
[syscall_execve] Successfully loaded ELF, entry=0xYYYY, stack=0xZZZZ
  ↓
Replace process image
  ↓
❌ GP FAULT - RIP=0x0000000000000000
```

### Expected Flow

```
Shell: nslookup
  ↓
find_executable("nslookup") → "/bin/nslookup"
  ↓
fork() → pid=X (child)
  ↓ (child process)
execve("/bin/nslookup", ["nslookup"], [])
  ↓
Kernel replaces process image
  ↓
✅ nslookup runs, prints output
  ↓
exits
  ↓ (parent process)
wait4() returns
  ↓
Shell prompt returns
```

## Test Commands

```bash
# Build system with debug logging
./scripts/build-all.sh

# Run in QEMU
./scripts/run-qemu.sh

# After boot and login:
ls /bin              # Should show: nslookup udp_test sh login
nslookup             # Triggers GP fault (bug to fix)
```

## Related Files

### Modified Files
- `userspace/shell.rs`: External command execution logic
- `src/syscall.rs`: Debug logging in `syscall_execve()`
- `scripts/build-rootfs.sh`: Temporarily disabled udp_test

### Key Functions
- `userspace/shell.rs::execute_external_command()` - Lines ~1497-1595
- `src/syscall.rs::syscall_execve()` - Lines ~1413-1530
- `src/process.rs::Process::from_elf()` - Lines 133-250
- `src/process.rs::build_initial_stack()` - Lines 378-480

## Environment

- **NexaOS Version**: v0.1
- **Architecture**: x86_64
- **Kernel**: Custom hybrid kernel
- **Shell**: Custom no_std shell
- **Tested Programs**: nslookup (143KB), login (111KB), sh (155KB)

## Known Limitations

Even after fixing the GP fault:

1. **No argv/envp**: Commands won't receive arguments or environment variables
2. **No PATH variable**: Search paths are hardcoded
3. **No pipes/redirections**: No `|`, `>`, `<` support
4. **No job control**: No background processes, Ctrl+Z, etc.
5. **No wildcard expansion**: `*`, `?` not expanded
6. **No command substitution**: `` `cmd` `` and `$(cmd)` not supported

## Future Work

### Phase 1: Fix GP Fault ⏳ IN PROGRESS
- Add debug logging ✅ DONE
- Analyze logs to find root cause 🔄 NEXT
- Fix process replacement or stack setup
- Test basic execution works

### Phase 2: Implement argv/envp
- Modify `syscall_execve` to read argv from user space
- Modify `build_initial_stack` to accept argv array
- Pass arguments through to executed program
- Test: `nslookup google.com` with argument

### Phase 3: Environment Variables
- Implement `getenv`/`setenv` system calls
- Maintain environment block per process
- Support `PATH`, `HOME`, `USER` variables
- Allow shell to configure environment

### Phase 4: Advanced Shell Features
- Pipes (`|`)
- Redirections (`>`, `<`, `>>`)
- Background execution (`&`)
- Job control (Ctrl+Z, `fg`, `bg`)
- Wildcard expansion
- Command history

### Phase 5: std::process::Command Support
- Ensure Rust's `std::process::Command` works
- Test with actual Rust programs using Command API
- Document any incompatibilities

## Success Criteria

### Minimum Viable
- [ ] Execute external command without GP fault
- [ ] Command produces output
- [ ] Shell prompt returns after command exits

### Full Feature
- [ ] Pass command-line arguments
- [ ] Set environment variables
- [ ] Support std::process::Command API
- [ ] Implement basic shell features (pipes, redirections)

---

**Status**: 🟡 Blocked on GP Fault  
**Priority**: P0 (Critical)  
**Assigned**: Development Team  
**Last Updated**: 2024-11-16

**Next Action**: Run with debug logs and analyze execve failure
