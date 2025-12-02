#![allow(dead_code)]

use core::sync::atomic::{AtomicI32, Ordering};

/// Global errno value shared across the kernel.
static ERRNO: AtomicI32 = AtomicI32::new(0);

/// POSIX style error numbers (subset).
pub mod errno {
    pub const EPERM: i32 = 1; // Operation not permitted
    pub const ENOENT: i32 = 2; // No such file or directory
    pub const ESRCH: i32 = 3; // No such process
    pub const EIO: i32 = 5; // I/O error
    pub const E2BIG: i32 = 7; // Argument list too long
    pub const EBADF: i32 = 9; // Bad file descriptor
    pub const ECHILD: i32 = 10; // No child processes
    pub const EAGAIN: i32 = 11; // Try again
    pub const ENOMEM: i32 = 12; // Out of memory
    pub const EACCES: i32 = 13; // Permission denied
    pub const EFAULT: i32 = 14; // Bad address
    pub const EEXIST: i32 = 17; // File exists
    pub const ENOTDIR: i32 = 20; // Not a directory
    pub const EISDIR: i32 = 21; // Is a directory
    pub const EINVAL: i32 = 22; // Invalid argument
    pub const EMFILE: i32 = 24; // Too many open files
    pub const ENOSPC: i32 = 28; // No space left on device
    pub const ESPIPE: i32 = 29; // Illegal seek
    pub const ENODEV: i32 = 19; // No such device
    pub const EPIPE: i32 = 32; // Broken pipe
    pub const ENOSYS: i32 = 38; // Function not implemented
    pub const ENOTSUP: i32 = 95; // Operation not supported
    pub const ENOTSOCK: i32 = 88; // Socket operation on non-socket
    pub const EAFNOSUPPORT: i32 = 97; // Address family not supported
    pub const EADDRINUSE: i32 = 98; // Address already in use
    pub const EADDRNOTAVAIL: i32 = 99; // Cannot assign requested address
    pub const ENETDOWN: i32 = 100; // Network is down
    pub const ENETUNREACH: i32 = 101; // Network is unreachable
    pub const ECONNRESET: i32 = 104; // Connection reset by peer
    pub const ETIMEDOUT: i32 = 110; // Connection timed out
    pub const ECONNREFUSED: i32 = 111; // Connection refused
    pub const EINPROGRESS: i32 = 115; // Operation in progress
}

/// POSIX file type enumeration used by the VFS layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    Symlink,
    Character,
    Block,
    Fifo,
    Socket,
    Unknown(u16),
}

impl FileType {
    pub const fn mode_bits(self) -> u16 {
        match self {
            FileType::Regular => 0o100000,
            FileType::Directory => 0o040000,
            FileType::Symlink => 0o120000,
            FileType::Character => 0o020000,
            FileType::Block => 0o060000,
            FileType::Fifo => 0o010000,
            FileType::Socket => 0o140000,
            FileType::Unknown(bits) => bits & 0o170000,
        }
    }
}

/// POSIX metadata description for files.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Metadata {
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub mtime: u64,
    pub file_type: FileType,
    pub nlink: u32,
    pub blocks: u64,
}

impl Metadata {
    pub const fn empty() -> Self {
        Self {
            mode: 0,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
            file_type: FileType::Regular,
            nlink: 1,
            blocks: 0,
        }
    }

    pub fn with_mode(mut self, mode: u16) -> Self {
        // Preserve file type bits (top 4 bits) and set permission bits (low 12 bits)
        self.mode = (self.mode & 0o170000) | (mode & 0o7777);
        self
    }

    pub fn with_type(mut self, file_type: FileType) -> Self {
        self.file_type = file_type;
        self.mode = (self.mode & !0o170000) | file_type.mode_bits();
        self
    }

    pub fn with_uid(mut self, uid: u32) -> Self {
        self.uid = uid;
        self
    }

    pub fn with_gid(mut self, gid: u32) -> Self {
        self.gid = gid;
        self
    }

    pub fn normalize(mut self) -> Self {
        self.mode = (self.mode & !0o170000) | self.file_type.mode_bits();
        self
    }
}

/// Userspace visible struct stat (matches Linux x86_64 layout closely).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_mode: u32,
    pub st_nlink: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: i64,
    pub st_atime_nsec: i64,
    pub st_mtime: i64,
    pub st_mtime_nsec: i64,
    pub st_ctime: i64,
    pub st_ctime_nsec: i64,
    pub st_reserved: [i64; 3],
}

impl Stat {
    pub fn from_metadata(meta: &Metadata) -> Self {
        let mut stat = Stat::default();
        stat.st_mode = meta.mode as u32;
        stat.st_nlink = meta.nlink;
        stat.st_uid = meta.uid;
        stat.st_gid = meta.gid;
        stat.st_size = meta.size as i64;
        stat.st_blocks = meta.blocks as i64;
        stat.st_mtime = meta.mtime as i64;
        stat
    }
}

/// Convert a raw Unix mode value to a FileType + mode pair.
pub fn split_mode(raw: u32) -> (u16, FileType) {
    let mode = raw as u16;
    let file_type = match raw & 0o170000 {
        0o100000 => FileType::Regular,
        0o040000 => FileType::Directory,
        0o120000 => FileType::Symlink,
        0o020000 => FileType::Character,
        0o060000 => FileType::Block,
        0o010000 => FileType::Fifo,
        0o140000 => FileType::Socket,
        other => FileType::Unknown(other as u16),
    };
    (mode, file_type)
}

/// Set the current errno value.
pub fn set_errno(value: i32) {
    ERRNO.store(value, Ordering::Relaxed);
}

/// Obtain the current errno value.
pub fn errno() -> i32 {
    ERRNO.load(Ordering::Relaxed)
}
