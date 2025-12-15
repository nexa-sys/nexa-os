//! Mock filesystem for testing
//!
//! Simulates a VFS-like filesystem without actual disk I/O.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::posix::{errno, FileType, Metadata};

/// Inode number type
pub type InodeNumber = u64;

/// A file descriptor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileDescriptor(pub i32);

/// Open file flags
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    pub create: bool,
    pub truncate: bool,
    pub exclusive: bool,
}

impl OpenFlags {
    pub const O_RDONLY: u32 = 0;
    pub const O_WRONLY: u32 = 1;
    pub const O_RDWR: u32 = 2;
    pub const O_CREAT: u32 = 0o100;
    pub const O_EXCL: u32 = 0o200;
    pub const O_TRUNC: u32 = 0o1000;
    pub const O_APPEND: u32 = 0o2000;

    pub fn from_flags(flags: u32) -> Self {
        let access = flags & 3;
        Self {
            read: access == Self::O_RDONLY || access == Self::O_RDWR,
            write: access == Self::O_WRONLY || access == Self::O_RDWR,
            append: flags & Self::O_APPEND != 0,
            create: flags & Self::O_CREAT != 0,
            truncate: flags & Self::O_TRUNC != 0,
            exclusive: flags & Self::O_EXCL != 0,
        }
    }
}

/// A mock inode representing a file or directory
#[derive(Debug, Clone)]
pub struct MockInode {
    pub inode: InodeNumber,
    pub file_type: FileType,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub mtime: u64,
    pub nlink: u32,
    pub data: Vec<u8>,
    pub children: HashMap<String, InodeNumber>, // For directories
}

impl MockInode {
    pub fn new_file(inode: InodeNumber, mode: u16) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            inode,
            file_type: FileType::Regular,
            mode,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: now,
            nlink: 1,
            data: Vec::new(),
            children: HashMap::new(),
        }
    }

    pub fn new_dir(inode: InodeNumber, mode: u16) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            inode,
            file_type: FileType::Directory,
            mode,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: now,
            nlink: 2, // . and parent
            data: Vec::new(),
            children: HashMap::new(),
        }
    }

    pub fn metadata(&self) -> Metadata {
        Metadata {
            mode: self.mode | self.file_type.mode_bits(),
            uid: self.uid,
            gid: self.gid,
            size: self.size,
            mtime: self.mtime,
            file_type: self.file_type,
            nlink: self.nlink,
            blocks: (self.size + 511) / 512,
        }
    }
}

/// An open file handle
#[derive(Debug)]
struct OpenFile {
    inode: InodeNumber,
    flags: OpenFlags,
    offset: u64,
}

/// A mock filesystem
pub struct MockFilesystem {
    inodes: HashMap<InodeNumber, MockInode>,
    next_inode: InodeNumber,
    open_files: HashMap<FileDescriptor, OpenFile>,
    next_fd: i32,
    root_inode: InodeNumber,
}

impl Default for MockFilesystem {
    fn default() -> Self {
        Self::new()
    }
}

impl MockFilesystem {
    pub fn new() -> Self {
        let root_inode = 1;
        let mut inodes = HashMap::new();
        
        // Create root directory
        let root = MockInode::new_dir(root_inode, 0o755);
        inodes.insert(root_inode, root);
        
        Self {
            inodes,
            next_inode: 2,
            open_files: HashMap::new(),
            next_fd: 3, // 0, 1, 2 are stdin, stdout, stderr
            root_inode,
        }
    }

    /// Resolve a path to an inode number
    fn resolve_path(&self, path: &str) -> Result<InodeNumber, i32> {
        if path.is_empty() {
            return Err(errno::ENOENT);
        }

        let path = path.trim_start_matches('/');
        if path.is_empty() {
            return Ok(self.root_inode);
        }

        let mut current = self.root_inode;
        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }

            let inode = self.inodes.get(&current).ok_or(errno::ENOENT)?;
            if inode.file_type != FileType::Directory {
                return Err(errno::ENOTDIR);
            }

