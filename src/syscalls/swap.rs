//! Swap management system calls
//!
//! Implements: swapon, swapoff

use crate::mm::swap;
use crate::posix::{self, errno};
use crate::{kerror, kinfo, ktrace};

/// Maximum path length for swap device
const MAX_PATH_LEN: usize = 256;

/// SYS_SWAPON - Enable swapping on a device or file
///
/// # Arguments
/// * `path` - Path to the swap device or file
/// * `flags` - Swap flags (SWAP_FLAG_*)
///
/// # Returns
/// * 0 on success
/// * -1 on error with errno set
///
/// # Linux Compatibility
/// This syscall is compatible with Linux's swapon(2)
pub fn swapon(path: *const u8, flags: u32) -> u64 {
    ktrace!("[swapon] path={:?}, flags={:#x}", path, flags);

    // Validate path pointer
    if path.is_null() {
        kerror!("[swapon] Invalid path: null pointer");
        posix::set_errno(errno::EFAULT);
        return u64::MAX; // -1
    }

    // Read path from user space
    let path_slice = unsafe {
        let mut len = 0;
        while len < MAX_PATH_LEN {
            if *path.add(len) == 0 {
                break;
            }
            len += 1;
        }
        if len == 0 || len >= MAX_PATH_LEN {
            kerror!("[swapon] Invalid path length: {}", len);
            posix::set_errno(errno::EINVAL);
            return u64::MAX;
        }
        core::slice::from_raw_parts(path, len)
    };

    // Debug: log the parsed path
    kinfo!(
        "[sys_swapon] path={}, len={}",
        core::str::from_utf8(path_slice).unwrap_or("<invalid>"),
        path_slice.len()
    );

    // Check if swap module is loaded
    if !swap::is_swap_available() {
        kerror!("[swapon] Swap module not loaded");
        posix::set_errno(errno::ENOSYS);
        return u64::MAX;
    }

    // Call the swap subsystem
    let result = swap::swapon(path_slice, flags);

    if result < 0 {
        kerror!("[swapon] Failed with error: {}", result);
        posix::set_errno(errno::EINVAL); // Generic error
        return u64::MAX;
    }

    kinfo!(
        "[swapon] Activated swap: {}",
        core::str::from_utf8(path_slice).unwrap_or("<invalid>")
    );

    posix::set_errno(0);
    0
}

/// SYS_SWAPOFF - Disable swapping on a device or file
///
/// # Arguments
/// * `path` - Path to the swap device or file
///
/// # Returns
/// * 0 on success
/// * -1 on error with errno set
///
/// # Linux Compatibility
/// This syscall is compatible with Linux's swapoff(2)
pub fn swapoff(path: *const u8) -> u64 {
    ktrace!("[swapoff] path={:?}", path);

    // Validate path pointer
    if path.is_null() {
        kerror!("[swapoff] Invalid path: null pointer");
        posix::set_errno(errno::EFAULT);
        return u64::MAX;
    }

    // Read path from user space
    let path_slice = unsafe {
        let mut len = 0;
        while len < MAX_PATH_LEN {
            if *path.add(len) == 0 {
                break;
            }
            len += 1;
        }
        if len == 0 || len >= MAX_PATH_LEN {
            kerror!("[swapoff] Invalid path length: {}", len);
            posix::set_errno(errno::EINVAL);
            return u64::MAX;
        }
        core::slice::from_raw_parts(path, len)
    };

    // Check if swap module is loaded
    if !swap::is_swap_available() {
        kerror!("[swapoff] Swap module not loaded");
        posix::set_errno(errno::ENOSYS);
        return u64::MAX;
    }

    // Call the swap subsystem
    let result = swap::swapoff(path_slice);

    if result < 0 {
        kerror!("[swapoff] Failed with error: {}", result);
        // EBUSY if pages are still in use
        posix::set_errno(errno::EBUSY);
        return u64::MAX;
    }

    kinfo!(
        "[swapoff] Deactivated swap: {}",
        core::str::from_utf8(path_slice).unwrap_or("<invalid>")
    );

    posix::set_errno(0);
    0
}

/// Enable swap on a device from kernel context (e.g., fstab processing)
///
/// This is a convenience wrapper for internal kernel use that takes a path string
/// instead of a raw pointer.
///
/// # Arguments
/// * `path` - Path to the swap device as a &str
/// * `flags` - Swap flags as i32 (converted from fstab options)
///
/// # Returns
/// * Ok(()) on success
/// * Err(errno) on failure
pub fn sys_swapon(path: &str, flags: i32) -> Result<(), i32> {
    use crate::mm::swap;

    kinfo!("[sys_swapon] path={}, flags={:#x}", path, flags);

    // Check if swap module is loaded
    if !swap::is_swap_available() {
        kerror!("[sys_swapon] Swap module not loaded");
        return Err(errno::ENOSYS);
    }

    // Call the swap subsystem directly
    let result = swap::swapon(path.as_bytes(), flags as u32);

    if result < 0 {
        kerror!("[sys_swapon] Failed with error: {}", result);
        return Err(errno::EINVAL);
    }

    kinfo!("[sys_swapon] Activated swap: {}", path);
    Ok(())
}
