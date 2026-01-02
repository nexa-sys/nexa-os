# NexaOS AI Coding Guide

## Architecture Overview

NexaOS is a Rust `no_std` hybrid kernel with 6-stage boot (`src/boot/stages.rs`): 
**Bootloader → KernelInit → Initramfs → RootSwitch → RealRoot → UserSpace**. 
The kernel runs in Ring 0, userspace in Ring 3 with full POSIX compliance.

### Key Subsystems

| Component | Location | Purpose |
|-----------|----------|---------|
| Boot entry | `src/main.rs` → `src/lib.rs` | Multiboot2 → `kernel_main()`, UEFI → `kernel_main_uefi()` |
| Memory | `src/mm/paging.rs`, `src/process/types.rs` | Identity-mapped kernel, isolated userspace with 4-level paging |
| Scheduler | `src/scheduler/` | **EEVDF algorithm** (Linux 6.6+): vruntime, deadlines, per-CPU queues |
| Syscalls | `src/syscalls/` | 60+ POSIX syscalls, organized by domain (file, process, signal, network, memory, thread, time) |
| Filesystems | `src/fs/initramfs.rs`, `src/fs/` | CPIO initramfs → ext2 rootfs after pivot_root (stage 4) |
| Safety helpers | `src/safety/` | Centralized unsafe wrappers (volatile, MMIO, port I/O, packet casting) |
| Networking | `src/net/` | Full UDP/IPv4 stack, ARP, DNS resolver; TCP in progress |
| Kernel modules | `modules/`, `src/kmod/` | Loadable `.nkm` modules (ext2, e1000, virtio) with PKCS#7 signing |
| Init system | `src/boot/init.rs` | PID 1 service management (System V runlevels, /etc/inittab parsing) |
| NVM Hypervisor | `nvm/` | Enterprise hypervisor platform (VT-x/AMD-V, live migration, HA) |

### Critical Memory Layout (`src/process/types.rs`)

```rust
USER_VIRT_BASE: 0x1000000      // Userspace code base (16MB)
HEAP_BASE:      0x1200000      // User heap (8MB: 0x1200000–0x1A00000)
STACK_BASE:     0x1A00000      // User stack (2MB, placed after heap)
INTERP_BASE:    0x1C00000      // Dynamic linker region (16MB reserved)
```

**⚠️ Critical Invariant**: Changes to these constants require simultaneous updates in:
- `src/mm/paging.rs` (map_user_region, identity mapping)
- `src/process/loader.rs` (ELF loading, segment placement)
- `src/security/elf.rs` (auxiliary vector setup)

Failure to sync these causes memory corruption or segfaults during ELF loading.

### EEVDF Scheduler (`src/scheduler/`)

The scheduler uses EEVDF (Earliest Eligible Virtual Deadline First), same as Linux 6.6+:
- **vruntime**: Tracks weighted CPU time consumption per process
- **Virtual Deadline**: `vruntime + slice/weight` provides latency guarantees
- **Per-CPU queues**: Each CPU has its own run queue to minimize lock contention
- **Eligibility**: Only processes with `lag >= 0` can preempt current task

Key files: `types.rs` (constants), `priority.rs` (vruntime/deadline), `percpu.rs` (per-CPU state)

## Build & Test Workflows

### Standard Commands

```bash
./ndk build full              # ALWAYS START WITH THIS: kernel → nrlib → userspace → modules → rootfs → ISO
./ndk build quick             # Fast: kernel + initramfs + ISO (skip rootfs rebuild)
./ndk build kernel            # Kernel only (use after .rs changes)
./ndk build userspace rootfs iso  # Rebuild after userspace/etc/ changes
./ndk run                     # Boot in QEMU (uses last built ISO)
./ndk dev --quick             # Build + run in one command
./ndk test                    # Run unit tests (tests/ crate)
./ndk test --filter bitmap    # Run specific test pattern
./ndk coverage html           # Generate HTML coverage report
./ndk run --debug             # Start GDB server at 127.0.0.1:1234
```

### Environment Variables

