//! User database and hostname functions
//!
//! Provides getpwuid_r, getpwnam_r, gethostname, and related functions.

use crate::{c_char, c_int, c_void, set_errno, size_t, uid_t, EINVAL, ENOENT, ERANGE};
use core::ptr;

// ============================================================================
// passwd Structure
// ============================================================================

/// Password database entry
#[repr(C)]
pub struct passwd {
    pub pw_name: *mut c_char,    // Username
    pub pw_passwd: *mut c_char,  // Password (usually "x" on modern systems)
    pub pw_uid: uid_t,           // User ID
    pub pw_gid: uid_t,           // Group ID (using uid_t as gid_t equivalent)
    pub pw_gecos: *mut c_char,   // User information
    pub pw_dir: *mut c_char,     // Home directory
    pub pw_shell: *mut c_char,   // Shell program
}

// Static buffers for simple user database
// In a real system, this would read from /etc/passwd
static mut ROOT_NAME: [u8; 8] = *b"root\0\0\0\0";
static mut ROOT_PASSWD: [u8; 4] = *b"x\0\0\0";
static mut ROOT_GECOS: [u8; 16] = *b"root\0\0\0\0\0\0\0\0\0\0\0\0";
static mut ROOT_DIR: [u8; 8] = *b"/root\0\0\0";
static mut ROOT_SHELL: [u8; 16] = *b"/bin/nsh\0\0\0\0\0\0\0\0";

static mut NOBODY_NAME: [u8; 8] = *b"nobody\0\0";
static mut NOBODY_PASSWD: [u8; 4] = *b"x\0\0\0";
static mut NOBODY_GECOS: [u8; 16] = *b"Nobody\0\0\0\0\0\0\0\0\0\0";
static mut NOBODY_DIR: [u8; 16] = *b"/nonexistent\0\0\0\0";
static mut NOBODY_SHELL: [u8; 16] = *b"/sbin/nologin\0\0\0";

// ============================================================================
// User Database Functions
// ============================================================================

/// Get password database entry by UID (reentrant)
///
/// # Arguments
/// * `uid` - User ID to look up
/// * `pwd` - passwd structure to fill
/// * `buf` - Buffer for string data
/// * `buflen` - Size of buffer
/// * `result` - Pointer to store result (or NULL if not found)
///
/// # Returns
/// 0 on success (result points to pwd), ENOENT if not found, or other error
#[no_mangle]
pub unsafe extern "C" fn getpwuid_r(
    uid: uid_t,
    pwd: *mut passwd,
    buf: *mut c_char,
    buflen: size_t,
    result: *mut *mut passwd,
) -> c_int {
    if pwd.is_null() || result.is_null() {
        return EINVAL;
    }

    // Initialize result to NULL
    *result = ptr::null_mut();

    // Look up user by UID
    match uid {
        0 => {
            // root user
            (*pwd).pw_name = ROOT_NAME.as_mut_ptr() as *mut c_char;
            (*pwd).pw_passwd = ROOT_PASSWD.as_mut_ptr() as *mut c_char;
            (*pwd).pw_uid = 0;
            (*pwd).pw_gid = 0;
            (*pwd).pw_gecos = ROOT_GECOS.as_mut_ptr() as *mut c_char;
            (*pwd).pw_dir = ROOT_DIR.as_mut_ptr() as *mut c_char;
            (*pwd).pw_shell = ROOT_SHELL.as_mut_ptr() as *mut c_char;
            *result = pwd;
            0
        }
        65534 => {
            // nobody user
            (*pwd).pw_name = NOBODY_NAME.as_mut_ptr() as *mut c_char;
            (*pwd).pw_passwd = NOBODY_PASSWD.as_mut_ptr() as *mut c_char;
            (*pwd).pw_uid = 65534;
            (*pwd).pw_gid = 65534;
            (*pwd).pw_gecos = NOBODY_GECOS.as_mut_ptr() as *mut c_char;
            (*pwd).pw_dir = NOBODY_DIR.as_mut_ptr() as *mut c_char;
            (*pwd).pw_shell = NOBODY_SHELL.as_mut_ptr() as *mut c_char;
            *result = pwd;
            0
        }
        _ => {
            // Unknown user - return ENOENT (but don't set errno for reentrant version)
            ENOENT
        }
    }
}

/// Get password database entry by name (reentrant)
#[no_mangle]
pub unsafe extern "C" fn getpwnam_r(
    name: *const c_char,
    pwd: *mut passwd,
    buf: *mut c_char,
    buflen: size_t,
    result: *mut *mut passwd,
) -> c_int {
    if name.is_null() || pwd.is_null() || result.is_null() {
        return EINVAL;
    }

    // Initialize result to NULL
    *result = ptr::null_mut();

    // Compare name with known users
    let name_len = crate::strlen(name as *const u8);

    if name_len == 4 && super::strncmp(name, b"root\0".as_ptr() as *const c_char, 4) == 0 {
        return getpwuid_r(0, pwd, buf, buflen, result);
    }

    if name_len == 6 && super::strncmp(name, b"nobody\0".as_ptr() as *const c_char, 6) == 0 {
        return getpwuid_r(65534, pwd, buf, buflen, result);
    }

    ENOENT
}

