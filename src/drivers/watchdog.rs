//! Hardware Watchdog Timer Driver
//!
//! This module implements support for hardware watchdog timers.
//! Currently supports:
//! - Intel ICH (I/O Controller Hub) TCO watchdog timer
//! - QEMU virtual watchdog (i6300esb emulation)
//!
//! The watchdog timer automatically resets the system if not periodically "fed"
//! (i.e., if the kernel becomes unresponsive).

use crate::safety::{inb, inl, inw, outb, outl, outw};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// TCO base I/O port (typically 0x60 for TCO1, 0x64 for TCO2 on ICH)
/// In QEMU with i6300esb, the device is at 0x400-0x40F
const TCO_BASE: u16 = 0x60;

/// Intel i6300esb registers (QEMU default watchdog)
const I6300ESB_BASE: u16 = 0x400;
const I6300ESB_TIMER1_REG: u16 = I6300ESB_BASE + 0x00;
const I6300ESB_TIMER2_REG: u16 = I6300ESB_BASE + 0x04;
const I6300ESB_GINTSR_REG: u16 = I6300ESB_BASE + 0x08;
const I6300ESB_RELOAD_REG: u16 = I6300ESB_BASE + 0x0C;

/// Legacy Super I/O watchdog port (Winbond W83627 compatible)
const SUPERIO_WDT_BASE: u16 = 0x2E;

/// ACPI PM Timer port
const ACPI_PM_TMR: u16 = 0x608;

/// Global watchdog state
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(false);
static TIMEOUT_SECS: AtomicU32 = AtomicU32::new(60);

/// Watchdog type currently in use
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchdogType {
    None,
    /// Intel i6300esb (common in QEMU)
    I6300ESB,
    /// Intel ICH TCO timer
    IntelTCO,
    /// Super I/O chipset watchdog
    SuperIO,
    /// Software watchdog (timer-based fallback)
    Software,
}

static mut WATCHDOG_TYPE: WatchdogType = WatchdogType::None;

/// Watchdog information structure (for userspace query)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WatchdogInfo {
    /// Watchdog options (bitmask)
    pub options: u32,
    /// Firmware version (0 if not applicable)
    pub firmware_version: u32,
    /// Identity string (null-terminated, max 32 chars)
    pub identity: [u8; 32],
}

/// Watchdog option flags
pub mod options {
    /// Watchdog can be stopped once started
    pub const WDIOF_SETTIMEOUT: u32 = 0x0080;
    /// Watchdog triggered a magic close (clean shutdown)
    pub const WDIOF_MAGICCLOSE: u32 = 0x0100;
    /// Watchdog keeps running after close
    pub const WDIOF_KEEPALIVEPING: u32 = 0x8000;
}

/// ioctl commands for /dev/watchdog
pub mod ioctls {
    /// Get watchdog support info
    pub const WDIOC_GETSUPPORT: u64 = 0x80285700;
    /// Get watchdog status
    pub const WDIOC_GETSTATUS: u64 = 0x80045701;
    /// Get boot status
    pub const WDIOC_GETBOOTSTATUS: u64 = 0x80045702;
    /// Get timeout value
    pub const WDIOC_GETTIMEOUT: u64 = 0x80045707;
    /// Set timeout value
    pub const WDIOC_SETTIMEOUT: u64 = 0xC0045706;
    /// Send keepalive ping
    pub const WDIOC_KEEPALIVE: u64 = 0x80045705;
    /// Set options (enable/disable)
    pub const WDIOC_SETOPTIONS: u64 = 0x80045704;
}

/// Watchdog set options
pub mod setoptions {
    pub const WDIOS_DISABLECARD: u32 = 0x0001;
    pub const WDIOS_ENABLECARD: u32 = 0x0002;
}

/// Initialize the watchdog subsystem
///
/// Probes for available hardware watchdog timers in order of preference:
/// 1. Intel i6300esb (QEMU)
/// 2. Intel TCO
/// 3. Super I/O
/// 4. Falls back to software watchdog
pub fn init() {
    if INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    crate::kinfo!("[watchdog] Probing for hardware watchdog...");

    // Try Intel i6300esb first (common in QEMU/KVM)
    if probe_i6300esb() {
        unsafe {
            WATCHDOG_TYPE = WatchdogType::I6300ESB;
        }
        crate::kinfo!("[watchdog] Found Intel i6300esb watchdog");
        INITIALIZED.store(true, Ordering::Release);
        return;
    }

    // Try Intel TCO
    if probe_intel_tco() {
        unsafe {
            WATCHDOG_TYPE = WatchdogType::IntelTCO;
        }
        crate::kinfo!("[watchdog] Found Intel TCO watchdog");
        INITIALIZED.store(true, Ordering::Release);
        return;
    }

    // Try Super I/O watchdog
    if probe_superio() {
        unsafe {
            WATCHDOG_TYPE = WatchdogType::SuperIO;
        }
        crate::kinfo!("[watchdog] Found Super I/O watchdog");
        INITIALIZED.store(true, Ordering::Release);
        return;
    }

    // Fall back to software watchdog
    unsafe {
        WATCHDOG_TYPE = WatchdogType::Software;
    }
    crate::kinfo!("[watchdog] Using software watchdog fallback");
    INITIALIZED.store(true, Ordering::Release);
}