            current = *inode.children.get(component).ok_or(errno::ENOENT)?;
        }

        Ok(current)
    }

    /// Get parent directory and filename from path
    fn split_path(path: &str) -> (&str, &str) {
        let path = path.trim_end_matches('/');
        match path.rfind('/') {
            Some(idx) if idx == 0 => ("/", &path[1..]),
            Some(idx) => (&path[..idx], &path[idx + 1..]),
            None => (".", path),
        }
    }

    /// Create a new file
    pub fn create(&mut self, path: &str, mode: u16) -> Result<FileDescriptor, i32> {
        let (parent_path, name) = Self::split_path(path);
        
        if name.is_empty() {
            return Err(errno::EINVAL);
        }

        let parent_inode = self.resolve_path(parent_path)?;
        
        // Check if parent is a directory
        let parent = self.inodes.get_mut(&parent_inode).ok_or(errno::ENOENT)?;
        if parent.file_type != FileType::Directory {
            return Err(errno::ENOTDIR);
        }

        // Check if file already exists
        if parent.children.contains_key(name) {
            return Err(errno::EEXIST);
        }

        // Create new inode
        let new_inode = self.next_inode;
        self.next_inode += 1;
        
        let file = MockInode::new_file(new_inode, mode);
        self.inodes.insert(new_inode, file);

        // Add to parent directory
        let parent = self.inodes.get_mut(&parent_inode).unwrap();
        parent.children.insert(name.to_string(), new_inode);

        // Open the new file
        let fd = FileDescriptor(self.next_fd);
        self.next_fd += 1;

        self.open_files.insert(fd, OpenFile {
            inode: new_inode,
            flags: OpenFlags {
                read: true,
                write: true,
                ..Default::default()
            },
            offset: 0,
        });

        Ok(fd)
    }

    /// Open a file
    pub fn open(&mut self, path: &str, flags: u32) -> Result<FileDescriptor, i32> {
        let open_flags = OpenFlags::from_flags(flags);
        let inode = match self.resolve_path(path) {
            Ok(inode) => {
                if open_flags.exclusive && open_flags.create {
                    return Err(errno::EEXIST);
                }
                inode
            }
            Err(errno::ENOENT) if open_flags.create => {
                // Create the file
                let fd = self.create(path, 0o644)?;
                return Ok(fd);
            }
            Err(e) => return Err(e),
        };

        // Check if it's a directory
        {
            let file = self.inodes.get(&inode).ok_or(errno::ENOENT)?;
            if file.file_type == FileType::Directory && open_flags.write {
                return Err(errno::EISDIR);
            }
        }

        // Truncate if requested
        if open_flags.truncate && open_flags.write {
            let file = self.inodes.get_mut(&inode).unwrap();
            file.data.clear();
            file.size = 0;
        }

        let fd = FileDescriptor(self.next_fd);
        self.next_fd += 1;

        let file = self.inodes.get(&inode).ok_or(errno::ENOENT)?;
        let offset = if open_flags.append {
            file.size
        } else {
            0
        };

        self.open_files.insert(fd, OpenFile {
            inode,
            flags: open_flags,
            offset,
        });

        Ok(fd)
    }

    /// Close a file
    pub fn close(&mut self, fd: FileDescriptor) -> Result<(), i32> {
        self.open_files.remove(&fd).ok_or(errno::EBADF)?;
        Ok(())
    }

    /// Read from a file
    pub fn read(&mut self, fd: FileDescriptor, buf: &mut [u8]) -> Result<usize, i32> {
        let open_file = self.open_files.get(&fd).ok_or(errno::EBADF)?;
        
        if !open_file.flags.read {
            return Err(errno::EBADF);
        }

        let inode = self.inodes.get(&open_file.inode).ok_or(errno::EIO)?;
        let offset = open_file.offset as usize;
        
        if offset >= inode.data.len() {
            return Ok(0);
        }

        let available = inode.data.len() - offset;
        let to_read = buf.len().min(available);
        
        buf[..to_read].copy_from_slice(&inode.data[offset..offset + to_read]);
        
        // Update offset
        let open_file = self.open_files.get_mut(&fd).unwrap();
        open_file.offset += to_read as u64;

        Ok(to_read)
    }

    /// Write to a file
    pub fn write(&mut self, fd: FileDescriptor, buf: &[u8]) -> Result<usize, i32> {
        let open_file = self.open_files.get(&fd).ok_or(errno::EBADF)?;
        
        if !open_file.flags.write {
            return Err(errno::EBADF);
        }

        let inode_num = open_file.inode;
        let mut offset = open_file.offset as usize;
        let append = open_file.flags.append;

        let inode = self.inodes.get_mut(&inode_num).ok_or(errno::EIO)?;
        
        if append {
            offset = inode.data.len();
        }

        // Extend data if necessary
        if offset + buf.len() > inode.data.len() {
            inode.data.resize(offset + buf.len(), 0);
        }

        inode.data[offset..offset + buf.len()].copy_from_slice(buf);
        inode.size = inode.data.len() as u64;

        // Update offset
        let open_file = self.open_files.get_mut(&fd).unwrap();
        open_file.offset = (offset + buf.len()) as u64;

        Ok(buf.len())
    }

    /// Seek in a file
    pub fn seek(&mut self, fd: FileDescriptor, offset: i64, whence: i32) -> Result<u64, i32> {
        const SEEK_SET: i32 = 0;
        const SEEK_CUR: i32 = 1;
        const SEEK_END: i32 = 2;

        let open_file = self.open_files.get(&fd).ok_or(errno::EBADF)?;
        let inode = self.inodes.get(&open_file.inode).ok_or(errno::EIO)?;

        let new_offset = match whence {
            SEEK_SET => offset,
            SEEK_CUR => open_file.offset as i64 + offset,
            SEEK_END => inode.size as i64 + offset,
            _ => return Err(errno::EINVAL),
        };

        if new_offset < 0 {
            return Err(errno::EINVAL);
        }

        let open_file = self.open_files.get_mut(&fd).unwrap();
        open_file.offset = new_offset as u64;

        Ok(new_offset as u64)
    }

    /// Get file metadata
    pub fn stat(&self, path: &str) -> Result<Metadata, i32> {
        let inode_num = self.resolve_path(path)?;
        let inode = self.inodes.get(&inode_num).ok_or(errno::ENOENT)?;
        Ok(inode.metadata())
    }

    /// Get file metadata by fd
    pub fn fstat(&self, fd: FileDescriptor) -> Result<Metadata, i32> {
        let open_file = self.open_files.get(&fd).ok_or(errno::EBADF)?;
        let inode = self.inodes.get(&open_file.inode).ok_or(errno::EIO)?;
        Ok(inode.metadata())
    }

    /// Create a directory
    pub fn mkdir(&mut self, path: &str, mode: u16) -> Result<(), i32> {
        let (parent_path, name) = Self::split_path(path);
        
        if name.is_empty() {
            return Err(errno::EINVAL);
        }

        let parent_inode = self.resolve_path(parent_path)?;
        
        let parent = self.inodes.get_mut(&parent_inode).ok_or(errno::ENOENT)?;
        if parent.file_type != FileType::Directory {
            return Err(errno::ENOTDIR);
        }

        if parent.children.contains_key(name) {
            return Err(errno::EEXIST);
        }

        let new_inode = self.next_inode;
        self.next_inode += 1;
        
        let dir = MockInode::new_dir(new_inode, mode);
        self.inodes.insert(new_inode, dir);

        let parent = self.inodes.get_mut(&parent_inode).unwrap();
        parent.children.insert(name.to_string(), new_inode);
        parent.nlink += 1;

        Ok(())
    }

    /// Remove a file
    pub fn unlink(&mut self, path: &str) -> Result<(), i32> {
        let (parent_path, name) = Self::split_path(path);
        
        if name.is_empty() {
            return Err(errno::EINVAL);
        }

        let parent_inode_num = self.resolve_path(parent_path)?;
        let inode_num = self.resolve_path(path)?;
        
        let inode = self.inodes.get(&inode_num).ok_or(errno::ENOENT)?;
        if inode.file_type == FileType::Directory {
            return Err(errno::EISDIR);
        }

        // Remove from parent
        let parent = self.inodes.get_mut(&parent_inode_num).unwrap();
        parent.children.remove(name);

        // Decrement nlink
        let inode = self.inodes.get_mut(&inode_num).unwrap();
        inode.nlink -= 1;
        
        // Remove inode if no more links and not open
        if inode.nlink == 0 {
            let is_open = self.open_files.values().any(|f| f.inode == inode_num);
            if !is_open {
                self.inodes.remove(&inode_num);
            }
        }

        Ok(())
    }

    /// Remove a directory
    pub fn rmdir(&mut self, path: &str) -> Result<(), i32> {
        let (parent_path, name) = Self::split_path(path);
        
        if name.is_empty() || path == "/" {
            return Err(errno::EINVAL);
        }

        let parent_inode_num = self.resolve_path(parent_path)?;
        let inode_num = self.resolve_path(path)?;
        
        let inode = self.inodes.get(&inode_num).ok_or(errno::ENOENT)?;
        if inode.file_type != FileType::Directory {
            return Err(errno::ENOTDIR);
        }

        if !inode.children.is_empty() {
            return Err(errno::ENOTEMPTY);
        }

        // Remove from parent
        let parent = self.inodes.get_mut(&parent_inode_num).unwrap();
        parent.children.remove(name);
        parent.nlink -= 1;

        // Remove inode
        self.inodes.remove(&inode_num);

        Ok(())
    }

    /// List directory contents
    pub fn readdir(&self, path: &str) -> Result<Vec<String>, i32> {
        let inode_num = self.resolve_path(path)?;
        let inode = self.inodes.get(&inode_num).ok_or(errno::ENOENT)?;
        
        if inode.file_type != FileType::Directory {
            return Err(errno::ENOTDIR);
        }

        let mut entries: Vec<String> = inode.children.keys().cloned().collect();
        entries.sort();
        Ok(entries)
    }

    /// Check if a path exists
    pub fn exists(&self, path: &str) -> bool {
        self.resolve_path(path).is_ok()
    }

    /// Check if path is a file
    pub fn is_file(&self, path: &str) -> bool {
        self.resolve_path(path)
            .ok()
            .and_then(|i| self.inodes.get(&i))
            .map(|n| n.file_type == FileType::Regular)
            .unwrap_or(false)
    }

    /// Check if path is a directory
    pub fn is_dir(&self, path: &str) -> bool {
        self.resolve_path(path)
            .ok()
            .and_then(|i| self.inodes.get(&i))
            .map(|n| n.file_type == FileType::Directory)
            .unwrap_or(false)
    }
}

