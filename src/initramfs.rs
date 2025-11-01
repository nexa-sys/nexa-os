/// Initial RAM Filesystem support
/// Loads files from a CPIO archive embedded in the kernel
///
/// 初始 RAM 文件系统支持（initramfs）
/// 从内核嵌入的 CPIO 归档加载文件，用于启动阶段提供最小用户态程序和资源。
use core::slice;

#[allow(dead_code)]
pub fn debug_dump_state(label: &str) {
    unsafe {
        let present = INITRAMFS_PRESENT;
        let instance_ptr = core::ptr::addr_of!(INITRAMFS_INSTANCE);
        let storage_addr = instance_ptr as usize;
        let base = (*instance_ptr).base as usize;
        let size = (*instance_ptr).size;
        crate::kdebug!(
            "initramfs::debug_dump_state[{}]: present={} storage={:#x} base={:#x} size={:#x}",
            label,
            present,
            storage_addr,
            base,
            size
        );
    }
}

// GS data for syscall and Ring 3 switch - moved to top to avoid memory layout conflicts
#[repr(C, align(64))]
pub struct GsData(pub [u64; 16]);

#[link_section = ".gs_data"]
pub static mut GS_DATA: GsData = GsData([0; 16]);

#[used]
#[link_section = ".gs_data_pad"]
static mut GS_DATA_PADDING: [u8; 4096] = [0; 4096];

/// CPIO newc format header (110 bytes ASCII)
///
/// CPIO newc 格式的头部（110 字节 ASCII），字段都是十六进制 ASCII 文本表示。
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct CpioNewcHeader {
    pub magic: [u8; 6], // "070701" or "070702"
    // magic 字段：应为 ASCII "070701"（newc）或 "070702"
    pub ino: [u8; 8], // Inode number
    // inode 编号（ASCII hex）
    pub mode: [u8; 8], // File mode
    // 文件模式（权限和类型，以 ASCII hex 表示）
    pub uid: [u8; 8], // User ID
    // 所属用户 ID（ASCII hex）
    pub gid: [u8; 8], // Group ID
    // 所属组 ID（ASCII hex）
    pub nlink: [u8; 8], // Number of links
    // 硬链接数量（ASCII hex）
    pub mtime: [u8; 8], // Modification time
    // 修改时间（ASCII hex，UNIX 时间戳）
    pub filesize: [u8; 8], // File size
    // 文件大小（字节，ASCII hex）
    pub devmajor: [u8; 8], // Device major
    // 设备主号（ASCII hex，通常为 0）
    pub devminor: [u8; 8], // Device minor
    // 设备次号（ASCII hex，通常为 0）
    pub rdevmajor: [u8; 8], // Real device major
    // 特殊设备（rdev）主号（ASCII hex）
    pub rdevminor: [u8; 8], // Real device minor
    // 特殊设备（rdev）次号（ASCII hex）
    pub namesize: [u8; 8], // Filename length
    // 文件名长度（包括末尾的 NUL 字节，ASCII hex）
    pub check: [u8; 8], // Checksum
                        // 校验和字段（通常未使用，ASCII hex）
}

impl CpioNewcHeader {
    const MAGIC_NEWC: &'static [u8; 6] = b"070701";
    const TRAILER: &'static str = "TRAILER!!!";

    fn parse_hex(bytes: &[u8]) -> u64 {
        let mut result = 0u64;
        for &b in bytes {
            result = result * 16
                + match b {
                    b'0'..=b'9' => (b - b'0') as u64,
                    b'a'..=b'f' => (b - b'a' + 10) as u64,
                    b'A'..=b'F' => (b - b'A' + 10) as u64,
                    _ => 0,
                };
        }
        result
    }

    pub fn is_valid(&self) -> bool {
        &self.magic == Self::MAGIC_NEWC
    }

    pub fn filesize(&self) -> usize {
        Self::parse_hex(&self.filesize) as usize
    }

    pub fn namesize(&self) -> usize {
        Self::parse_hex(&self.namesize) as usize
    }

    pub fn mode(&self) -> u32 {
        Self::parse_hex(&self.mode) as u32
    }
}

