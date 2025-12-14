//! Core Dump Implementation
//!
//! This module implements Linux-compatible core dump functionality for NexaOS.
//! It supports the /proc/sys/kernel/core_pattern configuration and generates
//! ELF core files for crashed processes.
//!
//! ## Core Pattern Format
//!
//! The core_pattern supports the following format specifiers:
//! - `%p` - PID of dumped process
//! - `%u` - Real UID of dumped process
//! - `%g` - Real GID of dumped process
//! - `%s` - Signal number causing dump
//! - `%t` - Time of dump (seconds since epoch)
//! - `%h` - Hostname
//! - `%e` - Executable filename (without path prefix)
//! - `%E` - Pathname of executable (/ replaced by !)
//! - `%%` - A single % character
//!
//! If core_pattern starts with '|', it specifies a pipe to a program.
//! Otherwise, it's a path template for the core file.

use crate::process::Pid;
use core::fmt::Write;
use spin::Mutex;

/// Maximum length of core_pattern string
pub const MAX_CORE_PATTERN: usize = 256;

/// Maximum length of generated core filename
pub const MAX_CORE_FILENAME: usize = 512;

/// Default core pattern - produces files like "core.1234"
pub const DEFAULT_CORE_PATTERN: &str = "core.%p";

/// Buffer for core pattern storage
static CORE_PATTERN: Mutex<CorePatternConfig> = Mutex::new(CorePatternConfig::new());

/// Core dump configuration
pub struct CorePatternConfig {
    /// The pattern string
    pattern: [u8; MAX_CORE_PATTERN],
    /// Length of the pattern
    len: usize,
    /// Whether core dumps are enabled (pattern is not empty)
    enabled: bool,
    /// Core pipe limit (max concurrent pipe handlers)
    pipe_limit: u32,
    /// Whether to use pipe mode (pattern starts with '|')
    pipe_mode: bool,
}

impl CorePatternConfig {
    const fn new() -> Self {
        let mut pattern = [0u8; MAX_CORE_PATTERN];
        // Initialize with default pattern "core.%p"
        let default = DEFAULT_CORE_PATTERN.as_bytes();
        let mut i = 0;
        while i < default.len() && i < MAX_CORE_PATTERN {
            pattern[i] = default[i];
            i += 1;
        }
        Self {
            pattern,
            len: DEFAULT_CORE_PATTERN.len(),
            enabled: true,
            pipe_limit: 0,
            pipe_mode: false,
        }
    }

    /// Get the pattern as a string slice
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.pattern[..self.len]).unwrap_or("")
    }

    /// Set a new pattern
    fn set_pattern(&mut self, new_pattern: &str) {
        let bytes = new_pattern.as_bytes();
        let copy_len = bytes.len().min(MAX_CORE_PATTERN);
        self.pattern[..copy_len].copy_from_slice(&bytes[..copy_len]);
        self.len = copy_len;
        self.enabled = copy_len > 0;
        self.pipe_mode = bytes.first() == Some(&b'|');
    }
}

/// Information needed to generate a core dump
#[derive(Clone, Copy)]
pub struct CoreDumpInfo {
    /// Process ID
    pub pid: Pid,
    /// User ID
    pub uid: u32,
    /// Group ID
    pub gid: u32,
    /// Signal that caused the dump
    pub signal: u32,
    /// Instruction pointer at crash
    pub rip: u64,
    /// Stack pointer at crash
    pub rsp: u64,
    /// Flags register
    pub rflags: u64,
    /// CR2 register (faulting address for page faults)
    pub cr2: u64,
    /// Error code (for exceptions that provide one)
    pub error_code: u64,
    /// Physical base of process memory
    pub memory_base: u64,
    /// Size of process memory region
    pub memory_size: usize,
    /// RAX register
    pub rax: u64,
    /// RBX register
    pub rbx: u64,
    /// RCX register
    pub rcx: u64,
    /// RDX register
    pub rdx: u64,
    /// RSI register
    pub rsi: u64,
    /// RDI register
    pub rdi: u64,
    /// RBP register
    pub rbp: u64,
}