/// Thread-safe filesystem wrapper
pub struct SharedFilesystem(Arc<RwLock<MockFilesystem>>);

impl Default for SharedFilesystem {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedFilesystem {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(MockFilesystem::new())))
    }

    pub fn with_fs<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut MockFilesystem) -> R,
    {
        let mut fs = self.0.write().unwrap();
        f(&mut fs)
    }
}

impl Clone for SharedFilesystem {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_create_and_stat() {
        let mut fs = MockFilesystem::new();
        
        let fd = fs.create("/test.txt", 0o644).unwrap();
        assert!(fd.0 >= 3);
        
        let meta = fs.stat("/test.txt").unwrap();
        assert_eq!(meta.file_type, FileType::Regular);
        assert_eq!(meta.size, 0);
        
        fs.close(fd).unwrap();
    }

    #[test]
    fn test_fs_write_read() {
        let mut fs = MockFilesystem::new();
        
        let fd = fs.create("/hello.txt", 0o644).unwrap();
        
        let data = b"Hello, World!";
        let written = fs.write(fd, data).unwrap();
        assert_eq!(written, data.len());
        
        // Seek to beginning
        fs.seek(fd, 0, 0).unwrap();
        
        // Read back
        let mut buf = [0u8; 20];
        let read = fs.read(fd, &mut buf).unwrap();
        assert_eq!(read, data.len());
        assert_eq!(&buf[..read], data);
        
        fs.close(fd).unwrap();
    }

