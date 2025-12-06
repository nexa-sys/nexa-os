//! Filesystem operations for libc compatibility
//!
//! Provides POSIX filesystem functions: mkdir, rmdir, unlink, rename, getcwd, chdir, etc.

use crate::{c_char, c_int, c_void, mode_t, size_t, ssize_t};
use core::ptr;

// System call numbers for filesystem operations
const SYS_GETCWD: u64 = 79;
const SYS_CHDIR: u64 = 80;
const SYS_FCHDIR: u64 = 81;
const SYS_RENAME: u64 = 82;
const SYS_MKDIR: u64 = 83;
const SYS_RMDIR: u64 = 84;
const SYS_UNLINK: u64 = 87;
const SYS_SYMLINK: u64 = 88;
const SYS_LINK: u64 = 86;
const SYS_CHMOD: u64 = 90;
const SYS_FCHMOD: u64 = 91;
const SYS_CHOWN: u64 = 92;
const SYS_FCHOWN: u64 = 93;
const SYS_TRUNCATE: u64 = 76;
const SYS_FTRUNCATE: u64 = 77;
const SYS_GETDENTS64: u64 = 217;
const SYS_FACCESSAT: u64 = 269;
const SYS_FCHMODAT: u64 = 268;
const SYS_MKDIRAT: u64 = 258;
const SYS_UNLINKAT: u64 = 263;
const SYS_RENAMEAT: u64 = 264;
const SYS_RENAMEAT2: u64 = 316;

// Special directory fd for *at functions
pub const AT_FDCWD: c_int = -100;
pub const AT_REMOVEDIR: c_int = 0x200;
pub const AT_SYMLINK_NOFOLLOW: c_int = 0x100;

// Current working directory storage
const CWD_MAX: usize = 256;
static mut CURRENT_WORKING_DIR: [u8; CWD_MAX] = [0; CWD_MAX];
static mut CWD_LEN: usize = 1; // Start with "/"

// Initialize CWD to root
#[used]
#[link_section = ".init_array"]
static CWD_INIT: extern "C" fn() = {
    extern "C" fn init_cwd() {
        unsafe {
            CURRENT_WORKING_DIR[0] = b'/';
            CWD_LEN = 1;
        }
    }
    init_cwd
};

// ============================================================================
// Directory Operations
// ============================================================================

/// Get current working directory
#[no_mangle]
pub unsafe extern "C" fn getcwd(buf: *mut c_char, size: size_t) -> *mut c_char {
    if buf.is_null() || size == 0 {
        crate::set_errno(crate::EINVAL);
        return ptr::null_mut();
    }

    // First try the syscall
    let ret = crate::syscall2(SYS_GETCWD, buf as u64, size as u64);
    
    if ret != u64::MAX && ret > 0 {
        // Kernel returned a valid path
        crate::set_errno(0);
        return buf;
    }
    
    // Fall back to our tracked CWD
    let cwd_len = CWD_LEN;
    if cwd_len >= size {
        crate::set_errno(34); // ERANGE
        return ptr::null_mut();
    }

    ptr::copy_nonoverlapping(CURRENT_WORKING_DIR.as_ptr(), buf as *mut u8, cwd_len);
    *(buf.add(cwd_len)) = 0; // null terminate

    crate::set_errno(0);
    buf
}

