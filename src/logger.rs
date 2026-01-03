extern crate alloc;

use core::fmt::{self, Write};
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

use crate::safety::rdtsc;
use crate::serial;
use crate::vga_buffer::{self, Color};

// Static buffer pool for log lines to avoid stack allocation >1KB
// This is safe to use before heap initialization
static mut LOG_BUFFER_POOL: [[u8; 1024]; 2] = [[0; 1024]; 2];
static LOG_BUFFER_STATE: AtomicU8 = AtomicU8::new(0);

static LOGGER_INITIALIZED: AtomicBool = AtomicBool::new(false);
static BOOT_TSC: AtomicU64 = AtomicU64::new(0);
static TSC_FREQUENCY_HZ: AtomicU64 = AtomicU64::new(1_000_000_000);
static TSC_FREQ_GUESSED: AtomicBool = AtomicBool::new(true);
static LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::INFO.priority());
static SERIAL_RUNTIME_ENABLED: AtomicBool = AtomicBool::new(true);
static VGA_RUNTIME_ENABLED: AtomicBool = AtomicBool::new(true);
static INIT_STARTED: AtomicBool = AtomicBool::new(false);
/// Graphics mode flag - when set, only PANIC/FATAL logs are shown on screen
/// This prepares for graphics driver initialization
static GRAPHICS_MODE: AtomicBool = AtomicBool::new(true);

// 环形缓冲区用于存储内核日志（64KB）
pub const RINGBUF_SIZE: usize = 65536;
static RINGBUF: Mutex<RingBuffer> = Mutex::new(RingBuffer::new());

use spin::Mutex;

const DEFAULT_TSC_FREQUENCY_HZ: u64 = 1_000_000_000; // 1 GHz fallback

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    PANIC,
    FATAL,
    ERROR,
    WARN,
    INFO,
    DEBUG,
    TRACE,
}

impl LogLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            LogLevel::PANIC => "PANIC",
            LogLevel::FATAL => "FATAL",
            LogLevel::ERROR => "ERROR",
            LogLevel::WARN => "WARN",
            LogLevel::INFO => "INFO",
            LogLevel::DEBUG => "DEBUG",
            LogLevel::TRACE => "TRACE",
        }
    }

    fn serial_color(self) -> &'static str {
        match self {
            LogLevel::PANIC => "\x1b[1;37;41m",
            LogLevel::FATAL => "\x1b[1;37;41m",
            LogLevel::ERROR => "\x1b[1;31m",
            LogLevel::WARN => "\x1b[33m",
            LogLevel::INFO => "\x1b[32m",
            LogLevel::DEBUG => "\x1b[36m",
            LogLevel::TRACE => "\x1b[90m",
        }
    }

    fn badge_colors(self) -> (Color, Color) {
        match self {
            LogLevel::PANIC => (Color::White, Color::Red),
            LogLevel::FATAL => (Color::White, Color::Red),
            LogLevel::ERROR => (Color::LightRed, Color::Black),
            LogLevel::WARN => (Color::Yellow, Color::Black),
            LogLevel::INFO => (Color::LightGreen, Color::Black),
            LogLevel::DEBUG => (Color::LightCyan, Color::Black),
            LogLevel::TRACE => (Color::LightGray, Color::Black),
        }
    }

    fn message_colors(self) -> (Color, Color) {
        match self {
            LogLevel::PANIC => (Color::White, Color::Red),
            LogLevel::FATAL => (Color::White, Color::Red),
            LogLevel::ERROR => (Color::LightRed, Color::Black),
            LogLevel::WARN => (Color::Yellow, Color::Black),
            LogLevel::INFO => (Color::LightGreen, Color::Black),
            LogLevel::DEBUG => (Color::LightCyan, Color::Black),
            LogLevel::TRACE => (Color::LightGray, Color::Black),
        }
    }

    const fn priority(self) -> u8 {
        match self {
            LogLevel::PANIC => 0,
            LogLevel::FATAL => 1,
            LogLevel::ERROR => 2,
            LogLevel::WARN => 3,
            LogLevel::INFO => 4,
            LogLevel::DEBUG => 5,
            LogLevel::TRACE => 6,
        }
    }

    fn from_priority(value: u8) -> Self {
        match value {
            0 => LogLevel::PANIC,
            1 => LogLevel::FATAL,
            2 => LogLevel::ERROR,
            3 => LogLevel::WARN,
            4 => LogLevel::INFO,
            5 => LogLevel::DEBUG,
            _ => LogLevel::TRACE,
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("panic") {
            Some(LogLevel::PANIC)
        } else if value.eq_ignore_ascii_case("fatal") {
            Some(LogLevel::FATAL)
        } else if value.eq_ignore_ascii_case("error") {
            Some(LogLevel::ERROR)
        } else if value.eq_ignore_ascii_case("warn") || value.eq_ignore_ascii_case("warning") {
            Some(LogLevel::WARN)
        } else if value.eq_ignore_ascii_case("info") {
            Some(LogLevel::INFO)
        } else if value.eq_ignore_ascii_case("debug") {
            Some(LogLevel::DEBUG)
        } else if value.eq_ignore_ascii_case("trace") {
            Some(LogLevel::TRACE)
        } else {
            None
        }
    }
}

