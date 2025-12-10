//! VMA-based Memory Management Syscalls
//!
//! This module provides production-grade implementations of mmap, munmap,
//! mprotect, and brk syscalls using the VMA management system.
//!
//! # Usage
//!
//! These functions integrate with the per-process AddressSpace structure
//! to provide proper memory region tracking, permission management, and
//! page table integration.

use crate::mm::vma::{AddressSpace, VMABacking, VMAFlags, VMAPermissions, MAX_ADDRESS_SPACES, VMA};
use crate::posix::{self, errno};
use crate::process::{HEAP_BASE, USER_REGION_SIZE, USER_VIRT_BASE};
use crate::scheduler::current_pid;
use crate::{kdebug, kerror, kinfo, ktrace, kwarn};
use spin::Mutex;

// =============================================================================
// Constants
// =============================================================================

/// Page size constant
pub const PAGE_SIZE: u64 = 4096;

/// Special value indicating mmap failure
pub const MAP_FAILED: u64 = u64::MAX;

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
pub const MAP_GROWSDOWN: u64 = 0x0100;
pub const MAP_DENYWRITE: u64 = 0x0800;
pub const MAP_EXECUTABLE: u64 = 0x1000;
pub const MAP_LOCKED: u64 = 0x2000;
pub const MAP_STACK: u64 = 0x20000;

// =============================================================================
// Per-Process Address Space Management
// =============================================================================

/// Global table of process address spaces
pub(crate) static ADDRESS_SPACES: Mutex<[AddressSpace; MAX_ADDRESS_SPACES]> =
    Mutex::new([const { AddressSpace::empty() }; MAX_ADDRESS_SPACES]);

/// Get a locked reference to all address spaces (for internal use)
pub(crate) fn get_address_spaces() -> spin::MutexGuard<'static, [AddressSpace; MAX_ADDRESS_SPACES]>
{
    ADDRESS_SPACES.lock()
}

/// Get the address space for the current process
fn with_current_address_space<F, R>(f: F) -> Result<R, &'static str>
where
    F: FnOnce(&mut AddressSpace) -> R,
{
    let pid = match current_pid() {
        Some(p) => p,
        None => return Err("No current process"),
    };

    if pid >= MAX_ADDRESS_SPACES as u64 {
        return Err("PID out of range");
    }

    let mut spaces = ADDRESS_SPACES.lock();
    let space = &mut spaces[pid as usize];

    if !space.valid {
        // Lazy initialization for the first access
        space.init(pid, 0); // CR3 will be set properly by process management
    }

    Ok(f(space))
}

/// Initialize address space for a new process
pub fn init_process_address_space(pid: u64, cr3: u64) -> Result<(), &'static str> {
    if pid >= MAX_ADDRESS_SPACES as u64 {
        return Err("PID out of range");
    }

    let mut spaces = ADDRESS_SPACES.lock();
    spaces[pid as usize].init(pid, cr3);
    kinfo!("[vma] Initialized address space for PID {}", pid);
    Ok(())
}

/// Free address space for a process (on exit)
pub fn free_process_address_space(pid: u64) {
    if pid >= MAX_ADDRESS_SPACES as u64 {
        return;
    }

    let mut spaces = ADDRESS_SPACES.lock();
    let space = &mut spaces[pid as usize];

    if space.valid {
        // Log statistics before clearing
        let stats = space.vmas.stats();
        kinfo!(
            "[vma] Freeing address space for PID {}: {} VMAs, {} bytes mapped",
            pid,
            space.vmas.len(),
            stats.mapped_bytes
        );

        space.vmas.clear();
        space.valid = false;
    }
}