```bash
BUILD_TYPE=debug ./ndk build full     # Debug build (DEFAULT, STABLE)
BUILD_TYPE=release ./ndk build full   # Release (O3 may break fork/exec; avoid)
LOG_LEVEL=info ./ndk build kernel     # Kernel log level: debug|info|warn|error
SMP=8 ./ndk run                       # Boot with 8 CPU cores
MEMORY=2G ./ndk run                   # Boot with 2GB RAM
FEATURE_smp=true ./ndk build kernel   # Enable SMP at build time
```

**Build order is strict**: kernel → nrlib → userspace → modules → initramfs → rootfs → iso.
Skipping steps breaks subsequent builds.

## Coding Conventions

### Kernel Code (`src/`)

- **`no_std` only** — No heap allocations; use fixed-size buffers (StaticVec, ArrayVec)
- **Logging macros** (`src/logger.rs`): `kinfo!`, `kwarn!`, `kerror!`, `kdebug!`, `kfatal!`
  - **Never disable logging** — serial output is essential for boot debugging
  - Log level controlled by kernel command line (e.g., `log_level=debug`)
- **Error handling**: Return `Errno` (from `src/posix.rs`); never panic in syscall paths
- **Unsafe code**: Use `src/safety/` helpers exclusively:
  ```rust
  use crate::safety::{inb, outb, volatile_read, volatile_write, copy_from_user, copy_to_user, cast_header};
  ```
  Rationale: Centralizes x86_64 low-level details in one place for auditing.

### Process & Scheduler Consistency

Process state management is **critical** because three subsystems interact:
1. **Scheduler** (`src/scheduler/mod.rs`) — tracks Ready/Running/Sleeping/Zombie
2. **Signals** (`src/ipc/signal.rs`) — can transition processes to Sleeping/Running
3. **wait4 syscall** (`src/syscalls/process.rs`) — must see consistent Zombie state

