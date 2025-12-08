//! Directory entry operations for libc compatibility
//!
//! Provides POSIX directory operations: opendir, readdir, closedir, etc.

use crate::{c_char, c_int, off_t};
use core::ptr;

// System call numbers
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_GETDENTS64: u64 = 217;

// Open flags
const O_RDONLY: c_int = 0;
const O_DIRECTORY: c_int = 0o200000;
const O_CLOEXEC: c_int = 0o2000000;

// Buffer size for directory entries
const DIR_BUF_SIZE: usize = 8192;
// Maximum filename length
const NAME_MAX: usize = 255;

/// Directory entry structure (Linux compatible)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct dirent {
    /// Inode number
    pub d_ino: u64,
    /// Offset to next dirent
    pub d_off: off_t,
    /// Length of this record
    pub d_reclen: u16,
    /// File type
    pub d_type: u8,
    /// Filename (null-terminated)
    pub d_name: [c_char; NAME_MAX + 1],
}

impl Default for dirent {
    fn default() -> Self {
        Self {
            d_ino: 0,
            d_off: 0,
            d_reclen: 0,
            d_type: 0,
            d_name: [0; NAME_MAX + 1],
        }
    }
}

/// 64-bit directory entry (same as dirent on 64-bit systems)
pub type dirent64 = dirent;

/// Directory stream structure
#[repr(C)]
pub struct DIR {
    /// File descriptor
    fd: c_int,
    /// Buffer for directory entries
    buf: [u8; DIR_BUF_SIZE],
    /// Current position in buffer
    pos: usize,
    /// Number of bytes read into buffer
    size: usize,
    /// Current entry (for returning pointer)
    entry: dirent,
    /// End of directory reached
    eof: bool,
}

impl DIR {
    fn new(fd: c_int) -> Self {
        Self {
            fd,
            buf: [0; DIR_BUF_SIZE],
            pos: 0,
            size: 0,
            entry: dirent::default(),
            eof: false,
        }
    }
}

/// File types
pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_BLK: u8 = 6;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;
pub const DT_SOCK: u8 = 12;
pub const DT_WHT: u8 = 14;

/// Linux dirent64 structure as returned by getdents64
#[repr(C)]
struct linux_dirent64 {
    d_ino: u64,
    d_off: i64,
    d_reclen: u16,
    d_type: u8,
    // d_name follows (variable length)
}

/// Open a directory stream
/// 
/// Opens the directory specified by name and returns a pointer to a DIR structure
/// that can be used with readdir() to iterate over directory entries.
/// 
/// Returns NULL and sets errno on error.
#[no_mangle]
pub unsafe extern "C" fn opendir(name: *const c_char) -> *mut DIR {
    if name.is_null() {
        crate::set_errno(crate::EINVAL);
        return ptr::null_mut();
    }

    // Open the directory - pass (path, flags, mode)
    let fd = crate::syscall3(SYS_OPEN, name as u64, (O_RDONLY | O_DIRECTORY | O_CLOEXEC) as u64, 0);
    if fd == u64::MAX {
        crate::refresh_errno_from_kernel();
        return ptr::null_mut();
    }

    // Allocate DIR structure
    // Use a simple global pool for DIR structures (we don't have malloc in no_std)
    let dir_ptr = allocate_dir();
    if dir_ptr.is_null() {
        // Close fd and return error
        crate::syscall1(SYS_CLOSE, fd);
        crate::set_errno(crate::ENOMEM);
        return ptr::null_mut();
    }

    // Initialize DIR structure
    let dir = &mut *dir_ptr;
    *dir = DIR::new(fd as c_int);
    
    crate::set_errno(0);
    dir_ptr
}

/// Open a directory stream by file descriptor
#[no_mangle]
pub unsafe extern "C" fn fdopendir(fd: c_int) -> *mut DIR {
    if fd < 0 {
        crate::set_errno(crate::EINVAL);
        return ptr::null_mut();
    }

    let dir_ptr = allocate_dir();
    if dir_ptr.is_null() {
        crate::set_errno(crate::ENOMEM);
        return ptr::null_mut();
    }

    let dir = &mut *dir_ptr;
    *dir = DIR::new(fd);
    
    crate::set_errno(0);
    dir_ptr
}