/// Probe for Intel i6300esb watchdog
fn probe_i6300esb() -> bool {
    // Try to read the timer register
    // In QEMU, reading from this port should return non-0xFF values
    let val = inl(I6300ESB_TIMER1_REG);
    // If port not present, we typically get 0xFFFFFFFF
    val != 0xFFFFFFFF
}

/// Probe for Intel TCO watchdog
fn probe_intel_tco() -> bool {
    // TCO is typically at 0x60-0x6F in the LPC/eSPI config space
    // Read TCO1_STS to check if TCO is present
    let val = inw(TCO_BASE);
    val != 0xFFFF
}

/// Probe for Super I/O watchdog
fn probe_superio() -> bool {
    // Super I/O chips use a 2-register access protocol at 0x2E/0x2F
    // We try to read the chip ID
    outb(SUPERIO_WDT_BASE, 0x20); // DeviceID register
    let id = inb(SUPERIO_WDT_BASE + 1);
    // Common Winbond/ITE chip IDs are 0x52, 0x88, 0x87, etc.
    id != 0xFF && id != 0x00
}

/// Check if watchdog is initialized
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Get the watchdog type
pub fn get_type() -> WatchdogType {
    unsafe { WATCHDOG_TYPE }
}

/// Enable the watchdog timer
pub fn enable() -> Result<(), i32> {
    if !INITIALIZED.load(Ordering::Acquire) {
        return Err(crate::posix::errno::ENODEV);
    }

    let timeout = TIMEOUT_SECS.load(Ordering::Acquire);

    match unsafe { WATCHDOG_TYPE } {
        WatchdogType::I6300ESB => enable_i6300esb(timeout),
        WatchdogType::IntelTCO => enable_tco(timeout),
        WatchdogType::SuperIO => enable_superio(timeout),
        WatchdogType::Software => {
            // Software watchdog uses kernel timer
            ENABLED.store(true, Ordering::Release);
            Ok(())
        }
        WatchdogType::None => Err(crate::posix::errno::ENODEV),
    }
}

/// Disable the watchdog timer
pub fn disable() -> Result<(), i32> {
    if !INITIALIZED.load(Ordering::Acquire) {
        return Err(crate::posix::errno::ENODEV);
    }

    match unsafe { WATCHDOG_TYPE } {
        WatchdogType::I6300ESB => disable_i6300esb(),
        WatchdogType::IntelTCO => disable_tco(),
        WatchdogType::SuperIO => disable_superio(),
        WatchdogType::Software => {
            ENABLED.store(false, Ordering::Release);
            Ok(())
        }
        WatchdogType::None => Err(crate::posix::errno::ENODEV),
    }
}

/// Feed (pet) the watchdog to prevent system reset
pub fn feed() -> Result<(), i32> {
    if !INITIALIZED.load(Ordering::Acquire) {
        return Err(crate::posix::errno::ENODEV);
    }

    if !ENABLED.load(Ordering::Acquire) {
        return Ok(()); // Not enabled, no-op
    }

    match unsafe { WATCHDOG_TYPE } {
        WatchdogType::I6300ESB => feed_i6300esb(),
        WatchdogType::IntelTCO => feed_tco(),
        WatchdogType::SuperIO => feed_superio(),
        WatchdogType::Software => {
            // Update software watchdog timestamp
            Ok(())
        }
        WatchdogType::None => Err(crate::posix::errno::ENODEV),
    }
}

/// Set the watchdog timeout in seconds
pub fn set_timeout(seconds: u32) -> Result<u32, i32> {
    if !INITIALIZED.load(Ordering::Acquire) {
        return Err(crate::posix::errno::ENODEV);
    }

    // Validate timeout range (1 second to 1 hour)
    if seconds == 0 || seconds > 3600 {
        return Err(crate::posix::errno::EINVAL);
    }

    TIMEOUT_SECS.store(seconds, Ordering::Release);

    // If already enabled, update the hardware timeout
    if ENABLED.load(Ordering::Acquire) {
        match unsafe { WATCHDOG_TYPE } {
            WatchdogType::I6300ESB => {
                let _ = enable_i6300esb(seconds);
            }
            WatchdogType::IntelTCO => {
                let _ = enable_tco(seconds);
            }
            WatchdogType::SuperIO => {
                let _ = enable_superio(seconds);
            }
            _ => {}
        }
    }

    Ok(seconds)
}

