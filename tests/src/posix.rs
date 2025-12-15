//! POSIX compatibility types for testing
//!
//! These mirror the kernel's posix.rs definitions for testing purposes.

/// POSIX style error numbers (subset).
pub mod errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const ESRCH: i32 = 3;
    pub const EIO: i32 = 5;
    pub const ENXIO: i32 = 6;
    pub const E2BIG: i32 = 7;
    pub const EBADF: i32 = 9;
    pub const ECHILD: i32 = 10;
    pub const EAGAIN: i32 = 11;
    pub const ENOMEM: i32 = 12;
    pub const EACCES: i32 = 13;
    pub const EFAULT: i32 = 14;
    pub const EBUSY: i32 = 16;
    pub const EEXIST: i32 = 17;
    pub const ENODEV: i32 = 19;
    pub const ENOTDIR: i32 = 20;
    pub const EISDIR: i32 = 21;
    pub const EINVAL: i32 = 22;
    pub const EMFILE: i32 = 24;
    pub const ENOTTY: i32 = 25;
    pub const ENOSPC: i32 = 28;
    pub const ESPIPE: i32 = 29;
    pub const EROFS: i32 = 30;
    pub const EPIPE: i32 = 32;
    pub const ENOSYS: i32 = 38;
    pub const ENOEXEC: i32 = 8;
    pub const ENOTEMPTY: i32 = 39;
    pub const ENOTSOCK: i32 = 88;
    pub const ENOTSUP: i32 = 95;
    pub const EAFNOSUPPORT: i32 = 97;
    pub const EADDRINUSE: i32 = 98;
    pub const EADDRNOTAVAIL: i32 = 99;
    pub const ENETDOWN: i32 = 100;
    pub const ENETUNREACH: i32 = 101;
    pub const ECONNRESET: i32 = 104;
    pub const ETIMEDOUT: i32 = 110;
    pub const ECONNREFUSED: i32 = 111;
    pub const EINPROGRESS: i32 = 115;
    pub const EKEYREJECTED: i32 = 129;
}

/// POSIX file type enumeration
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

impl Default for Metadata {
    fn default() -> Self {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_errno_values() {
        // Verify errno values match Linux/POSIX standards
        assert_eq!(errno::EPERM, 1);
        assert_eq!(errno::ENOENT, 2);
        assert_eq!(errno::EINVAL, 22);
        assert_eq!(errno::ENOSYS, 38);
    }

    #[test]
    fn test_file_type_mode_bits() {
        assert_eq!(FileType::Regular.mode_bits(), 0o100000);
        assert_eq!(FileType::Directory.mode_bits(), 0o040000);
        assert_eq!(FileType::Symlink.mode_bits(), 0o120000);
        assert_eq!(FileType::Character.mode_bits(), 0o020000);
        assert_eq!(FileType::Block.mode_bits(), 0o060000);
        assert_eq!(FileType::Fifo.mode_bits(), 0o010000);
        assert_eq!(FileType::Socket.mode_bits(), 0o140000);
    }

    #[test]
    fn test_file_type_unknown() {
        let unknown = FileType::Unknown(0o170000);
        assert_eq!(unknown.mode_bits(), 0o170000);
        
        // Mask should only keep the type bits
        let unknown_with_perms = FileType::Unknown(0o170755);
        assert_eq!(unknown_with_perms.mode_bits(), 0o170000);
    }

    #[test]
    fn test_metadata_default() {
        let meta = Metadata::default();
        assert_eq!(meta.mode, 0);
        assert_eq!(meta.uid, 0);
        assert_eq!(meta.gid, 0);
        assert_eq!(meta.size, 0);
        assert_eq!(meta.file_type, FileType::Regular);
        assert_eq!(meta.nlink, 1);
    }
}