pub fn init() -> u64 {
    if LOGGER_INITIALIZED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return TSC_FREQUENCY_HZ.load(Ordering::Relaxed);
    }

    let current = read_tsc();
    BOOT_TSC.store(current, Ordering::Relaxed);

    let (frequency, guessed) = detect_tsc_frequency()
        .map(|freq| (freq, false))
        .unwrap_or((DEFAULT_TSC_FREQUENCY_HZ, true));
    TSC_FREQ_GUESSED.store(guessed, Ordering::Relaxed);
    TSC_FREQUENCY_HZ.store(frequency, Ordering::Relaxed);
    frequency
}

pub fn is_initialized() -> bool {
    LOGGER_INITIALIZED.load(Ordering::Relaxed)
}

pub fn tsc_frequency_is_guessed() -> bool {
    TSC_FREQ_GUESSED.load(Ordering::Relaxed)
}

pub fn log(level: LogLevel, args: fmt::Arguments<'_>) {
    // Some kernel entry paths (e.g. naked asm / interrupt glue) can violate the SysV
    // stack alignment rules. LLVM may emit `movaps` spills in `log_impl`, which will
    // #GP on misaligned stacks.
    // Route all logging through a tiny assembly shim that realigns RSP to 16 bytes.
    unsafe {
        log_aligned(
            level.priority(),
            (&args as *const fmt::Arguments<'_>).cast(),
        );
    }
}

/// Align RSP to 16 bytes and then call into the real logger.
///
/// Safety: `args` must be valid for the duration of this call.
#[unsafe(naked)]
unsafe extern "C" fn log_aligned(_level: u8, _args: *const fmt::Arguments<'static>) {
    core::arch::naked_asm!(
        // Save the original RSP in a stack slot (not a register) so IRQ/exception
        // paths that don't preserve all regs can't corrupt our restore value.
        "mov rax, rsp",
        // Align down to 16 so LLVM-generated `movaps` spills are safe.
        "and rsp, -16",
        // Keep RSP 16B-aligned before `call` (SysV requires this).
        "sub rsp, 16",
        "mov [rsp + 8], rax",
        // Call the Rust body (arguments already in rdi/rsi per SysV).
        "call {inner}",
        // Restore the original RSP (with our caller return address at [rsp]).
        "mov rsp, [rsp + 8]",
        "ret",
        inner = sym log_aligned_inner,
    );
}

#[inline(never)]
extern "C" fn log_aligned_inner(level: u8, args: *const fmt::Arguments<'static>) {
    // SAFETY: pointer is provided by `log()` and is valid for this call.
    let args = unsafe { core::ptr::read(args) };
    log_impl(LogLevel::from_priority(level), args);
}

#[inline(never)]
fn log_impl(level: LogLevel, args: fmt::Arguments<'_>) {
    // TEMPORARY: Disable stack alignment check - it's too strict and breaks SMP testing
    // TODO: Re-enable with proper understanding of when stack should be aligned
    /*
    #[allow(unused_mut)]
    let mut rsp_val: usize = 0;
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) rsp_val, options(nomem, preserves_flags));
    }
    if rsp_val & 0xF != 0 {
        use core::sync::atomic::AtomicBool;
        static REPORTED_UNALIGNED: AtomicBool = AtomicBool::new(false);
        if !REPORTED_UNALIGNED.swap(true, Ordering::SeqCst) {
            unsafe {
                use x86_64::instructions::port::Port;
                let mut port = Port::<u8>::new(0x3F8);
                for &byte in b"ALGN" {
                    port.write(byte);
                }
                port.write(b' ');
                for shift in (0..16).rev() {
                    let nibble = ((rsp_val >> (shift * 4)) & 0xF) as u8;
                    let ch = match nibble {
                        0..=9 => b'0' + nibble,
                        _ => b'A' + (nibble - 10),
                    };
                    port.write(ch);
                }
                port.write(b'\n');

                let ret_addr_ptr = (rsp_val + 0xD18) as *const usize;
                let ret_addr = core::ptr::read(ret_addr_ptr);
                for &byte in b"RET " {
                    port.write(byte);
                }
                for shift in (0..16).rev() {
                    let nibble = ((ret_addr >> (shift * 4)) & 0xF) as u8;
                    let ch = match nibble {
                        0..=9 => b'0' + nibble,
                        _ => b'A' + (nibble - 10),
                    };
                    port.write(ch);
                }
                port.write(b'\n');
            }
        }
    }
    */

    let current = LOG_LEVEL.load(Ordering::Relaxed);
    if level.priority() > current {
        return;
    }

    let init_started = INIT_STARTED.load(Ordering::Relaxed);
    let graphics_mode = GRAPHICS_MODE.load(Ordering::Relaxed);

    // 在 init 启动前，输出到显示器和串口；启动后，只输出到环形缓冲区
    // Panic 总是输出到显示器和串口
    // 图形模式下，只有 PANIC/FATAL 才输出到屏幕（VGA/framebuffer）
    let emit_serial = if init_started {
        level.priority() <= LogLevel::PANIC.priority()
    } else {
        should_emit_serial(level)
    };

    let emit_vga = if init_started || graphics_mode {
        // In graphics mode or after init, only PANIC/FATAL go to screen
        level.priority() <= LogLevel::FATAL.priority()
    } else {
        should_emit_vga(level)
    };

    let emit_framebuffer = emit_vga && crate::framebuffer::is_ready();

    let timestamp_us = boot_time_us();
    let mut ansi_line = None;
    if emit_serial || emit_framebuffer {
        ansi_line = build_color_log_line(level, timestamp_us, args.clone());
    }

    let mut plain_line: Option<LogLineBuffer> = None;

    if emit_serial {
        if let Some(buffer) = ansi_line.as_ref() {
            serial::write_bytes(buffer.as_bytes());
        } else {
            if plain_line.is_none() {
                plain_line = build_color_log_line_vga(level, timestamp_us, args.clone());
            }
            if let Some(buffer) = plain_line.as_ref() {
                serial::write_bytes(buffer.as_bytes());
            } else {
                emit_serial_fallback(level, timestamp_us, args.clone());
            }
        }
    }

    if emit_vga {
        emit_vga_line(level, timestamp_us, args.clone());
    }

    if emit_framebuffer {
        if let Some(buffer) = ansi_line.as_ref() {
            crate::framebuffer::write_bytes(buffer.as_bytes());
        } else {
            if plain_line.is_none() {
                plain_line = build_color_log_line_vga(level, timestamp_us, args.clone());
            }
            if let Some(buffer) = plain_line.as_ref() {
                crate::framebuffer::write_bytes(buffer.as_bytes());
            }
        }
    }

    // 总是向环形缓冲区写入日志
    if plain_line.is_none() && !emit_serial && !emit_vga {
        plain_line = build_color_log_line_vga(level, timestamp_us, args);
    }

    if let Some(buffer) = plain_line.as_ref() {
        // Use try_lock to avoid deadlock in interrupt context
        if let Some(mut ringbuf) = RINGBUF.try_lock() {
            ringbuf.write_bytes(buffer.as_bytes());
        }
    }
}

pub fn set_max_level(level: LogLevel) {
    LOG_LEVEL.store(level.priority(), Ordering::Relaxed);
}

pub fn max_level() -> LogLevel {
    LogLevel::from_priority(LOG_LEVEL.load(Ordering::Relaxed))
}

pub fn parse_level_directive(cmdline: &str) -> Option<LogLevel> {
    for token in cmdline.split_whitespace() {
        if let Some((key, value)) = token.split_once('=') {
            if key.eq_ignore_ascii_case("log") || key.eq_ignore_ascii_case("loglevel") {
                if let Some(level) = LogLevel::from_str(value) {
                    return Some(level);
                }
            }
        }
    }
    None
}

pub fn boot_time_us() -> u64 {
    let start = BOOT_TSC.load(Ordering::Relaxed);
    let freq = TSC_FREQUENCY_HZ.load(Ordering::Relaxed);
    if start == 0 || freq == 0 {
        return 0;
    }

    let now = read_tsc();
    let ticks = now.saturating_sub(start);
    ticks.saturating_mul(1_000_000) / freq
}

pub fn tsc_frequency_hz() -> u64 {
    TSC_FREQUENCY_HZ.load(Ordering::Relaxed)
}

fn should_emit_serial(level: LogLevel) -> bool {
    if SERIAL_RUNTIME_ENABLED.load(Ordering::Relaxed) {
        true
    } else {
        level.priority() <= LogLevel::ERROR.priority()
    }
}

fn should_emit_vga(level: LogLevel) -> bool {
    if VGA_RUNTIME_ENABLED.load(Ordering::Relaxed) {
        true
    } else {
        level.priority() <= LogLevel::ERROR.priority()
    }
}

fn emit_serial_fallback(level: LogLevel, timestamp_us: u64, args: fmt::Arguments<'_>) {
    serial::_print(format_args!(
        "{color}[{timestamp}] [{level:<5}] {message}\x1b[0m\n",
        color = level.serial_color(),
        timestamp = TimestampDisplay {
            microseconds: timestamp_us
        },
        level = LevelDisplay(level),
        message = args,
    ));
}

fn emit_vga_line(level: LogLevel, timestamp_us: u64, args: fmt::Arguments<'_>) {
    // Avoid writing to VGA until mapping is completed. Some early boot
    // code runs before the VGA buffer is safely mapped and writes to it
    // can cause page faults (observed as PF at 0xb8f00). Check the
    // VGA readiness flag and skip VGA output until it's set by paging.
    if !crate::vga_buffer::is_vga_ready() {
        return;
    }

    vga_buffer::with_writer(|writer| {
        writer.with_color(Color::LightGray, Color::Black, |writer| {
            let _ = write!(
                writer,
                "[{timestamp}] ",
                timestamp = TimestampDisplay {
                    microseconds: timestamp_us,
                }
            );
        });

        let (badge_fg, badge_bg) = level.badge_colors();
        writer.with_color(badge_fg, badge_bg, |writer| {
            let _ = write!(writer, "[{level}] ", level = LevelDisplay(level));
        });

        let (msg_fg, msg_bg) = level.message_colors();
        writer.with_color(msg_fg, msg_bg, |writer| {
            let _ = write!(writer, "{}", args);
        });

        let _ = writer.write_str("\n");
    });
}

fn build_color_log_line(
    level: LogLevel,
    timestamp_us: u64,
    args: fmt::Arguments<'_>,
) -> Option<LogLineBuffer> {
    let mut buffer = LogLineBuffer::new()?;
    if buffer.write_str(level.serial_color()).is_err() {
        return None;
    }
    if write!(
        buffer,
        "[{timestamp}] [{level:<5}] ",
        timestamp = TimestampDisplay {
            microseconds: timestamp_us,
        },
        level = LevelDisplay(level)
    )
    .is_err()
    {
        return None;
    }
    if fmt::write(&mut buffer, args).is_err() {
        return None;
    }
    if buffer.write_str("\x1b[0m\n").is_err() {
        return None;
    }
    Some(buffer)
}

fn build_color_log_line_vga(
    level: LogLevel,
    timestamp_us: u64,
    args: fmt::Arguments<'_>,
) -> Option<LogLineBuffer> {
    let mut buffer = LogLineBuffer::new()?;
    if write!(
        buffer,
        "[{timestamp}] [{level}] ",
        timestamp = TimestampDisplay {
            microseconds: timestamp_us,
        },
        level = LevelDisplay(level)
    )
    .is_err()
    {
        return None;
    }
    if fmt::write(&mut buffer, args).is_err() {
        return None;
    }
    if buffer.write_str("\n").is_err() {
        return None;
    }
    Some(buffer)
}

pub fn set_console_output_enabled(serial_enabled: bool, vga_enabled: bool) {
    SERIAL_RUNTIME_ENABLED.store(serial_enabled, Ordering::Relaxed);
    VGA_RUNTIME_ENABLED.store(vga_enabled, Ordering::Relaxed);
}

pub fn disable_runtime_console_output() {
    set_console_output_enabled(false, false);
}

pub fn enable_runtime_console_output() {
    set_console_output_enabled(true, true);
}

/// 标记 init 进程已启动
/// 在此之后，内核日志仅输出到环形缓冲区（除了 panic）
pub fn mark_init_started() {
    INIT_STARTED.store(true, Ordering::Relaxed);
}

/// 启用图形模式 - 之后只有 PANIC/FATAL 日志会输出到屏幕
/// 这为图形驱动初始化做准备，让屏幕变得干净
/// 日志仍然会输出到串口和环形缓冲区
pub fn enable_graphics_mode() {
    GRAPHICS_MODE.store(true, Ordering::Relaxed);
}

/// 禁用图形模式 - 恢复正常日志输出到屏幕
pub fn disable_graphics_mode() {
    GRAPHICS_MODE.store(false, Ordering::Relaxed);
}

/// 检查是否处于图形模式
pub fn is_graphics_mode() -> bool {
    GRAPHICS_MODE.load(Ordering::Relaxed)
}

/// 读取内核日志环形缓冲区
/// 注意：此函数返回64KB数组副本，在内核栈上使用会导致栈溢出！
/// 请使用 read_ringbuffer_to_slice() 替代
#[allow(dead_code)]
pub fn read_ringbuffer() -> [u8; RINGBUF_SIZE] {
    let ringbuf = RINGBUF.lock();
    ringbuf.buf
}

/// 获取环形缓冲区的写入位置（用于知道有效数据的范围）
pub fn ringbuffer_write_pos() -> usize {
    let ringbuf = RINGBUF.lock();
    ringbuf.write_pos
}

/// 直接从环形缓冲区读取数据到目标切片，避免栈上分配大数组
/// 返回 (实际复制的字节数, 缓冲区中有效数据的总长度)
pub fn read_ringbuffer_to_slice(dest: &mut [u8]) -> (usize, usize) {
    let ringbuf = RINGBUF.lock();
    let write_pos = ringbuf.write_pos;
    let buf = &ringbuf.buf;

    // Calculate how many valid bytes we have
    let valid_len = if write_pos == 0 {
        // Check if buffer is empty or full
        if buf[0] == 0 {
            0 // Empty buffer
        } else {
            RINGBUF_SIZE // Full buffer, wrapped around
        }
    } else {
        // Check if we've wrapped around
        let has_wrapped = buf[write_pos % RINGBUF_SIZE] != 0
            && write_pos > 0
            && buf[(write_pos.wrapping_sub(1)) % RINGBUF_SIZE] != 0;
        if has_wrapped && buf[0] != 0 {
            RINGBUF_SIZE
        } else {
            write_pos
        }
    };

    if valid_len == 0 {
        return (0, 0);
    }

    let copy_len = core::cmp::min(dest.len(), valid_len);

    if write_pos >= valid_len {
        // Linear data from start
        dest[..copy_len].copy_from_slice(&buf[..copy_len]);
    } else {
        // Wrapped buffer: oldest data starts at write_pos
        let start_pos = write_pos;
        let first_chunk_len = core::cmp::min(copy_len, RINGBUF_SIZE - start_pos);
        dest[..first_chunk_len].copy_from_slice(&buf[start_pos..start_pos + first_chunk_len]);

        if copy_len > first_chunk_len {
            let second_chunk_len = copy_len - first_chunk_len;
            dest[first_chunk_len..copy_len].copy_from_slice(&buf[..second_chunk_len]);
        }
    }

    (copy_len, valid_len)
}

fn read_tsc() -> u64 {
    rdtsc()
}

fn detect_tsc_frequency() -> Option<u64> {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::{__cpuid, __cpuid_count};

        let highest_leaf = __cpuid(0).eax;

        if highest_leaf >= 0x15 {
            let leaf = __cpuid_count(0x15, 0);
            let denom = leaf.eax as u64;
            let numer = leaf.ebx as u64;
            let freq = leaf.ecx as u64;

            if denom != 0 && numer != 0 {
                if freq != 0 {
                    return Some((freq * numer) / denom);
                } else if let Some(base_freq) = detect_base_frequency_mhz() {
                    return Some(((base_freq as u64) * 1_000_000 * numer) / denom);
                }
            } else if freq != 0 {
                return Some(freq);
            }
        }

        if highest_leaf >= 0x16 {
            if let Some(base_freq) = detect_base_frequency_mhz() {
                return Some(base_freq as u64 * 1_000_000);
            }
        }
    }

    None
}