/// realpath - return the canonicalized absolute pathname
/// 
/// This is a simplified implementation that handles basic path normalization.
/// It resolves `.` and `..` components but doesn't follow symlinks (NexaOS doesn't support them).
#[no_mangle]
pub unsafe extern "C" fn realpath(path: *const c_char, resolved_path: *mut c_char) -> *mut c_char {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return ptr::null_mut();
    }
    
    let path_len = crate::strlen(path as *const u8);
    if path_len == 0 {
        crate::set_errno(crate::ENOENT);
        return ptr::null_mut();
    }
    
    // Allocate buffer if not provided
    const PATH_MAX: usize = 4096;
    let result_buf = if resolved_path.is_null() {
        crate::malloc(PATH_MAX) as *mut c_char
    } else {
        resolved_path
    };
    
    if result_buf.is_null() {
        crate::set_errno(crate::ENOMEM);
        return ptr::null_mut();
    }
    
    let path_bytes = core::slice::from_raw_parts(path as *const u8, path_len);
    
    // Start with absolute path
    let mut result: [u8; PATH_MAX] = [0; PATH_MAX];
    let mut result_len: usize = 0;
    
    if path_bytes[0] != b'/' {
        // Relative path - prepend cwd
        let cwd_len = CWD_LEN;
        if cwd_len > 0 {
            ptr::copy_nonoverlapping(CURRENT_WORKING_DIR.as_ptr(), result.as_mut_ptr(), cwd_len);
            result_len = cwd_len;
        }
    }
    
    // Process path components
    let mut i = 0;
    while i < path_len {
        // Skip leading slashes
        while i < path_len && path_bytes[i] == b'/' {
            i += 1;
        }
        if i >= path_len {
            break;
        }
        
        // Find end of component
        let start = i;
        while i < path_len && path_bytes[i] != b'/' {
            i += 1;
        }
        let component = &path_bytes[start..i];
        
        if component == b"." {
            // Current directory - skip
            continue;
        } else if component == b".." {
            // Parent directory - remove last component
            if result_len > 1 {
                // Find last slash
                while result_len > 1 && result[result_len - 1] != b'/' {
                    result_len -= 1;
                }
                // Remove trailing slash (unless it's the root)
                if result_len > 1 {
                    result_len -= 1;
                }
            }
        } else {
            // Regular component - append
            if result_len == 0 || result[result_len - 1] != b'/' {
                if result_len + 1 >= PATH_MAX {
                    if resolved_path.is_null() {
                        crate::free(result_buf as *mut c_void);
                    }
                    crate::set_errno(36); // ENAMETOOLONG
                    return ptr::null_mut();
                }
                result[result_len] = b'/';
                result_len += 1;
            }
            
            if result_len + component.len() >= PATH_MAX {
                if resolved_path.is_null() {
                    crate::free(result_buf as *mut c_void);
                }
                crate::set_errno(36); // ENAMETOOLONG
                return ptr::null_mut();
            }
            
            ptr::copy_nonoverlapping(component.as_ptr(), result.as_mut_ptr().add(result_len), component.len());
            result_len += component.len();
        }
    }
    
    // Handle empty result (e.g., just ".." from root)
    if result_len == 0 {
        result[0] = b'/';
        result_len = 1;
    }
    
    // Copy result to output buffer
    ptr::copy_nonoverlapping(result.as_ptr(), result_buf as *mut u8, result_len);
    *(result_buf.add(result_len)) = 0;
    
    crate::set_errno(0);
    result_buf
}

/// Change current working directory
#[no_mangle]
pub unsafe extern "C" fn chdir(path: *const c_char) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let path_len = crate::strlen(path as *const u8);
    
    // Try the kernel syscall first
    let ret = crate::syscall1(SYS_CHDIR, path as u64);
    
    // Even if kernel doesn't support it, we track it ourselves
    // Update our internal CWD tracking
    if path_len > 0 && path_len < CWD_MAX {
        let path_slice = core::slice::from_raw_parts(path as *const u8, path_len);
        
        if path_slice[0] == b'/' {
            // Absolute path
            ptr::copy_nonoverlapping(path_slice.as_ptr(), CURRENT_WORKING_DIR.as_mut_ptr(), path_len);
            CWD_LEN = path_len;
        } else {
            // Relative path - append to current
            let old_len = CWD_LEN;
            let need_slash = old_len > 0 && CURRENT_WORKING_DIR[old_len - 1] != b'/';
            let new_len = old_len + (if need_slash { 1 } else { 0 }) + path_len;
            
            if new_len < CWD_MAX {
                if need_slash {
                    CURRENT_WORKING_DIR[old_len] = b'/';
                }
                let offset = old_len + (if need_slash { 1 } else { 0 });
                ptr::copy_nonoverlapping(path_slice.as_ptr(), CURRENT_WORKING_DIR.as_mut_ptr().add(offset), path_len);
                CWD_LEN = new_len;
            }
        }
        
        // Normalize: remove trailing slashes (except for root)
        while CWD_LEN > 1 && CURRENT_WORKING_DIR[CWD_LEN - 1] == b'/' {
            CWD_LEN -= 1;
        }
    }
    
    if ret == u64::MAX {
        // Kernel doesn't support chdir but we tracked it anyway
        // Return success since we handle CWD in userspace
        crate::set_errno(0);
    } else {
        crate::set_errno(0);
    }
    0
}

