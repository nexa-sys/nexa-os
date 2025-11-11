# Testing Guide for Release Build Buffer Error Fix

## Prerequisites

Before testing, ensure you have the following installed:
- Rust nightly toolchain
- QEMU (qemu-system-x86_64)
- GRUB tools (grub-mkrescue, grub-pc-bin)
- xorriso
- lld linker

## Build and Test Procedure

### 1. Build the System

```bash
# Clean previous builds
rm -rf build/ dist/ target/

# Build kernel in release mode
cargo build --release

# Build userspace in release mode
./scripts/build-userspace.sh

# Create bootable ISO
./scripts/build-iso.sh
```

### 2. Run in QEMU

```bash
./scripts/run-qemu.sh
```

### 3. Test Scenarios

Once the system boots and you get a shell prompt, test the following:

#### Basic Output Test
```bash
> help
```
Expected: Command list displayed without errors

#### String Output Test
```bash
> echo Hello World
```
Expected: "Hello World" displayed without buffer errors

#### File Listing Test
```bash
> ls
```
Expected: File list displayed without errors

#### Multiple Commands Test
Run several commands in succession:
```bash
> pwd
> ls
> echo test
> help
```
Expected: All commands work without "invalid user buffer" errors

### 4. Monitor for Errors

Watch the serial console output for any of these error messages:
```
[ERROR] sys_write: invalid user buffer fd=1 buf=0x... count=...
```

If you see these errors, the fix did not work correctly.

### 5. Check Kernel Logs

The kernel logs should show:
- Successful kernel initialization
- User process loading
- No buffer validation errors

Example of good output:
```
[    0.000000] [INFO] NexaOS kernel starting...
[    0.123456] [INFO] Userspace layout: phys_base=0x400000, virt_base=0x400000, stack_base=0x800000
[    0.234567] [INFO] ELF loaded successfully, physical_entry=0x400xxx
```

## Comparing Debug vs Release Builds

To verify the fix, you can compare debug and release builds:

### Build Debug Version
```bash
cargo build
./scripts/build-userspace.sh
./scripts/build-iso.sh
```

### Build Release Version
```bash
cargo build --release
./scripts/build-userspace.sh
./scripts/build-iso.sh
```

Both versions should now work identically without buffer errors.

## What to Look For

### Before Fix (Broken Release Build)
- Error messages about invalid buffers with addresses like `0x803e6e00`
- Commands fail to output text
- System appears to hang or behave erratically

### After Fix (Working Release Build)
- No buffer validation errors
- All commands produce expected output
- System behaves identically to debug build

## Debugging Tips

If you still see issues after applying the fix:

1. **Verify relocation model**:
   ```bash
   grep relocation x86_64-nexaos.json
   ```
   Should show: `"relocation-model": "static"`

2. **Verify LTO is disabled**:
   ```bash
   grep lto scripts/build-userspace.sh
   ```
   Should show: `lto = false`

3. **Check binary size**:
   ```bash
   ls -lh build/initramfs/bin/sh
   ```
   With LTO disabled, the binary should be larger (~50KB vs ~37KB)

4. **Inspect ELF binary**:
   ```bash
   readelf -h build/initramfs/bin/sh | grep Type
   ```
   Should show: `Type: EXEC (Executable file)`

5. **Check for PIC relocations**:
   ```bash
   readelf -r build/initramfs/bin/sh
   ```
   Should show minimal or no relocations (static linking)

## Automated Testing (Future)

Consider adding these automated tests:

1. QEMU expect script to run commands and check output
2. Unit tests for buffer validation logic
3. Integration tests that verify syscall behavior
4. Fuzzing for edge cases in buffer validation

## Reporting Issues

If you encounter problems:

1. Capture the full serial output
2. Note the exact commands that trigger the error
3. Record the buffer addresses shown in error messages
4. Try with different optimization levels (`opt-level`)
5. Test with different Rust/LLVM versions

Report to the issue tracker with this information.
