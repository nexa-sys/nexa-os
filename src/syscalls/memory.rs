//! Memory management syscalls
//!
//! Implements: mmap, munmap, mprotect, brk

use crate::posix::{self, errno};
use crate::{kdebug, kerror, kinfo, ktrace, kwarn};

/// MMAP protection flags (POSIX)
pub const PROT_NONE: u64 = 0x0;
pub const PROT_READ: u64 = 0x1;
pub const PROT_WRITE: u64 = 0x2;
pub const PROT_EXEC: u64 = 0x4;

/// MMAP flags (POSIX)
pub const MAP_SHARED: u64 = 0x01;
pub const MAP_PRIVATE: u64 = 0x02;
pub const MAP_FIXED: u64 = 0x10;
pub const MAP_ANONYMOUS: u64 = 0x20;
pub const MAP_ANON: u64 = MAP_ANONYMOUS;
pub const MAP_NORESERVE: u64 = 0x4000;
pub const MAP_POPULATE: u64 = 0x8000;

/// Special value indicating mmap failure
pub const MAP_FAILED: u64 = u64::MAX;

/// Page size constant
pub const PAGE_SIZE: u64 = 4096;

/// Simple memory region tracking for mmap
/// In a full implementation, this would be per-process
#[derive(Clone, Copy)]
struct MmapRegion {
    start: u64,
    size: u64,
    prot: u64,
    flags: u64,
    in_use: bool,
}

impl MmapRegion {
    const fn empty() -> Self {
        Self {
            start: 0,
            size: 0,
            prot: 0,
            flags: 0,
            in_use: false,
        }
    }
}

/// Maximum number of mmap regions per process
const MAX_MMAP_REGIONS: usize = 64;

/// Global mmap region table (simplified - should be per-process)
static mut MMAP_REGIONS: [MmapRegion; MAX_MMAP_REGIONS] = [MmapRegion::empty(); MAX_MMAP_REGIONS];

/// Next available mmap address (bump allocator for anonymous mappings)
/// Use a lazy initialization approach - start within userspace region
use core::sync::atomic::{AtomicU64, Ordering};
// NOTE: This value will be dynamically adjusted on first mmap call
// to fit within the current user address space bounds
static NEXT_MMAP_ADDR: AtomicU64 = AtomicU64::new(0);