/// Read a directory entry
/// 
/// Returns a pointer to a dirent structure representing the next directory entry.
/// Returns NULL when reaching end of directory or on error (check errno).
#[no_mangle]
pub unsafe extern "C" fn readdir(dirp: *mut DIR) -> *mut dirent {
    if dirp.is_null() {
        crate::set_errno(crate::EINVAL);
        return ptr::null_mut();
    }

    let dir = &mut *dirp;
    
    // If we've reached EOF, return NULL
    if dir.eof {
        crate::set_errno(0); // Not an error, just end of directory
        return ptr::null_mut();
    }

    // If buffer is empty or exhausted, read more entries
    if dir.pos >= dir.size {
        let n = crate::syscall3(
            SYS_GETDENTS64,
            dir.fd as u64,
            dir.buf.as_mut_ptr() as u64,
            DIR_BUF_SIZE as u64,
        );
        
        if n == u64::MAX {
            crate::refresh_errno_from_kernel();
            return ptr::null_mut();
        }
        
        if n == 0 {
            // End of directory
            dir.eof = true;
            crate::set_errno(0);
            return ptr::null_mut();
        }
        
        dir.pos = 0;
        dir.size = n as usize;
    }

    // Parse the next entry from buffer
    let entry_ptr = dir.buf.as_ptr().add(dir.pos) as *const linux_dirent64;
    let linux_entry = &*entry_ptr;
    
    // Copy to our dirent structure
    dir.entry.d_ino = linux_entry.d_ino;
    dir.entry.d_off = linux_entry.d_off;
    dir.entry.d_reclen = linux_entry.d_reclen;
    dir.entry.d_type = linux_entry.d_type;
    
    // Copy name (d_name starts at offset 19 in linux_dirent64)
    let name_ptr = (entry_ptr as *const u8).add(19) as *const c_char;
    
    let mut i = 0;
    while i < NAME_MAX {
        let c = *name_ptr.add(i);
        dir.entry.d_name[i] = c;
        if c == 0 {
            break;
        }
        i += 1;
    }
    dir.entry.d_name[NAME_MAX] = 0; // Ensure null termination
    
    // Move to next entry
    dir.pos += linux_entry.d_reclen as usize;
    
    crate::set_errno(0);
    &mut dir.entry as *mut dirent
}

/// Read directory entry (64-bit version, same as readdir on 64-bit systems)
#[no_mangle]
pub unsafe extern "C" fn readdir64(dirp: *mut DIR) -> *mut dirent64 {
    readdir(dirp)
}

/// Read directory entry (reentrant version)
#[no_mangle]
pub unsafe extern "C" fn readdir_r(
    dirp: *mut DIR,
    entry: *mut dirent,
    result: *mut *mut dirent,
) -> c_int {
    if dirp.is_null() || entry.is_null() || result.is_null() {
        return crate::EINVAL;
    }

    let ent = readdir(dirp);
    if ent.is_null() {
        *result = ptr::null_mut();
        // Check if it was an error or just EOF
        let err = crate::get_errno();
        if err != 0 {
            return err;
        }
        return 0; // EOF, not an error
    }

    // Copy entry
    ptr::copy_nonoverlapping(ent, entry, 1);
    *result = entry;
    
    0
}

/// Close a directory stream
#[no_mangle]
pub unsafe extern "C" fn closedir(dirp: *mut DIR) -> c_int {
    if dirp.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let dir = &*dirp;
    let ret = crate::syscall1(SYS_CLOSE, dir.fd as u64);
    
    // Free the DIR structure
    free_dir(dirp);
    
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        crate::set_errno(0);
        0
    }
}

/// Rewind directory stream to the beginning
#[no_mangle]
pub unsafe extern "C" fn rewinddir(dirp: *mut DIR) {
    if dirp.is_null() {
        return;
    }

    let dir = &mut *dirp;
    
    // Seek to beginning of directory
    const SYS_LSEEK: u64 = 8;
    const SEEK_SET: c_int = 0;
    let _ = crate::syscall3(SYS_LSEEK, dir.fd as u64, 0, SEEK_SET as u64);
    
    // Reset buffer state
    dir.pos = 0;
    dir.size = 0;
    dir.eof = false;
}

