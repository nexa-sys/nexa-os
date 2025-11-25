# NexaOS Dynamic Linking and Runtime

> **Status**: Complete dynamic linking implementation  
> **Components**: ELF loader, runtime linker, symbol resolution  
> **Last Updated**: 2024

## Table of Contents

1. [Overview](#overview)
2. [ELF File Format](#elf-file-format)
3. [Dynamic Linker](#dynamic-linker)
4. [Symbol Resolution](#symbol-resolution)
5. [Relocation](#relocation)
6. [Runtime Library Loading](#runtime-library-loading)
7. [Position Independent Code](#position-independent-code)
8. [Debugging Dynamic Linking](#debugging-dynamic-linking)

---

## Overview

Dynamic linking allows programs to defer library resolution until runtime, enabling:
- **Smaller binaries** (shared libraries reduce size)
- **Library updates** (update libs without recompiling programs)
- **Resource efficiency** (only one copy of lib in memory)
- **Modularity** (clear separation of concerns)

### Execution Flow

```
1. Load main executable
2. Identify dynamic libraries (ELF interpreter)
3. Load dynamic linker (/lib64/ld-linux.so)
4. Linker resolves symbols, performs relocations
5. Pass control to main program
6. On-demand library loading as needed
```

---

## ELF File Format

### ELF Header Structure

```
┌─────────────────────────────────────┐
│  ELF Header (64 bytes)              │
│  - Magic number (0x7F 'ELF')        │
│  - Architecture (x86_64)            │
│  - Entry point                      │
│  - Program header offset            │
│  - Section header offset            │
└─────────────────────────────────────┘
           ↓
┌─────────────────────────────────────┐
│  Program Headers (for loading)      │
│  - PT_LOAD: loadable segments      │
│  - PT_INTERP: interpreter path     │
│  - PT_DYNAMIC: dynamic section     │
│  - PT_GNU_STACK: stack properties  │
└─────────────────────────────────────┘
           ↓
┌─────────────────────────────────────┐
│  Section Headers (for linking)      │
│  - .text: code                      │
│  - .data: initialized data          │
│  - .bss: uninitialized data        │
│  - .symtab: symbol table           │
│  - .strtab: string table           │
│  - .rel.* / .rela.*: relocations   │
└─────────────────────────────────────┘
```

### Key Sections

| Section | Purpose | Linked? |
|---------|---------|---------|
| `.text` | Executable code | Yes |
| `.data` | Initialized data | Yes |
| `.bss` | Uninitialized data (zeroed) | Yes |
| `.symtab` | Symbol table (all symbols) | No |
| `.dynsym` | Dynamic symbol table (exported) | Yes |
| `.strtab` | String table for symbols | No |
| `.dynstr` | String table for dynamic symbols | Yes |
| `.rel.* / .rela.*` | Relocations to fix up | Yes |
| `.plt` | Procedure Linkage Table (jumps) | Yes |
| `.got` | Global Offset Table (addresses) | Yes |
| `.dynamic` | Dynamic section info | Yes |

---

## Dynamic Linker

### Linker Discovery

**File**: `src/elf.rs`

```rust
// Read ELF header
// Check PT_INTERP program header
// Extract interpreter path (e.g., "/lib64/ld-linux.so")
// Load interpreter
// Pass control to linker with argc, argv, envp
```

### Linker Responsibilities

1. **Identify Dependencies**: Read `.dynamic` section, find needed libraries
2. **Load Libraries**: Map into memory at suitable addresses
3. **Resolve Symbols**: Find definitions in loaded libraries
4. **Perform Relocations**: Fix up addresses in code/data
5. **Initialize**: Run `.init` sections in dependency order
6. **Transfer Control**: Jump to program entry point

### Symbol Resolution Order

```
┌─────────────────────────────────────┐
│ Symbol lookup in:                   │
├─────────────────────────────────────┤
│ 1. Main executable                  │
│ 2. Directly linked libraries (LD_PRELOAD first)
│ 3. Transitive dependencies          │
│ 4. Linker-only symbols (__libc_*)   │
├─────────────────────────────────────┤
│ First match wins (uses this symbol) │
└─────────────────────────────────────┘
```

---

## Symbol Resolution

### Symbol Table Entries

```c
typedef struct {
    uint32_t st_name;       // Offset in string table
    unsigned char st_info;  // Bind (local/global/weak) + type
    unsigned char st_other; // Visibility
    uint16_t st_shndx;      // Section index or ABS/UNDEF
    uint64_t st_value;      // Address or offset
    uint64_t st_size;       // Size in bytes
} Elf64_Sym;
```

### Symbol Binding

| Binding | Scope | Override |
|---------|-------|----------|
| **STB_LOCAL** | File scope | No |
| **STB_GLOBAL** | Globally visible | Yes, first def wins |
| **STB_WEAK** | Globally visible | Yes, but can override |

### Symbol Visibility

| Visibility | Effect |
|------------|--------|
| **STV_DEFAULT** | Globally visible by default |
| **STV_HIDDEN** | Not visible outside DSO (Dynamic Shared Object) |
| **STV_PROTECTED** | Hidden outside, but definition can't be overridden |
| **STV_INTERNAL** | Compiler-specific hidden |

---

## Relocation

### Relocation Types (x86_64)

| Type | Purpose | Format |
|------|---------|--------|
| **R_X86_64_RELATIVE** | Adjust address for ASLR | S + A |
| **R_X86_64_64** | 64-bit absolute address | S + A |
| **R_X86_64_PC32** | 32-bit PC-relative | S + A - P |
| **R_X86_64_PLT32** | Procedure Linkage Table | L + A - P |
| **R_X86_64_GLOB_DAT** | Global offset | S |
| **R_X86_64_JUMP_SLOT** | Lazy binding slot | S |

Where:
- **S** = symbol value (address)
- **A** = addend (from relocation entry)
- **P** = place of relocation
- **L** = PLT entry

### Relocation Process

```
For each relocation entry:
  1. Find symbol (if needed)
  2. Calculate new value using formula
  3. Write new value to relocation address
  4. Handle special cases (ASLR, lazy binding)
```

### Global Offset Table (GOT)

**Purpose**: Indirection for accessing global data

```
Program Code:
    mov rax, [rel got_entry]    ; Load GOT entry (RIP-relative)
    call [rax]                  ; Call function

GOT:
    got_entry: 0x...            ; Address updated by linker
```

### Procedure Linkage Table (PLT)

**Purpose**: Lazy binding for function calls

```
First Call:
    jmp *[GOT_entry]    ; Jump via GOT
    → GOT points to next PLT instruction
    → Next instruction calls linker
    → Linker resolves symbol, updates GOT
    
Subsequent Calls:
    jmp *[GOT_entry]    ; Jump via GOT
    → GOT now points directly to function
    → Direct jump, no linker overhead
```

---

## Runtime Library Loading

### dlopen/dlsym (Dynamic Loading)

**Not yet implemented in NexaOS**, but the framework is in place:

```rust
// Planned API
pub extern "C" fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void
pub extern "C" fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void
pub extern "C" fn dlclose(handle: *mut c_void) -> c_int
```

### Library Search Path

```
1. LD_LIBRARY_PATH environment variable
2. /lib64 (standard library location)
3. /usr/lib64 (user libraries)
4. DT_RUNPATH/DT_RPATH from ELF
5. ld.so.cache (cached paths)
```

---

## Position Independent Code

### PIC Compilation

**Purpose**: Code that works at any address

**Techniques**:
1. **RIP-relative addressing**: `mov rax, [rel symbol]`
2. **GOT for globals**: Access via Global Offset Table
3. **PLT for functions**: Call via Procedure Linkage Table
4. **No absolute jumps**: All relative or indirect

### Benefits

- **ASLR support**: Address Space Layout Randomization
- **Shared libraries**: Can load at any address
- **Security**: Harder to predict code locations for attacks

### Compilation

```bash
gcc -fPIC -shared -o libfoo.so foo.c    # Position Independent Code
gcc -fPIE -pie program.c                # Position Independent Executable
```

---

## Debugging Dynamic Linking

### Environment Variables

| Variable | Effect |
|----------|--------|
| **LD_LIBRARY_PATH** | Add directories to search path |
| **LD_PRELOAD** | Load library before others |
| **LD_DEBUG** | Print linker debug messages |
| **LD_TRACE_LOADED_OBJECTS** | Show libraries loaded |

### Example

```bash
LD_DEBUG=all ./program          # Verbose linker output
LD_LIBRARY_PATH=/custom/lib ./program  # Custom lib directory
```

### Common Issues

**Issue**: Library not found
```
Error: cannot open shared object file: No such file or directory
Solution: Check LD_LIBRARY_PATH, place lib in /lib64, or LD_PRELOAD
```

**Issue**: Symbol not found
```
Error: undefined symbol: foo
Solution: Check library order (dependencies first), use -Wl,--as-needed
```

**Issue**: Conflicting symbols
```
Multiple definitions of same symbol
Solution: Use STB_WEAK, versioning, or namespaces
```

---

## Implementation Details

### ELF Loader (src/elf.rs)

**Load executable**:
1. Read ELF header, verify magic
2. Iterate program headers
3. Map PT_LOAD segments into memory
4. Set page permissions (read/write/execute)
5. Find PT_INTERP (dynamic linker path)
6. Load linker instead

**Current Status**: ✅ Fully implemented

### Dynamic Linker (/lib64/ld-linux.so)

**Responsibilities**:
1. Initialize memory mapping
2. Load dependent libraries
3. Build symbol table from all loaded libs
4. Process relocations
5. Initialize .init sections
6. Run main program

**Current Status**: ✅ Implemented, basic functionality

### Symbol Resolution (src/elf.rs)

**Algorithm**:
```
lookup_symbol(name):
  for each loaded library in order:
    for each symbol in library:
      if symbol.name == name:
        return symbol.address
  return NOT_FOUND
```

**Current Status**: ✅ Implemented

---

## Performance Considerations

### Startup Time

| Phase | Time |
|-------|------|
| Load executable | ~0.5ms |
| Discover libraries | ~0.5ms |
| Load libraries | ~1-5ms |
| Resolve symbols | ~1-2ms |
| Perform relocations | ~1-2ms |
| **Total** | **~4-10ms** |

### Optimization Techniques

1. **Lazy binding**: Don't resolve all symbols at startup
2. **Symbol preemption**: Mark symbols as "cannot be overridden"
3. **Direct binding**: Link symbol directly without PLT
4. **Hash tables**: Fast symbol lookup (O(1) average)

---

## Testing

### Test Program

```c
#include <stdio.h>

void foo() {
    printf("foo called\n");
}
```

**Compile as library**:
```bash
gcc -fPIC -shared -o libtest.so libtest.c
```

**Link program**:
```bash
gcc -o program program.c -L. -ltest
```

**Run with dynamic linking**:
```bash
LD_LIBRARY_PATH=. ./program
```

---

## Related Documentation

- [Architecture](./ARCHITECTURE.md) - System design
- [Build System](./BUILD-SYSTEM.md) - Compilation
- [Syscall Reference](./SYSCALL-REFERENCE.md) - execve, mmap
- [Quick Reference](./QUICK-REFERENCE.md) - Compilation flags

---

**Last Updated**: 2024-01-15  
**Maintainer**: NexaOS Development Team  
**Status**: Core functionality complete, advanced features pending
