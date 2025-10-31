/// Initial RAM Filesystem support
/// Loads files from a CPIO archive embedded in the kernel
///
/// 初始 RAM 文件系统支持（initramfs）
/// 从内核嵌入的 CPIO 归档加载文件，用于启动阶段提供最小用户态程序和资源。
use core::slice;
/// CPIO newc format header (110 bytes ASCII)
///
/// CPIO newc 格式的头部（110 字节 ASCII），字段都是十六进制 ASCII 文本表示。
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct CpioNewcHeader {
    pub magic: [u8; 6],     // "070701" or "070702"
    // magic 字段：应为 ASCII "070701"（newc）或 "070702"
    pub ino: [u8; 8],       // Inode number
    // inode 编号（ASCII hex）
    pub mode: [u8; 8],      // File mode
    // 文件模式（权限和类型，以 ASCII hex 表示）
    pub uid: [u8; 8],       // User ID
    // 所属用户 ID（ASCII hex）
    pub gid: [u8; 8],       // Group ID
    // 所属组 ID（ASCII hex）
    pub nlink: [u8; 8],     // Number of links
    // 硬链接数量（ASCII hex）
    pub mtime: [u8; 8],     // Modification time
    // 修改时间（ASCII hex，UNIX 时间戳）
    pub filesize: [u8; 8],  // File size
    // 文件大小（字节，ASCII hex）
    pub devmajor: [u8; 8],  // Device major
    // 设备主号（ASCII hex，通常为 0）
    pub devminor: [u8; 8],  // Device minor
    // 设备次号（ASCII hex，通常为 0）
    pub rdevmajor: [u8; 8], // Real device major
    // 特殊设备（rdev）主号（ASCII hex）
    pub rdevminor: [u8; 8], // Real device minor
    // 特殊设备（rdev）次号（ASCII hex）
    pub namesize: [u8; 8],  // Filename length
    // 文件名长度（包括末尾的 NUL 字节，ASCII hex）
    pub check: [u8; 8],     // Checksum
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

pub struct Initramfs {
    base: *const u8,
    size: usize,
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

    /// Find a specific file by path
    /// Find a specific file by path
    ///
    /// 在归档中查找指定路径的条目并返回其拷贝（InitramfsEntry）。此操作会遍历所有条目，
    /// 适合少量文件的 initramfs 场景。
    pub fn find(&self, path: &str) -> Option<InitramfsEntry> {
        crate::ktrace!("Initramfs::find searching for '{}'", path);
        for entry in self.entries() {
            crate::ktrace!("Checking entry: '{}'", entry.name);
            if entry.name == path {
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

impl Iterator for InitramfsIter {
    type Item = InitramfsEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.end {
            return None;
        }

        unsafe {
            // Ensure we have enough space for header
            if self.current.add(core::mem::size_of::<CpioNewcHeader>()) > self.end {
                return None;
            }

            // Read CPIO header
            let header = &*(self.current as *const CpioNewcHeader);

            if !header.is_valid() {
                return None;
            }

            let namesize = header.namesize();
            let filesize = header.filesize();

            // Move past header
            let mut ptr = self.current.add(core::mem::size_of::<CpioNewcHeader>());

            // Check bounds for name
            if ptr.add(namesize) > self.end {
                return None;
            }

            // Read filename
            // 读取文件名（namesize 包含结尾 NUL，因此取 namesize - 1）
            let name_bytes = slice::from_raw_parts(ptr, namesize.saturating_sub(1)); // -1 for null terminator
            let name = core::str::from_utf8(name_bytes).unwrap_or("");

            // Check for trailer
            if name == CpioNewcHeader::TRAILER {
                return None;
            }

            // Align to 4 bytes after name
            ptr = ptr.add(namesize);
            let align = (4 - (ptr as usize % 4)) % 4;
            ptr = ptr.add(align);

            // Check bounds for data
            if ptr.add(filesize) > self.end {
                return None;
            }

            // Read file data
            let data = slice::from_raw_parts(ptr, filesize);

            // Align to 4 bytes after data
            ptr = ptr.add(filesize);
            let align = (4 - (ptr as usize % 4)) % 4;
            ptr = ptr.add(align);

            self.current = ptr;

            Some(InitramfsEntry {
                // 返回的 name/data 指向原始内存或复制缓冲区，调用者不应修改它们
                name: core::str::from_utf8(name_bytes).unwrap_or(""),
                data,
                mode: header.mode(),
            })
        }
    }
}

// Global initramfs instance
static mut INITRAMFS: Option<Initramfs> = None;

// Backup buffer for initramfs data so we keep a kernel-owned copy
// in case page tables change the accessibility of the original module
// address provided by the bootloader. 64 KiB should be plenty for our
// small user-space programs used in tests.
static mut INITRAMFS_COPY_BUF: [u8; 64 * 1024] = [0; 64 * 1024];
const INITRAMFS_COPY_BUF_SIZE: usize = 64 * 1024;

// 全局 initramfs 实例的备忘：INITRAMFS 保存了 Initramfs 对象的可选值。
// INITRAMFS_COPY_BUF 是内核拥有的备份缓冲区，用于在需要时复制模块数据以确保可访问性。

/// Get global initramfs instance
pub fn get() -> Option<&'static Initramfs> {
    unsafe {
        let p: *const Option<Initramfs> = &raw const INITRAMFS;
        (*p).as_ref()
    }
}

/// Find a file in initramfs
pub fn find_file(path: &str) -> Option<&'static [u8]> {
    crate::kdebug!("Searching for file: '{}'", path);
    get()?.find(path).map(|e| {
        crate::kdebug!("Found file '{}' with {} bytes", e.name, e.data.len());
        e.data
    })
}

/// Iterate over all initramfs entries and call the provided callback for each one.
///
/// 这个函数不会进行堆分配，直接遍历归档并将每个 `InitramfsEntry`（按值）传给回调。
/// 回调签名例如 `|entry: InitramfsEntry| { ... }`。
pub fn for_each_entry<F>(mut cb: F)
where
    F: FnMut(InitramfsEntry),
{
    if let Some(ramfs) = get() {
        for entry in ramfs.entries() {
            cb(entry);
        }
    }
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
        // If the module fits into our kernel-owned buffer, copy it there
        if size <= INITRAMFS_COPY_BUF_SIZE {
            let dst: *mut u8 = &raw mut INITRAMFS_COPY_BUF as *mut _ as *mut u8;
            core::ptr::copy_nonoverlapping(base, dst, size);
            INITRAMFS = Some(Initramfs::new(dst as *const u8, size));
            crate::kinfo!("Initramfs copied into kernel buffer ({} bytes)", size);
        } else {
            // Fallback: reference original module memory
            INITRAMFS = Some(Initramfs::new(base, size));
            crate::kwarn!(
                "Initramfs module too large to copy ({} bytes), using original pointer",
                size
            );
        }
    }

    crate::kinfo!(
        "Initramfs initialized at {:#x}, size {} bytes",
        base as usize,
        size
    );

    // List all files
    // Safely iterate over entries using a raw pointer to avoid creating
    // shared references to mutable statics.
    unsafe {
        let p: *const Option<Initramfs> = &raw const INITRAMFS;
        if let Some(ref ramfs) = (*p).as_ref() {
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
}
