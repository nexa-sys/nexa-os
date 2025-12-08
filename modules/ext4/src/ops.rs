//! ext4 Filesystem Operations
//!
//! Module operations table and filesystem implementation.

use core::cmp;
use crate::superblock::{Ext4Superblock, SUPERBLOCK_OFFSET, SUPERBLOCK_SIZE};
use crate::inode::{Ext4Inode, GroupDescriptor};
use crate::extent::ExtentTree;
use crate::{mod_info, mod_error, mod_warn, EXT4_FS_INSTANCE, EXT4_WRITABLE};

extern "C" {
    fn kmod_blk_read_bytes(device_index: usize, offset: u64, buf: *mut u8, len: usize) -> i64;
    fn kmod_blk_write_bytes(device_index: usize, offset: u64, buf: *const u8, len: usize) -> i64;
    fn kmod_blk_find_rootfs() -> i32;
}

// ============================================================================
// Types
// ============================================================================

pub type Ext4Handle = *mut u8;

#[repr(C)]
pub struct FileRefHandle {
    pub fs: Ext4Handle,
    pub inode: u32,
    pub size: u64,
    pub mode: u16,
    pub blocks: u64,
    pub mtime: u64,
    pub nlink: u32,
    pub uid: u16,
    pub gid: u16,
}

pub type DirEntryCallback = extern "C" fn(*const u8, usize, u32, u8, *mut u8);

#[repr(C)]
#[derive(Default)]
pub struct Ext4Stats {
    pub inodes_count: u32,
    pub blocks_count_lo: u32,
    pub blocks_count_hi: u32,
    pub free_blocks: u32,
    pub free_inodes: u32,
    pub block_size: u32,
    pub has_extents: bool,
    pub has_64bit: bool,
}

// ============================================================================
// Module Operations Table
// ============================================================================

#[repr(C)]
pub struct Ext4ModuleOps {
    pub new: Option<extern "C" fn(*const u8, usize) -> Ext4Handle>,
    pub destroy: Option<extern "C" fn(Ext4Handle)>,
    pub lookup: Option<extern "C" fn(Ext4Handle, *const u8, usize, *mut FileRefHandle) -> i32>,
    pub read_at: Option<extern "C" fn(*const FileRefHandle, usize, *mut u8, usize) -> i32>,
    pub write_at: Option<extern "C" fn(*const FileRefHandle, usize, *const u8, usize) -> i32>,
    pub list_dir: Option<extern "C" fn(Ext4Handle, *const u8, usize, DirEntryCallback, *mut u8) -> i32>,
    pub get_stats: Option<extern "C" fn(Ext4Handle, *mut Ext4Stats) -> i32>,
    pub set_writable: Option<extern "C" fn(bool)>,
    pub is_writable: Option<extern "C" fn() -> bool>,
    pub create_file: Option<extern "C" fn(Ext4Handle, *const u8, usize, u16) -> i32>,
    pub journal_sync: Option<extern "C" fn(Ext4Handle) -> i32>,
}

impl Ext4ModuleOps {
    pub fn new() -> Self {
        Self {
            new: Some(ext4_new),
            destroy: Some(ext4_destroy),
            lookup: Some(ext4_lookup),
            read_at: Some(ext4_read_at),
            write_at: Some(ext4_write_at),
            list_dir: Some(ext4_list_dir),
            get_stats: Some(ext4_get_stats),
            set_writable: Some(ext4_set_writable),
            is_writable: Some(ext4_is_writable),
            create_file: Some(ext4_create_file),
            journal_sync: Some(ext4_journal_sync),
        }
    }
}

// ============================================================================
// Filesystem structure
// ============================================================================

pub struct Ext4Filesystem {
    pub device_index: usize,
    pub block_size: usize,
    pub inode_size: usize,
    pub inodes_per_group: u32,
    pub blocks_per_group: u32,
    pub total_groups: u32,
    pub first_data_block: u32,
    pub desc_size: usize,
    // Superblock info
    pub sb_inodes_count: u32,
    pub sb_blocks_count: u64,
    // Features
    pub has_extents: bool,
    pub has_64bit: bool,
    pub has_journal: bool,
    pub journal_initialized: bool,
}

impl Ext4Filesystem {
    fn read_bytes(&self, offset: u64, buf: &mut [u8]) -> bool {
        let result = unsafe { kmod_blk_read_bytes(self.device_index, offset, buf.as_mut_ptr(), buf.len()) };
        result >= 0
    }

    fn write_bytes(&self, offset: u64, buf: &[u8]) -> bool {
        let result = unsafe { kmod_blk_write_bytes(self.device_index, offset, buf.as_ptr(), buf.len()) };
        result >= 0
    }