/// Seek to a location in the directory stream
#[no_mangle]
pub unsafe extern "C" fn seekdir(dirp: *mut DIR, loc: c_int) {
    if dirp.is_null() {
        return;
    }

    let dir = &mut *dirp;
    
    const SYS_LSEEK: u64 = 8;
    const SEEK_SET: c_int = 0;
    let _ = crate::syscall3(SYS_LSEEK, dir.fd as u64, loc as u64, SEEK_SET as u64);
    
    // Reset buffer state (need to re-read after seek)
    dir.pos = 0;
    dir.size = 0;
    dir.eof = false;
}

/// Get current location in directory stream
#[no_mangle]
pub unsafe extern "C" fn telldir(dirp: *mut DIR) -> c_int {
    if dirp.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let dir = &*dirp;
    
    const SYS_LSEEK: u64 = 8;
    const SEEK_CUR: c_int = 1;
    let ret = crate::syscall3(SYS_LSEEK, dir.fd as u64, 0, SEEK_CUR as u64);
    
    if ret == u64::MAX {
        crate::refresh_errno_from_kernel();
        -1
    } else {
        ret as c_int
    }
}

/// Get directory file descriptor
#[no_mangle]
pub unsafe extern "C" fn dirfd(dirp: *mut DIR) -> c_int {
    if dirp.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    (*dirp).fd
}

/// Scan a directory for matching entries
#[no_mangle]
pub unsafe extern "C" fn scandir(
    dirp: *const c_char,
    namelist: *mut *mut *mut dirent,
    filter: Option<extern "C" fn(*const dirent) -> c_int>,
    compar: Option<extern "C" fn(*const *const dirent, *const *const dirent) -> c_int>,
) -> c_int {
    // Simplified implementation - not fully functional without malloc
    // Would need dynamic memory allocation for full implementation
    crate::set_errno(crate::ENOSYS);
    -1
}

// ============================================================================
// DIR structure pool (simple static allocation for no_std)
// ============================================================================

const MAX_OPEN_DIRS: usize = 32;

struct DirPool {
    dirs: [Option<DIR>; MAX_OPEN_DIRS],
    in_use: [bool; MAX_OPEN_DIRS],
}

static mut DIR_POOL: DirPool = DirPool {
    dirs: [const { None }; MAX_OPEN_DIRS],
    in_use: [false; MAX_OPEN_DIRS],
};

unsafe fn allocate_dir() -> *mut DIR {
    for i in 0..MAX_OPEN_DIRS {
        if !DIR_POOL.in_use[i] {
            DIR_POOL.in_use[i] = true;
            DIR_POOL.dirs[i] = Some(DIR::new(-1));
            return DIR_POOL.dirs[i].as_mut().unwrap() as *mut DIR;
        }
    }
    ptr::null_mut()
}

unsafe fn free_dir(dirp: *mut DIR) {
    for i in 0..MAX_OPEN_DIRS {
        if let Some(ref dir) = DIR_POOL.dirs[i] {
            if dir as *const DIR == dirp as *const DIR {
                DIR_POOL.in_use[i] = false;
                DIR_POOL.dirs[i] = None;
                return;
            }
        }
    }
}

// ============================================================================
// Convenience functions 
// ============================================================================

/// alphasort - comparison function for scandir
#[no_mangle]
pub unsafe extern "C" fn alphasort(a: *const *const dirent, b: *const *const dirent) -> c_int {
    if a.is_null() || b.is_null() || (*a).is_null() || (*b).is_null() {
        return 0;
    }
    
    let name_a = (**a).d_name.as_ptr();
    let name_b = (**b).d_name.as_ptr();
    
    // Simple strcmp
    let mut i = 0;
    loop {
        let ca = *name_a.add(i) as u8;
        let cb = *name_b.add(i) as u8;
        
        if ca == 0 && cb == 0 {
            return 0;
        }
        if ca < cb {
            return -1;
        }
        if ca > cb {
            return 1;
        }
        i += 1;
    }
}

/// versionsort - version-aware comparison function for scandir
#[no_mangle]
pub unsafe extern "C" fn versionsort(a: *const *const dirent, b: *const *const dirent) -> c_int {
    // For simplicity, just use alphasort
    alphasort(a, b)
}