pub struct InitramfsEntry {
    pub name: &'static str,
    pub data: &'static [u8],
    pub mode: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Initramfs {
    base: *const u8,
    size: usize,
}

impl Initramfs {
    const fn empty() -> Self {
        Self {
            base: core::ptr::null(),
            size: 0,
        }
    }
}

impl Initramfs {
    /// Create from embedded data
    /// Create from embedded data
    ///
    /// 从原始内存区域创建 Initramfs 实例。调用者需保证 base 指向有效的 CPIO 数据
    /// 且在该 Initramfs 生命周期内保持可读（或已被内核复制）。这是 unsafe 的因为
    /// 函数接受裸指针并依赖调用方保证内存有效性。
    pub unsafe fn new(base: *const u8, size: usize) -> Self {
        Self { base, size }
    }

    /// Parse CPIO archive and return all entries
    /// Parse CPIO archive and return all entries
    ///
    /// 返回一个迭代器，用于按顺序遍历归档中的每个条目。
    pub fn entries(&self) -> InitramfsIter {
        InitramfsIter {
            current: self.base,
            end: unsafe { self.base.add(self.size) },
        }
    }

    pub fn base_ptr(&self) -> *const u8 {
        self.base
    }

    /// Find a specific file by path
    /// Find a specific file by path
    ///
    /// 在归档中查找指定路径的条目并返回其拷贝（InitramfsEntry）。此操作会遍历所有条目，
    /// 适合少量文件的 initramfs 场景。
    pub fn find(&self, path: &str) -> Option<InitramfsEntry> {
        crate::ktrace!("Initramfs::find searching for '{}'", path);

        // Normalize the search path by removing leading slash
        let search_path = path.strip_prefix('/').unwrap_or(path);

        for entry in self.entries() {
            crate::ktrace!("Checking entry: '{}'", entry.name);
            if entry.name == search_path {
                crate::ktrace!("Found matching entry: '{}'", entry.name);
                return Some(entry);
            }
        }
        crate::ktrace!("File '{}' not found in initramfs", path);
        None
    }
}

pub struct InitramfsIter {
    current: *const u8,
    end: *const u8,
}

#[inline(always)]
const fn align4(value: usize) -> usize {
    (value + 3) & !3
}

impl Iterator for InitramfsIter {
    type Item = InitramfsEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.end {
            return None;
        }

        unsafe {
            let header_size = core::mem::size_of::<CpioNewcHeader>();
            let base_addr = self.current as usize;
            let end_addr = self.end as usize;

            if base_addr + header_size > end_addr {
                return None;
            }

            // Read CPIO header
            let header = &*(self.current as *const CpioNewcHeader);

            if !header.is_valid() {
                return None;
            }

            let namesize = header.namesize();
            let filesize = header.filesize();

            let name_ptr = base_addr + header_size;
            let name_end = match name_ptr.checked_add(namesize) {
                Some(end) => end,
                None => return None,
            };

            if name_end > end_addr {
                return None;
            }

            // Read filename
            // 读取文件名（namesize 包含结尾 NUL，因此取 namesize - 1）
            let name_bytes =
                slice::from_raw_parts(name_ptr as *const u8, namesize.saturating_sub(1));
            let name = core::str::from_utf8(name_bytes).unwrap_or("");

            crate::kdebug!(
                "CPIO entry: name='{}', header={:#x}, name_ptr={:#x}, name_end={:#x}, filesize={:#x}",
                name,
                base_addr,
                name_ptr,
                name_end,
                filesize
            );

            // Check for trailer
            if name == CpioNewcHeader::TRAILER {
                self.current = self.end;
                return None;
            }

            let relative_name_end = header_size + namesize;
            let data_offset = base_addr + align4(relative_name_end);

            if data_offset > end_addr {
                return None;
            }

            let data_offset_rel = align4(relative_name_end);
            let data_end_rel = match data_offset_rel.checked_add(filesize) {
                Some(end) => end,
                None => return None,
            };

            let data_end = base_addr + data_end_rel;

            if data_end > end_addr {
                return None;
            }

            let next_offset_rel = align4(data_end_rel);
            let next_offset = base_addr + next_offset_rel;

            if next_offset > end_addr {
                return None;
            }

            // Read file data
            let data = slice::from_raw_parts(data_offset as *const u8, filesize);

            crate::kdebug!(
                "CPIO entry data: name='{}', data_offset={:#x}, first_bytes={:02x} {:02x} {:02x} {:02x}",
                name,
                data_offset,
                data.get(0).copied().unwrap_or(0),
                data.get(1).copied().unwrap_or(0),
                data.get(2).copied().unwrap_or(0),
                data.get(3).copied().unwrap_or(0)
            );

            self.current = next_offset as *const u8;

            Some(InitramfsEntry {
                // 返回的 name/data 指向原始内存或复制缓冲区，调用者不应修改它们
                name,
                data,
                mode: header.mode(),
            })
        }
    }
}