fn detect_base_frequency_mhz() -> Option<u32> {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::__cpuid;
        let leaf = __cpuid(0x16);
        if leaf.eax != 0 {
            return Some(leaf.eax);
        }
    }
    None
}

struct TimestampDisplay {
    microseconds: u64,
}

impl fmt::Display for TimestampDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seconds = self.microseconds / 1_000_000;
        let micros = self.microseconds % 1_000_000;
        write!(f, "{:>5}.{:06}", seconds, micros)
    }
}

struct LevelDisplay(LogLevel);

impl fmt::Display for LevelDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<5}", self.0.as_str())
    }
}

struct LogLineBuffer {
    buf: &'static mut [u8; 1024],
    len: usize,
    buf_index: u8,
}

impl LogLineBuffer {
    fn new() -> Option<Self> {
        // Try to acquire a static buffer from the pool
        // Bit 0: Buffer 0 in use
        // Bit 1: Buffer 1 in use
        let mut current_state = LOG_BUFFER_STATE.load(Ordering::Relaxed);

        loop {
            if current_state & 1 == 0 {
                // Try to acquire buffer 0
                match LOG_BUFFER_STATE.compare_exchange(
                    current_state,
                    current_state | 1,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        // SAFETY: We acquired the lock for buffer 0
                        let buf_ptr = unsafe { addr_of_mut!(LOG_BUFFER_POOL[0]) };
                        return Some(Self {
                            buf: unsafe { &mut *buf_ptr },
                            len: 0,
                            buf_index: 0,
                        });
                    }
                    Err(s) => current_state = s,
                }
            } else if current_state & 2 == 0 {
                // Try to acquire buffer 1
                match LOG_BUFFER_STATE.compare_exchange(
                    current_state,
                    current_state | 2,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        // SAFETY: We acquired the lock for buffer 1
                        let buf_ptr = unsafe { addr_of_mut!(LOG_BUFFER_POOL[1]) };
                        return Some(Self {
                            buf: unsafe { &mut *buf_ptr },
                            len: 0,
                            buf_index: 1,
                        });
                    }
                    Err(s) => current_state = s,
                }
            } else {
                // Both buffers in use. We cannot log this line.
                return None;
            }
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl Drop for LogLineBuffer {
    fn drop(&mut self) {
        // Release the buffer back to the pool
        let mask = !(1 << self.buf_index);
        LOG_BUFFER_STATE.fetch_and(mask, Ordering::Release);
    }
}

impl fmt::Write for LogLineBuffer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        if self.len + bytes.len() > self.buf.len() {
            return Err(fmt::Error);
        }
        self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(())
    }
}

/// 内核日志环形缓冲区
struct RingBuffer {
    buf: [u8; RINGBUF_SIZE],
    write_pos: usize,
}

impl RingBuffer {
    const fn new() -> Self {
        Self {
            buf: [0; RINGBUF_SIZE],
            write_pos: 0,
        }
    }

    /// 向环形缓冲区写入字节
    fn write_bytes(&mut self, bytes: &[u8]) {
        if self.write_pos >= RINGBUF_SIZE {
            // Any caller that corrupted the write_pos will be forced
            // back into range before we try to touch the buffer.
            self.write_pos %= RINGBUF_SIZE;
        }

        for &byte in bytes {
            self.buf[self.write_pos] = byte;
            self.write_pos += 1;
            if self.write_pos >= RINGBUF_SIZE {
                self.write_pos = 0;
            }
        }
    }

    /// 返回整个缓冲区（当已满时为循环buffer，否则为有效数据）
    #[allow(dead_code)]
    fn get_buf(&self) -> &[u8] {
        &self.buf
    }

    /// 获取写入位置（用于确定有效数据的范围）
    #[allow(dead_code)]
    fn write_pos(&self) -> usize {
        self.write_pos
    }
}
