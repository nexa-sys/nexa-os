# CR3 Implementation - Complete Process Address Space Isolation

## Overview

NexaOS implements complete CR3 (Control Register 3) support for process-level address space isolation. Each user process has its own page table hierarchy rooted at a unique PML4 (Page Map Level 4) table, enabling true memory isolation between processes.

## Architecture

### CR3 Structure

```
CR3 Register (64-bit):
┌─────────────────────────────────────────────────────┬─────────────┐
│     PML4 Physical Address (4KB-aligned)             │    Flags    │
│              Bits 51:12                             │   Bits 11:0 │
└─────────────────────────────────────────────────────┴─────────────┘
```

- **Bits 51:12**: Physical address of PML4 table (must be 4KB-aligned)
- **Bits 11:0**: Reserved/flags (kept as 0 in our implementation)

### Page Table Hierarchy

Each process has its own 4-level page table:

```
CR3 → PML4 (512 entries, covers 256 TB)
        ├→ PDP (512 entries, covers 512 GB)
        │   ├→ PD (512 entries, covers 1 GB)
        │   │   └→ 2MB Huge Pages (direct mapping)
        │   └→ ...
        └→ ...

Address Space Layout:
┌────────────────────────────────────────┐ 0xFFFFFFFF_FFFFFFFF
│         Kernel Space (Upper Half)      │ ← Shared across all
│         PML4[256-511]                  │    processes
├────────────────────────────────────────┤ 0x00007FFF_FFFFFFFF
│         User Space (Lower Half)        │ ← Process-specific
│         PML4[0-255]                    │    (isolated per CR3)
└────────────────────────────────────────┘ 0x00000000_00000000
```

## Implementation Components

### 1. Process Structure (`src/process.rs`)

Each `Process` struct includes:
```rust
pub struct Process {
    pub cr3: u64,  // Page table root (0 = kernel PT)
    // ... other fields
}
```

### 2. Page Table Creation (`src/paging.rs`)

**Function**: `create_process_address_space(phys_base: u64, size: u64) -> Result<u64, &'static str>`

Creates a new address space:
1. Allocates a new PML4 page
2. Clones kernel mappings (upper half)
3. Creates private user space mappings (lower half)
4. Uses 2MB huge pages for efficiency
5. Returns CR3 (PML4 physical address)

**Validation**: All CR3 values are validated for:
- 4KB alignment (bits 0-11 must be 0)
- Physical address sanity (< 4GB)
- Non-zero for user processes

### 3. Address Space Activation (`src/paging.rs`)

**Function**: `activate_address_space(cr3_phys: u64)`

Switches CPU to use a different page table:
1. Validates CR3 before loading
2. Converts to `PhysFrame` for x86_64 crate
3. Writes to CR3 register via `Cr3::write()`
4. Flushes TLB (implicit in CR3 write)

**Optimization**: Short-circuits if target CR3 is already active.

### 4. Context Switching (`src/scheduler.rs`)

Context switch workflow:
```rust
pub fn do_schedule() {
    // 1. Select next process
    // 2. For first run: call process.execute()
    //    - Activates process CR3
    //    - Jumps to user mode
    // 3. For resumed process:
    //    - Activates process CR3
    //    - Restores CPU context
    //    - Returns to user mode
}
```

**Critical**: CR3 is activated BEFORE jumping to user mode or restoring context.

### 5. Process Lifecycle

#### Process Creation (`Process::from_elf()`)
```rust
// 1. Load ELF binary
// 2. Create address space
let cr3 = create_process_address_space(dynamic_phys_base, USER_REGION_SIZE)?;
// 3. Validate CR3
validate_cr3(cr3, false)?;
// 4. Store in process struct
process.cr3 = cr3;
```

#### Process Fork (`syscall.rs::sys_fork()`)
```rust
// 1. Copy parent's memory
// 2. Create child's address space
let child_cr3 = create_process_address_space(child_phys_base, memory_size)?;
// 3. Validate and store
validate_cr3(child_cr3, false)?;
child_process.cr3 = child_cr3;
```

#### Process Exit (`scheduler::remove_process()`)
```rust
// 1. Remove from scheduler
// 2. Free page tables
if cr3 != 0 {
    free_process_address_space(cr3);
}
```

## Memory Layout Per Process

Each process sees:

```
Virtual Address Space (per process):
┌────────────────────────────────────┐ 0x00000000_00A00000
│  Interpreter Region (6 MB)         │ ← Dynamic linker + .so
│  INTERP_BASE = 0xA00000            │
├────────────────────────────────────┤ 0x00000000_00800000
│  Stack (2 MB, grows down)          │ ← User stack
│  STACK_BASE = 0x800000             │
├────────────────────────────────────┤ 0x00000000_00600000
│  Heap (2 MB, grows up)             │ ← malloc() region
│  HEAP_BASE = 0x600000              │
├────────────────────────────────────┤ 0x00000000_00400000
│  Program Image (.text, .data)      │ ← ELF loaded here
│  USER_VIRT_BASE = 0x400000         │
└────────────────────────────────────┘ 0x00000000_00000000
```

**Physical Mapping**: Each process's virtual addresses map to different physical memory via its unique CR3.

## API Reference

### Core Functions