    fn group_descriptor(&self, group: u32) -> Option<GroupDescriptor> {
        let sb_block = if self.block_size == 1024 { 1 } else { 0 };
        let table_block = sb_block + 1;
        let offset = table_block as u64 * self.block_size as u64 + group as u64 * self.desc_size as u64;
        
        let mut buf = [0u8; 64];
        if !self.read_bytes(offset, &mut buf[..self.desc_size]) {
            return None;
        }
        GroupDescriptor::parse(&buf, self.desc_size).ok()
    }

    fn load_inode(&self, inode_num: u32) -> Option<Ext4Inode> {
        if inode_num == 0 { return None; }
        
        let idx = inode_num - 1;
        let group = idx / self.inodes_per_group;
        let index_in_group = idx % self.inodes_per_group;
        
        let desc = self.group_descriptor(group)?;
        let table_offset = desc.inode_table() * self.block_size as u64;
        let inode_offset = table_offset + index_in_group as u64 * self.inode_size as u64;
        
        let mut buf = [0u8; 256];
        let read_size = self.inode_size.min(256);
        if !self.read_bytes(inode_offset, &mut buf[..read_size]) {
            return None;
        }
        
        Ext4Inode::parse(&buf[..read_size], self.inode_size).ok()
    }

    fn lookup_internal(&self, path: &str) -> Option<FileRef> {
        let trimmed = path.trim_matches('/');
        let mut inode_num = 2u32;
        
        if trimmed.is_empty() {
            return self.file_ref_from_inode(inode_num);
        }
        
        let mut inode = self.load_inode(inode_num)?;
        
        for segment in trimmed.split('/') {
            if segment.is_empty() { continue; }
            inode_num = self.find_in_dir(&inode, segment)?;
            inode = self.load_inode(inode_num)?;
        }
        
        self.file_ref_from_inode(inode_num)
    }

    fn file_ref_from_inode(&self, inode_num: u32) -> Option<FileRef> {
        let inode = self.load_inode(inode_num)?;
        Some(FileRef {
            inode: inode_num,
            size: inode.size(),
            mode: inode.mode,
            blocks: inode.blocks_lo as u64,
            mtime: inode.mtime as u64,
            nlink: inode.links_count as u32,
            uid: inode.uid,
            gid: inode.gid,
        })
    }

    fn find_in_dir(&self, inode: &Ext4Inode, target: &str) -> Option<u32> {
        let mut found = None;
        self.for_each_dir_entry(inode, |name, ino, _| {
            if name == target { found = Some(ino); }
        });
        found
    }

    fn for_each_dir_entry<F>(&self, inode: &Ext4Inode, mut cb: F)
    where F: FnMut(&str, u32, u8)
    {
        let bs = self.block_size;
        let mut buf = [0u8; 4096];
        
        // Handle extents or indirect blocks
        if inode.uses_extents() {
            if let Some(tree) = inode.extent_tree(bs) {
                for ext in tree.extents() {
                    for i in 0..ext.length() {
                        let pblock = ext.start_block() + i as u64;
                        self.read_dir_block(pblock, &mut buf, &mut cb);
                    }
                }
            }
        } else {
            // Traditional indirect blocks
            for i in 0..12 {
                let offset = i * 4;
                let block = u32::from_le_bytes([
                    inode.block[offset], inode.block[offset+1],
                    inode.block[offset+2], inode.block[offset+3],
                ]);
                if block != 0 {
                    self.read_dir_block(block as u64, &mut buf, &mut cb);
                }
            }
        }
    }