/// Get password database entry by UID (non-reentrant)
#[no_mangle]
pub unsafe extern "C" fn getpwuid(uid: uid_t) -> *mut passwd {
    static mut STATIC_PWD: passwd = passwd {
        pw_name: ptr::null_mut(),
        pw_passwd: ptr::null_mut(),
        pw_uid: 0,
        pw_gid: 0,
        pw_gecos: ptr::null_mut(),
        pw_dir: ptr::null_mut(),
        pw_shell: ptr::null_mut(),
    };

    let mut result: *mut passwd = ptr::null_mut();
    if getpwuid_r(uid, &mut STATIC_PWD, ptr::null_mut(), 0, &mut result) == 0 {
        result
    } else {
        ptr::null_mut()
    }
}

/// Get password database entry by name (non-reentrant)
#[no_mangle]
pub unsafe extern "C" fn getpwnam(name: *const c_char) -> *mut passwd {
    static mut STATIC_PWD: passwd = passwd {
        pw_name: ptr::null_mut(),
        pw_passwd: ptr::null_mut(),
        pw_uid: 0,
        pw_gid: 0,
        pw_gecos: ptr::null_mut(),
        pw_dir: ptr::null_mut(),
        pw_shell: ptr::null_mut(),
    };

    let mut result: *mut passwd = ptr::null_mut();
    if getpwnam_r(name, &mut STATIC_PWD, ptr::null_mut(), 0, &mut result) == 0 {
        result
    } else {
        ptr::null_mut()
    }
}

// ============================================================================
// Hostname Functions
// ============================================================================

/// Static hostname storage
static mut HOSTNAME: [u8; 256] = [0; 256];
static mut HOSTNAME_SET: bool = false;

/// Get the hostname
///
/// # Arguments
/// * `name` - Buffer to store hostname
/// * `len` - Size of buffer
///
/// # Returns
/// 0 on success, -1 on error
#[no_mangle]
pub unsafe extern "C" fn gethostname(name: *mut c_char, len: size_t) -> c_int {
    if name.is_null() || len == 0 {
        set_errno(EINVAL);
        return -1;
    }

    // Initialize hostname if not set
    if !HOSTNAME_SET {
        // Default hostname
        let default = b"nexaos\0";
        for (i, &b) in default.iter().enumerate() {
            HOSTNAME[i] = b;
        }
        HOSTNAME_SET = true;
    }

    // Find hostname length
    let mut hostname_len = 0;
    while hostname_len < 255 && HOSTNAME[hostname_len] != 0 {
        hostname_len += 1;
    }

    if hostname_len >= len {
        // Buffer too small - truncate
        ptr::copy_nonoverlapping(HOSTNAME.as_ptr(), name as *mut u8, len - 1);
        *(name.add(len - 1)) = 0;
        set_errno(ERANGE);
        return -1;
    }

    ptr::copy_nonoverlapping(HOSTNAME.as_ptr(), name as *mut u8, hostname_len + 1);
    set_errno(0);
    0
}

/// Set the hostname
///
/// # Arguments
/// * `name` - New hostname
/// * `len` - Length of hostname
///
/// # Returns
/// 0 on success, -1 on error
#[no_mangle]
pub unsafe extern "C" fn sethostname(name: *const c_char, len: size_t) -> c_int {
    if name.is_null() {
        set_errno(EINVAL);
        return -1;
    }

    if len >= 255 {
        set_errno(EINVAL);
        return -1;
    }

    ptr::copy_nonoverlapping(name as *const u8, HOSTNAME.as_mut_ptr(), len);
    HOSTNAME[len] = 0;
    HOSTNAME_SET = true;
    set_errno(0);
    0
}

// ============================================================================
// uname Structure and Function
// ============================================================================

/// System identification structure
#[repr(C)]
pub struct utsname {
    pub sysname: [c_char; 65],
    pub nodename: [c_char; 65],
    pub release: [c_char; 65],
    pub version: [c_char; 65],
    pub machine: [c_char; 65],
    pub domainname: [c_char; 65],
}

/// Get system identification
#[no_mangle]
pub unsafe extern "C" fn uname(buf: *mut utsname) -> c_int {
    if buf.is_null() {
        set_errno(EINVAL);
        return -1;
    }

    // Clear the structure
    ptr::write_bytes(buf, 0, 1);

    // Fill in system information
    let sysname = b"NexaOS\0";
    let release = b"1.0.0\0";
    let version = b"#1 SMP\0";
    let machine = b"x86_64\0";
    let domainname = b"(none)\0";

    for (i, &b) in sysname.iter().enumerate() {
        (*buf).sysname[i] = b as c_char;
    }
    for (i, &b) in release.iter().enumerate() {
        (*buf).release[i] = b as c_char;
    }
    for (i, &b) in version.iter().enumerate() {
        (*buf).version[i] = b as c_char;
    }
    for (i, &b) in machine.iter().enumerate() {
        (*buf).machine[i] = b as c_char;
    }
    for (i, &b) in domainname.iter().enumerate() {
        (*buf).domainname[i] = b as c_char;
    }

    // Get hostname for nodename
    gethostname((*buf).nodename.as_mut_ptr(), 65);

    set_errno(0);
    0
}
