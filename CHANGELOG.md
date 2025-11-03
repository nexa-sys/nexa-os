# NexaOS Changelog

## [Unreleased] - 2025-11-03

### Fixed
- **Critical**: Fixed keyboard input in UEFI mode - characters now properly echo to screen
- **Shell**: Resolved borrow checker errors in tab completion implementation
- **Syscall**: Changed stdin read to raw mode (no kernel echo) for proper userspace control

### Added
- **Shell Tab Completion**: Added intelligent tab completion for:
  - Command names (all 19 built-in commands)
  - Path arguments for ls, cat, stat, cd, mkdir
  - Longest common prefix expansion
  - Multiple match display with directory indicators
- **New Shell Commands**:
  - `pwd` - Print working directory
  - `cd <path>` - Change current directory
  - `echo [text...]` - Print text to output
  - `uname [-a]` - Display system information
  - `mkdir <path>` - Create directory (stub for future implementation)
- **Enhanced Line Editing**:
  - Ctrl-C: Cancel current line
  - Ctrl-D: Exit shell (on empty line)
  - Ctrl-U: Clear entire line
  - Ctrl-W: Delete previous word
  - Ctrl-L: Refresh screen with current line
  - Backspace/Delete: Character deletion with proper cursor handling
  - Tab: Smart command/path completion
- **Improved Help System**: Extended help command with editing keys reference

### Changed
- **Kernel Keyboard Driver**: Added `read_raw()` function for non-echoing input
- **Syscall Interface**: Simplified `read_from_keyboard()` to use raw input mode
- **Shell Input Handling**: Moved echo responsibility from kernel to userspace
- **Path Completion**: Enhanced to show directory indicators (/) and filter hidden files intelligently

### Performance
- Reduced syscall overhead by eliminating double-buffering in stdin reads
- Optimized tab completion with pre-allocated buffers and early exits

### Code Quality
- Removed unused `MAX_STDIN_LINE` constant
- Fixed all compiler warnings in userspace shell
- Improved error handling in keyboard input loop
- Better separation of concerns between kernel and userspace input handling

## [0.1.0] - Previous Release

### Core Features
- x86_64 hybrid kernel with Multiboot2 boot
- POSIX-inspired syscall interface
- ELF userspace program loading
- PS/2 keyboard driver with US QWERTY layout
- VGA text mode and serial console output
- Basic filesystem (initramfs CPIO + runtime memory fs)
- User authentication system
- IPC channel support
- Interactive shell with 14+ commands