/// Change directory by file descriptor
#[no_mangle]
pub unsafe extern "C" fn fchdir(fd: c_int) -> c_int {
    let ret = crate::syscall1(SYS_FCHDIR, fd as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Create a directory
#[no_mangle]
pub unsafe extern "C" fn mkdir(path: *const c_char, mode: mode_t) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall2(SYS_MKDIR, path as u64, mode as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Create directory relative to directory fd
#[no_mangle]
pub unsafe extern "C" fn mkdirat(dirfd: c_int, path: *const c_char, mode: mode_t) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall3(SYS_MKDIRAT, dirfd as u64, path as u64, mode as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Remove a directory
#[no_mangle]
pub unsafe extern "C" fn rmdir(path: *const c_char) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall1(SYS_RMDIR, path as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

// ============================================================================
// File Operations
// ============================================================================

/// Remove a file
#[no_mangle]
pub unsafe extern "C" fn unlink(path: *const c_char) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall1(SYS_UNLINK, path as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Remove file or directory relative to directory fd
#[no_mangle]
pub unsafe extern "C" fn unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall3(SYS_UNLINKAT, dirfd as u64, path as u64, flags as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Rename a file
#[no_mangle]
pub unsafe extern "C" fn rename(oldpath: *const c_char, newpath: *const c_char) -> c_int {
    if oldpath.is_null() || newpath.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall2(SYS_RENAME, oldpath as u64, newpath as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Rename file relative to directory fds
#[no_mangle]
pub unsafe extern "C" fn renameat(
    olddirfd: c_int,
    oldpath: *const c_char,
    newdirfd: c_int,
    newpath: *const c_char,
) -> c_int {
    if oldpath.is_null() || newpath.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall4(
        SYS_RENAMEAT,
        olddirfd as u64,
        oldpath as u64,
        newdirfd as u64,
        newpath as u64,
    );
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Create a hard link
#[no_mangle]
pub unsafe extern "C" fn link(oldpath: *const c_char, newpath: *const c_char) -> c_int {
    if oldpath.is_null() || newpath.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall2(SYS_LINK, oldpath as u64, newpath as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Create a symbolic link
#[no_mangle]
pub unsafe extern "C" fn symlink(target: *const c_char, linkpath: *const c_char) -> c_int {
    if target.is_null() || linkpath.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall2(SYS_SYMLINK, target as u64, linkpath as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

// ============================================================================
// Permission Operations
// ============================================================================

/// Change file mode
#[no_mangle]
pub unsafe extern "C" fn chmod(path: *const c_char, mode: mode_t) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall2(SYS_CHMOD, path as u64, mode as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Change file mode by fd
#[no_mangle]
pub unsafe extern "C" fn fchmod(fd: c_int, mode: mode_t) -> c_int {
    let ret = crate::syscall2(SYS_FCHMOD, fd as u64, mode as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Change file mode relative to directory fd
#[no_mangle]
pub unsafe extern "C" fn fchmodat(
    dirfd: c_int,
    path: *const c_char,
    mode: mode_t,
    flags: c_int,
) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall4(SYS_FCHMODAT, dirfd as u64, path as u64, mode as u64, flags as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Change file owner
#[no_mangle]
pub unsafe extern "C" fn chown(path: *const c_char, owner: crate::uid_t, group: crate::gid_t) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall3(SYS_CHOWN, path as u64, owner as u64, group as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Change file owner by fd
#[no_mangle]
pub unsafe extern "C" fn fchown(fd: c_int, owner: crate::uid_t, group: crate::gid_t) -> c_int {
    let ret = crate::syscall3(SYS_FCHOWN, fd as u64, owner as u64, group as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Change file owner (don't follow symlinks)
#[no_mangle]
pub unsafe extern "C" fn lchown(path: *const c_char, owner: crate::uid_t, group: crate::gid_t) -> c_int {
    // Same as chown since we don't have symlinks yet
    chown(path, owner, group)
}

// ============================================================================
// File Size Operations
// ============================================================================

/// Truncate a file to a specified length
#[no_mangle]
pub unsafe extern "C" fn truncate(path: *const c_char, length: crate::off_t) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall2(SYS_TRUNCATE, path as u64, length as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Truncate a file by fd
#[no_mangle]
pub unsafe extern "C" fn ftruncate(fd: c_int, length: crate::off_t) -> c_int {
    let ret = crate::syscall2(SYS_FTRUNCATE, fd as u64, length as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// 64-bit version
#[no_mangle]
pub unsafe extern "C" fn truncate64(path: *const c_char, length: i64) -> c_int {
    truncate(path, length)
}

/// 64-bit version
#[no_mangle]
pub unsafe extern "C" fn ftruncate64(fd: c_int, length: i64) -> c_int {
    ftruncate(fd, length)
}

// ============================================================================
// File Access Check
// ============================================================================

/// Check user's permissions for a file relative to directory fd
#[no_mangle]
pub unsafe extern "C" fn faccessat(
    dirfd: c_int,
    path: *const c_char,
    mode: c_int,
    flags: c_int,
) -> c_int {
    if path.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall4(SYS_FACCESSAT, dirfd as u64, path as u64, mode as u64, flags as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

// ============================================================================
// Directory Reading
// ============================================================================

/// Read directory entries
#[no_mangle]
pub unsafe extern "C" fn getdents64(fd: c_int, dirp: *mut c_void, count: size_t) -> ssize_t {
    if dirp.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let ret = crate::syscall3(SYS_GETDENTS64, fd as u64, dirp as u64, count as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        ret as ssize_t
    }
}

// ============================================================================
// File Sync Operations
// ============================================================================

const SYS_FSYNC: u64 = 74;
const SYS_FDATASYNC: u64 = 75;
const SYS_SYNC: u64 = 162;
const SYS_SYNCFS: u64 = 306;

/// Synchronize a file's in-core state with storage device
#[no_mangle]
pub unsafe extern "C" fn fsync(fd: c_int) -> c_int {
    let ret = crate::syscall1(SYS_FSYNC, fd as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Synchronize a file's data without metadata
#[no_mangle]
pub unsafe extern "C" fn fdatasync(fd: c_int) -> c_int {
    let ret = crate::syscall1(SYS_FDATASYNC, fd as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Commit filesystem caches to disk
#[no_mangle]
pub unsafe extern "C" fn sync() {
    let _ = crate::syscall0(SYS_SYNC);
}

/// Commit filesystem containing file referred to by fd to disk
#[no_mangle]
pub unsafe extern "C" fn syncfs(fd: c_int) -> c_int {
    let ret = crate::syscall1(SYS_SYNCFS, fd as u64);
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

// ============================================================================
// Miscellaneous
// ============================================================================

/// Change file access and modification times (stub)
#[no_mangle]
pub unsafe extern "C" fn utimes(_path: *const c_char, _times: *const c_void) -> c_int {
    // Stub: not implemented
    0
}

/// Change file access and modification times (nanosecond precision, stub)
#[no_mangle]
pub unsafe extern "C" fn utimensat(
    _dirfd: c_int,
    _path: *const c_char,
    _times: *const c_void,
    _flags: c_int,
) -> c_int {
    // Stub: not implemented
    0
}

/// Change file access and modification times by fd (stub)
#[no_mangle]
pub unsafe extern "C" fn futimens(_fd: c_int, _times: *const c_void) -> c_int {
    // Stub: not implemented
    0
}

/// umask - set file mode creation mask
static mut UMASK_VALUE: mode_t = 0o022;

#[no_mangle]
pub unsafe extern "C" fn umask(mask: mode_t) -> mode_t {
    let old = UMASK_VALUE;
    UMASK_VALUE = mask & 0o777;
    old
}