/// Copy address space for fork (with COW)
pub fn copy_address_space_for_fork(parent_pid: u64, child_pid: u64) -> Result<(), &'static str> {
    if parent_pid >= MAX_ADDRESS_SPACES as u64 || child_pid >= MAX_ADDRESS_SPACES as u64 {
        return Err("PID out of range");
    }

    let mut spaces = ADDRESS_SPACES.lock();

    if !spaces[parent_pid as usize].valid {
        return Err("Parent address space not valid");
    }

    // Copy layout values from parent
    let heap_start = spaces[parent_pid as usize].heap_start;
    let heap_end = spaces[parent_pid as usize].heap_end;
    let stack_start = spaces[parent_pid as usize].stack_start;
    let stack_end = spaces[parent_pid as usize].stack_end;
    let mmap_base = spaces[parent_pid as usize].mmap_base;
    let mmap_current = spaces[parent_pid as usize].mmap_current;

    // Collect VMAs from parent (copy to avoid borrow conflict)
    let mut vmas_to_copy: [Option<VMA>; 64] = [None; 64];
    let mut vma_count = 0;

    for vma in spaces[parent_pid as usize].vmas.iter() {
        if vma_count < 64 {
            let mut child_vma = *vma;
            // Mark as COW if writable and private
            if vma.perm.is_write() && vma.flags.is_private() {
                child_vma.flags.insert(VMAFlags::COW);
            }
            vmas_to_copy[vma_count] = Some(child_vma);
            vma_count += 1;
        }
    }

    // Initialize child address space
    spaces[child_pid as usize].init(child_pid, 0);
    spaces[child_pid as usize].heap_start = heap_start;
    spaces[child_pid as usize].heap_end = heap_end;
    spaces[child_pid as usize].stack_start = stack_start;
    spaces[child_pid as usize].stack_end = stack_end;
    spaces[child_pid as usize].mmap_base = mmap_base;
    spaces[child_pid as usize].mmap_current = mmap_current;

    // Insert copied VMAs into child
    for i in 0..vma_count {
        if let Some(vma) = vmas_to_copy[i] {
            spaces[child_pid as usize].vmas.insert(vma);
        }
    }

    kinfo!(
        "[vma] Copied address space from PID {} to PID {} ({} VMAs)",
        parent_pid,
        child_pid,
        spaces[child_pid as usize].vmas.len()
    );

    Ok(())
}

// =============================================================================
// mmap Implementation
// =============================================================================

/// SYS_MMAP - Memory map system call (VMA-based implementation)
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
pub fn mmap_vma(addr: u64, length: u64, prot: u64, flags: u64, fd: i64, offset: u64) -> u64 {
    ktrace!(
        "[mmap_vma] addr={:#x}, length={:#x}, prot={:#x}, flags={:#x}, fd={}, offset={:#x}",
        addr,
        length,
        prot,
        flags,
        fd,
        offset
    );

    // Validate length
    if length == 0 {
        kerror!("[mmap_vma] Invalid length: 0");
        posix::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }

    // Round up length to page size
    let aligned_length = (length + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    // Validate flags - must have either SHARED or PRIVATE
    if (flags & MAP_SHARED) == 0 && (flags & MAP_PRIVATE) == 0 {
        // Default to private if neither specified
        // (some programs don't set this properly)
    }

    let is_anonymous = (flags & MAP_ANONYMOUS) != 0;
    let is_fixed = (flags & MAP_FIXED) != 0;

    // Perform the mapping
    let result = with_current_address_space(|space| {
        // Determine mapping address
        let map_addr = if is_fixed {
            // MAP_FIXED: Use the exact address specified
            if addr == 0 || (addr & (PAGE_SIZE - 1)) != 0 {
                kerror!("[mmap_vma] MAP_FIXED with invalid address: {:#x}", addr);
                return Err(errno::EINVAL);
            }

            // For MAP_FIXED, we may need to unmap existing mappings
            let _ = space.munmap(addr, aligned_length);
            addr
        } else if addr != 0 {
            // Hint address provided
            let aligned_hint = (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

            // Check if hint is available
            if space.vmas.find(aligned_hint).is_none() {
                // Check the entire range is free
                let mut overlapping = [0i32; 16];
                let count = space.vmas.find_overlapping(
                    aligned_hint,
                    aligned_hint + aligned_length,
                    &mut overlapping,
                );
                if count == 0 {
                    aligned_hint
                } else {
                    // Hint not usable, find a free region
                    find_free_mmap_region(space, aligned_length)?
                }
            } else {
                find_free_mmap_region(space, aligned_length)?
            }
        } else {
            // No address specified, allocate one
            find_free_mmap_region(space, aligned_length)?
        };

        // Validate the address is within user space
        let user_end = USER_VIRT_BASE + USER_REGION_SIZE;
        if map_addr < USER_VIRT_BASE || map_addr + aligned_length > user_end {
            kerror!(
                "[mmap_vma] Address {:#x} out of user space bounds",
                map_addr
            );
            return Err(errno::ENOMEM);
        }

        // Create VMA flags
        let mut vma_flags = VMAFlags::from_mmap_flags(flags);

        // Set appropriate flags based on mapping type
        if is_anonymous {
            vma_flags.insert(VMAFlags::ANONYMOUS);
        }
        if (flags & MAP_STACK) != 0 {
            vma_flags.insert(VMAFlags::STACK);
            vma_flags.insert(VMAFlags::GROWSDOWN);
        }
        if (flags & MAP_POPULATE) == 0 && is_anonymous {
            vma_flags.insert(VMAFlags::DEMAND); // Demand paging for anonymous mappings
        }

        // Create VMA permissions
        let vma_perm = VMAPermissions::from_prot(prot);

        // Determine backing type
        let backing = if is_anonymous {
            VMABacking::Anonymous
        } else if fd >= 0 {
            VMABacking::File {
                inode: fd as u64, // Simplified - should use actual inode
                offset,
            }
        } else {
            VMABacking::Anonymous
        };

        // Create and insert VMA
        let vma = VMA::new(
            map_addr,
            map_addr + aligned_length,
            vma_perm,
            vma_flags,
            backing,
        );

        if space.add_vma(vma).is_none() {
            kerror!("[mmap_vma] Failed to add VMA");
            return Err(errno::ENOMEM);
        }

        space.vmas.stats_mut().mmap_count += 1;

        // For non-demand-paged mappings, populate the memory
        if (flags & MAP_POPULATE) != 0 || !is_anonymous {
            // Zero the memory for anonymous mappings
            if is_anonymous {
                unsafe {
                    core::ptr::write_bytes(map_addr as *mut u8, 0, aligned_length as usize);
                }
            } else if fd >= 0 {
                // Read file contents into mapping
                if let Err(e) = read_file_into_mapping(fd as u64, offset, map_addr, aligned_length)
                {
                    kwarn!("[mmap_vma] Failed to read file into mapping: {}", e);
                    // Don't fail - the mapping exists, just empty
                }
            }
        }

        // Try to merge with adjacent VMAs
        let _ = space.vmas.try_merge(map_addr);

        kinfo!(
            "[mmap_vma] Mapped {:#x} bytes at {:#x} (prot={:#x}, flags={:#x})",
            aligned_length,
            map_addr,
            prot,
            flags
        );

        Ok(map_addr)
    });

    match result {
        Ok(Ok(addr)) => {
            posix::set_errno(0);
            addr
        }
        Ok(Err(e)) => {
            posix::set_errno(e);
            MAP_FAILED
        }
        Err(e) => {
            kerror!("[mmap_vma] Error: {}", e);
            posix::set_errno(errno::ENOMEM);
            MAP_FAILED
        }
    }
}

/// Find a free region for mmap
fn find_free_mmap_region(space: &mut AddressSpace, size: u64) -> Result<u64, i32> {
    let user_end = USER_VIRT_BASE + USER_REGION_SIZE;

    // Try from current mmap pointer
    if let Some(addr) = space
        .vmas
        .find_free_region(space.mmap_current, user_end, size)
    {
        space.mmap_current = addr + size;
        return Ok(addr);
    }

    // Try from mmap base if we've wrapped around
    if let Some(addr) = space.vmas.find_free_region(space.mmap_base, user_end, size) {
        space.mmap_current = addr + size;
        return Ok(addr);
    }

    kerror!("[mmap_vma] No free region for {} bytes", size);
    Err(errno::ENOMEM)
}

/// Read file contents into a mapped memory region
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
                let dest_slice = core::slice::from_raw_parts_mut(dest_addr as *mut u8, to_read);
                let bytes_read = file_ref.read_at(read_start as usize, dest_slice);
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
                core::ptr::write_bytes(dest_addr as *mut u8, 0, length as usize);
                return Err("Unsupported file backing type for mmap");
            }
        }
    }

    unsafe {
        core::ptr::write_bytes(dest_addr as *mut u8, 0, length as usize);
    }
    Ok(0)
}

