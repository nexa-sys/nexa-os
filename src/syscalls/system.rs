//! System management syscalls
//!
//! Implements: reboot, shutdown, runlevel, mount, umount, chroot, pivot_root, syslog

use super::types::*;
use crate::posix;
use crate::process::{USER_REGION_SIZE, USER_VIRT_BASE};
use core::slice;
use core::str;

// Syslog command types (compatible with Linux)
const SYSLOG_ACTION_READ: i32 = 2; // Read from the log buffer
const SYSLOG_ACTION_READ_ALL: i32 = 3; // Read all messages remaining in the ring buffer
const SYSLOG_ACTION_SIZE_BUFFER: i32 = 10; // Return number of bytes in the log buffer

/// SYS_REBOOT - System reboot (requires privilege)
/// cmd values: 0x01234567=RESTART, 0x4321FEDC=HALT, 0xCDEF0123=POWER_OFF
pub fn reboot(cmd: i32) -> u64 {
    crate::kinfo!("reboot(cmd={:#x}) called", cmd);

    // Check if caller is root (UID 0) or has CAP_SYS_BOOT
    // For now, we allow any process to reboot (simplified security)
    if !crate::auth::is_superuser() {
        crate::kwarn!("Reboot attempted by non-root user");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    // Linux reboot magic numbers
    const LINUX_REBOOT_CMD_RESTART: i32 = 0x01234567;
    const LINUX_REBOOT_CMD_HALT: i32 = 0x4321FEDC_u32 as i32;
    const LINUX_REBOOT_CMD_POWER_OFF: i32 = 0xCDEF0123_u32 as i32;

    match cmd {
        LINUX_REBOOT_CMD_RESTART => {
            crate::kinfo!("System reboot requested via syscall");
            crate::init::reboot();
        }
        LINUX_REBOOT_CMD_HALT => {
            crate::kinfo!("System halt requested via syscall");
            crate::init::shutdown();
        }
        LINUX_REBOOT_CMD_POWER_OFF => {
            crate::kinfo!("System power off requested via syscall");
            crate::init::shutdown();
        }
        _ => {
            crate::kwarn!("Invalid reboot command: {:#x}", cmd);
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    }

    // Never returns
    posix::set_errno(0);
    0
}

/// SYS_SHUTDOWN - System shutdown (power off the system)
pub fn shutdown() -> u64 {
    crate::kinfo!("shutdown() called");

    // Check privilege
    if !crate::auth::is_superuser() {
        crate::kwarn!("Shutdown attempted by non-root user");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    crate::kinfo!("System shutdown requested via syscall");
    crate::init::shutdown();

    // Never returns
    posix::set_errno(0);
    0
}

/// SYS_RUNLEVEL - Get or set system runlevel
/// arg < 0: get current runlevel (return value)
/// arg >= 0: set runlevel (requires root)
pub fn runlevel(level: i32) -> u64 {
    if level < 0 {
        // Get current runlevel
        let current = crate::init::current_runlevel();
        crate::kinfo!("runlevel: get -> {:?}", current);
        posix::set_errno(0);
        return current as u64;
    }

    // Set runlevel (requires privilege)
    if !crate::auth::is_superuser() {
        crate::kwarn!("Runlevel change attempted by non-root user");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    // Validate runlevel
    let new_level = match level {
        0 => crate::init::RunLevel::Halt,
        1 => crate::init::RunLevel::SingleUser,
        2 => crate::init::RunLevel::MultiUser,
        3 => crate::init::RunLevel::MultiUserNetwork,
        4 => crate::init::RunLevel::Unused,
        5 => crate::init::RunLevel::MultiUserGUI,
        6 => crate::init::RunLevel::Reboot,
        _ => {
            crate::kwarn!("Invalid runlevel: {}", level);
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    crate::kinfo!("runlevel: set -> {:?}", new_level);

    match crate::init::change_runlevel(new_level) {
        Ok(_) => {
            posix::set_errno(0);
            0
        }
        Err(e) => {
            crate::kerror!("Failed to change runlevel: {}", e);
            posix::set_errno(posix::errno::EINVAL);
            u64::MAX
        }
    }
}

/// SYS_MOUNT - Mount a filesystem
///
/// Supports mounting various filesystem types including:
/// - tmpfs: In-memory temporary filesystem
/// - proc: Process information pseudo-filesystem
/// - sysfs: System information pseudo-filesystem
/// - devtmpfs: Device pseudo-filesystem
/// - ext2/ext3/ext4: Block device filesystems (limited support)
pub fn mount(req_ptr: *const MountRequest) -> u64 {
    crate::kinfo!("syscall: mount");

    // Check privilege
    if !crate::auth::is_superuser() {
        crate::kwarn!("mount: permission denied");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    // Validate pointer is in user space (simplified check)
    let ptr_addr = req_ptr as usize;
    if req_ptr.is_null()
        || ptr_addr < USER_VIRT_BASE as usize
        || ptr_addr >= (USER_VIRT_BASE + USER_REGION_SIZE) as usize
    {
        crate::kwarn!("mount: invalid request pointer: {:#x}", ptr_addr);
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Read request structure
    let req = unsafe { &*req_ptr };

    // Read strings from userspace
    let source_slice =
        unsafe { slice::from_raw_parts(req.source_ptr as *const u8, req.source_len as usize) };
    let source = match str::from_utf8(source_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let target_slice =
        unsafe { slice::from_raw_parts(req.target_ptr as *const u8, req.target_len as usize) };
    let target = match str::from_utf8(target_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let fstype_slice =
        unsafe { slice::from_raw_parts(req.fstype_ptr as *const u8, req.fstype_len as usize) };
    let fstype = match str::from_utf8(fstype_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    crate::kinfo!(
        "mount: source='{}' target='{}' fstype='{}'",
        source,
        target,
        fstype
    );

    // Handle different filesystem types
    match fstype {
        "tmpfs" => {
            // Create mount point if it doesn't exist
            if !crate::fs::file_exists(target) {
                crate::fs::add_directory(leak_str(target));
            }

            // Mount tmpfs with default options
            let options = crate::fs::TmpfsMountOptions::default();
            match crate::fs::mount_tmpfs(leak_str(target), options) {
                Ok(()) => {
                    crate::kinfo!("mount: tmpfs mounted at {}", target);
                    posix::set_errno(0);
                    0
                }
                Err(_) => {
                    crate::kwarn!("mount: failed to mount tmpfs at {}", target);
                    posix::set_errno(posix::errno::EBUSY);
                    u64::MAX
                }
            }
        }

        "proc" => {
            // Create mount point if it doesn't exist
            if !crate::fs::file_exists(target) {
                crate::fs::add_directory(leak_str(target));
            }
            crate::boot::stages::mark_mounted("proc");
            crate::kinfo!("mount: proc mounted at {}", target);
            posix::set_errno(0);
            0
        }

        "sysfs" => {
            // Create mount point if it doesn't exist
            if !crate::fs::file_exists(target) {
                crate::fs::add_directory(leak_str(target));
            }
            crate::boot::stages::mark_mounted("sys");
            crate::kinfo!("mount: sysfs mounted at {}", target);
            posix::set_errno(0);
            0
        }

        "devtmpfs" | "devfs" => {
            // Create mount point if it doesn't exist
            if !crate::fs::file_exists(target) {
                crate::fs::add_directory(leak_str(target));
            }
            crate::boot::stages::mark_mounted("dev");
            crate::kinfo!("mount: devtmpfs mounted at {}", target);
            posix::set_errno(0);
            0
        }

        "ext2" | "ext3" | "ext4" => {
            // Block device filesystem - requires block device access
            crate::kwarn!("mount: ext2/3/4 mount requires block device access");
            crate::kwarn!("mount: use boot configuration for root filesystem");
            posix::set_errno(posix::errno::ENOSYS);
            u64::MAX
        }

        _ => {
            crate::kwarn!("mount: unsupported filesystem type: {}", fstype);
            posix::set_errno(posix::errno::ENODEV);
            u64::MAX
        }
    }
}

/// Leak a &str to get a &'static str for VFS paths
fn leak_str(s: &str) -> &'static str {
    use alloc::string::String;
    alloc::boxed::Box::leak(String::from(s).into_boxed_str())
}

/// SYS_UMOUNT - Unmount a filesystem
pub fn umount(target_ptr: *const u8, target_len: usize) -> u64 {
    crate::kinfo!("syscall: umount");

    // Check privilege
    if !crate::auth::is_superuser() {
        crate::kwarn!("umount: permission denied");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    if target_ptr.is_null() || target_len == 0 {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let target_slice = unsafe { slice::from_raw_parts(target_ptr, target_len) };
    let target = match str::from_utf8(target_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    crate::kinfo!("umount: target='{}'", target);

    // Try to unmount tmpfs
    if crate::fs::is_tmpfs_mounted(target) {
        match crate::fs::unmount_tmpfs(target) {
            Ok(()) => {
                crate::kinfo!("umount: successfully unmounted tmpfs at {}", target);
                posix::set_errno(0);
                return 0;
            }
            Err(_) => {
                crate::kwarn!("umount: failed to unmount tmpfs at {}", target);
                posix::set_errno(posix::errno::EBUSY);
                return u64::MAX;
            }
        }
    }

    // Other filesystems not yet supported for unmount
    crate::kwarn!("umount: unmounting non-tmpfs not yet supported");
    posix::set_errno(posix::errno::ENOSYS);
    u64::MAX
}

/// SYS_CHROOT - Change root directory
pub fn chroot(path_ptr: *const u8, path_len: usize) -> u64 {
    crate::kinfo!("syscall: chroot");

    // Check privilege
    if !crate::auth::is_superuser() {
        crate::kwarn!("chroot: permission denied");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    if path_ptr.is_null() || path_len == 0 {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let path_slice = unsafe { slice::from_raw_parts(path_ptr, path_len) };
    let path = match str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    crate::kinfo!("chroot: path='{}'", path);

    // PLACEHOLDER: Return not implemented
    // Real implementation would update process root directory in PCB
    crate::kwarn!("chroot syscall not fully implemented, returning ENOSYS");
    posix::set_errno(posix::errno::ENOSYS);
    u64::MAX
}

/// SYS_PIVOT_ROOT - Change root filesystem
///
/// TODO: This is a placeholder that validates arguments but doesn't perform actual pivot.
/// Real implementation requires:
/// - VFS root switching
/// - Mount point migration
/// - Process root directory updates
/// - Initramfs memory cleanup
pub fn pivot_root(req_ptr: *const PivotRootRequest) -> u64 {
    crate::kinfo!("syscall: pivot_root");

    // Check privilege
    if !crate::auth::is_superuser() {
        crate::kwarn!("pivot_root: permission denied");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    // Validate pointer is in user space
    let ptr_addr = req_ptr as usize;
    if req_ptr.is_null()
        || ptr_addr < USER_VIRT_BASE as usize
        || ptr_addr >= (USER_VIRT_BASE + USER_REGION_SIZE) as usize
    {
        crate::kwarn!("pivot_root: invalid request pointer: {:#x}", ptr_addr);
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Read request structure
    let req = unsafe { &*req_ptr };

    let new_root_slice =
        unsafe { slice::from_raw_parts(req.new_root_ptr as *const u8, req.new_root_len as usize) };
    let new_root = match str::from_utf8(new_root_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let put_old_slice =
        unsafe { slice::from_raw_parts(req.put_old_ptr as *const u8, req.put_old_len as usize) };
    let put_old = match str::from_utf8(put_old_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    crate::kinfo!("pivot_root: new_root='{}' put_old='{}'", new_root, put_old);

    // PLACEHOLDER: Return not implemented
    // Real implementation would:
    // 1. Verify new_root is a mount point
    // 2. Verify put_old is under new_root
    // 3. Swap root filesystem
    // 4. Move old root to put_old
    // 5. Update all process root directories
    crate::kwarn!("pivot_root syscall not fully implemented, returning ENOSYS");
    posix::set_errno(posix::errno::ENOSYS);
    u64::MAX
}

/// SYS_SYSLOG - Read kernel ring buffer log
/// type: action to perform
///   SYSLOG_ACTION_READ (2): Read up to len bytes from log buffer
///   SYSLOG_ACTION_READ_ALL (3): Read all messages from log buffer
///   SYSLOG_ACTION_SIZE_BUFFER (10): Return size of log buffer
/// buf: user buffer to write log data to
/// len: length of user buffer
/// Returns: number of bytes read, or buffer size for SIZE_BUFFER, or -1 on error
pub fn syslog(type_: i32, buf_ptr: *mut u8, len: usize) -> u64 {
    // Validate buffer address for read operations
    if type_ == SYSLOG_ACTION_READ || type_ == SYSLOG_ACTION_READ_ALL {
        if buf_ptr.is_null() || len == 0 {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        // Validate that buffer is in user space
        let buf_addr = buf_ptr as u64;
        if buf_addr < USER_VIRT_BASE || buf_addr >= USER_VIRT_BASE + USER_REGION_SIZE {
            posix::set_errno(posix::errno::EFAULT);
            return u64::MAX;
        }
    }

    match type_ {
        SYSLOG_ACTION_SIZE_BUFFER => {
            // Return the total size of the ring buffer
            posix::set_errno(0);
            crate::logger::RINGBUF_SIZE as u64
        }
        SYSLOG_ACTION_READ | SYSLOG_ACTION_READ_ALL => {
            // Read from ring buffer directly to user buffer
            // This avoids allocating 64KB on the kernel stack which would cause overflow
            let user_buf = unsafe { slice::from_raw_parts_mut(buf_ptr, len) };
            let (copy_len, _valid_len) = crate::logger::read_ringbuffer_to_slice(user_buf);

            posix::set_errno(0);
            copy_len as u64
        }
        _ => {
            // Unsupported action
            posix::set_errno(posix::errno::EINVAL);
            u64::MAX
        }
    }
}
