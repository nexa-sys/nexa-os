//! ELF Loader - Loads shared objects from filesystem
//!
//! This module provides functionality to load ELF shared objects
//! into memory using mmap and prepare them for execution.

use crate::{c_int, c_void, size_t};
use core::ptr;

use super::elf::*;
use super::rtld::*;

// ============================================================================
// System Call Interface
// ============================================================================

const SYS_READ: u64 = 0;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_FSTAT: u64 = 5;
const SYS_LSEEK: u64 = 8;
const SYS_MMAP: u64 = 9;
const SYS_MUNMAP: u64 = 11;

// Syscall wrappers
#[inline]
unsafe fn syscall1(nr: u64, a1: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        in("rax") nr,
        in("rdi") a1,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline]
unsafe fn syscall2(nr: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        in("rax") nr,
        in("rdi") a1,
        in("rsi") a2,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        in("rax") nr,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline]
unsafe fn syscall6(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        in("rax") nr,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        in("r8") a5,
        in("r9") a6,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

// ============================================================================
// File Operations
// ============================================================================

/// Open a file and return file descriptor
unsafe fn open_file(path: &[u8]) -> Result<i32, LoadError> {
    let fd = syscall3(SYS_OPEN, path.as_ptr() as u64, path.len() as u64, 0) as i64;
    if fd < 0 {
        Err(LoadError::FileNotFound)
    } else {
        Ok(fd as i32)
    }
}

/// Close a file descriptor
unsafe fn close_file(fd: i32) {
    syscall1(SYS_CLOSE, fd as u64);
}

/// Read from file at current position
unsafe fn read_file(fd: i32, buf: &mut [u8]) -> Result<usize, LoadError> {
    let ret = syscall3(SYS_READ, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as i64;
    if ret < 0 {
        Err(LoadError::ReadError)
    } else {
        Ok(ret as usize)
    }
}

/// Seek constants
const SEEK_SET: u64 = 0;
const SEEK_CUR: u64 = 1;
const SEEK_END: u64 = 2;

/// Seek to a position in file
unsafe fn lseek(fd: i32, offset: i64, whence: u64) -> Result<u64, LoadError> {
    let ret = syscall3(SYS_LSEEK, fd as u64, offset as u64, whence) as i64;
    if ret < 0 {
        Err(LoadError::ReadError)
    } else {
        Ok(ret as u64)
    }
}

/// Read from file at a specific offset without changing file position
unsafe fn read_at(fd: i32, offset: u64, buf: &mut [u8]) -> Result<usize, LoadError> {
    // Save current position
    let saved_pos = lseek(fd, 0, SEEK_CUR)?;
    
    // Seek to desired offset
    lseek(fd, offset as i64, SEEK_SET)?;
    
    // Read data
    let result = read_file(fd, buf);
    
    // Restore position
    let _ = lseek(fd, saved_pos as i64, SEEK_SET);
    
    result
}

/// Stat structure (simplified)
#[repr(C)]
struct Stat {
    st_dev: u64,
    st_ino: u64,
    st_mode: u32,
    st_nlink: u32,
    st_uid: u32,
    st_gid: u32,
    st_rdev: u64,
    st_size: i64,
    st_blksize: i64,
    st_blocks: i64,
    st_atime: i64,
    st_atime_nsec: i64,
    st_mtime: i64,
    st_mtime_nsec: i64,
    st_ctime: i64,
    st_ctime_nsec: i64,
    _unused: [i64; 3],
}

/// Get file size
unsafe fn get_file_size(fd: i32) -> Result<u64, LoadError> {
    let mut stat = core::mem::zeroed::<Stat>();
    let ret = syscall3(SYS_FSTAT, fd as u64, &mut stat as *mut _ as u64, 0) as i64;
    if ret < 0 {
        Err(LoadError::ReadError)
    } else {
        Ok(stat.st_size as u64)
    }
}

// ============================================================================
// Memory Mapping
// ============================================================================

const PROT_NONE: u64 = 0x0;
const PROT_READ: u64 = 0x1;
const PROT_WRITE: u64 = 0x2;
const PROT_EXEC: u64 = 0x4;

const MAP_PRIVATE: u64 = 0x02;
const MAP_FIXED: u64 = 0x10;
const MAP_ANONYMOUS: u64 = 0x20;

const MAP_FAILED: u64 = u64::MAX;

/// Map memory region
unsafe fn mmap(
    addr: u64,
    length: u64,
    prot: u64,
    flags: u64,
    fd: i64,
    offset: u64,
) -> Result<u64, LoadError> {
    let ret = syscall6(SYS_MMAP, addr, length, prot, flags, fd as u64, offset);
    if ret == MAP_FAILED {
        Err(LoadError::MmapFailed)
    } else {
        Ok(ret)
    }
}

/// Unmap memory region
unsafe fn munmap(addr: u64, length: u64) -> Result<(), LoadError> {
    let ret = syscall2(SYS_MUNMAP, addr, length) as i64;
    if ret < 0 {
        Err(LoadError::MmapFailed)
    } else {
        Ok(())
    }
}

// ============================================================================
// Load Errors
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub enum LoadError {
    FileNotFound,
    ReadError,
    InvalidElf,
    UnsupportedArch,
    MmapFailed,
    TooLarge,
    NoLoadableSegments,
    OverlappingSegments,
}

impl LoadError {
    pub fn as_bytes(&self) -> &'static [u8] {
        match self {
            LoadError::FileNotFound => b"file not found",
            LoadError::ReadError => b"read error",
            LoadError::InvalidElf => b"invalid ELF format",
            LoadError::UnsupportedArch => b"unsupported architecture",
            LoadError::MmapFailed => b"mmap failed",
            LoadError::TooLarge => b"file too large",
            LoadError::NoLoadableSegments => b"no loadable segments",
            LoadError::OverlappingSegments => b"overlapping segments",
        }
    }
}

// ============================================================================
// ELF Loading
// ============================================================================

/// Maximum ELF file size we'll handle (64 MB)
const MAX_ELF_SIZE: u64 = 64 * 1024 * 1024;

/// Page size
const PAGE_SIZE: u64 = 4096;

/// Align down to page boundary
fn page_align_down(addr: u64) -> u64 {
    addr & !(PAGE_SIZE - 1)
}

/// Align up to page boundary
fn page_align_up(addr: u64) -> u64 {
    (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

/// Convert ELF protection flags to mmap protection
fn elf_to_mmap_prot(p_flags: u32) -> u64 {
    let mut prot = PROT_NONE;
    if (p_flags & PF_R) != 0 {
        prot |= PROT_READ;
    }
    if (p_flags & PF_W) != 0 {
        prot |= PROT_WRITE;
    }
    if (p_flags & PF_X) != 0 {
        prot |= PROT_EXEC;
    }
    prot
}

/// Result of loading an ELF file
pub struct ElfLoadResult {
    /// Base address where the library was loaded
    pub base_addr: u64,
    /// Total size of mapped region
    pub total_size: u64,
    /// Load bias (runtime base - link-time base)
    pub load_bias: i64,
    /// Entry point address (adjusted for load bias)
    pub entry: u64,
    /// Address of program headers in memory
    pub phdr_addr: u64,
    /// Number of program headers
    pub phnum: u16,
    /// Address of dynamic section
    pub dynamic_addr: u64,
    /// Number of dynamic entries
    pub dynamic_count: usize,
}

/// Load an ELF shared object from a file
///
/// # Safety
/// This function performs memory mapping and must be called with valid paths.
pub unsafe fn load_elf_file(path: &[u8]) -> Result<ElfLoadResult, LoadError> {
    // Open the file
    let fd = open_file(path)?;

    // Get file size
    let file_size = get_file_size(fd)?;
    if file_size > MAX_ELF_SIZE {
        close_file(fd);
        return Err(LoadError::TooLarge);
    }

    // Read ELF header
    let mut ehdr_buf = [0u8; 64]; // sizeof(Elf64Ehdr)
    let bytes_read = read_file(fd, &mut ehdr_buf)?;
    if bytes_read < 64 {
        close_file(fd);
        return Err(LoadError::InvalidElf);
    }

    let ehdr = &*(ehdr_buf.as_ptr() as *const Elf64Ehdr);

    // Validate ELF header
    if !ehdr.is_valid() {
        close_file(fd);
        return Err(LoadError::InvalidElf);
    }

    // Must be a shared object or PIE
    if ehdr.e_type != ET_DYN && ehdr.e_type != ET_EXEC {
        close_file(fd);
        return Err(LoadError::InvalidElf);
    }

    // Read program headers using lseek
    let phdr_size = (ehdr.e_phentsize as usize) * (ehdr.e_phnum as usize);
    if phdr_size > 4096 {
        close_file(fd);
        return Err(LoadError::TooLarge);
    }

    // Seek to program headers offset
    if lseek(fd, ehdr.e_phoff as i64, SEEK_SET).is_err() {
        close_file(fd);
        return Err(LoadError::ReadError);
    }

    // Read program headers
    let mut phdr_buf = [0u8; 4096];
    let mut total_read = 0;
    while total_read < phdr_size {
        let to_read = core::cmp::min(phdr_size - total_read, 4096);
        let read = read_file(fd, &mut phdr_buf[total_read..total_read + to_read])?;
        if read == 0 {
            break;
        }
        total_read += read;
    }

    if total_read < phdr_size {
        close_file(fd);
        return Err(LoadError::ReadError);
    }

    // Parse program headers
    let phdrs = core::slice::from_raw_parts(
        phdr_buf.as_ptr() as *const Elf64Phdr,
        ehdr.e_phnum as usize,
    );

    // Find the extent of loadable segments
    let mut load_addr_min: u64 = u64::MAX;
    let mut load_addr_max: u64 = 0;
    let mut has_load = false;

    for phdr in phdrs {
        if phdr.p_type == PT_LOAD {
            has_load = true;
            let seg_start = page_align_down(phdr.p_vaddr);
            let seg_end = page_align_up(phdr.p_vaddr + phdr.p_memsz);

            if seg_start < load_addr_min {
                load_addr_min = seg_start;
            }
            if seg_end > load_addr_max {
                load_addr_max = seg_end;
            }
        }
    }

    if !has_load {
        close_file(fd);
        return Err(LoadError::NoLoadableSegments);
    }

    // Calculate total mapping size
    let total_size = load_addr_max - load_addr_min;

    // For PIE/shared objects, allocate new address space
    // For executables with fixed addresses, use those
    let base_addr = if ehdr.e_type == ET_DYN || load_addr_min == 0 {
        // Allocate anonymous region for the whole library
        let addr = mmap(
            0,
            total_size,
            PROT_NONE,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        )?;
        addr
    } else {
        // Fixed address executable
        load_addr_min
    };

    // Calculate load bias
    let load_bias = base_addr as i64 - load_addr_min as i64;

    // Now map each loadable segment
    for phdr in phdrs {
        if phdr.p_type != PT_LOAD {
            continue;
        }

        let seg_start = page_align_down(phdr.p_vaddr);
        let seg_end = page_align_up(phdr.p_vaddr + phdr.p_memsz);
        let seg_size = seg_end - seg_start;

        let map_addr = (seg_start as i64 + load_bias) as u64;
        let prot = elf_to_mmap_prot(phdr.p_flags);

        // Map with write permission first (we need to copy data)
        let map_result = mmap(
            map_addr,
            seg_size,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED,
            -1,
            0,
        );

        if map_result.is_err() {
            // Clean up already mapped regions
            munmap(base_addr, total_size).ok();
            close_file(fd);
            return Err(LoadError::MmapFailed);
        }

        // Read file data into the segment using lseek
        if phdr.p_filesz > 0 {
            // Seek to segment offset
            if lseek(fd, phdr.p_offset as i64, SEEK_SET).is_err() {
                munmap(base_addr, total_size).ok();
                close_file(fd);
                return Err(LoadError::ReadError);
            }

            // Read segment data directly into mapped memory
            let data_addr = ((phdr.p_vaddr as i64) + load_bias) as *mut u8;
            let mut read_total = 0u64;

            while read_total < phdr.p_filesz {
                let to_read = core::cmp::min(phdr.p_filesz - read_total, 4096) as usize;
                let dest = core::slice::from_raw_parts_mut(
                    data_addr.add(read_total as usize),
                    to_read,
                );
                let read = read_file(fd, dest)?;
                if read == 0 {
                    break;
                }
                read_total += read as u64;
            }
        }

        // Zero BSS region (memsz > filesz)
        if phdr.p_memsz > phdr.p_filesz {
            let bss_start = ((phdr.p_vaddr + phdr.p_filesz) as i64 + load_bias) as *mut u8;
            let bss_size = (phdr.p_memsz - phdr.p_filesz) as usize;
            ptr::write_bytes(bss_start, 0, bss_size);
        }

        // TODO: Set correct protection after loading
        // For now, keep read/write/exec
    }

    // Find dynamic section
    let mut dynamic_addr = 0u64;
    let mut dynamic_count = 0usize;
    let mut phdr_addr = 0u64;

    for phdr in phdrs {
        match phdr.p_type {
            PT_DYNAMIC => {
                dynamic_addr = (phdr.p_vaddr as i64 + load_bias) as u64;
                dynamic_count = (phdr.p_memsz / core::mem::size_of::<Elf64Dyn>() as u64) as usize;
            }
            PT_PHDR => {
                phdr_addr = (phdr.p_vaddr as i64 + load_bias) as u64;
            }
            _ => {}
        }
    }

    // If no PT_PHDR, calculate from ehdr
    if phdr_addr == 0 {
        phdr_addr = base_addr + ehdr.e_phoff;
    }

    // Calculate entry point
    let entry = (ehdr.e_entry as i64 + load_bias) as u64;

    // Close file descriptor
    close_file(fd);

    Ok(ElfLoadResult {
        base_addr,
        total_size,
        load_bias,
        entry,
        phdr_addr,
        phnum: ehdr.e_phnum,
        dynamic_addr,
        dynamic_count,
    })
}

/// Search for a library in standard paths
pub fn search_library(name: &[u8]) -> Option<[u8; MAX_LIB_PATH]> {
    let mut path_buf = [0u8; MAX_LIB_PATH];

    // If absolute path, use directly
    if !name.is_empty() && name[0] == b'/' {
        let len = core::cmp::min(name.len(), MAX_LIB_PATH - 1);
        path_buf[..len].copy_from_slice(&name[..len]);
        return Some(path_buf);
    }

    // Search in standard paths
    for search_path in DEFAULT_LIB_PATHS {
        let search_bytes = search_path.as_bytes();
        let total_len = search_bytes.len() + 1 + name.len();

        if total_len >= MAX_LIB_PATH {
            continue;
        }

        // Build path: search_path + "/" + name
        let mut pos = 0;
        path_buf[pos..pos + search_bytes.len()].copy_from_slice(search_bytes);
        pos += search_bytes.len();
        path_buf[pos] = b'/';
        pos += 1;
        path_buf[pos..pos + name.len()].copy_from_slice(name);
        pos += name.len();
        path_buf[pos] = 0;

        // Try to open to check if it exists
        unsafe {
            if let Ok(fd) = open_file(&path_buf[..pos]) {
                close_file(fd);
                return Some(path_buf);
            }
        }
    }

    None
}
