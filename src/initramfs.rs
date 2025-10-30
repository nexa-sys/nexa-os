/// Initial RAM Filesystem support
/// Loads files from a CPIO archive embedded in the kernel
use core::slice;

/// CPIO newc format header (110 bytes ASCII)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct CpioNewcHeader {
    pub magic: [u8; 6],     // "070701" or "070702"
    pub ino: [u8; 8],       // Inode number
    pub mode: [u8; 8],      // File mode
    pub uid: [u8; 8],       // User ID
    pub gid: [u8; 8],       // Group ID
    pub nlink: [u8; 8],     // Number of links
    pub mtime: [u8; 8],     // Modification time
    pub filesize: [u8; 8],  // File size
    pub devmajor: [u8; 8],  // Device major
    pub devminor: [u8; 8],  // Device minor
    pub rdevmajor: [u8; 8], // Real device major
    pub rdevminor: [u8; 8], // Real device minor
    pub namesize: [u8; 8],  // Filename length
    pub check: [u8; 8],     // Checksum
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
    pub unsafe fn new(base: *const u8, size: usize) -> Self {
        Self { base, size }
    }

    /// Parse CPIO archive and return all entries
    pub fn entries(&self) -> InitramfsIter {
        InitramfsIter {
            current: self.base,
            end: unsafe { self.base.add(self.size) },
        }
    }

    /// Find a specific file by path
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

/// Initialize initramfs from multiboot module
pub fn init(base: *const u8, size: usize) {
    // Assume GRUB has already mapped the initramfs region

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
