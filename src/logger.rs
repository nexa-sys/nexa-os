use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

use crate::serial;
use crate::vga_buffer::{self, Color};

static LOGGER_INITIALIZED: AtomicBool = AtomicBool::new(false);
static BOOT_TSC: AtomicU64 = AtomicU64::new(0);
static TSC_FREQUENCY_HZ: AtomicU64 = AtomicU64::new(1_000_000_000);
static TSC_FREQ_GUESSED: AtomicBool = AtomicBool::new(true);
static LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::INFO.priority());

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
    let current = LOG_LEVEL.load(Ordering::Relaxed);
    if level.priority() > current {
        return;
    }

    let timestamp_us = boot_time_us();

    let args_for_vga = args.clone();
    emit_serial(level, timestamp_us, args);
    emit_vga(level, timestamp_us, args_for_vga);
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

fn emit_serial(level: LogLevel, timestamp_us: u64, args: fmt::Arguments<'_>) {
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

fn emit_vga(level: LogLevel, timestamp_us: u64, args: fmt::Arguments<'_>) {
    // Avoid writing to VGA until mapping is completed. Some early boot
    // code runs before the VGA buffer is safely mapped and writes to it
    // can cause page faults (observed as PF at 0xb8f00). Check the
    // VGA readiness flag and skip VGA output until it's set by paging.
    if !crate::vga_buffer::is_vga_ready() {
        return;
    }

    use core::fmt::Write;

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

fn read_tsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
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