// =============================================================================
// munmap Implementation
// =============================================================================

/// SYS_MUNMAP - Unmap memory region (VMA-based implementation)
///
/// # Arguments
/// * `addr` - Start address of region to unmap (must be page-aligned)
/// * `length` - Length of region to unmap
///
/// # Returns
/// * 0 on success
/// * -1 on error with errno set
pub fn munmap_vma(addr: u64, length: u64) -> u64 {
    ktrace!("[munmap_vma] addr={:#x}, length={:#x}", addr, length);

    // Validate address alignment
    if (addr & (PAGE_SIZE - 1)) != 0 {
        kerror!("[munmap_vma] Address not page-aligned: {:#x}", addr);
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    // Validate length
    if length == 0 {
        kerror!("[munmap_vma] Invalid length: 0");
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    let aligned_length = (length + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    let result = with_current_address_space(|space| space.munmap(addr, aligned_length));

    match result {
        Ok(Ok(())) => {
            kdebug!("[munmap_vma] Unmapped region at {:#x}", addr);
            posix::set_errno(0);
            0
        }
        Ok(Err(e)) => {
            // munmap on non-mapped memory is allowed in POSIX
            kdebug!("[munmap_vma] {}, returning success", e);
            posix::set_errno(0);
            0
        }
        Err(e) => {
            kerror!("[munmap_vma] Error: {}", e);
            posix::set_errno(errno::EINVAL);
            u64::MAX
        }
    }
}

// =============================================================================
// mprotect Implementation
// =============================================================================

/// SYS_MPROTECT - Change memory protection (VMA-based implementation)
///
/// # Arguments
/// * `addr` - Start address of region (must be page-aligned)
/// * `length` - Length of region
/// * `prot` - New protection flags
///
/// # Returns
/// * 0 on success
/// * -1 on error with errno set
pub fn mprotect_vma(addr: u64, length: u64, prot: u64) -> u64 {
    ktrace!(
        "[mprotect_vma] addr={:#x}, length={:#x}, prot={:#x}",
        addr,
        length,
        prot
    );

    // Validate address alignment
    if (addr & (PAGE_SIZE - 1)) != 0 {
        kerror!("[mprotect_vma] Address not page-aligned: {:#x}", addr);
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    // Zero length is OK
    if length == 0 {
        posix::set_errno(0);
        return 0;
    }

    let aligned_length = (length + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    let result = with_current_address_space(|space| space.mprotect(addr, aligned_length, prot));

    match result {
        Ok(Ok(())) => {
            kdebug!(
                "[mprotect_vma] Changed protection at {:#x} to {:#x}",
                addr,
                prot
            );
            posix::set_errno(0);
            0
        }
        Ok(Err(e)) => {
            kerror!("[mprotect_vma] Error: {}", e);
            posix::set_errno(errno::ENOMEM);
            u64::MAX
        }
        Err(e) => {
            kerror!("[mprotect_vma] Error: {}", e);
            posix::set_errno(errno::EINVAL);
            u64::MAX
        }
    }
}

// =============================================================================
// brk Implementation
// =============================================================================

/// SYS_BRK - Change data segment size (VMA-based implementation)
///
/// # Arguments
/// * `addr` - New end of data segment (0 to query current)
///
/// # Returns
/// * Current or new end of data segment
/// * Current break on error (brk traditionally doesn't return -1)
pub fn brk_vma(addr: u64) -> u64 {
    let result = with_current_address_space(|space| -> Result<u64, &'static str> {
        if addr == 0 {
            // Query current break
            ktrace!("[brk_vma] Query: current={:#x}", space.heap_end);
            return Ok(space.heap_end);
        }

        match space.brk(addr) {
            Ok(new_brk) => {
                ktrace!("[brk_vma] Set: new={:#x}", new_brk);

                // Zero new memory if expanding
                if new_brk > space.heap_end {
                    unsafe {
                        let old_end = space.heap_end;
                        core::ptr::write_bytes(old_end as *mut u8, 0, (new_brk - old_end) as usize);
                    }
                }

                Ok(new_brk)
            }
            Err(e) => {
                kerror!("[brk_vma] Error: {}", e);
                posix::set_errno(errno::ENOMEM);
                Ok(space.heap_end) // Return current break on error
            }
        }
    });

    match result {
        Ok(Ok(brk)) => {
            posix::set_errno(0);
            brk
        }
        Ok(Err(e)) => {
            kerror!("[brk_vma] Error: {}", e);
            HEAP_BASE // Fallback
        }
        Err(e) => {
            kerror!("[brk_vma] Error: {}", e);
            HEAP_BASE // Fallback
        }
    }
}

// =============================================================================
// Page Fault Handling
// =============================================================================

/// Handle a page fault for a user process
///
/// This is called from the page fault handler when a user-mode fault occurs.
/// It checks if the fault is within a valid VMA and handles demand paging,
/// COW, or reports an error.
///
/// # Arguments
/// * `fault_addr` - The faulting virtual address
/// * `is_write` - True if this was a write access
/// * `is_user` - True if the fault occurred in user mode
///
/// # Returns
/// * Ok(()) if the fault was handled
/// * Err(reason) if the fault should result in a signal
pub fn handle_user_page_fault(
    fault_addr: u64,
    is_write: bool,
    _is_user: bool,
) -> Result<(), &'static str> {
    ktrace!("[vma] Page fault at {:#x}, write={}", fault_addr, is_write);

    with_current_address_space(|space| space.handle_fault(fault_addr, is_write))?
}

// =============================================================================
// Debug/Statistics Functions
// =============================================================================

/// Print memory maps for the current process (like /proc/self/maps)
pub fn print_current_maps() {
    let _ = with_current_address_space(|space| {
        space.print_maps();
    });
}

/// Get VMA statistics for the current process
pub fn get_vma_stats() -> Option<crate::mm::vma::VMAStats> {
    with_current_address_space(|space| *space.vmas.stats()).ok()
}

/// Print VMA statistics for all processes
pub fn print_all_vma_stats() {
    let spaces = ADDRESS_SPACES.lock();

    kinfo!("=== VMA Statistics (All Processes) ===");

    for (i, space) in spaces.iter().enumerate() {
        if space.valid {
            let stats = space.vmas.stats();
            kinfo!(
                "PID {}: {} VMAs, {} bytes, {} page faults",
                i,
                space.vmas.len(),
                stats.mapped_bytes,
                stats.page_faults
            );
        }
    }

    kinfo!("=== End VMA Statistics ===");
}