    fn read_dir_block<F>(&self, block: u64, buf: &mut [u8], cb: &mut F)
    where F: FnMut(&str, u32, u8)
    {
        let offset = block * self.block_size as u64;
        if !self.read_bytes(offset, &mut buf[..self.block_size]) {
            return;
        }
        
        let mut pos = 0;
        while pos + 8 <= self.block_size {
            let ino = u32::from_le_bytes([buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]]);
            let rec_len = u16::from_le_bytes([buf[pos+4], buf[pos+5]]) as usize;
            if rec_len == 0 { break; }
            
            let name_len = buf[pos+6] as usize;
            let file_type = buf[pos+7];
            
            if ino != 0 && name_len > 0 && pos + 8 + name_len <= self.block_size {
                if let Ok(name) = core::str::from_utf8(&buf[pos+8..pos+8+name_len]) {
                    cb(name, ino, file_type);
                }
            }
            pos += rec_len;
        }
    }

    fn read_file(&self, inode_num: u32, offset: usize, buf: &mut [u8]) -> usize {
        let inode = match self.load_inode(inode_num) {
            Some(i) => i,
            None => return 0,
        };
        
        if !inode.is_regular_file() { return 0; }
        
        let file_size = inode.size() as usize;
        if offset >= file_size { return 0; }
        
        let to_read = cmp::min(buf.len(), file_size - offset);
        let mut read = 0;
        let bs = self.block_size;
        let mut block_buf = [0u8; 4096];
        
        if inode.uses_extents() {
            if let Some(tree) = inode.extent_tree(bs) {
                read = self.read_via_extents(&tree, offset, &mut buf[..to_read], &mut block_buf);
            }
        } else {
            read = self.read_via_indirect(&inode, offset, &mut buf[..to_read], &mut block_buf);
        }
        
        read
    }

    fn read_via_extents(&self, tree: &ExtentTree, offset: usize, buf: &mut [u8], block_buf: &mut [u8]) -> usize {
        let bs = self.block_size;
        let mut remaining = buf.len();
        let mut written = 0;
        let mut cur_offset = offset;
        
        while remaining > 0 {
            let logical_block = (cur_offset / bs) as u32;
            let within_block = cur_offset % bs;
            
            let extent = match tree.find_extent(logical_block) {
                Some(e) => e,
                None => break,
            };
            
            let pblock = match extent.physical_block(logical_block) {
                Some(pb) => pb,
                None => break,
            };
            
            if !self.read_bytes(pblock * bs as u64, &mut block_buf[..bs]) {
                break;
            }
            
            let avail = cmp::min(bs - within_block, remaining);
            buf[written..written+avail].copy_from_slice(&block_buf[within_block..within_block+avail]);
            
            written += avail;
            remaining -= avail;
            cur_offset += avail;
        }
        
        written
    }

    fn read_via_indirect(&self, inode: &Ext4Inode, offset: usize, buf: &mut [u8], block_buf: &mut [u8]) -> usize {
        let bs = self.block_size;
        let mut remaining = buf.len();
        let mut written = 0;
        let mut cur_offset = offset;
        
        while remaining > 0 {
            let block_idx = cur_offset / bs;
            let within_block = cur_offset % bs;
            
            let block_num = match inode.indirect_block(block_idx, bs) {
                Some(bn) if bn != 0 => bn,
                _ => break,
            };
            
            if !self.read_bytes(block_num as u64 * bs as u64, &mut block_buf[..bs]) {
                break;
            }
            
            let avail = cmp::min(bs - within_block, remaining);
            buf[written..written+avail].copy_from_slice(&block_buf[within_block..within_block+avail]);
            
            written += avail;
            remaining -= avail;
            cur_offset += avail;
        }
        
        written
    }
}

struct FileRef {
    inode: u32,
    size: u64,
    mode: u16,
    blocks: u64,
    mtime: u64,
    nlink: u32,
    uid: u16,
    gid: u16,
}

// ============================================================================
// Operation implementations
// ============================================================================

extern "C" fn ext4_new(_image: *const u8, _size: usize) -> Ext4Handle {
    mod_info!(b"ext4: initializing from block device");
    
    let device = unsafe { kmod_blk_find_rootfs() };
    if device < 0 {
        mod_error!(b"ext4: no block device");
        return core::ptr::null_mut();
    }
    
    let mut sb_buf = [0u8; SUPERBLOCK_SIZE];
    let result = unsafe { kmod_blk_read_bytes(device as usize, SUPERBLOCK_OFFSET as u64, sb_buf.as_mut_ptr(), SUPERBLOCK_SIZE) };
    if result < 0 {
        mod_error!(b"ext4: read superblock failed");
        return core::ptr::null_mut();
    }
    
    let sb = match Ext4Superblock::parse(&sb_buf) {
        Ok(s) => s,
        Err(_) => {
            mod_error!(b"ext4: parse superblock failed");
            return core::ptr::null_mut();
        }
    };
    
    let inode_size = if sb.rev_level >= 1 && sb.inode_size != 0 {
        sb.inode_size as usize
    } else { 128 };
    
    let fs = Ext4Filesystem {
        device_index: device as usize,
        block_size: sb.block_size(),
        inode_size,
        inodes_per_group: sb.inodes_per_group,
        blocks_per_group: sb.blocks_per_group,
        total_groups: sb.total_groups(),
        first_data_block: sb.first_data_block,
        desc_size: sb.group_desc_size(),
        sb_inodes_count: sb.inodes_count,
        sb_blocks_count: sb.blocks_count(),
        has_extents: sb.has_extents(),
        has_64bit: sb.has_64bit(),
        has_journal: sb.has_journal(),
        journal_initialized: false,
    };
    
    if fs.has_extents {
        mod_info!(b"ext4: extent-based filesystem");
    }
    
    unsafe {
        EXT4_FS_INSTANCE = Some(fs);
        EXT4_FS_INSTANCE.as_mut().map(|f| f as *mut Ext4Filesystem as Ext4Handle).unwrap_or(core::ptr::null_mut())
    }
}

