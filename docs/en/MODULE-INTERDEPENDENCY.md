# NexaOS Module Inter-Dependency Support

This document describes how kernel modules can declare dependencies on other modules
and export symbols for inter-module FFI (Foreign Function Interface).

## Overview

NexaOS supports:
- **Module dependencies**: Modules can depend on other modules
- **Inter-module FFI**: Modules can export functions/data for use by other modules
- **Circular dependency detection**: The kernel prevents circular dependencies at load time
- **GPL-only symbols**: Some symbols can be restricted to GPL-compatible modules

## Declaring Dependencies

Modules declare dependencies by defining special symbols with the `__kmod_depends_` prefix.

### Example: Declaring a Dependency on "ext2"

```rust
// This module depends on the "ext2" module being loaded first
#[no_mangle]
#[used]
static __kmod_depends_ext2: u8 = 0;
```

### Example: Multiple Dependencies

```rust
// Depends on both ext2 and virtio_common modules
#[no_mangle]
#[used]
static __kmod_depends_ext2: u8 = 0;

#[no_mangle]
#[used]
static __kmod_depends_virtio_common: u8 = 0;
```

### Macro Helper

You can use a macro to simplify dependency declarations:

```rust
/// Declare a module dependency
macro_rules! kmod_depends {
    ($name:ident) => {
        paste::paste! {
            #[no_mangle]
            #[used]
            static [<__kmod_depends_ $name>]: u8 = 0;
        }
    };
}

// Usage:
kmod_depends!(ext2);
kmod_depends!(virtio_common);
```

## Exporting Symbols for Inter-Module FFI

Modules can export functions and data for use by other modules by using the
`kmod_export_` or `__kmod_export_` prefix.

### Exporting a Function

```rust
/// A function that can be called by other modules
#[no_mangle]
pub extern "C" fn kmod_export_my_helper_function(arg: u32) -> i32 {
    // Implementation
    arg as i32
}
```

The exported symbol name will be `my_helper_function` (prefix is stripped).

### Exporting GPL-Only Symbols

Append `_gpl` suffix to restrict the symbol to GPL-compatible modules:

```rust
/// GPL-only function (only callable by GPL-compatible modules)
#[no_mangle]
pub extern "C" fn kmod_export_internal_api_gpl(arg: u32) -> i32 {
    // Implementation
    arg as i32
}
```

### Exporting Data

```rust
/// Exported configuration structure
#[no_mangle]
#[used]
pub static kmod_export_my_config: MyConfig = MyConfig {
    version: 1,
    flags: 0,
};
```

## Consuming Symbols from Other Modules

To use symbols exported by other modules, declare them as external:

```rust
// First, declare the dependency
#[no_mangle]
#[used]
static __kmod_depends_ext2: u8 = 0;

// Then declare the external symbol
extern "C" {
    // This symbol is exported by the ext2 module
    fn ext2_helper_function(arg: u32) -> i32;
}

// Use it in your code
fn my_function() {
    unsafe {
        let result = ext2_helper_function(42);
    }
}
```

## Load Order and Dependency Resolution

The kernel automatically:

1. **Parses dependencies** from the ELF binary's symbols
2. **Checks for circular dependencies** using topological sort
3. **Verifies all dependencies are loaded** before loading the module
4. **Increments reference counts** on dependent modules
5. **Prevents unloading** of modules that others depend on

### Error Cases

- **Missing dependency**: If a required module is not loaded, the load fails
- **Circular dependency**: If modules form a dependency cycle, the load fails
- **GPL violation**: If a non-GPL module tries to use a GPL-only symbol, the link fails

## Example: ext3 Module Depending on ext2

```rust
//! ext3 Filesystem Kernel Module
//! 
//! Extends ext2 with journaling support

#![no_std]

// ============================================================================
// Module Dependencies
// ============================================================================

// ext3 is built on top of ext2, so we depend on it
#[no_mangle]
#[used]
static __kmod_depends_ext2: u8 = 0;

// ============================================================================
// External APIs from ext2 module
// ============================================================================

extern "C" {
    // Functions exported by ext2 module
    fn ext2_read_inode(inode_num: u32) -> *mut u8;
    fn ext2_write_inode(inode_num: u32, data: *const u8) -> i32;
    fn ext2_allocate_block() -> u64;
}

// ============================================================================
// ext3-specific exports (for future ext4)
// ============================================================================

/// Journal entry structure
#[repr(C)]
pub struct JournalEntry {
    pub block_num: u64,
    pub data: [u8; 4096],
}

/// Export the journal write function for potential ext4 use
#[no_mangle]
pub extern "C" fn kmod_export_journal_write(entry: *const JournalEntry) -> i32 {
    // Journal write implementation
    0
}

/// Export journal replay (GPL-only for now)
#[no_mangle]
pub extern "C" fn kmod_export_journal_replay_gpl() -> i32 {
    // Journal replay implementation
    0
}

// ============================================================================
// Module Entry Points
// ============================================================================

#[no_mangle]
pub extern "C" fn module_init() -> i32 {
    // ext2 is guaranteed to be loaded at this point
    // We can safely use ext2 APIs
    0
}

#[no_mangle]
pub extern "C" fn module_exit() -> i32 {
    0
}
```

## API Reference

### Kernel Functions

```rust
// Check for circular dependencies before loading
pub fn check_circular_dependencies(
    module_name: &str,
    deps: &[&str],
) -> Result<(), CircularDependencyError>;

// Load module with automatic dependency handling
pub fn load_module_with_dependency_check(
    name: &str,
    data: &[u8],
    deps: &[&str],
) -> Result<(), ModuleError>;

// Lookup a symbol from loaded modules
pub fn lookup_module_symbol(name: &str) -> Option<(String, u64)>;

// Register an exported symbol
pub fn register_module_symbol(
    module_name: &str,
    symbol_name: &str,
    address: u64,
    sym_type: ExportedSymbolType,
    gpl_only: bool,
) -> Result<(), ModuleError>;

// Get dependency graph for debugging
pub fn print_dependency_graph();
```

## Best Practices

1. **Minimize dependencies**: Only depend on what you actually need
2. **Use GPL-only sparingly**: Only for truly internal APIs
3. **Document exported APIs**: Other modules will rely on them
4. **Version your APIs**: Use versioned symbol names if needed
5. **Test load order**: Ensure modules load correctly in various orders

## Debugging

Use the kernel's `print_dependency_graph()` function to visualize module dependencies:

```
Module Dependency Graph:
========================
  ext2 (no dependencies)
    exports: read_inode, write_inode, allocate_block
  ext3 -> [ext2]
    exports: journal_write, journal_replay_gpl
  ext4 -> [ext3]
    exports: extent_allocate, extent_lookup
```