impl CoreDumpInfo {
    /// Create new CoreDumpInfo with default values
    pub const fn new(pid: Pid) -> Self {
        Self {
            pid,
            uid: 0,
            gid: 0,
            signal: 0,
            rip: 0,
            rsp: 0,
            rflags: 0,
            cr2: 0,
            error_code: 0,
            memory_base: 0,
            memory_size: 0,
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
        }
    }
}

/// Simple writer for core filename generation
struct FilenameWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> FilenameWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn len(&self) -> usize {
        self.pos
    }

    fn write_byte(&mut self, b: u8) {
        if self.pos < self.buf.len() {
            self.buf[self.pos] = b;
            self.pos += 1;
        }
    }
}

impl<'a> Write for FilenameWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            self.write_byte(b);
        }
        Ok(())
    }
}

// =============================================================================
// Public API
// =============================================================================

/// Get current core pattern
pub fn get_core_pattern() -> (&'static [u8], usize) {
    let config = CORE_PATTERN.lock();
    let len = config.len;
    // Safe: we hold the lock and return a reference to static data
    let slice = unsafe { core::slice::from_raw_parts(config.pattern.as_ptr(), len) };
    (slice, len)
}

/// Set core pattern
/// Returns Ok(()) on success, Err on invalid pattern
pub fn set_core_pattern(pattern: &str) -> Result<(), &'static str> {
    if pattern.len() > MAX_CORE_PATTERN {
        return Err("core_pattern too long");
    }

    // Validate pattern - don't allow .. for security
    if pattern.contains("..") {
        return Err("core_pattern cannot contain '..'");
    }

    let mut config = CORE_PATTERN.lock();
    config.set_pattern(pattern.trim_end_matches('\n'));

    crate::kinfo!("core_pattern set to: {}", pattern.trim_end_matches('\n'));
    Ok(())
}

/// Get core pipe limit
pub fn get_core_pipe_limit() -> u32 {
    CORE_PATTERN.lock().pipe_limit
}

/// Set core pipe limit
pub fn set_core_pipe_limit(limit: u32) -> Result<(), &'static str> {
    CORE_PATTERN.lock().pipe_limit = limit;
    crate::kinfo!("core_pipe_limit set to: {}", limit);
    Ok(())
}

/// Check if core dumps are enabled
pub fn is_enabled() -> bool {
    CORE_PATTERN.lock().enabled
}

/// Check if pipe mode is enabled
pub fn is_pipe_mode() -> bool {
    CORE_PATTERN.lock().pipe_mode
}

/// Generate core filename from pattern and process info
pub fn generate_core_filename(
    info: &CoreDumpInfo,
    exe_name: &str,
) -> Option<([u8; MAX_CORE_FILENAME], usize)> {
    let config = CORE_PATTERN.lock();
    if !config.enabled {
        return None;
    }

    let pattern = config.as_str();
    if pattern.is_empty() {
        return None;
    }

    // Don't generate file for pipe mode
    if config.pipe_mode {
        return None;
    }

    drop(config); // Release lock before writing

    let mut buf = [0u8; MAX_CORE_FILENAME];
    let mut writer = FilenameWriter::new(&mut buf);

    let config = CORE_PATTERN.lock();
    let pattern = config.as_str();

    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('p') => {
                    let _ = write!(writer, "{}", info.pid);
                }
                Some('u') => {
                    let _ = write!(writer, "{}", info.uid);
                }
                Some('g') => {
                    let _ = write!(writer, "{}", info.gid);
                }
                Some('s') => {
                    let _ = write!(writer, "{}", info.signal);
                }
                Some('t') => {
                    // Time since boot in seconds (simplified)
                    let ticks = crate::scheduler::get_tick();
                    let seconds = ticks / 100; // Assuming 100 Hz tick
                    let _ = write!(writer, "{}", seconds);
                }
                Some('h') => {
                    let _ = write!(writer, "nexaos");
                }
                Some('e') => {
                    // Executable name without path
                    let name = exe_name.rsplit('/').next().unwrap_or(exe_name);
                    let _ = write!(writer, "{}", name);
                }
                Some('E') => {
                    // Full path with / replaced by !
                    for ch in exe_name.chars() {
                        if ch == '/' {
                            writer.write_byte(b'!');
                        } else {
                            let _ = write!(writer, "{}", ch);
                        }
                    }
                }
                Some('%') => {
                    writer.write_byte(b'%');
                }
                Some(other) => {
                    // Unknown specifier, keep as-is
                    writer.write_byte(b'%');
                    let _ = write!(writer, "{}", other);
                }
                None => {
                    // Trailing %, keep it
                    writer.write_byte(b'%');
                }
            }
        } else {
            let _ = write!(writer, "{}", c);
        }
    }

    let len = writer.len();
    Some((buf, len))
}

