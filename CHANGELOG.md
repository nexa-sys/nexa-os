# NexaOS Changelog

## [Unreleased] - 2025-11-03

### Added - Dynamic Service Configuration System

#### Configuration File Support
- **Dynamic Service Loading** (`userspace/init.rs`)
  - `load_config()` function to read `/etc/inittab` at boot time
  - `parse_config_line()` for parsing service definitions
  - `run_service_loop()` for individual service supervision
  - Support for comments (`#` prefixed lines) and empty lines
  - Format: `SERVICE_PATH RUNLEVEL` (simplified from traditional init)

#### System Call Wrappers
- Added `syscall2()` helper for two-argument syscalls
- `open()` - open configuration files
- `read()` - read file content into buffer
- `close()` - close file descriptors

#### Filesystem Integration
- Auto-created `/etc/inittab` default configuration in `src/fs.rs`
- Default configuration includes helpful comments and example format
- Automatic fallback if initramfs doesn't include custom inittab

#### User Experience
- Improved boot logging showing "Loaded services from /etc/inittab"
- Service count display during initialization
- Individual service status messages with runlevel information
- Seamless multi-service startup capability

### Changed - Init System (POSIX/Unix-like Compliant)

#### Core Init System
- **Complete Init System** (`src/init.rs`, 540 lines)
  - PID 1 process management following Unix conventions
  - System V runlevel support (0-6: halt, single, multi-user, network, unused, GUI, reboot)
  - Service management with automatic respawn capability
  - Fork bomb prevention (max 5 respawns per minute)
  - `/etc/inittab` configuration file parser
  - System shutdown and reboot functionality
  - Orphan process reparenting to init

#### New System Calls
- `SYS_REBOOT (169)`: Linux-compatible reboot with magic numbers
  - `0x01234567` - Restart
  - `0x4321FEDC` - Halt
  - `0xCDEF0123` - Power off
- `SYS_SHUTDOWN (230)`: Graceful system shutdown
- `SYS_RUNLEVEL (231)`: Get/set system runlevel (0-6)

#### Process Management
- PPID (Parent Process ID) support in process structure
- New process methods: `set_ppid()`, `pid()`, `ppid()`, `state()`
- Enhanced process exit handling with init notification
- Automatic service respawn on crash

#### Authentication & Security
- `is_superuser()`: Check for root/admin privileges
- `current_uid()`: Get current user ID
- `current_gid()`: Get current group ID
- Privilege checks for all init operations

#### Configuration & Documentation
- `/etc/inittab`: Standard Unix init configuration example
- `docs/zh/init-system.md`: Complete design documentation (Chinese)
- `docs/zh/INIT_IMPLEMENTATION_SUMMARY.md`: Implementation guide

#### Standards Compliance
- ✅ POSIX process management (PID hierarchy, signals)
- ✅ Unix-like init conventions (PID 1, runlevels, inittab)
- ✅ Hybrid kernel architecture optimizations
- ✅ System V init compatibility

### Changed
- Enhanced kernel initialization sequence (`src/lib.rs`)
  - Added `init::init()` to subsystem initialization order
  - Improved init program search with detailed logging
  - Added `/etc/inittab` loading at boot time
  - Better error messages for missing init programs
- Updated `syscall_exit()` for init system integration
  - Process death notification to init
  - Zombie state handling
  - Automatic service respawn trigger

### Security
- All init syscalls require superuser privileges (UID 0 or admin flag)
- Fork bomb protection with respawn rate limiting
- PID 1 protected from termination (kernel panic if attempted)
- Privilege separation between kernel and user mode

### Technical Details
- **New code**: ~1225 lines across 6 files
- **Compilation**: ✅ Success (3 harmless warnings)
- **Architecture**: Hybrid kernel (kernel-mode management + user-mode init)

## [Previous] - 2025-11-03

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