    #[test]
    fn test_fs_mkdir_and_readdir() {
        let mut fs = MockFilesystem::new();
        
        fs.mkdir("/mydir", 0o755).unwrap();
        assert!(fs.is_dir("/mydir"));
        
        fs.create("/mydir/file1.txt", 0o644).unwrap();
        fs.create("/mydir/file2.txt", 0o644).unwrap();
        
        let entries = fs.readdir("/mydir").unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&"file1.txt".to_string()));
        assert!(entries.contains(&"file2.txt".to_string()));
    }

    #[test]
    fn test_fs_unlink() {
        let mut fs = MockFilesystem::new();
        
        fs.create("/todelete.txt", 0o644).unwrap();
        assert!(fs.exists("/todelete.txt"));
        
        fs.unlink("/todelete.txt").unwrap();
        assert!(!fs.exists("/todelete.txt"));
    }

    #[test]
    fn test_fs_rmdir() {
        let mut fs = MockFilesystem::new();
        
        fs.mkdir("/emptydir", 0o755).unwrap();
        fs.rmdir("/emptydir").unwrap();
        assert!(!fs.exists("/emptydir"));
    }

    #[test]
    fn test_fs_rmdir_not_empty() {
        let mut fs = MockFilesystem::new();
        
        fs.mkdir("/notempty", 0o755).unwrap();
        fs.create("/notempty/file.txt", 0o644).unwrap();
        
        let result = fs.rmdir("/notempty");
        assert_eq!(result, Err(errno::ENOTEMPTY));
    }

    #[test]
    fn test_fs_open_flags() {
        let mut fs = MockFilesystem::new();
        
        // Create with O_CREAT
        let fd = fs.open("/created.txt", OpenFlags::O_RDWR | OpenFlags::O_CREAT).unwrap();
        fs.close(fd).unwrap();
        assert!(fs.exists("/created.txt"));
        
        // O_EXCL should fail on existing file
        let result = fs.open("/created.txt", OpenFlags::O_RDWR | OpenFlags::O_CREAT | OpenFlags::O_EXCL);
        assert_eq!(result, Err(errno::EEXIST));
    }

    #[test]
    fn test_fs_seek() {
        let mut fs = MockFilesystem::new();
        
        let fd = fs.create("/seek.txt", 0o644).unwrap();
        fs.write(fd, b"0123456789").unwrap();
        
        // SEEK_SET
        assert_eq!(fs.seek(fd, 5, 0).unwrap(), 5);
        
        // SEEK_CUR
        assert_eq!(fs.seek(fd, 2, 1).unwrap(), 7);
        
        // SEEK_END
        assert_eq!(fs.seek(fd, -3, 2).unwrap(), 7);
        
        fs.close(fd).unwrap();
    }

    #[test]
    fn test_fs_append_mode() {
        let mut fs = MockFilesystem::new();
        
        // Create and write initial data
        let fd = fs.create("/append.txt", 0o644).unwrap();
        fs.write(fd, b"Hello").unwrap();
        fs.close(fd).unwrap();
        
        // Open in append mode
        let fd = fs.open("/append.txt", OpenFlags::O_WRONLY | OpenFlags::O_APPEND).unwrap();
        fs.write(fd, b" World").unwrap();
        fs.close(fd).unwrap();
        
        // Verify content
        let fd = fs.open("/append.txt", OpenFlags::O_RDONLY).unwrap();
        let mut buf = [0u8; 20];
        let read = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..read], b"Hello World");
        fs.close(fd).unwrap();
    }

    #[test]
    fn test_fs_truncate() {
        let mut fs = MockFilesystem::new();
        
        let fd = fs.create("/trunc.txt", 0o644).unwrap();
        fs.write(fd, b"Some long content").unwrap();
        fs.close(fd).unwrap();
        
        // Open with truncate
        let fd = fs.open("/trunc.txt", OpenFlags::O_RDWR | OpenFlags::O_TRUNC).unwrap();
        
        let meta = fs.fstat(fd).unwrap();
        assert_eq!(meta.size, 0);
        
        fs.close(fd).unwrap();
    }

    #[test]
    fn test_fs_nested_directories() {
        let mut fs = MockFilesystem::new();
        
        fs.mkdir("/a", 0o755).unwrap();
        fs.mkdir("/a/b", 0o755).unwrap();
        fs.mkdir("/a/b/c", 0o755).unwrap();
        
        fs.create("/a/b/c/deep.txt", 0o644).unwrap();
        
        assert!(fs.is_file("/a/b/c/deep.txt"));
        assert!(fs.is_dir("/a/b/c"));
        assert!(fs.is_dir("/a/b"));
        assert!(fs.is_dir("/a"));
    }

    #[test]
    fn test_fs_root_operations() {
        let mut fs = MockFilesystem::new();
        
        assert!(fs.is_dir("/"));
        
        let entries = fs.readdir("/").unwrap();
        assert!(entries.is_empty()); // Root starts empty
        
        fs.create("/root_file.txt", 0o644).unwrap();
        
        let entries = fs.readdir("/").unwrap();
        assert_eq!(entries, vec!["root_file.txt"]);
    }

    #[test]
    fn test_fs_bad_fd() {
        let mut fs = MockFilesystem::new();
        
        let bad_fd = FileDescriptor(999);
        
        assert_eq!(fs.close(bad_fd), Err(errno::EBADF));
        
        let mut buf = [0u8; 10];
        assert_eq!(fs.read(bad_fd, &mut buf), Err(errno::EBADF));
        assert_eq!(fs.write(bad_fd, &buf), Err(errno::EBADF));
    }

    #[test]
    fn test_fs_enotdir() {
        let mut fs = MockFilesystem::new();
        
        fs.create("/file.txt", 0o644).unwrap();
        
        // Try to create inside a file
        let result = fs.create("/file.txt/nested.txt", 0o644);
        assert_eq!(result, Err(errno::ENOTDIR));
    }

    #[test]
    fn test_shared_filesystem() {
        let fs = SharedFilesystem::new();
        
        fs.with_fs(|f| {
            f.create("/shared.txt", 0o644).unwrap();
        });
        
        let exists = fs.with_fs(|f| f.exists("/shared.txt"));
        assert!(exists);
    }
}