/// Signal numbers that should generate core dumps (per POSIX)
pub fn should_dump_core(signal: u32) -> bool {
    use crate::ipc::signal::*;

    matches!(
        signal,
        SIGQUIT | SIGILL | SIGTRAP | SIGABRT | SIGBUS | SIGFPE | SIGSEGV
    )
}

/// Get signal name for logging
pub fn signal_name(signal: u32) -> &'static str {
    use crate::ipc::signal::*;

    match signal {
        SIGHUP => "SIGHUP",
        SIGINT => "SIGINT",
        SIGQUIT => "SIGQUIT",
        SIGILL => "SIGILL",
        SIGTRAP => "SIGTRAP",
        SIGABRT => "SIGABRT",
        SIGBUS => "SIGBUS",
        SIGFPE => "SIGFPE",
        SIGKILL => "SIGKILL",
        SIGUSR1 => "SIGUSR1",
        SIGSEGV => "SIGSEGV",
        SIGUSR2 => "SIGUSR2",
        SIGPIPE => "SIGPIPE",
        SIGALRM => "SIGALRM",
        SIGTERM => "SIGTERM",
        SIGCHLD => "SIGCHLD",
        SIGCONT => "SIGCONT",
        SIGSTOP => "SIGSTOP",
        SIGTSTP => "SIGTSTP",
        _ => "UNKNOWN",
    }
}

/// Attempt to generate a core dump for a crashed process
/// This logs the dump info and optionally writes to filesystem
pub fn dump_core(info: &CoreDumpInfo, exe_name: &str) -> Result<(), &'static str> {
    if !is_enabled() {
        crate::kdebug!("Core dumps disabled, skipping");
        return Ok(());
    }

    // Log detailed core dump information
    crate::kerror!("=== CORE DUMP FOR PID {} ===", info.pid);
    crate::kerror!("  Signal: {} ({})", info.signal, signal_name(info.signal));
    crate::kerror!("  Executable: {}", exe_name);
    crate::kerror!("  UID/GID: {}/{}", info.uid, info.gid);
    crate::kerror!("  Register State:");
    crate::kerror!("    RIP: {:#018x}", info.rip);
    crate::kerror!("    RSP: {:#018x}", info.rsp);
    crate::kerror!("    RBP: {:#018x}", info.rbp);
    crate::kerror!("    RFLAGS: {:#018x}", info.rflags);
    crate::kerror!("    RAX: {:#018x}", info.rax);
    crate::kerror!("    RBX: {:#018x}", info.rbx);
    crate::kerror!("    RCX: {:#018x}", info.rcx);
    crate::kerror!("    RDX: {:#018x}", info.rdx);
    crate::kerror!("    RSI: {:#018x}", info.rsi);
    crate::kerror!("    RDI: {:#018x}", info.rdi);

    if info.signal == crate::ipc::signal::SIGSEGV || info.signal == crate::ipc::signal::SIGBUS {
        crate::kerror!("    CR2 (fault addr): {:#018x}", info.cr2);
        crate::kerror!("    Error code: {:#018x}", info.error_code);
    }

    crate::kerror!(
        "  Memory: base={:#x}, size={:#x}",
        info.memory_base,
        info.memory_size
    );

    // Try to dump stack trace
    dump_stack_trace(info);

    // Generate filename
    if let Some((filename_buf, len)) = generate_core_filename(info, exe_name) {
        if let Ok(filename) = core::str::from_utf8(&filename_buf[..len]) {
            crate::kerror!("  Core file would be: {}", filename);

            // For now, we just log. Full ELF core file generation would go here.
            // Writing actual core files requires writable filesystem support
            // and significant memory for ELF header construction.
            // TODO: Implement write_core_file() when ext2 write support is ready
        }
    }

    crate::kerror!("=== END CORE DUMP ===");

    Ok(())
}