extern "C" fn ext4_destroy(_handle: Ext4Handle) {
    unsafe { EXT4_FS_INSTANCE = None; }
}

extern "C" fn ext4_lookup(handle: Ext4Handle, path: *const u8, path_len: usize, out: *mut FileRefHandle) -> i32 {
    if handle.is_null() || path.is_null() || out.is_null() { return -1; }
    
    let fs = unsafe { &*(handle as *const Ext4Filesystem) };
    let path_bytes = unsafe { core::slice::from_raw_parts(path, path_len) };
    let path_str = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    
    match fs.lookup_internal(path_str) {
        Some(fr) => {
            unsafe {
                (*out).fs = handle;
                (*out).inode = fr.inode;
                (*out).size = fr.size;
                (*out).mode = fr.mode;
                (*out).blocks = fr.blocks;
                (*out).mtime = fr.mtime;
                (*out).nlink = fr.nlink;
                (*out).uid = fr.uid;
                (*out).gid = fr.gid;
            }
            0
        }
        None => -1,
    }
}

extern "C" fn ext4_read_at(file: *const FileRefHandle, offset: usize, buf: *mut u8, len: usize) -> i32 {
    if file.is_null() || buf.is_null() { return -1; }
    
    let fr = unsafe { &*file };
    let fs = unsafe { &*(fr.fs as *const Ext4Filesystem) };
    let read = fs.read_file(fr.inode, offset, unsafe { core::slice::from_raw_parts_mut(buf, len) });
    read as i32
}

extern "C" fn ext4_write_at(_file: *const FileRefHandle, _offset: usize, _data: *const u8, _len: usize) -> i32 {
    if unsafe { !EXT4_WRITABLE } {
        mod_warn!(b"ext4: write denied - read-only");
        return -7; // ReadOnly
    }
    // TODO: Implement write
    -1
}

extern "C" fn ext4_list_dir(handle: Ext4Handle, path: *const u8, path_len: usize, cb: DirEntryCallback, ctx: *mut u8) -> i32 {
    if handle.is_null() || path.is_null() { return -1; }
    
    let fs = unsafe { &*(handle as *const Ext4Filesystem) };
    let path_bytes = unsafe { core::slice::from_raw_parts(path, path_len) };
    let path_str = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    
    let fr = match fs.lookup_internal(path_str) {
        Some(f) => f,
        None => return -1,
    };
    
    if (fr.mode & 0o170000) != 0o040000 { return -1; }
    
    let inode = match fs.load_inode(fr.inode) {
        Some(i) => i,
        None => return -1,
    };
    
    fs.for_each_dir_entry(&inode, |name, ino, ft| {
        cb(name.as_ptr(), name.len(), ino, ft, ctx);
    });
    
    0
}

extern "C" fn ext4_get_stats(handle: Ext4Handle, stats: *mut Ext4Stats) -> i32 {
    if handle.is_null() || stats.is_null() { return -1; }
    
    let fs = unsafe { &*(handle as *const Ext4Filesystem) };
    unsafe {
        (*stats).inodes_count = fs.sb_inodes_count;
        (*stats).blocks_count_lo = fs.sb_blocks_count as u32;
        (*stats).blocks_count_hi = (fs.sb_blocks_count >> 32) as u32;
        (*stats).block_size = fs.block_size as u32;
        (*stats).has_extents = fs.has_extents;
        (*stats).has_64bit = fs.has_64bit;
    }
    0
}

extern "C" fn ext4_set_writable(writable: bool) {
    unsafe { EXT4_WRITABLE = writable; }
    if writable { mod_info!(b"ext4: write mode ENABLED"); }
    else { mod_info!(b"ext4: write mode DISABLED"); }
}

extern "C" fn ext4_is_writable() -> bool {
    unsafe { EXT4_WRITABLE }
}

extern "C" fn ext4_create_file(_handle: Ext4Handle, _path: *const u8, _path_len: usize, _mode: u16) -> i32 {
    if unsafe { !EXT4_WRITABLE } { return -1; }
    // TODO: Implement
    -1
}

extern "C" fn ext4_journal_sync(handle: Ext4Handle) -> i32 {
    if handle.is_null() { return -1; }
    let fs = unsafe { &*(handle as *const Ext4Filesystem) };
    crate::journal::sync_journal(fs);
    0
}
