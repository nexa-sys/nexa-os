/// Simple in-memory filesystem
/// 简单的内存文件系统（in-memory filesystem）
///
/// This module provides a very small, fixed-size in-memory file table used by the
/// kernel during early boot to offer basic file read and listing capabilities.
/// Files are stored as static string references and are suitable for initramfs
/// or built-in read-only kernel content.
use spin::Mutex;

#[derive(Clone, Copy)]
pub struct File {
    // File name (static string slice)
    // 文件名（静态字符串切片）
    pub name: &'static str,
    // File content as a byte slice (supports both text and binary)
    // 文件内容（静态字节切片），既可表示文本也可表示二进制
    pub content: &'static [u8],
    // Whether this entry is a directory (used to distinguish files/dirs)
    // 是否为目录（用于区分文件和目录条目）
    pub is_dir: bool,
}

// Maximum number of file entries: avoid dynamic allocation by using a fixed-size array
// 文件表最大容量：避免动态分配，使用固定大小数组
const MAX_FILES: usize = 64;

// Protect global static data with spin::Mutex (suitable for no_std environments
// and very short critical sections)
// 使用 spin::Mutex 保护全局静态数据（适合 no_std 环境与非常短的临界区）
static FILES: Mutex<[Option<File>; MAX_FILES]> = Mutex::new([None; MAX_FILES]);
// Current number of added files
// 当前已添加的文件数
static FILE_COUNT: Mutex<usize> = Mutex::new(0);

/// Initialize the filesystem
/// 初始化文件系统
pub fn init() {
    crate::kinfo!("Initializing filesystem...");
    // Register all initramfs entries (register raw bytes so executables like ELF are included)
    crate::initramfs::for_each_entry(|entry| {
        let name = entry.name.strip_prefix('/').unwrap_or(entry.name);
        crate::fs::add_file_bytes(name, entry.data, false);
    });
    crate::kinfo!("Filesystem initialized with {} files", *FILE_COUNT.lock());
}

/// Add a file or directory entry to the in-memory file table
/// 向内存文件表添加一个文件或目录条目
///
/// - name: file name (static lifetime)
/// - name: 文件名（静态生命周期）
/// - content: file content (static lifetime), pass empty string for directories
/// - content: 文件内容（静态生命周期），目录可传空字符串
/// - is_dir: whether this entry is a directory
/// - is_dir: 是否为目录
///
/// Note: For simplicity there is no deduplication and no resizing. When MAX_FILES
/// is reached, further adds are silently ignored.
/// 注意：为了简单起见没有去重检查，也不会扩容；当达到 MAX_FILES 时后续添加会被静默忽略。
pub fn add_file(name: &'static str, content: &'static str, is_dir: bool) {
    // Wrapper: register text file by converting to bytes
    add_file_bytes(name, content.as_bytes(), is_dir);
}

/// Register raw bytes as a file (supports both text and binary)
pub fn add_file_bytes(name: &'static str, content: &'static [u8], is_dir: bool) {
    let mut files = FILES.lock();
    let mut count = FILE_COUNT.lock();

    if *count < MAX_FILES {
        files[*count] = Some(File { name, content, is_dir });
        *count += 1;
    }
}

/// List all entries in the root directory (returns a fixed-size array view)
/// 列举根目录下的所有条目（返回整个固定长度的数组视图）
///
/// The return type is a fixed-size slice of Option<File> that contains both used
/// and unused slots. We use unsafe to create a static slice from the locked array
/// pointer; callers should avoid mutating the returned slice.
/// 返回类型是固定长度的 Option<File> 切片视图，包含已占用与未占用的槽位。
/// 这里使用 unsafe 从锁得到的数组指针构造静态切片视图，调用者应当避免修改内容。
pub fn list_files() -> &'static [Option<File>] {
    let files = FILES.lock();
    // SAFETY: FILES is a global static array; we convert its pointer into a fixed-size
    // slice for read-only inspection. The returned slice's lifetime is extended to
    // 'static which is a common kernel simplification—do not write through this slice.
    // SAFETY: FILES 是一个全局静态数组，我们将其指针转换为固定大小的切片用于只读查看。
    // 返回的切片生命周期被扩展为 'static，这是常见的内核简化手段，但请注意不要在切片上做可变操作。
    unsafe { core::slice::from_raw_parts(files.as_ptr(), MAX_FILES) }
}

/// Read file content by name (returns regular file content only; directories return None)
/// 根据名字读取文件内容（只返回常规文件的内容，目录不会返回）
///
/// Returns Option<&'static str>, or None if the file does not exist or is a directory.
/// 返回 Option<&'static str>，当文件不存在或是目录时返回 None。
pub fn read_file_bytes(name: &str) -> Option<&'static [u8]> {
    let files = FILES.lock();
    let count = *FILE_COUNT.lock();

    for i in 0..count {
        if let Some(file) = files[i] {
            if file.name == name && !file.is_dir {
                return Some(file.content);
            }
        }
    }
    None
}

/// Try to read a file as UTF-8 text; returns None if not found or not valid UTF-8
pub fn read_file(name: &str) -> Option<&'static str> {
    read_file_bytes(name).and_then(|b| core::str::from_utf8(b).ok())
}

/// Check whether an entry with the given name exists (includes files and directories)
/// 检查指定名称的条目是否存在（包括文件和目录）
pub fn file_exists(name: &str) -> bool {
    let files = FILES.lock();
    let count = *FILE_COUNT.lock();

    for i in 0..count {
        if let Some(file) = files[i] {
            if file.name == name {
                return true;
            }
        }
    }
    false
}