/// Dump a basic stack trace from the crash location
fn dump_stack_trace(info: &CoreDumpInfo) {
    crate::kerror!("  Stack Trace (top of stack):");

    // Read up to 8 stack entries
    let stack_ptr = info.rsp as *const u64;

    for i in 0..8 {
        // Safety: We're reading from the process's stack region
        // This could fault if the stack pointer is invalid
        let entry = unsafe {
            let ptr = stack_ptr.add(i);
            // Check if pointer is in valid user space range
            let addr = ptr as u64;
            if addr >= crate::process::USER_VIRT_BASE
                && addr < crate::process::USER_VIRT_BASE + crate::process::USER_REGION_SIZE
            {
                // Try to read - might still fault for unmapped pages
                core::ptr::read_volatile(ptr)
            } else if addr >= info.memory_base && addr < info.memory_base + info.memory_size as u64
            {
                // Physical address in kernel mapping
                core::ptr::read_volatile(ptr)
            } else {
                continue;
            }
        };

        // Filter likely return addresses (in user code range)
        if entry >= crate::process::USER_VIRT_BASE
            && entry < crate::process::USER_VIRT_BASE + crate::process::USER_REGION_SIZE
        {
            crate::kerror!("    [{}] {:#018x}", i, entry);
        }
    }
}

// =============================================================================
// sysfs/procfs interface helpers
// =============================================================================

/// Generate content for /proc/sys/kernel/core_pattern
pub fn generate_core_pattern_content() -> (&'static [u8], usize) {
    static BUFFER: Mutex<[u8; MAX_CORE_PATTERN + 1]> = Mutex::new([0u8; MAX_CORE_PATTERN + 1]);

    let config = CORE_PATTERN.lock();
    let pattern = config.as_str();
    let mut buf = BUFFER.lock();

    let len = pattern.len();
    buf[..len].copy_from_slice(pattern.as_bytes());
    buf[len] = b'\n';

    let final_len = len + 1;
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), final_len) };
    (slice, final_len)
}

/// Generate content for /proc/sys/kernel/core_pipe_limit
pub fn generate_core_pipe_limit_content() -> (&'static [u8], usize) {
    static BUFFER: Mutex<[u8; 32]> = Mutex::new([0u8; 32]);

    let limit = get_core_pipe_limit();
    let mut buf = BUFFER.lock();
    let mut writer = FilenameWriter::new(&mut buf[..]);
    let _ = write!(writer, "{}\n", limit);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate content for /proc/sys/kernel/core_uses_pid
pub fn generate_core_uses_pid_content() -> (&'static [u8], usize) {
    // Check if pattern contains %p
    let config = CORE_PATTERN.lock();
    let uses_pid = config.as_str().contains("%p");
    if uses_pid {
        (b"1\n", 2)
    } else {
        (b"0\n", 2)
    }
}

/// Initialize the coredump subsystem
pub fn init() {
    crate::kinfo!("Core dump subsystem initialized");
    crate::kinfo!("  Default pattern: {}", DEFAULT_CORE_PATTERN);
}