// Copy buffer for initramfs data
static mut INITRAMFS_COPY_BUF: core::mem::MaybeUninit<[u8; 64 * 1024]> =
    core::mem::MaybeUninit::uninit();
const INITRAMFS_COPY_BUF_SIZE: usize = 64 * 1024;

// Global initramfs state
#[link_section = ".initramfs_meta"]
static mut INITRAMFS_INSTANCE: Initramfs = Initramfs::empty();

#[link_section = ".initramfs_flag"]
static mut INITRAMFS_PRESENT: bool = false;

/// Get global initramfs instance
pub fn get() -> Option<&'static Initramfs> {
    unsafe {
        let present = INITRAMFS_PRESENT;
        let instance_ptr = core::ptr::addr_of!(INITRAMFS_INSTANCE);
        let storage_addr = instance_ptr as usize;
        crate::kdebug!(
            "initramfs::get state: present={} storage={:#x} base={:#x} size={:#x}",
            present,
            storage_addr,
            (*instance_ptr).base as usize,
            (*instance_ptr).size
        );
        if present {
            Some(&*instance_ptr)
        } else {
            None
        }
    }
}

/// Find a file in initramfs
pub fn find_file(path: &str) -> Option<&'static [u8]> {
    crate::kdebug!("Searching for file: '{}'", path);
    if let Some(ramfs) = get() {
        ramfs.find(path).map(|e| {
            crate::kdebug!("Found file '{}' with {} bytes", e.name, e.data.len());
            e.data
        })
    } else {
        None
    }
}

/// Iterate over all initramfs entries and call the provided callback for each one.
///
/// 这个函数不会进行堆分配，直接遍历归档并将每个 `InitramfsEntry`（按值）传给回调。
/// 回调签名例如 `|entry: InitramfsEntry| { ... }`。
pub fn for_each_entry<F>(mut cb: F)
where
    F: FnMut(InitramfsEntry),
{
    crate::kdebug!("initramfs::for_each_entry start");
    if let Some(ramfs) = get() {
        for entry in ramfs.entries() {
            cb(entry);
        }
    } else {
        crate::kwarn!("initramfs::for_each_entry: no initramfs instance available");
    }
    crate::kdebug!("initramfs::for_each_entry end");
}

/// Iterate over all filenames (paths) in the initramfs and call `cb` with each path.
///
/// 这是一个更轻量的便利函数，回调接收 `&str`，通常用于列举或构建外部索引。
pub fn for_each_path<F>(mut cb: F)
where
    F: FnMut(&'static str),
{
    for_each_entry(|entry| cb(entry.name));
}

/// Initialize initramfs from multiboot module
pub fn init(base: *const u8, size: usize) {
    // Assume GRUB has already mapped the initramfs region
    // 假设 GRUB 或引导程序已经将 initramfs 模块映射到内存中并传递了基地址/大小

    unsafe {
        if size == 0 {
            INITRAMFS_PRESENT = false;
            INITRAMFS_INSTANCE = Initramfs::empty();
            crate::kwarn!("Initramfs module reported size 0; skipping load");
            return;
        }

        // If the module fits into our kernel-owned buffer, copy it there
        if size <= INITRAMFS_COPY_BUF_SIZE {
            let dst = (&raw mut INITRAMFS_COPY_BUF as *mut core::mem::MaybeUninit<[u8; 64 * 1024]>)
                .cast::<u8>();
            core::ptr::copy_nonoverlapping(base, dst, size);
            INITRAMFS_INSTANCE = Initramfs::new(dst as *const u8, size);
            crate::kinfo!("Initramfs copied into kernel buffer ({} bytes)", size);
        } else {
            // Fallback: reference original module memory
            INITRAMFS_INSTANCE = Initramfs::new(base, size);
            crate::kwarn!(
                "Initramfs module too large to copy ({} bytes), using original pointer",
                size
            );
        }

        INITRAMFS_PRESENT = true;
    }

    crate::kinfo!(
        "Initramfs initialized at {:#x}, size {} bytes",
        base as usize,
        size
    );

    debug_dump_state("after-init");

    crate::kinfo!("INITRAMFS after init: {}", get().is_some());

    // List all files
    if let Some(ramfs) = get() {
        crate::kinfo!("Initramfs contents:");
        for entry in ramfs.entries() {
            crate::kinfo!(
                "  '{}' ({} bytes, mode {:#o})",
                entry.name,
                entry.data.len(),
                entry.mode
            );
        }
    }
}