**Pattern to follow**:
- Always acquire process lock before modifying `ProcessState`
- After signal delivery, update scheduler queue (don't just change state)
- When marking Zombie, ensure parent PID is set so wait4 can find it
- See `src/scheduler/mod.rs:update_process_state()` for reference

### Adding New Syscalls (`src/syscalls/`)

1. **Define syscall number** in `numbers.rs`:
   ```rust
   pub const SYS_MYPROCEDURE: u64 = 450;  // Check for conflicts in Linux source
   ```

2. **Implement logic** in domain file (file.rs, process.rs, network.rs, memory.rs, etc.):
   ```rust
   pub fn my_procedure(arg1: u64, arg2: u64) -> Result<u64, Errno> {
       // Validate inputs
       // Perform operation
       // Return Errno on failure
   }
   ```

3. **Wire up dispatcher** in `mod.rs:syscall_dispatch()`:
   ```rust
   SYS_MYPROCEDURE => my_procedure(arg1, arg2),
   ```

4. **Update nrlib** (`userspace/nrlib/src/lib.rs`) if Rust std needs this syscall:
   ```rust
   pub unsafe fn myprocedure(arg1: u64, arg2: u64) -> i64 {
       raw_syscall2(SYS_MYPROCEDURE, arg1, arg2)
   }
   ```

### Userspace Programs & Libraries

**Workspace structure** (`userspace/`):
- **nrlib** — C-compatible libc shim (pthread stubs, TLS, malloc, stdio, socket). **Always linked, statically**.
- **ld-nrlib** — Dynamic linker at `/lib64/ld-nrlib-x86_64.so.1`. Loads .so files, sets up auxiliary vectors.
- **programs/** — Organized by category (core, user, network, coreutils, power). Each is a separate crate.
- **lib/** — Shared libraries (.so files): ncryptolib, nssl, nzip, nh2, nh3, ntcp2.

**Target triple** (`targets/`): `x86_64-nexaos-userspace.json`
- **pic (Position Independent Code)** variant (`x86_64-nexaos-userspace-pic.json`) used for .so files
- **lib variant** (`x86_64-nexaos-userspace-lib.json`) for static libraries

**Adding a new program**:
```bash
mkdir -p userspace/programs/category/myprogram
cat > userspace/programs/category/myprogram/Cargo.toml << 'EOF'
[package]
name = "myprogram"
version = "0.1.0"
edition = "2021"

[dependencies]
nrlib = { path = "../../nrlib" }
EOF
```

Then add to `userspace/Cargo.toml` workspace members. Build with `./ndk build userspace`.

### Service Registration (`etc/inittab`)

Format: `id:runlevels:action:process`

```ini
1:2345:respawn:/sbin/getty 38400 tty1
2:345:once:/sbin/uefi-compatd
3:6:ctrlaltdel:/sbin/shutdown -h now
```

- **Runlevels**: bitmask (0=halt, 1=single, 2=multi-user, 3=multi-network, 5=GUI, 6=reboot)
- **Actions**: respawn (auto-restart), once, ctrlaltdel, sysinit
- **Init binary**: `/sbin/ni` (implemented in `userspace/programs/core/init`)

Parsed by `src/boot/init.rs:parse_inittab()`. See `etc/inittab` for examples.

### Testing

**Unit tests** live in separate `tests/` crate (uses standard Rust std environment):

```bash
./ndk test                    # Run unit tests (tests/ crate)
./ndk test --filter bitmap    # Run specific test pattern
./ndk coverage html           # Generate HTML coverage report
```

测试套件通过 `build.rs` 预处理内核源码，用硬件 mock 层运行**真正的内核代码**。
- `build.rs` 复制内核源码到 `build/kernel_src/`，移除与 std 冲突的属性
- 硬件操作通过 mock 模块模拟（CPU、APIC、PIT、UART 等）
- 内核的分配器逻辑运行在 std 分配器之上
- **mock 模块由 NVM (`nvm/`) 提供**

NVM 是完整的虚拟机平台（`pub use nvm as mock;`），提供：
- `nvm/src/cpu.rs` — x86-64 CPU 模拟（寄存器、CR0-CR8、MSR、CPUID）
- `nvm/src/memory.rs` — 物理内存模拟（单一 mmap 区域，类似 QEMU）
- `nvm/src/hal.rs` — 硬件抽象层（替代 `src/safety/x86.rs` 的 port I/O）
- `nvm/src/devices/` — 设备模拟（PIC、PIT、LAPIC、IOAPIC、UART、RTC）

**Coverage 工具**（`scripts/src/coverage.ts`）是自研的静态分析覆盖率工具：
- 解析 Rust 源码提取函数、impl 块、分支信息
- 分析测试文件中的函数调用匹配覆盖
- 生成 Jest 风格的文本报告和 HTML/JSON 报告
- 支持模块级、文件级、行级覆盖率统计

**⚠️ 测试原则**：**不要重新实现或模拟内核逻辑**。
- ❌ 错误：写 "Simulates kernel behavior" 的伪实现

测试按子系统组织在 `tests/src/{fs,mm,net,ipc,process,scheduler,kmod}/`。

## Critical Pitfalls

1. **ProcessState must stay synchronized** across scheduler, signals, and wait4. Lock process before modifying state.
2. **Memory constants changes** (USER_VIRT_BASE, etc.) require coordinated updates in paging.rs + loader.rs + elf.rs.
3. **Dynamic linker mismatch** — PT_INTERP must always be `/lib64/ld-nrlib-x86_64.so.1`; hardcoded in loader.
4. **Userspace rebuild** — After modifying `userspace/` or `etc/`, run `./ndk build userspace rootfs iso` (not just `build kernel`).
5. **Release builds** — O3 optimization can break fork/exec; stick with debug builds for stability.
6. **Never panic in syscalls** — Return `Errno` instead; panics crash the entire kernel.

## Debugging Techniques

```bash
# Boot with debugger paused at start
./ndk run --debug

# In another terminal
gdb -ex "target remote :1234" target/x86_64-nexaos/debug/nexa-os
(gdb) c           # Continue execution
(gdb) info proc   # Show current PID
(gdb) break fork  # Break on specific symbol (if available in debug build)

# View kernel ring buffer (64KB circular)
dmesg            # In userspace shell

# Verify multiboot compliance
grub-file --is-x86-multiboot2 target/x86_64-nexaos/debug/nexa-os

# Module signing for loadable drivers
./scripts/sign-module.sh module_name.nkm
```

Serial console output shows all kernel logs. QEMU's `-serial stdio` redirects to terminal.
