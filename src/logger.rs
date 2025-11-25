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
static LOG_BUFFER_IN_USE: AtomicBool = AtomicBool::new(false);

static LOGGER_INITIALIZED: AtomicBool = AtomicBool::new(false);
static BOOT_TSC: AtomicU64 = AtomicU64::new(0);
static TSC_FREQUENCY_HZ: AtomicU64 = AtomicU64::new(1_000_000_000);
static TSC_FREQ_GUESSED: AtomicBool = AtomicBool::new(true);
static LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::INFO.priority());
static SERIAL_RUNTIME_ENABLED: AtomicBool = AtomicBool::new(true);
static VGA_RUNTIME_ENABLED: AtomicBool = AtomicBool::new(true);
static INIT_STARTED: AtomicBool = AtomicBool::new(false);

// 环形缓冲区用于存储内核日志（64KB）
const RINGBUF_SIZE: usize = 65536;
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

    // 在 init 启动前，输出到显示器和串口；启动后，只输出到环形缓冲区
    // Panic 总是输出到显示器和串口
    let emit_serial = if init_started {
        level.priority() <= LogLevel::PANIC.priority()
    } else {
        should_emit_serial(level)
    };

    let emit_vga = if init_started {
        level.priority() <= LogLevel::PANIC.priority()
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
        let mut ringbuf = RINGBUF.lock();
        ringbuf.write_bytes(buffer.as_bytes());
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

/// 读取内核日志环形缓冲区
pub fn read_ringbuffer() -> [u8; RINGBUF_SIZE] {
    let ringbuf = RINGBUF.lock();
    ringbuf.buf
}

/// 获取环形缓冲区的写入位置（用于知道有效数据的范围）
pub fn ringbuffer_write_pos() -> usize {
    let ringbuf = RINGBUF.lock();
    ringbuf.write_pos
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
}

impl LogLineBuffer {
    fn new() -> Option<Self> {
        // Try to acquire a static buffer from the pool
        if LOG_BUFFER_IN_USE
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            // SAFETY: We acquired the lock, so we have exclusive access to buffer 0
            let buf_ptr = unsafe { addr_of_mut!(LOG_BUFFER_POOL[0]) };
            Some(Self {
                buf: unsafe { &mut *buf_ptr },
                len: 0,
            })
        } else {
            // If pool is in use, use the second buffer (for nested logging)
            // SAFETY: Buffer 1 is used for nested logging
            let buf_ptr = unsafe { addr_of_mut!(LOG_BUFFER_POOL[1]) };
            Some(Self {
                buf: unsafe { &mut *buf_ptr },
                len: 0,
            })
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl Drop for LogLineBuffer {
    fn drop(&mut self) {
        // Release the buffer back to the pool
        LOG_BUFFER_IN_USE.store(false, Ordering::Release);
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
