# Rust std::io::stdout() Initialization Hang - Diagnostic Report

**Date**: 2024-11-10
**Status**: Identified Root Cause (Not Yet Fixed)
**Severity**: High (Blocks use of Rust std I/O)

## Problem Summary

Calling `std::io::stdout()` in NexaOS userspace programs causes the entire process to hang indefinitely. This issue affects any code using Rust's standard I/O functions.

### Test Results

```
[init] Test 3.3: Calling io::stdout()...
[SYSTEM HANGS - No progress after 30+ seconds]
```

**Key Findings**:
- ✅ Direct `write()` syscall works perfectly (STDOUT and STDERR)
- ✅ `eprintln!()` macro works perfectly (uses STDERR internally)
- ✅ All nrlib libc functions work correctly when called
- ❌ `std::io::stdout()` call itself causes hang
- ❌ No nrlib libc functions are called during the hang
- ❌ `pthread_once()` diagnostics never appear (not reached)
- ❌ `isatty()` diagnostics never appear (not called)

## Root Cause Analysis

The hang occurs in **Rust std's internal initialization code**, not in NexaOS kernel or nrlib libc layer.

**Evidence**:
1. The hang happens BEFORE any libc function calls
2. None of our diagnostic messages in pthread_once, isatty, fcntl appear
3. The nrlib/libc layer is working correctly (direct syscalls and eprintln work)
4. This suggests Rust std itself has initialization code that gets stuck

**Possible Causes** (in order of likelihood):
1. Rust std's `Once` or `OnceLock` implementation has a deadlock
2. Rust std is trying to access TLS (thread-local storage) which isn't properly initialized
3. Some global variable initialization code in Rust std is stuck in a loop
4. Missing symbol or incorrect function signature in nrlib causing undefined behavior

## Workaround Status

**Current**: Use `eprintln!()` for output instead of `println!()` or `std::io::stdout()`
- Works perfectly - prints to STDERR
- No performance issues
- Suitable for diagnostic and logging output

## Root Cause Hypotheses to Investigate

### Hypothesis 1: Once/OnceLock Issue
Rust std might use `Once` to initialize stdout, and our `pthread_once()` implementation might have issues.

**Next Steps**:
- Check if `pthread_once()` is working correctly by testing it directly
- Look for race conditions in the Once-based initialization

### Hypothesis 2: TLS (Thread-Local Storage) Issue
Rust std accesses thread-local storage during initialization.

**Next Steps**:
- Check if `pthread_key_create()` and `pthread_setspecific()` work correctly
- Verify GS register setup for TLS
- Test with minimal TLS access

### Hypothesis 3: Static Variable Initialization
Some global variable in Rust std is getting stuck during initialization.

**Next Steps**:
- Enable Rust std's debug logging to see what's happening
- Instrument the Rust std source directly
- Use strace-like syscall tracing

### Hypothesis 4: Memory Allocation
Rust std might be trying to allocate memory and getting stuck.

**Next Steps**:
- Check malloc/free implementation in nrlib
- Verify heap is properly initialized
- Test memory allocation directly

## Technical Details

### Code Path Analysis

When `std::io::stdout()` is called:
1. Rust std accesses a global `STDOUT` static variable
2. The static is probably wrapped in `OnceLock` or similar
3. Initialization should call `pthread_once()` or similar synchronization
4. **Hang occurs here** - somewhere in the initialization chain

### Rust std Relevant Code

The relevant code is in Rust std's `std/src/io/stdio.rs`:
- `pub fn stdout() -> Stdout`
- Internal use of `Once` or `OnceLock` for initialization
- Call to some libc function (likely `isatty()` or `fcntl()`)

### nrlib State

Current nrlib implementation:
- `pthread_once()` ✅ Implemented with diagnostics
- `pthread_key_create()` ✅ Implemented
- `pthread_setspecific()` ✅ Implemented
- `pthread_getspecific()` ✅ Implemented
- `isatty()` ✅ Implemented with diagnostics
- `fcntl()` ✅ Implemented with diagnostics

All these functions have diagnostic output but none are reached.

## Impact

**Affected Functionality**:
- `println!()` macro - doesn't work
- `std::io::stdout()` - doesn't work
- `std::io::print!()` - doesn't work

**Working Alternatives**:
- `eprintln!()` - works perfectly
- `write(fd, buf, len)` syscall - works perfectly
- Direct write to STDERR - works perfectly

## Recommendations

1. **Short-term** (Current): Use eprintln!() for all output
2. **Medium-term**: Create diagnostic tools to trace Rust std initialization
3. **Long-term**: Fix the underlying issue in either:
   - nrlib's pthread_once or TLS implementation
   - Rust std's initialization code
   - NexaOS kernel's thread/TLS support

## Testing

To reproduce:
```rust
use std::io;
let _h = io::stdout();  // This line hangs indefinitely
```

To test the workaround:
```rust
eprintln!("This works perfectly!");  // ✅ Prints successfully
```

## Build Configuration

- **Rust**: nightly
- **Build Std**: `-Z build-std=std,panic_abort`
- **Target**: `x86_64-nexaos-userspace.json`
- **Linker**: rust-lld
- **libc Replacement**: nrlib

## Next Actions

1. Run detailed diagnostic on `pthread_once()` behavior
2. Test if the issue is specific to initialization or runtime use
3. Check if using a different synchronization primitive helps
4. Consider disabling stdout initialization in Rust std (if possible)
5. Trace syscall sequence with custom syscall logging
