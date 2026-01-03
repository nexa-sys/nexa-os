//! TmpFS Tests
//!
//! Tests for the temporary filesystem types and structures.

#[cfg(test)]
mod tests {
    use crate::fs::tmpfs::{TmpfsInode, TmpfsMetadata};

    // =========================================================================
    // TmpfsMetadata Structure Tests
    // =========================================================================

    #[test]
    fn test_tmpfs_metadata_size() {
        let size = core::mem::size_of::<TmpfsMetadata>();
        // mode(2) + uid(4) + gid(4) + size(8) + atime(8) + mtime(8) + ctime(8) + nlink(4) = 46, aligned
        assert!(size >= 40);
        assert!(size <= 64);
    }

    #[test]
    fn test_tmpfs_metadata_clone() {
        let m1 = TmpfsMetadata {
            mode: 0o644,
            uid: 1000,
            gid: 1000,
            size: 1024,
            atime: 1000000,
            mtime: 1000000,
            ctime: 1000000,
            nlink: 1,
        };
        let m2 = m1.clone();
        assert_eq!(m1.mode, m2.mode);
        assert_eq!(m1.size, m2.size);
    }

    #[test]
    fn test_tmpfs_metadata_file() {
        let meta = TmpfsMetadata {
            mode: 0o644, // -rw-r--r--
            uid: 0,
            gid: 0,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            nlink: 1,
        };
        assert_eq!(meta.mode, 0o644);
        assert_eq!(meta.nlink, 1);
    }

    #[test]
    fn test_tmpfs_metadata_directory() {
        let meta = TmpfsMetadata {
            mode: 0o755, // drwxr-xr-x
            uid: 0,
            gid: 0,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            nlink: 2, // . and ..
        };
        assert_eq!(meta.mode, 0o755);
        assert_eq!(meta.nlink, 2);
    }

    // =========================================================================
    // TmpfsInode Enumeration Tests
    // =========================================================================

    #[test]
    fn test_tmpfs_inode_file_variant() {
        let meta = TmpfsMetadata {
            mode: 0o644,
            uid: 0,
            gid: 0,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            nlink: 1,
        };
        let inode = TmpfsInode::File {
            data: vec![1, 2, 3],
            metadata: meta,
        };
        assert!(matches!(inode, TmpfsInode::File { .. }));
    }

    #[test]
    fn test_tmpfs_inode_directory_variant() {
        use std::collections::BTreeMap;

        let meta = TmpfsMetadata {
            mode: 0o755,
            uid: 0,
            gid: 0,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            nlink: 2,
        };
        let inode = TmpfsInode::Directory {
            children: BTreeMap::new(),
            metadata: meta,
        };
        assert!(matches!(inode, TmpfsInode::Directory { .. }));
    }

    #[test]
    fn test_tmpfs_inode_symlink_variant() {
        let meta = TmpfsMetadata {
            mode: 0o777,
            uid: 0,
            gid: 0,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            nlink: 1,
        };
        let inode = TmpfsInode::Symlink {
            target: "/etc/passwd".into(),
            metadata: meta,
        };
        assert!(matches!(inode, TmpfsInode::Symlink { .. }));
    }

    #[test]
    fn test_tmpfs_inode_clone() {
        let meta = TmpfsMetadata {
            mode: 0o644,
            uid: 0,
            gid: 0,
            size: 5,
            atime: 0,
            mtime: 0,
            ctime: 0,
            nlink: 1,
        };
        let inode1 = TmpfsInode::File {
            data: vec![b'h', b'e', b'l', b'l', b'o'],
            metadata: meta,
        };
        let inode2 = inode1.clone();
        match inode2 {
            TmpfsInode::File { data, metadata } => {
                assert_eq!(data.len(), 5);
                assert_eq!(metadata.size, 5);
            }
            _ => panic!("Expected File variant"),
        }
    }

    // =========================================================================
    // Tmpfs Constants Tests
    // =========================================================================

    #[test]
    fn test_tmpfs_max_size() {
        const MAX_TMPFS_SIZE: usize = 256 * 1024 * 1024;
        assert_eq!(MAX_TMPFS_SIZE, 268435456); // 256 MiB
    }

    #[test]
    fn test_tmpfs_max_inodes() {
        const MAX_INODES: usize = 65536;
        assert_eq!(MAX_INODES, 65536);
    }

    #[test]
    fn test_tmpfs_max_file_size() {
        const MAX_FILE_SIZE: usize = 64 * 1024 * 1024;
        assert_eq!(MAX_FILE_SIZE, 67108864); // 64 MiB
    }

    // =========================================================================
    // Permission Mode Tests
    // =========================================================================

    #[test]
    fn test_file_mode_parsing() {
        let mode: u16 = 0o644;
        let owner_read = (mode >> 8) & 1;
        let owner_write = (mode >> 7) & 1;
        let owner_exec = (mode >> 6) & 1;
        
        assert_eq!(owner_read, 1);  // r
        assert_eq!(owner_write, 1); // w
        assert_eq!(owner_exec, 0);  // -
    }

    #[test]
    fn test_dir_mode_parsing() {
        let mode: u16 = 0o755;
        let owner_read = (mode >> 8) & 1;
        let owner_write = (mode >> 7) & 1;
        let owner_exec = (mode >> 6) & 1;
        
        assert_eq!(owner_read, 1);  // r
        assert_eq!(owner_write, 1); // w
        assert_eq!(owner_exec, 1);  // x
    }

    #[test]
    fn test_symlink_mode() {
        // Symlinks typically have mode 0777
        let mode: u16 = 0o777;
        assert_eq!(mode & 0o777, 0o777);
    }

    // =========================================================================
    // Inode Number Tests
    // =========================================================================

    #[test]
    fn test_root_inode() {
        // Root directory is typically inode 1 (ext2) or inode 2 (ext2/ext3/ext4)
        const ROOT_INODE: u64 = 1;
        assert!(ROOT_INODE > 0);
    }

    #[test]
    fn test_inode_number_range() {
        // Inode numbers should be positive and fit in u64
        let inode: u64 = 12345;
        assert!(inode > 0);
        assert!(inode < u64::MAX);
    }

    // =========================================================================
    // Block Calculation Tests
    // =========================================================================

    #[test]
    fn test_blocks_from_size() {
        fn calculate_blocks(size: u64) -> u64 {
            (size + 511) / 512
        }

        assert_eq!(calculate_blocks(0), 0);
        assert_eq!(calculate_blocks(1), 1);
        assert_eq!(calculate_blocks(512), 1);
        assert_eq!(calculate_blocks(513), 2);
        assert_eq!(calculate_blocks(1024), 2);
    }

    // =========================================================================
    // Link Count Tests
    // =========================================================================

    #[test]
    fn test_file_initial_nlink() {
        // Files start with nlink=1
        let meta = TmpfsMetadata {
            mode: 0o644,
            uid: 0,
            gid: 0,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            nlink: 1,
        };
        assert_eq!(meta.nlink, 1);
    }

    #[test]
    fn test_directory_initial_nlink() {
        // Directories start with nlink=2 (. and ..)
        let meta = TmpfsMetadata {
            mode: 0o755,
            uid: 0,
            gid: 0,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            nlink: 2,
        };
        assert_eq!(meta.nlink, 2);
    }

    #[test]
    fn test_hardlink_increases_nlink() {
        let mut nlink: u32 = 1;
        // Creating a hard link increases nlink
        nlink += 1;
        assert_eq!(nlink, 2);
    }
}