/// Get the current timeout in seconds
pub fn get_timeout() -> u32 {
    TIMEOUT_SECS.load(Ordering::Acquire)
}

/// Get watchdog info
pub fn get_info() -> WatchdogInfo {
    let wtype = unsafe { WATCHDOG_TYPE };
    let mut info = WatchdogInfo {
        options: options::WDIOF_SETTIMEOUT | options::WDIOF_KEEPALIVEPING,
        firmware_version: 0,
        identity: [0; 32],
    };

    let name: &[u8] = match wtype {
        WatchdogType::I6300ESB => b"Intel i6300ESB\0",
        WatchdogType::IntelTCO => b"Intel TCO WDT\0",
        WatchdogType::SuperIO => b"Super I/O WDT\0",
        WatchdogType::Software => b"NexaOS Software WDT\0",
        WatchdogType::None => b"No Watchdog\0",
    };

    let copy_len = name.len().min(31);
    info.identity[..copy_len].copy_from_slice(&name[..copy_len]);

    info
}

/// Check if watchdog is enabled
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

// ============================================================================
// Intel i6300esb implementation
// ============================================================================

fn enable_i6300esb(timeout_secs: u32) -> Result<(), i32> {
    // Convert seconds to timer ticks (i6300esb uses ~1Hz resolution for simplicity)
    let ticks = timeout_secs.min(0xFFFF) as u16;

    // Write timer value
    outw(I6300ESB_TIMER1_REG, ticks);

    // Reload and enable
    outl(I6300ESB_RELOAD_REG, 0x8000_0000); // Set enable bit

    ENABLED.store(true, Ordering::Release);
    Ok(())
}

fn disable_i6300esb() -> Result<(), i32> {
    // Clear enable bit
    outl(I6300ESB_RELOAD_REG, 0);
    ENABLED.store(false, Ordering::Release);
    Ok(())
}

fn feed_i6300esb() -> Result<(), i32> {
    // Reload the timer by writing to reload register
    let current = inl(I6300ESB_RELOAD_REG);
    outl(I6300ESB_RELOAD_REG, current | 0x0100_0000); // Reload bit
    Ok(())
}

// ============================================================================
// Intel TCO implementation
// ============================================================================

const TCO1_RLD: u16 = TCO_BASE + 0x00; // TCO Timer Reload
const TCO1_STS: u16 = TCO_BASE + 0x04; // TCO1 Status
const TCO2_STS: u16 = TCO_BASE + 0x06; // TCO2 Status
const TCO1_CNT: u16 = TCO_BASE + 0x08; // TCO1 Control

fn enable_tco(timeout_secs: u32) -> Result<(), i32> {
    // TCO timer runs at approximately 0.6 second intervals
    // timeout = seconds * 1.6 (approximately)
    let ticks = ((timeout_secs as u64 * 16) / 10).min(0x3FF) as u16;

    // Write timer value (bits 9:0)
    let cnt = inw(TCO1_CNT);
    outw(TCO1_CNT, (cnt & 0xFC00) | ticks);

    // Clear TCO_TMR_HLT bit to enable timer
    let cnt = inw(TCO1_CNT);
    outw(TCO1_CNT, cnt & !0x0800);

    ENABLED.store(true, Ordering::Release);
    Ok(())
}

fn disable_tco() -> Result<(), i32> {
    // Set TCO_TMR_HLT bit to stop timer
    let cnt = inw(TCO1_CNT);
    outw(TCO1_CNT, cnt | 0x0800);
    ENABLED.store(false, Ordering::Release);
    Ok(())
}

fn feed_tco() -> Result<(), i32> {
    // Write to reload register to reset the timer
    outw(TCO1_RLD, 0x01);
    Ok(())
}

// ============================================================================
// Super I/O implementation (generic Winbond-compatible)
// ============================================================================