/// SYS_MMAP - Memory map system call
///
/// # Arguments
/// * `addr` - Hint address (or required if MAP_FIXED)
/// * `length` - Length of mapping in bytes
/// * `prot` - Protection flags (PROT_READ, PROT_WRITE, PROT_EXEC)
/// * `flags` - Mapping flags (MAP_SHARED, MAP_PRIVATE, MAP_ANONYMOUS, etc.)
/// * `fd` - File descriptor (ignored for MAP_ANONYMOUS)
/// * `offset` - Offset in file (ignored for MAP_ANONYMOUS)
///
/// # Returns
/// * Starting address of mapped region on success
/// * MAP_FAILED (-1) on error with errno set
pub fn mmap(addr: u64, length: u64, prot: u64, flags: u64, fd: i64, offset: u64) -> u64 {
    ktrace!(
        "[mmap] addr={:#x}, length={:#x}, prot={:#x}, flags={:#x}, fd={}, offset={:#x}",
        addr,
        length,
        prot,
        flags,
        fd,
        offset
    );

    // Validate length
    if length == 0 {
        kerror!("[mmap] Invalid length: 0");
        posix::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }

    // Round up length to page size
    let aligned_length = (length + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    // Check for anonymous mapping
    let is_anonymous = (flags & MAP_ANONYMOUS) != 0;

    // Determine mapping address
    let map_addr = if (flags & MAP_FIXED) != 0 {
        // MAP_FIXED: Use the exact address specified
        if addr == 0 || (addr & (PAGE_SIZE - 1)) != 0 {
            kerror!("[mmap] MAP_FIXED with invalid address: {:#x}", addr);
            posix::set_errno(errno::EINVAL);
            return MAP_FAILED;
        }
        addr
    } else if addr != 0 {
        // Hint address provided, try to use it if aligned
        let aligned_hint = (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        // Check if hint is usable (simple check - in production would check existing mappings)
        if can_map_at(aligned_hint, aligned_length) {
            aligned_hint
        } else {
            // Fall back to allocating a new address
            allocate_mmap_address(aligned_length)
        }
    } else {
        // No address specified, allocate one
        allocate_mmap_address(aligned_length)
    };

    if map_addr == 0 || map_addr == MAP_FAILED {
        kerror!("[mmap] Failed to allocate mapping address");
        posix::set_errno(errno::ENOMEM);
        return MAP_FAILED;
    }

    // Record the mapping
    if !record_mmap_region(map_addr, aligned_length, prot, flags) {
        kerror!("[mmap] Failed to record mapping region");
        posix::set_errno(errno::ENOMEM);
        return MAP_FAILED;
    }

    // Handle the mapping based on whether it's anonymous or file-backed
    if is_anonymous {
        // Zero the memory for anonymous mappings
        unsafe {
            core::ptr::write_bytes(map_addr as *mut u8, 0, aligned_length as usize);
        }
    } else if fd >= 0 {
        // File-backed mapping: read file contents into the mapped region
        ktrace!(
            "[mmap] File-backed mapping: fd={}, offset={:#x}",
            fd,
            offset
        );

        // Read file contents into the mapped region
        if let Err(e) = read_file_into_mapping(fd as u64, offset, map_addr, aligned_length) {
            kerror!("[mmap] Failed to read file into mapping: {}", e);
            // Still return success - the mapping exists, just empty
            // In a real implementation, we'd handle page faults lazily
        }
    } else {
        // Anonymous mapping with invalid fd - just zero the memory
        unsafe {
            core::ptr::write_bytes(map_addr as *mut u8, 0, aligned_length as usize);
        }
    }

    kinfo!(
        "[mmap] Mapped {:#x} bytes at {:#x} (prot={:#x}, flags={:#x})",
        aligned_length,
        map_addr,
        prot,
        flags
    );

    posix::set_errno(0);
    map_addr
}

/// Read file contents into a mapped memory region
///
/// # Arguments
/// * `fd` - File descriptor
/// * `offset` - Offset in file to start reading
/// * `dest_addr` - Destination address in memory
/// * `length` - Number of bytes to read
fn read_file_into_mapping(
    fd: u64,
    offset: u64,
    dest_addr: u64,
    length: u64,
) -> Result<usize, &'static str> {
    use super::types::{get_file_handle, FileBacking, FD_BASE, MAX_OPEN_FILES};

    // Validate file descriptor
    if fd < FD_BASE {
        return Err("Invalid file descriptor");
    }

    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        return Err("Invalid file descriptor");
    }

    // Get file handle and read content
    unsafe {
        let handle = match get_file_handle(idx) {
            Some(h) => h,
            None => return Err("File not open"),
        };

        // Determine file size
        let file_size = handle.metadata.size as u64;

        // Calculate how much to read
        let read_start = offset.min(file_size);
        let available = file_size.saturating_sub(read_start);
        let to_read = length.min(available) as usize;

        if to_read == 0 {
            // Nothing to read, just zero the memory
            core::ptr::write_bytes(dest_addr as *mut u8, 0, length as usize);
            return Ok(0);
        }

        // Read file content based on backing type
        match &handle.backing {
            FileBacking::Inline(data) => {
                let start = read_start as usize;
                let end = (read_start as usize + to_read).min(data.len());
                if start < data.len() {
                    let copy_len = end - start;
                    core::ptr::copy_nonoverlapping(
                        data[start..].as_ptr(),
                        dest_addr as *mut u8,
                        copy_len,
                    );
                    // Zero remaining
                    if copy_len < length as usize {
                        core::ptr::write_bytes(
                            (dest_addr + copy_len as u64) as *mut u8,
                            0,
                            length as usize - copy_len,
                        );
                    }
                    return Ok(copy_len);
                }
            }
            FileBacking::Ext2(file_ref) => {
                // Read from ext2 file
                let dest_slice = core::slice::from_raw_parts_mut(dest_addr as *mut u8, to_read);
                let bytes_read = file_ref.read_at(read_start as usize, dest_slice);

                // Zero remaining
                if bytes_read < length as usize {
                    core::ptr::write_bytes(
                        (dest_addr + bytes_read as u64) as *mut u8,
                        0,
                        length as usize - bytes_read,
                    );
                }
                return Ok(bytes_read);
            }
            _ => {
                // Other backing types (sockets, etc.) - just zero the memory
                core::ptr::write_bytes(dest_addr as *mut u8, 0, length as usize);
                return Err("Unsupported file backing type for mmap");
            }
        }
    }

    // Zero the memory if we couldn't read
    unsafe {
        core::ptr::write_bytes(dest_addr as *mut u8, 0, length as usize);
    }
    Ok(0)
}

