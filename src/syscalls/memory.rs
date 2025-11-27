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
/// Starts after interpreter region to avoid conflicts
use core::sync::atomic::{AtomicU64, Ordering};
static NEXT_MMAP_ADDR: AtomicU64 = AtomicU64::new(0x1000_0000); // Start at 256MB

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
pub fn mmap(
    addr: u64,
    length: u64,
    prot: u64,
    flags: u64,
    fd: i64,
    offset: u64,
) -> u64 {
    ktrace!(
        "[mmap] addr={:#x}, length={:#x}, prot={:#x}, flags={:#x}, fd={}, offset={:#x}",
        addr, length, prot, flags, fd, offset
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

    // For now, we only support anonymous mappings
    if !is_anonymous {
        // File-backed mappings would require:
        // 1. Validating the file descriptor
        // 2. Reading file contents into the mapped region
        // 3. Tracking the mapping for synchronization
        kwarn!("[mmap] File-backed mappings not fully implemented, treating as anonymous");
        // Fall through to anonymous handling for now
    }

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

    // For anonymous mappings, zero the memory
    // In a real implementation, this would involve:
    // 1. Allocating physical pages
    // 2. Creating page table entries with appropriate permissions
    // 3. Optionally pre-faulting pages (MAP_POPULATE)
    
    // For now, we rely on the existing page table setup
    // The memory should already be accessible in the user region
    
    // Zero the memory if it's a new anonymous mapping
    if is_anonymous {
        unsafe {
            // Safety: We've validated the address and length
            // In a real kernel, we'd check page table permissions
            core::ptr::write_bytes(map_addr as *mut u8, 0, aligned_length as usize);
        }
    }

    kinfo!(
        "[mmap] Mapped {:#x} bytes at {:#x} (prot={:#x}, flags={:#x})",
        aligned_length, map_addr, prot, flags
    );

    posix::set_errno(0);
    map_addr
}

/// Allocate a new mmap address using bump allocator
fn allocate_mmap_address(size: u64) -> u64 {
    use crate::process::{USER_VIRT_BASE, USER_REGION_SIZE};
    
    let user_end = USER_VIRT_BASE + USER_REGION_SIZE;
    
    // Try to allocate from the bump allocator
    let addr = NEXT_MMAP_ADDR.fetch_add(size, Ordering::SeqCst);
    
    // Check bounds
    if addr + size > user_end {
        // Out of virtual address space
        return MAP_FAILED;
    }
    
    addr
}

/// Check if we can map at a given address (simple check)
fn can_map_at(addr: u64, _size: u64) -> bool {
    use crate::process::{USER_VIRT_BASE, USER_REGION_SIZE};
    
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
    kdebug!("[munmap] No mapping found at {:#x}, returning success", addr);
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
    ktrace!("[mprotect] addr={:#x}, length={:#x}, prot={:#x}", addr, length, prot);

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
                kdebug!("[mprotect] Updated protection for region at {:#x}", region.start);
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
    let _ = CURRENT_BRK.compare_exchange(
        0,
        heap_start,
        Ordering::SeqCst,
        Ordering::Relaxed
    );
    
    if addr == 0 {
        // Query current break
        let current = CURRENT_BRK.load(Ordering::SeqCst);
        ktrace!("[brk] Query: current={:#x}", current);
        return current;
    }
    
    // Validate new break address
    if addr < heap_start || addr > heap_max {
        kerror!("[brk] Address {:#x} out of heap bounds [{:#x}, {:#x}]", 
                addr, heap_start, heap_max);
        posix::set_errno(errno::ENOMEM);
        return CURRENT_BRK.load(Ordering::SeqCst);
    }
    
    // Update break
    let old_brk = CURRENT_BRK.swap(addr, Ordering::SeqCst);
    
    // Zero new memory if expanding
    if addr > old_brk {
        unsafe {
            core::ptr::write_bytes(
                old_brk as *mut u8,
                0,
                (addr - old_brk) as usize
            );
        }
    }
    
    ktrace!("[brk] Set: old={:#x}, new={:#x}", old_brk, addr);
    posix::set_errno(0);
    addr
}