fn enable_superio(timeout_secs: u32) -> Result<(), i32> {
    // Enter Super I/O configuration mode
    outb(SUPERIO_WDT_BASE, 0x87);
    outb(SUPERIO_WDT_BASE, 0x87);

    // Select watchdog device (LDN 8 is typical for WDT)
    outb(SUPERIO_WDT_BASE, 0x07);
    outb(SUPERIO_WDT_BASE + 1, 0x08);

    // Set timeout (register 0xF6)
    outb(SUPERIO_WDT_BASE, 0xF6);
    outb(SUPERIO_WDT_BASE + 1, timeout_secs.min(255) as u8);

    // Enable watchdog (register 0x30, bit 0)
    outb(SUPERIO_WDT_BASE, 0x30);
    let val = inb(SUPERIO_WDT_BASE + 1);
    outb(SUPERIO_WDT_BASE + 1, val | 0x01);

    // Exit configuration mode
    outb(SUPERIO_WDT_BASE, 0xAA);

    ENABLED.store(true, Ordering::Release);
    Ok(())
}

fn disable_superio() -> Result<(), i32> {
    // Enter configuration mode
    outb(SUPERIO_WDT_BASE, 0x87);
    outb(SUPERIO_WDT_BASE, 0x87);

    // Select watchdog device
    outb(SUPERIO_WDT_BASE, 0x07);
    outb(SUPERIO_WDT_BASE + 1, 0x08);

    // Disable watchdog
    outb(SUPERIO_WDT_BASE, 0x30);
    let val = inb(SUPERIO_WDT_BASE + 1);
    outb(SUPERIO_WDT_BASE + 1, val & !0x01);

    // Exit configuration mode
    outb(SUPERIO_WDT_BASE, 0xAA);

    ENABLED.store(false, Ordering::Release);
    Ok(())
}

fn feed_superio() -> Result<(), i32> {
    // Enter configuration mode
    outb(SUPERIO_WDT_BASE, 0x87);
    outb(SUPERIO_WDT_BASE, 0x87);

    // Select watchdog device
    outb(SUPERIO_WDT_BASE, 0x07);
    outb(SUPERIO_WDT_BASE + 1, 0x08);

    // Re-write timeout to reload
    let timeout = TIMEOUT_SECS.load(Ordering::Acquire);
    outb(SUPERIO_WDT_BASE, 0xF6);
    outb(SUPERIO_WDT_BASE + 1, timeout.min(255) as u8);

    // Exit configuration mode
    outb(SUPERIO_WDT_BASE, 0xAA);

    Ok(())
}

/// Handle watchdog ioctl for /dev/watchdog
pub fn watchdog_ioctl(request: u64, arg: u64) -> Result<u64, i32> {
    use crate::syscalls::user_buffer_in_range;
    use core::mem::size_of;

    match request {
        ioctls::WDIOC_GETSUPPORT => {
            if arg == 0 || !user_buffer_in_range(arg, size_of::<WatchdogInfo>() as u64) {
                return Err(crate::posix::errno::EFAULT);
            }
            let info = get_info();
            unsafe {
                core::ptr::write(arg as *mut WatchdogInfo, info);
            }
            Ok(0)
        }
        ioctls::WDIOC_GETSTATUS | ioctls::WDIOC_GETBOOTSTATUS => {
            if arg == 0 || !user_buffer_in_range(arg, size_of::<u32>() as u64) {
                return Err(crate::posix::errno::EFAULT);
            }
            unsafe {
                core::ptr::write(arg as *mut u32, 0);
            }
            Ok(0)
        }
        ioctls::WDIOC_GETTIMEOUT => {
            if arg == 0 || !user_buffer_in_range(arg, size_of::<u32>() as u64) {
                return Err(crate::posix::errno::EFAULT);
            }
            unsafe {
                core::ptr::write(arg as *mut u32, get_timeout());
            }
            Ok(0)
        }
        ioctls::WDIOC_SETTIMEOUT => {
            if arg == 0 || !user_buffer_in_range(arg, size_of::<u32>() as u64) {
                return Err(crate::posix::errno::EFAULT);
            }
            let timeout = unsafe { core::ptr::read(arg as *const u32) };
            let actual = set_timeout(timeout)?;
            unsafe {
                core::ptr::write(arg as *mut u32, actual);
            }
            Ok(0)
        }
        ioctls::WDIOC_KEEPALIVE => {
            feed()?;
            Ok(0)
        }
        ioctls::WDIOC_SETOPTIONS => {
            if arg == 0 || !user_buffer_in_range(arg, size_of::<u32>() as u64) {
                return Err(crate::posix::errno::EFAULT);
            }
            let opts = unsafe { core::ptr::read(arg as *const u32) };
            if opts & setoptions::WDIOS_DISABLECARD != 0 {
                disable()?;
            }
            if opts & setoptions::WDIOS_ENABLECARD != 0 {
                enable()?;
            }
            Ok(0)
        }
        _ => Err(crate::posix::errno::ENOTTY),
    }
}
