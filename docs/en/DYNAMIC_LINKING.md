# Dynamic Linking Support in NexaOS

## Overview

NexaOS now includes support for loading dynamically linked executables via the PT_INTERP mechanism. This document describes the implementation, current status, and future work needed for complete dynamic linking support.

## Implementation

### Kernel-Side Support

#### ELF Loader Enhancements (`src/elf.rs`)

The ELF loader has been enhanced with the following methods:

1. **`has_interpreter()`**: Checks if an ELF file has a PT_INTERP segment
2. **`get_interpreter()`**: Extracts the interpreter path from the PT_INTERP segment (typically `/lib64/ld-linux-x86-64.so.2`)

These methods scan the program headers looking for `PhType::Interp` segments and extract the null-terminated interpreter path string.

#### Process Loading (`src/process.rs`)

The `Process::from_elf()` function has been modified to:

1. Check if the executable has a PT_INTERP segment
2. If present, attempt to load the interpreter from the filesystem
3. Load and execute the interpreter instead of the original program
4. Fall back to static linking if the interpreter is not found

### Dynamic Linker Availability

The system includes the host's dynamic linker (`ld-linux-x86-64.so.2`) in both:
- **Initramfs**: `/lib64/ld-linux-x86-64.so.2` (for early boot programs)
- **Rootfs**: `/lib64/ld-linux-x86-64.so.2` (for regular programs)

This is added during the build process via:
- `scripts/build-userspace.sh` (initramfs)
- `scripts/build-rootfs.sh` (rootfs)

## Current Status

### What Works

✅ **PT_INTERP Detection**: The kernel can detect dynamically linked executables
✅ **Interpreter Loading**: The kernel loads the dynamic linker instead of the program
✅ **Filesystem Integration**: Dynamic linker is available in both initramfs and rootfs
✅ **Static Binary Fallback**: Static executables continue to work as before

### What's Missing

❌ **Auxiliary Vectors**: The dynamic linker needs information about the original program via auxiliary vectors (AT_PHDR, AT_ENTRY, AT_PHNUM, etc.)
❌ **Program Loading**: Currently, only the interpreter is loaded; the original program needs to be loaded into a separate memory region
❌ **Shared Libraries**: No support for loading shared libraries (`.so` files)
❌ **Symbol Resolution**: No runtime symbol resolution or relocation processing
❌ **GOT/PLT Support**: No support for Global Offset Table or Procedure Linkage Table

## How Dynamic Linking Should Work

### Complete Flow

1. **Kernel loads program**:
   - Parse ELF headers and find PT_INTERP segment
   - Load interpreter (`ld-linux.so`) at a suitable address
   - Load original program at its requested virtual addresses
   - Set up auxiliary vectors on the stack with program information
   - Transfer control to interpreter's entry point

2. **Interpreter executes**:
   - Read auxiliary vectors to find program info
   - Parse program's PT_DYNAMIC segment
   - Load required shared libraries (from DT_NEEDED entries)
   - Perform relocations (DT_RELA, DT_REL)
   - Resolve symbols (DT_SYMTAB, DT_STRTAB)
   - Initialize libraries (DT_INIT, DT_INIT_ARRAY)
   - Transfer control to program's entry point

3. **Program runs**:
   - Access library functions via PLT/GOT
   - Dynamic linker handles lazy binding if configured

### Required Auxiliary Vectors

The kernel should pass these on the user stack:

```c
AT_PHDR    = 3   // Address of program headers
AT_PHENT   = 4   // Size of program header entry
AT_PHNUM   = 5   // Number of program headers
AT_PAGESZ  = 6   // System page size
AT_BASE    = 7   // Interpreter base address
AT_FLAGS   = 8   // Flags
AT_ENTRY   = 9   // Program entry point
AT_UID     = 11  // Real user ID
AT_EUID    = 12  // Effective user ID
AT_GID     = 13  // Real group ID
AT_EGID    = 14  // Effective group ID
```

## Testing Dynamic Linking

### Create a Test Program

```bash
# Create a simple dynamically linked program
cat > test_dynamic.c << 'EOF'
#include <stdio.h>
int main() {
    printf("Hello from dynamic program!\n");
    return 0;
}
EOF

gcc test_dynamic.c -o test_dynamic

# Verify it's dynamically linked
readelf -l test_dynamic | grep INTERP
# Should show: [Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]
```

### Current Behavior

When a dynamically linked program is loaded:
1. Kernel detects PT_INTERP segment
2. Kernel loads the dynamic linker
3. Kernel jumps to linker's entry point
4. ⚠️ **Linker fails** because it doesn't receive auxiliary vectors with program information

### Expected Behavior

The program should execute successfully after the kernel passes proper auxiliary vectors to the linker.

## Future Work

### Phase 1: Auxiliary Vectors (High Priority)

Implement stack setup to pass auxiliary vectors to the dynamic linker:

```rust
// In process.rs, before jumping to user mode
fn setup_auxv_stack(stack: &mut [u64], program_info: &ProgramInfo) {
    // Push auxiliary vectors onto stack
    // Format: [type, value, type, value, ..., AT_NULL, 0]
}
```

### Phase 2: Memory Layout (High Priority)

Load both the interpreter and program into memory:
- Interpreter at a fixed base (e.g., 0x7000_0000_0000)
- Program at its requested virtual addresses
- Map both with proper permissions

### Phase 3: Shared Library Support (Medium Priority)

Add filesystem support for:
- `/lib64/*.so` - 64-bit shared libraries
- `/lib/*.so` - 32-bit shared libraries (optional)

### Phase 4: Custom Dynamic Linker (Low Priority)

Optionally implement a minimal NexaOS-specific dynamic linker for:
- Better integration with the kernel
- Reduced size and complexity
- Custom security features

## Architecture Considerations

### Memory Layout with Dynamic Linking

```
┌────────────────────────┐ 0x7FFF_FFFF_FFFF
│      User Stack        │
├────────────────────────┤ 0x7000_0000_0000 (proposed)
│  Dynamic Linker        │
│  (ld-linux.so)         │
├────────────────────────┤ 0x1000_0000_0000
│  Shared Libraries      │
│  (.so files)           │
├────────────────────────┤ 0x0040_0000 (current USER_VIRT_BASE)
│  Program Text/Data     │
└────────────────────────┘ 0x0000_0000_0000
```

### Security Considerations

- **ASLR**: Consider Address Space Layout Randomization for security
- **RELRO**: Support for read-only relocations
- **BIND_NOW**: Force immediate binding instead of lazy binding
- **DT_RUNPATH**: Restrict library search paths

## References

- [ELF Specification](https://refspecs.linuxfoundation.org/elf/elf.pdf)
- [System V ABI](https://refspecs.linuxfoundation.org/elf/x86_64-abi-0.99.pdf)
- [ld.so(8) Manual](https://man7.org/linux/man-pages/man8/ld.so.8.html)
- [Auxiliary Vectors](https://lwn.net/Articles/519085/)

## Summary

The current implementation provides the foundation for dynamic linking by:
1. Detecting dynamically linked executables
2. Loading the dynamic linker
3. Including the linker in the filesystem

To fully support dynamically linked programs, auxiliary vectors must be implemented to pass program information to the dynamic linker. This is the critical missing piece that prevents dynamic programs from executing correctly.