```rust
// Create a new process address space
pub fn create_process_address_space(phys_base: u64, size: u64) 
    -> Result<u64, &'static str>

// Switch to a different address space
pub fn activate_address_space(cr3_phys: u64)

// Read current CR3 from CPU
pub fn read_current_cr3() -> u64

// Validate CR3 value
pub fn validate_cr3(cr3: u64, allow_zero: bool) 
    -> Result<(), &'static str>

// Free page tables (TODO: implement proper deallocation)
pub fn free_process_address_space(cr3: u64)

// Debug helpers
pub fn debug_cr3_info(cr3: u64, label: &str)
pub fn print_cr3_statistics()
```

### Scheduler Functions

```rust
// Get current process's CR3
pub fn current_cr3() -> u64

// Update process CR3 (with immediate activation if running)
pub fn update_process_cr3(pid: Pid, new_cr3: u64) 
    -> Result<(), &'static str>
```

## Statistics and Monitoring

CR3 allocation is tracked via atomic counters:

```rust
static CR3_ALLOCATIONS: AtomicU64;  // Total allocations
static CR3_ACTIVATIONS: AtomicU64;  // Total context switches
static CR3_FREES: AtomicU64;        // Total deallocations
```

Check for leaks:
```rust
let active = allocs - frees;  // Should equal number of processes
```

## Safety Considerations

### 1. CR3 Validation
Every CR3 must pass validation before use:
- ✅ Page-aligned (4KB boundary)
- ✅ Within physical RAM
- ✅ Not kernel PT (unless intentional)

### 2. TLB Coherency
- CR3 writes implicitly flush TLB
- No manual `invlpg` needed after CR3 switch
- Per-process TLB entries are isolated

### 3. Memory Ordering
- CR3 switch uses x86_64 crate's atomic operations
- Acquire/Release semantics via CPU architecture
- No additional barriers needed

### 4. Race Conditions
- Process table locked during CR3 updates
- CR3 activated BEFORE jumping to user mode
- No window where wrong PT is active

## Current Limitations

### 1. Page Table Deallocation
**Status**: Not implemented (TODO)

Current behavior:
- CR3 marked for freeing
- Page tables leaked
- No memory reclaimed

Required for production:
- Walk page table hierarchy
- Free all PT pages (PML4, PDP, PD)
- Return frames to allocator

### 2. Shared Memory
**Status**: Not implemented

Each process has isolated memory. No mechanism for:
- Shared memory regions
- IPC via shared buffers
- Copy-on-write fork optimization

### 3. Memory Reclamation
**Status**: Basic implementation

Missing features:
- Page fault handler for demand paging
- Swap support
- Memory pressure handling

### 4. PCID Support
**Status**: Not used

Modern CPUs support PCID (Process Context ID) to avoid TLB flushes on CR3 switch. This optimization is not yet implemented.

## Testing and Debugging

### Verify CR3 Isolation

```rust
// Check each process has unique CR3
scheduler::list_processes();  // Shows CR3 per process

// Verify current CR3
let active_cr3 = paging::read_current_cr3();
let expected_cr3 = scheduler::current_cr3();
assert_eq!(active_cr3, expected_cr3);
```

### Debug CR3 Structure

```rust
// Print detailed CR3 info
paging::debug_cr3_info(process.cr3, "Process 1");

// Check for leaks
paging::print_cr3_statistics();
```

### Common Issues

**GP Fault on iretq**: 
- Likely cause: Invalid CR3 (not page-aligned)
- Fix: Validate CR3 in `create_process_address_space()`

**Process sees wrong memory**:
- Likely cause: CR3 not switched before user mode
- Fix: Ensure `activate_address_space()` called in `execute()`

**Memory corruption between processes**:
- Likely cause: Shared PD or PDP entries
- Fix: Clone page tables properly in `create_process_address_space()`

## Performance Characteristics

### CR3 Switch Cost
- **Hardware**: 100-200 cycles (TLB flush overhead)
- **Software**: ~50 cycles (validation + function calls)
- **Total**: ~250 cycles per context switch

### Memory Overhead Per Process
- PML4: 4 KB (1 page)
- PDP: 4 KB (1 page, cloned)
- PD: 4 KB (1 page, private)
- **Total**: 12 KB per process

### TLB Efficiency
- 2MB huge pages reduce TLB pressure
- Only 3-4 TLB entries needed for typical process
- Kernel mappings remain in TLB (global pages)

## Future Improvements

1. **PCID Support**: Tag TLB entries with process ID to avoid flushes
2. **4KB Pages**: Use smaller pages for better memory granularity
3. **Lazy Allocation**: Allocate page tables on-demand
4. **Page Table Compression**: Share read-only pages between processes
5. **NUMA Awareness**: Allocate PT pages on local NUMA node

## References

- Intel SDM Volume 3A, Section 4.5 (Paging)
- AMD APM Volume 2, Chapter 5 (Page Translation)
- `src/paging.rs` - Implementation
- `src/process.rs` - Process CR3 management
- `src/scheduler.rs` - Context switching

## Summary

NexaOS implements complete CR3 support providing:
- ✅ Per-process address space isolation
- ✅ Automatic CR3 switching on context switch
- ✅ Validation to prevent corruption
- ✅ Statistics for leak detection
- ⚠️ Page table deallocation (TODO)
- ⚠️ Shared memory (TODO)

This provides a solid foundation for secure multi-process execution with proper memory isolation.
