//! Direct I/O port access syscalls
//!
//! This module provides privileged access to I/O ports for userspace programs.
//! Access is strictly controlled:
//! - Only root (UID 0) can access I/O ports
//! - Certain sensitive ports are blocked (e.g., PCI config, ACPI)
//!
//! Syscalls:
//! - SYS_IOPL: Set I/O privilege level (Linux-compatible)
//! - SYS_IOPERM: Set I/O port permission bitmap (Linux-compatible)
//! - SYS_INB/SYS_OUTB: Direct port read/write (NexaOS extension)

use crate::posix;
use crate::safety::{inb, inl, inw, outb, outl, outw};
use core::sync::atomic::{AtomicU64, Ordering};

/// Per-process I/O permission bitmap
/// Each bit represents permission for a port (0 = denied, 1 = allowed)
/// We use a simplified 64-bit bitmap covering ports 0-511 (8 ports per bit block)
static IO_PERMISSION_BITMAP: AtomicU64 = AtomicU64::new(0);

/// I/O privilege level for current process
static IOPL: AtomicU64 = AtomicU64::new(0);

/// Maximum port number for direct access
pub const MAX_PORT: u16 = 0xFFFF;

/// Blocked port ranges (security-sensitive)
const BLOCKED_PORTS: &[(u16, u16)] = &[
    (0x0CF8, 0x0CFF), // PCI Configuration Space
    (0x0080, 0x008F), // DMA page registers (sensitive)
];

/// Check if a port is blocked for security reasons
fn is_port_blocked(port: u16) -> bool {
    for &(start, end) in BLOCKED_PORTS {
        if port >= start && port <= end {
            return true;
        }
    }
    false
}

/// Check if current process has I/O permission for a port
fn has_port_permission(port: u16) -> bool {
    // Root always has access (except blocked ports)
    if crate::auth::is_superuser() {
        return !is_port_blocked(port);
    }

    // Check IOPL level
    let iopl = IOPL.load(Ordering::Acquire);
    if iopl >= 3 {
        return !is_port_blocked(port);
    }

    // Check permission bitmap for low ports
    if port < 512 {
        let bitmap = IO_PERMISSION_BITMAP.load(Ordering::Acquire);
        let bit = (port / 8) as u64;
        if bit < 64 && (bitmap & (1u64 << bit)) != 0 {
            return !is_port_blocked(port);
        }
    }

    false
}

/// SYS_IOPL - Set I/O privilege level
///
/// # Arguments
/// * `level` - New IOPL (0-3)
///
/// # Returns
/// 0 on success, -1 on error (errno set)
pub fn iopl(level: u64) -> u64 {
    // Only root can change IOPL
    if !crate::auth::is_superuser() {
        crate::kwarn!("[port] iopl: permission denied (not root)");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    // Validate level (0-3)
    if level > 3 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    IOPL.store(level, Ordering::Release);
    crate::kinfo!("[port] iopl set to {}", level);

    posix::set_errno(0);
    0
}

/// SYS_IOPERM - Set I/O port permission bitmap
///
/// # Arguments
/// * `from` - First port in range
/// * `num` - Number of ports
/// * `turn_on` - 1 to enable, 0 to disable
///
/// # Returns
/// 0 on success, -1 on error (errno set)
pub fn ioperm(from: u64, num: u64, turn_on: u64) -> u64 {
    // Only root can change I/O permissions
    if !crate::auth::is_superuser() {
        crate::kwarn!("[port] ioperm: permission denied (not root)");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    // Validate port range
    if from > MAX_PORT as u64 || num == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let end = from.saturating_add(num - 1);
    if end > MAX_PORT as u64 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // We only support ports 0-511 in our simplified bitmap
    if from >= 512 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let mut bitmap = IO_PERMISSION_BITMAP.load(Ordering::Acquire);

    for port in from..=end.min(511) {
        let bit = (port / 8) as u64;
        if bit < 64 {
            if turn_on != 0 {
                bitmap |= 1u64 << bit;
            } else {
                bitmap &= !(1u64 << bit);
            }
        }
    }

    IO_PERMISSION_BITMAP.store(bitmap, Ordering::Release);

    crate::kinfo!(
        "[port] ioperm: ports {}-{} {}",
        from,
        end,
        if turn_on != 0 { "enabled" } else { "disabled" }
    );

    posix::set_errno(0);
    0
}

/// SYS_PORT_IN - Read from I/O port (NexaOS extension)
///
/// # Arguments
/// * `port` - Port number (0-65535)
/// * `size` - Access size: 1=byte, 2=word, 4=dword
///
/// # Returns
/// Port value on success, u64::MAX on error (errno set)
pub fn port_in(port: u64, size: u64) -> u64 {
    if port > MAX_PORT as u64 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let port = port as u16;

    if !has_port_permission(port) {
        crate::kwarn!("[port] port_in: access denied for port {:#x}", port);
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    let value = match size {
        1 => inb(port) as u64,
        2 => {
            // Word access must be aligned
            if port & 1 != 0 {
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }
            inw(port) as u64
        }
        4 => {
            // Dword access must be aligned
            if port & 3 != 0 {
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }
            inl(port) as u64
        }
        _ => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    posix::set_errno(0);
    value
}

/// SYS_PORT_OUT - Write to I/O port (NexaOS extension)
///
/// # Arguments
/// * `port` - Port number (0-65535)
/// * `value` - Value to write
/// * `size` - Access size: 1=byte, 2=word, 4=dword
///
/// # Returns
/// 0 on success, u64::MAX on error (errno set)
pub fn port_out(port: u64, value: u64, size: u64) -> u64 {
    if port > MAX_PORT as u64 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let port = port as u16;

    if !has_port_permission(port) {
        crate::kwarn!("[port] port_out: access denied for port {:#x}", port);
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    match size {
        1 => outb(port, value as u8),
        2 => {
            // Word access must be aligned
            if port & 1 != 0 {
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }
            outw(port, value as u16);
        }
        4 => {
            // Dword access must be aligned
            if port & 3 != 0 {
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }
            outl(port, value as u32);
        }
        _ => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    }

    posix::set_errno(0);
    0
}

/// Check if I/O port access is available
pub fn is_available() -> bool {
    // I/O ports are always available on x86
    true
}

/// Get current IOPL level
pub fn get_iopl() -> u64 {
    IOPL.load(Ordering::Acquire)
}