/// Allocate a new mmap address using bump allocator
fn allocate_mmap_address(size: u64) -> u64 {
    use crate::process::{INTERP_BASE, USER_REGION_SIZE, USER_VIRT_BASE};

    let user_end = USER_VIRT_BASE + USER_REGION_SIZE;

    // Lazy initialize: start after interpreter base + some offset for ld itself
    // The dynamic linker is loaded at INTERP_BASE, so start mmap allocations
    // after it (at INTERP_BASE + 0x100000 to leave 1MB for ld)
    let mmap_start = INTERP_BASE + 0x100000; // 0xB00000

    let current = NEXT_MMAP_ADDR.load(Ordering::SeqCst);
    if current == 0 || current < mmap_start {
        // First allocation - initialize to start of mmap region
        let _ = NEXT_MMAP_ADDR.compare_exchange(
            current,
            mmap_start,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    // Try to allocate from the bump allocator
    let addr = NEXT_MMAP_ADDR.fetch_add(size, Ordering::SeqCst);

    // Check bounds
    if addr + size > user_end {
        // Out of virtual address space
        kerror!(
            "[mmap] Out of address space: addr={:#x} size={:#x} user_end={:#x}",
            addr,
            size,
            user_end
        );
        return MAP_FAILED;
    }

    addr
}

/// Check if we can map at a given address (simple check)
fn can_map_at(addr: u64, _size: u64) -> bool {
    use crate::process::{USER_REGION_SIZE, USER_VIRT_BASE};

    // Basic bounds check
    let user_end = USER_VIRT_BASE + USER_REGION_SIZE;
    addr >= USER_VIRT_BASE && addr < user_end
}

/// Record an mmap region in the tracking table
fn record_mmap_region(start: u64, size: u64, prot: u64, flags: u64) -> bool {
    unsafe {
        for region in MMAP_REGIONS.iter_mut() {
            if !region.in_use {
                *region = MmapRegion {
                    start,
                    size,
                    prot,
                    flags,
                    in_use: true,
                };
                return true;
            }
        }
    }
    false
}

/// SYS_MUNMAP - Unmap memory region
///
/// # Arguments
/// * `addr` - Start address of region to unmap
/// * `length` - Length of region to unmap
///
/// # Returns
/// * 0 on success
/// * -1 on error with errno set
pub fn munmap(addr: u64, length: u64) -> u64 {
    ktrace!("[munmap] addr={:#x}, length={:#x}", addr, length);

    // Validate address alignment
    if (addr & (PAGE_SIZE - 1)) != 0 {
        kerror!("[munmap] Address not page-aligned: {:#x}", addr);
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    // Validate length
    if length == 0 {
        kerror!("[munmap] Invalid length: 0");
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    let aligned_length = (length + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    // Find and remove the mapping
    unsafe {
        for region in MMAP_REGIONS.iter_mut() {
            if region.in_use && region.start == addr {
                // Found the region
                region.in_use = false;
                kdebug!("[munmap] Unmapped region at {:#x}", addr);
                posix::set_errno(0);
                return 0;
            }
        }
    }

    // Region not found - this is not necessarily an error in POSIX
    // munmap on non-mapped memory is allowed
    kdebug!(
        "[munmap] No mapping found at {:#x}, returning success",
        addr
    );
    posix::set_errno(0);
    0
}

/// SYS_MPROTECT - Change memory protection
///
/// # Arguments
/// * `addr` - Start address of region
/// * `length` - Length of region
/// * `prot` - New protection flags
///
/// # Returns
/// * 0 on success
/// * -1 on error with errno set
pub fn mprotect(addr: u64, length: u64, prot: u64) -> u64 {
    ktrace!(
        "[mprotect] addr={:#x}, length={:#x}, prot={:#x}",
        addr,
        length,
        prot
    );

    // Validate address alignment
    if (addr & (PAGE_SIZE - 1)) != 0 {
        kerror!("[mprotect] Address not page-aligned: {:#x}", addr);
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    // Validate length
    if length == 0 {
        // Zero length is OK, just return success
        posix::set_errno(0);
        return 0;
    }

    // In a full implementation, we would:
    // 1. Find the affected memory regions
    // 2. Split regions if necessary
    // 3. Update page table entries with new permissions

    // For now, just update our tracking and return success
    unsafe {
        for region in MMAP_REGIONS.iter_mut() {
            if region.in_use && region.start <= addr && addr < region.start + region.size {
                region.prot = prot;
                kdebug!(
                    "[mprotect] Updated protection for region at {:#x}",
                    region.start
                );
            }
        }
    }

    posix::set_errno(0);
    0
}

/// SYS_BRK - Change data segment size (heap management)
///
/// # Arguments
/// * `addr` - New end of data segment (0 to query current)
///
/// # Returns
/// * Current or new end of data segment
/// * 0 on error (historically, brk returns 0 on failure)
pub fn brk(addr: u64) -> u64 {
    use crate::process::{HEAP_BASE, HEAP_SIZE};

    // Get current process heap bounds
    let heap_start = HEAP_BASE;
    let heap_max = HEAP_BASE + HEAP_SIZE;

    // Track current break per process (simplified - using static for now)
    static CURRENT_BRK: AtomicU64 = AtomicU64::new(0);

    // Initialize break to heap start if not set
    let _ = CURRENT_BRK.compare_exchange(0, heap_start, Ordering::SeqCst, Ordering::Relaxed);

    if addr == 0 {
        // Query current break
        let current = CURRENT_BRK.load(Ordering::SeqCst);
        ktrace!("[brk] Query: current={:#x}", current);
        return current;
    }

    // Validate new break address
    if addr < heap_start || addr > heap_max {
        kerror!(
            "[brk] Address {:#x} out of heap bounds [{:#x}, {:#x}]",
            addr,
            heap_start,
            heap_max
        );
        posix::set_errno(errno::ENOMEM);
        return CURRENT_BRK.load(Ordering::SeqCst);
    }

    // Update break
    let old_brk = CURRENT_BRK.swap(addr, Ordering::SeqCst);

    // Zero new memory if expanding
    if addr > old_brk {
        unsafe {
            core::ptr::write_bytes(old_brk as *mut u8, 0, (addr - old_brk) as usize);
        }
    }

    ktrace!("[brk] Set: old={:#x}, new={:#x}", old_brk, addr);
    posix::set_errno(0);
    addr
}
