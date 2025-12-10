//! Advanced Memory Management Syscalls
//!
//! This module implements advanced memory management syscalls similar to Linux:
//! - mremap: Remap a virtual memory address
//! - madvise: Give advice about use of memory
//! - mincore: Determine whether pages are resident in memory
//! - msync: Synchronize a file with a memory map
//! - mlock/munlock: Lock/unlock memory
//! - getrlimit/setrlimit: Get/set resource limits

use crate::mm::vma::{AddressSpace, VMABacking, VMAFlags, VMAPermissions, MAX_ADDRESS_SPACES, VMA};
use crate::posix::{self, errno};
use crate::process::{
    HEAP_BASE, HEAP_SIZE, STACK_BASE, STACK_SIZE, USER_REGION_SIZE, USER_VIRT_BASE,
};
use crate::scheduler::current_pid;
use crate::{kdebug, kerror, kinfo, ktrace, kwarn};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// =============================================================================
// Constants
// =============================================================================

/// Page size constant
pub const PAGE_SIZE: u64 = 4096;

// mremap flags
pub const MREMAP_MAYMOVE: u64 = 1;
pub const MREMAP_FIXED: u64 = 2;
pub const MREMAP_DONTUNMAP: u64 = 4;

// madvise advice values
pub const MADV_NORMAL: i32 = 0;
pub const MADV_RANDOM: i32 = 1;
pub const MADV_SEQUENTIAL: i32 = 2;
pub const MADV_WILLNEED: i32 = 3;
pub const MADV_DONTNEED: i32 = 4;
pub const MADV_FREE: i32 = 8;
pub const MADV_REMOVE: i32 = 9;
pub const MADV_DONTFORK: i32 = 10;
pub const MADV_DOFORK: i32 = 11;
pub const MADV_MERGEABLE: i32 = 12;
pub const MADV_UNMERGEABLE: i32 = 13;
pub const MADV_HUGEPAGE: i32 = 14;
pub const MADV_NOHUGEPAGE: i32 = 15;
pub const MADV_DONTDUMP: i32 = 16;
pub const MADV_DODUMP: i32 = 17;
pub const MADV_COLD: i32 = 20;
pub const MADV_PAGEOUT: i32 = 21;

// mlock flags
pub const MCL_CURRENT: i32 = 1;
pub const MCL_FUTURE: i32 = 2;
pub const MCL_ONFAULT: i32 = 4;

// msync flags
pub const MS_ASYNC: i32 = 1;
pub const MS_INVALIDATE: i32 = 2;
pub const MS_SYNC: i32 = 4;

// Resource limit identifiers
pub const RLIMIT_CPU: i32 = 0;
pub const RLIMIT_FSIZE: i32 = 1;
pub const RLIMIT_DATA: i32 = 2;
pub const RLIMIT_STACK: i32 = 3;
pub const RLIMIT_CORE: i32 = 4;
pub const RLIMIT_RSS: i32 = 5;
pub const RLIMIT_NPROC: i32 = 6;
pub const RLIMIT_NOFILE: i32 = 7;
pub const RLIMIT_MEMLOCK: i32 = 8;
pub const RLIMIT_AS: i32 = 9;
pub const RLIMIT_LOCKS: i32 = 10;
pub const RLIMIT_SIGPENDING: i32 = 11;
pub const RLIMIT_MSGQUEUE: i32 = 12;
pub const RLIMIT_NICE: i32 = 13;
pub const RLIMIT_RTPRIO: i32 = 14;
pub const RLIMIT_RTTIME: i32 = 15;
pub const RLIMIT_NLIMITS: i32 = 16;

/// Special value meaning unlimited
pub const RLIM_INFINITY: u64 = u64::MAX;

// =============================================================================
// Resource Limits Structure
// =============================================================================

/// Resource limit structure (like Linux rlimit)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RLimit {
    /// Soft limit (current limit)
    pub rlim_cur: u64,
    /// Hard limit (maximum for non-privileged)
    pub rlim_max: u64,
}

impl Default for RLimit {
    fn default() -> Self {
        Self {
            rlim_cur: RLIM_INFINITY,
            rlim_max: RLIM_INFINITY,
        }
    }
}

/// Per-process resource limits
#[derive(Clone, Copy)]
pub struct ProcessLimits {
    limits: [RLimit; RLIMIT_NLIMITS as usize],
}

impl Default for ProcessLimits {
    fn default() -> Self {
        let mut limits = [RLimit::default(); RLIMIT_NLIMITS as usize];

        // Set some reasonable defaults
        limits[RLIMIT_STACK as usize] = RLimit {
            rlim_cur: STACK_SIZE,
            rlim_max: STACK_SIZE * 4, // Max 8MB stack
        };
        limits[RLIMIT_DATA as usize] = RLimit {
            rlim_cur: HEAP_SIZE,
            rlim_max: HEAP_SIZE * 2, // Max 16MB data
        };
        limits[RLIMIT_AS as usize] = RLimit {
            rlim_cur: USER_REGION_SIZE,
            rlim_max: USER_REGION_SIZE * 2,
        };
        limits[RLIMIT_NOFILE as usize] = RLimit {
            rlim_cur: 1024,
            rlim_max: 4096,
        };
        limits[RLIMIT_MEMLOCK as usize] = RLimit {
            rlim_cur: 64 * 1024,  // 64KB
            rlim_max: 256 * 1024, // 256KB
        };
        limits[RLIMIT_NPROC as usize] = RLimit {
            rlim_cur: 32,
            rlim_max: 64,
        };

        Self { limits }
    }
}

impl ProcessLimits {
    pub fn get(&self, resource: i32) -> Option<&RLimit> {
        if resource >= 0 && resource < RLIMIT_NLIMITS {
            Some(&self.limits[resource as usize])
        } else {
            None
        }
    }

    pub fn set(&mut self, resource: i32, limit: RLimit) -> Result<(), &'static str> {
        if resource < 0 || resource >= RLIMIT_NLIMITS {
            return Err("Invalid resource");
        }

        // Soft limit must not exceed hard limit
        if limit.rlim_cur > limit.rlim_max {
            return Err("Soft limit exceeds hard limit");
        }

        // Non-privileged users cannot raise hard limit (simplified check)
        // In a real system, we'd check capabilities
        let current = &self.limits[resource as usize];
        if limit.rlim_max > current.rlim_max {
            // Allow for now - in production, check CAP_SYS_RESOURCE
        }

        self.limits[resource as usize] = limit;
        Ok(())
    }
}

/// Global process limits table
static PROCESS_LIMITS: Mutex<[ProcessLimits; MAX_ADDRESS_SPACES]> = Mutex::new(
    [ProcessLimits {
        limits: [RLimit {
            rlim_cur: RLIM_INFINITY,
            rlim_max: RLIM_INFINITY,
        }; RLIMIT_NLIMITS as usize],
    }; MAX_ADDRESS_SPACES],
);

/// Initialize default limits for a process
pub fn init_process_limits(pid: u64) {
    if pid >= MAX_ADDRESS_SPACES as u64 {
        return;
    }
    let mut limits = PROCESS_LIMITS.lock();
    limits[pid as usize] = ProcessLimits::default();
}

// =============================================================================
// Memory Statistics per Process
// =============================================================================

/// Per-process memory statistics (like /proc/PID/status)
#[derive(Clone, Copy, Default, Debug)]
pub struct ProcessMemoryStats {
    /// Virtual memory size (bytes)
    pub vm_size: u64,
    /// Resident set size (bytes)
    pub vm_rss: u64,
    /// Shared memory (bytes)
    pub vm_shared: u64,
    /// Code/text segment size
    pub vm_text: u64,
    /// Data segment size (heap)
    pub vm_data: u64,
    /// Stack size
    pub vm_stack: u64,
    /// Locked memory
    pub vm_locked: u64,
    /// Peak virtual memory size
    pub vm_peak: u64,
    /// Peak RSS
    pub vm_hwm: u64, // High water mark
    /// Anonymous memory
    pub anon_pages: u64,
    /// Number of page faults
    pub page_faults: u64,
    /// Number of major faults (requiring disk I/O)
    pub major_faults: u64,
}

static PROCESS_MEMORY_STATS: Mutex<[ProcessMemoryStats; MAX_ADDRESS_SPACES]> = Mutex::new(
    [ProcessMemoryStats {
        vm_size: 0,
        vm_rss: 0,
        vm_shared: 0,
        vm_text: 0,
        vm_data: 0,
        vm_stack: 0,
        vm_locked: 0,
        vm_peak: 0,
        vm_hwm: 0,
        anon_pages: 0,
        page_faults: 0,
        major_faults: 0,
    }; MAX_ADDRESS_SPACES],
);

/// Update memory statistics for current process
pub fn update_memory_stats<F>(f: F)
where
    F: FnOnce(&mut ProcessMemoryStats),
{
    if let Some(pid) = current_pid() {
        if pid < MAX_ADDRESS_SPACES as u64 {
            let mut stats = PROCESS_MEMORY_STATS.lock();
            f(&mut stats[pid as usize]);

            // Update peak values
            let stat = &mut stats[pid as usize];
            if stat.vm_size > stat.vm_peak {
                stat.vm_peak = stat.vm_size;
            }
            if stat.vm_rss > stat.vm_hwm {
                stat.vm_hwm = stat.vm_rss;
            }
        }
    }
}

/// Get memory statistics for a process
pub fn get_memory_stats(pid: u64) -> Option<ProcessMemoryStats> {
    if pid >= MAX_ADDRESS_SPACES as u64 {
        return None;
    }
    let stats = PROCESS_MEMORY_STATS.lock();
    Some(stats[pid as usize])
}

// =============================================================================
// Address Space Access Helper
// =============================================================================

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

    let mut spaces = super::memory_vma::get_address_spaces();
    let space = &mut spaces[pid as usize];

    if !space.valid {
        space.init(pid, 0);
    }

    Ok(f(space))
}

// =============================================================================
// mremap Implementation
// =============================================================================

/// SYS_MREMAP - Remap a virtual memory address
///
/// This is used to resize or relocate memory mappings.
///
/// # Arguments
/// * `old_address` - Current start address of mapping
/// * `old_size` - Current size of mapping
/// * `new_size` - New desired size
/// * `flags` - MREMAP_MAYMOVE, MREMAP_FIXED, MREMAP_DONTUNMAP
/// * `new_address` - New address (only with MREMAP_FIXED)
///
/// # Returns
/// * New address on success
/// * MAP_FAILED (-1) on error
pub fn mremap(old_address: u64, old_size: u64, new_size: u64, flags: u64, new_address: u64) -> u64 {
    ktrace!(
        "[mremap] old={:#x}, old_size={:#x}, new_size={:#x}, flags={:#x}, new_addr={:#x}",
        old_address,
        old_size,
        new_size,
        flags,
        new_address
    );

    // Validate addresses are page-aligned
    if (old_address & (PAGE_SIZE - 1)) != 0 {
        kerror!("[mremap] old_address not page-aligned");
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    if new_size == 0 {
        kerror!("[mremap] new_size is 0");
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    let aligned_old_size = (old_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    let aligned_new_size = (new_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    let result = with_current_address_space(|space| {
        // First, extract all VMA properties we need without holding a reference
        let (vma_start, vma_end, vma_perm, vma_flags, vma_backing) = {
            let vma = match space.vmas.find(old_address) {
                Some(v) => v,
                None => {
                    kerror!("[mremap] No VMA at {:#x}", old_address);
                    return Err(errno::EFAULT);
                }
            };

            // Verify the mapping covers the old region
            if vma.start > old_address || vma.end < old_address + aligned_old_size {
                kerror!("[mremap] VMA doesn't cover entire region");
                return Err(errno::EFAULT);
            }

            (vma.start, vma.end, vma.perm, vma.flags, vma.backing)
        };
        // Now `vma` reference is dropped

        // Handle different scenarios
        if aligned_new_size == aligned_old_size {
            // No size change - nothing to do
            return Ok(old_address);
        }

        if aligned_new_size < aligned_old_size {
            // Shrinking - unmap the excess
            let unmap_start = old_address + aligned_new_size;
            let unmap_size = aligned_old_size - aligned_new_size;
            space
                .munmap(unmap_start, unmap_size)
                .map_err(|_| errno::ENOMEM)?;

            update_memory_stats(|stats| {
                stats.vm_size = stats.vm_size.saturating_sub(unmap_size);
            });

            return Ok(old_address);
        }

        // Growing - need to extend or move
        let extra_size = aligned_new_size - aligned_old_size;
        let extend_start = old_address + aligned_old_size;

        // Try to extend in place first
        let mut overlap_buf = [0i32; 8];
        let overlap_count =
            space
                .vmas
                .find_overlapping(extend_start, extend_start + extra_size, &mut overlap_buf);

        if overlap_count == 0 {
            // Can extend in place - update the VMA's end
            if let Some(vma_mut) = space.vmas.find_mut(old_address) {
                vma_mut.end = old_address + aligned_new_size;

                // Zero the new memory
                unsafe {
                    core::ptr::write_bytes(extend_start as *mut u8, 0, extra_size as usize);
                }

                update_memory_stats(|stats| {
                    stats.vm_size += extra_size;
                });

                return Ok(old_address);
            }
        }

        // Cannot extend in place
        if (flags & MREMAP_MAYMOVE) == 0 {
            kerror!("[mremap] Cannot extend without MREMAP_MAYMOVE");
            return Err(errno::ENOMEM);
        }

        // Find a new location
        let user_end = USER_VIRT_BASE + USER_REGION_SIZE;
        let new_addr = if (flags & MREMAP_FIXED) != 0 {
            if (new_address & (PAGE_SIZE - 1)) != 0 {
                return Err(errno::EINVAL);
            }
            // Unmap any existing mapping at new_address
            let _ = space.munmap(new_address, aligned_new_size);
            new_address
        } else {
            // Find a free region
            space
                .vmas
                .find_free_region(space.mmap_current, user_end, aligned_new_size)
                .ok_or(errno::ENOMEM)?
        };

        // Copy data from old to new location
        unsafe {
            core::ptr::copy_nonoverlapping(
                old_address as *const u8,
                new_addr as *mut u8,
                aligned_old_size.min(aligned_new_size) as usize,
            );

            // Zero any extra space
            if aligned_new_size > aligned_old_size {
                core::ptr::write_bytes(
                    (new_addr + aligned_old_size) as *mut u8,
                    0,
                    (aligned_new_size - aligned_old_size) as usize,
                );
            }
        }

        // Unmap old region (unless MREMAP_DONTUNMAP)
        if (flags & MREMAP_DONTUNMAP) == 0 {
            space.munmap(old_address, aligned_old_size).ok();
        }

        // Create new VMA with the properties we extracted earlier
        let new_vma = VMA::new(
            new_addr,
            new_addr + aligned_new_size,
            vma_perm,
            vma_flags,
            vma_backing,
        );
        space.add_vma(new_vma).ok_or(errno::ENOMEM)?;

        // Update mmap pointer
        if new_addr >= space.mmap_current {
            space.mmap_current = new_addr + aligned_new_size;
        }

        update_memory_stats(|stats| {
            if (flags & MREMAP_DONTUNMAP) == 0 {
                stats.vm_size = stats.vm_size.saturating_sub(aligned_old_size);
            }
            stats.vm_size += aligned_new_size;
        });

        Ok(new_addr)
    });

    match result {
        Ok(Ok(addr)) => {
            posix::set_errno(0);
            addr
        }
        Ok(Err(e)) => {
            posix::set_errno(e);
            u64::MAX
        }
        Err(e) => {
            kerror!("[mremap] Error: {}", e);
            posix::set_errno(errno::EFAULT);
            u64::MAX
        }
    }
}

// =============================================================================
// madvise Implementation
// =============================================================================

/// SYS_MADVISE - Give advice about use of memory
///
/// # Arguments
/// * `addr` - Start of address range
/// * `length` - Length of address range
/// * `advice` - Advice flag (MADV_*)
///
/// # Returns
/// * 0 on success
/// * -1 on error
pub fn madvise(addr: u64, length: u64, advice: i32) -> u64 {
    ktrace!(
        "[madvise] addr={:#x}, length={:#x}, advice={}",
        addr,
        length,
        advice
    );

    // Validate alignment
    if (addr & (PAGE_SIZE - 1)) != 0 {
        kerror!("[madvise] Address not page-aligned");
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    if length == 0 {
        posix::set_errno(0);
        return 0;
    }

    let aligned_length = (length + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    let result = with_current_address_space(|space| {
        // Find overlapping VMAs
        let mut overlapping = [0i32; 32];
        let count = space
            .vmas
            .find_overlapping(addr, addr + aligned_length, &mut overlapping);

        if count == 0 {
            return Err("No mapping at address");
        }

        match advice {
            MADV_NORMAL | MADV_RANDOM | MADV_SEQUENTIAL | MADV_WILLNEED => {
                // These are hints for the page cache/readahead
                // We acknowledge them but don't do anything special (yet)
                kdebug!("[madvise] Advice {} acknowledged", advice);
            }

            MADV_DONTNEED => {
                // Mark pages as not needed - can be discarded
                // For anonymous mappings, zero the pages
                for i in 0..count {
                    let idx = overlapping[i];
                    if let Some(vma) = space.vmas.get_vma_by_index(idx) {
                        if vma.flags.is_anonymous() {
                            let start = addr.max(vma.start);
                            let end = (addr + aligned_length).min(vma.end);

                            // Zero the memory
                            unsafe {
                                core::ptr::write_bytes(start as *mut u8, 0, (end - start) as usize);
                            }
                        }
                    }
                }
            }

            MADV_FREE => {
                // Similar to DONTNEED but lazier - pages can be reused when needed
                // For now, treat like DONTNEED
                for i in 0..count {
                    let idx = overlapping[i];
                    if let Some(vma) = space.vmas.get_vma_by_index(idx) {
                        if vma.flags.is_anonymous() {
                            let start = addr.max(vma.start);
                            let end = (addr + aligned_length).min(vma.end);

                            unsafe {
                                core::ptr::write_bytes(start as *mut u8, 0, (end - start) as usize);
                            }
                        }
                    }
                }
            }

            MADV_DONTFORK => {
                // Mark VMA to not be copied on fork
                for i in 0..count {
                    let idx = overlapping[i];
                    if let Some(vma) = space.vmas.get_vma_by_index_mut(idx) {
                        vma.flags.insert(VMAFlags::DONTCOPY);
                    }
                }
            }

            MADV_DOFORK => {
                // Remove DONTCOPY flag
                for i in 0..count {
                    let idx = overlapping[i];
                    if let Some(vma) = space.vmas.get_vma_by_index_mut(idx) {
                        vma.flags.remove(VMAFlags::DONTCOPY);
                    }
                }
            }

            MADV_MERGEABLE | MADV_UNMERGEABLE | MADV_HUGEPAGE | MADV_NOHUGEPAGE | MADV_DONTDUMP
            | MADV_DODUMP | MADV_COLD | MADV_PAGEOUT => {
                // These require more advanced memory management features
                // Accept them silently for compatibility
                kdebug!("[madvise] Advice {} accepted (no-op)", advice);
            }

            MADV_REMOVE => {
                // Requires file-backed mapping with hole punch support
                // Not fully implemented
                kwarn!("[madvise] MADV_REMOVE not fully supported");
            }

            _ => {
                kerror!("[madvise] Unknown advice: {}", advice);
                return Err("Invalid advice");
            }
        }

        Ok(())
    });

    match result {
        Ok(Ok(())) => {
            posix::set_errno(0);
            0
        }
        Ok(Err(e)) | Err(e) => {
            kerror!("[madvise] Error: {}", e);
            posix::set_errno(errno::EINVAL);
            u64::MAX
        }
    }
}

// =============================================================================
// mincore Implementation
// =============================================================================

/// SYS_MINCORE - Determine whether pages are resident in memory
///
/// # Arguments
/// * `addr` - Start of address range (must be page-aligned)
/// * `length` - Length of range
/// * `vec` - Output vector (1 byte per page)
///
/// # Returns
/// * 0 on success
/// * -1 on error
pub fn mincore(addr: u64, length: u64, vec: *mut u8) -> u64 {
    ktrace!(
        "[mincore] addr={:#x}, length={:#x}, vec={:p}",
        addr,
        length,
        vec
    );

    // Validate alignment
    if (addr & (PAGE_SIZE - 1)) != 0 {
        kerror!("[mincore] Address not page-aligned");
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    if vec.is_null() {
        posix::set_errno(errno::EFAULT);
        return u64::MAX;
    }

    if length == 0 {
        posix::set_errno(0);
        return 0;
    }

    let num_pages = ((length + PAGE_SIZE - 1) / PAGE_SIZE) as usize;

    let result = with_current_address_space(|space| {
        // Check each page
        for i in 0..num_pages {
            let page_addr = addr + (i as u64 * PAGE_SIZE);

            // Check if page is in a valid VMA
            let in_core = if space.vmas.find(page_addr).is_some() {
                // For now, assume all mapped pages are resident
                // In a real implementation, we'd check page tables
                1u8
            } else {
                0u8
            };

            unsafe {
                *vec.add(i) = in_core;
            }
        }

        Ok(())
    });

    match result {
        Ok(Ok(())) => {
            posix::set_errno(0);
            0
        }
        Ok(Err(e)) | Err(e) => {
            kerror!("[mincore] Error: {}", e);
            posix::set_errno(errno::ENOMEM);
            u64::MAX
        }
    }
}

// =============================================================================
// mlock/munlock Implementation
// =============================================================================

/// SYS_MLOCK - Lock memory pages
///
/// # Arguments
/// * `addr` - Start of address range
/// * `len` - Length of range
///
/// # Returns
/// * 0 on success
/// * -1 on error
pub fn mlock(addr: u64, len: u64) -> u64 {
    ktrace!("[mlock] addr={:#x}, len={:#x}", addr, len);

    if len == 0 {
        posix::set_errno(0);
        return 0;
    }

    let aligned_start = addr & !(PAGE_SIZE - 1);
    let aligned_len = ((addr + len + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)) - aligned_start;

    let result = with_current_address_space(|space| {
        // Check resource limit
        let pid = current_pid().unwrap_or(0);
        let limits = PROCESS_LIMITS.lock();
        let memlock_limit = limits[pid as usize]
            .get(RLIMIT_MEMLOCK)
            .map(|l| l.rlim_cur)
            .unwrap_or(RLIM_INFINITY);
        drop(limits);

        let mut stats = PROCESS_MEMORY_STATS.lock();
        let current_locked = stats[pid as usize].vm_locked;
        drop(stats);

        if current_locked + aligned_len > memlock_limit {
            return Err("RLIMIT_MEMLOCK exceeded");
        }

        // Find and lock VMAs
        let mut overlapping = [0i32; 32];
        let count = space.vmas.find_overlapping(
            aligned_start,
            aligned_start + aligned_len,
            &mut overlapping,
        );

        if count == 0 {
            return Err("No mapping at address");
        }

        for i in 0..count {
            let idx = overlapping[i];
            if let Some(vma) = space.vmas.get_vma_by_index_mut(idx) {
                vma.flags.insert(VMAFlags::LOCKED);
            }
        }

        // Update statistics
        update_memory_stats(|stats| {
            stats.vm_locked += aligned_len;
        });

        Ok(())
    });

    match result {
        Ok(Ok(())) => {
            posix::set_errno(0);
            0
        }
        Ok(Err(e)) => {
            kerror!("[mlock] Error: {}", e);
            posix::set_errno(errno::ENOMEM);
            u64::MAX
        }
        Err(e) => {
            kerror!("[mlock] Error: {}", e);
            posix::set_errno(errno::EFAULT);
            u64::MAX
        }
    }
}

/// SYS_MUNLOCK - Unlock memory pages
pub fn munlock(addr: u64, len: u64) -> u64 {
    ktrace!("[munlock] addr={:#x}, len={:#x}", addr, len);

    if len == 0 {
        posix::set_errno(0);
        return 0;
    }

    let aligned_start = addr & !(PAGE_SIZE - 1);
    let aligned_len = ((addr + len + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)) - aligned_start;

    let result = with_current_address_space(|space| {
        let mut overlapping = [0i32; 32];
        let count = space.vmas.find_overlapping(
            aligned_start,
            aligned_start + aligned_len,
            &mut overlapping,
        );

        for i in 0..count {
            let idx = overlapping[i];
            if let Some(vma) = space.vmas.get_vma_by_index_mut(idx) {
                vma.flags.remove(VMAFlags::LOCKED);
            }
        }

        update_memory_stats(|stats| {
            stats.vm_locked = stats.vm_locked.saturating_sub(aligned_len);
        });

        Ok::<(), i32>(())
    });

    match result {
        Ok(Ok(())) | Ok(Err(_)) => {
            posix::set_errno(0);
            0
        }
        Err(e) => {
            kerror!("[munlock] Error: {}", e);
            posix::set_errno(errno::EFAULT);
            u64::MAX
        }
    }
}

/// SYS_MLOCKALL - Lock all memory
pub fn mlockall(flags: i32) -> u64 {
    ktrace!("[mlockall] flags={:#x}", flags);

    let result = with_current_address_space(|space| {
        // Lock all existing VMAs
        if (flags & MCL_CURRENT) != 0 {
            // First collect all VMA starts to avoid borrowing issues
            let mut vma_starts: [u64; 64] = [0; 64];
            let mut count = 0;
            for vma in space.vmas.iter() {
                if count < 64 {
                    vma_starts[count] = vma.start;
                    count += 1;
                }
            }
            // Now modify each VMA
            for i in 0..count {
                if let Some(vma_mut) = space.vmas.find_mut(vma_starts[i]) {
                    vma_mut.flags.insert(VMAFlags::LOCKED);
                }
            }
        }

        // For MCL_FUTURE, we'd need to track this flag per-process
        // and apply it to future mappings
        if (flags & MCL_FUTURE) != 0 {
            kdebug!("[mlockall] MCL_FUTURE noted (not fully implemented)");
        }

        Ok::<(), i32>(())
    });

    match result {
        Ok(Ok(())) => {
            posix::set_errno(0);
            0
        }
        _ => {
            posix::set_errno(errno::ENOMEM);
            u64::MAX
        }
    }
}

/// SYS_MUNLOCKALL - Unlock all memory
pub fn munlockall() -> u64 {
    ktrace!("[munlockall]");

    let result = with_current_address_space(|space| {
        // First collect all VMA starts to avoid borrowing issues
        let mut vma_starts: [u64; 64] = [0; 64];
        let mut count = 0;
        for vma in space.vmas.iter() {
            if count < 64 {
                vma_starts[count] = vma.start;
                count += 1;
            }
        }
        // Now modify each VMA
        for i in 0..count {
            if let Some(vma_mut) = space.vmas.find_mut(vma_starts[i]) {
                vma_mut.flags.remove(VMAFlags::LOCKED);
            }
        }

        update_memory_stats(|stats| {
            stats.vm_locked = 0;
        });

        Ok::<(), i32>(())
    });

    match result {
        Ok(Ok(())) => {
            posix::set_errno(0);
            0
        }
        _ => {
            posix::set_errno(errno::ENOMEM);
            u64::MAX
        }
    }
}

// =============================================================================
// msync Implementation
// =============================================================================

/// SYS_MSYNC - Synchronize a file with a memory map
pub fn msync(addr: u64, length: u64, flags: i32) -> u64 {
    ktrace!(
        "[msync] addr={:#x}, length={:#x}, flags={:#x}",
        addr,
        length,
        flags
    );

    // Validate alignment
    if (addr & (PAGE_SIZE - 1)) != 0 {
        kerror!("[msync] Address not page-aligned");
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    // Validate flags
    if (flags & MS_ASYNC) != 0 && (flags & MS_SYNC) != 0 {
        kerror!("[msync] Cannot specify both MS_ASYNC and MS_SYNC");
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    if length == 0 {
        posix::set_errno(0);
        return 0;
    }

    let result = with_current_address_space(|space| {
        // Check that the range is mapped
        let mut overlapping = [0i32; 32];
        let count = space
            .vmas
            .find_overlapping(addr, addr + length, &mut overlapping);

        if count == 0 {
            return Err("No mapping at address");
        }

        // For file-backed mappings, we would sync to the backing file
        // For anonymous mappings, this is essentially a no-op
        for i in 0..count {
            let idx = overlapping[i];
            if let Some(vma) = space.vmas.get_vma_by_index(idx) {
                if let VMABacking::File { inode, .. } = vma.backing {
                    kdebug!("[msync] Would sync file inode {} to disk", inode);
                    // In a real implementation, we'd:
                    // 1. Find dirty pages in the range
                    // 2. Write them back to the file
                    // 3. Update the file's mtime
                }
            }
        }

        Ok(())
    });

    match result {
        Ok(Ok(())) => {
            posix::set_errno(0);
            0
        }
        Ok(Err(e)) | Err(e) => {
            kerror!("[msync] Error: {}", e);
            posix::set_errno(errno::ENOMEM);
            u64::MAX
        }
    }
}

// =============================================================================
// Resource Limit Syscalls
// =============================================================================

/// SYS_GETRLIMIT - Get resource limits
pub fn getrlimit(resource: i32, rlim: *mut RLimit) -> u64 {
    ktrace!("[getrlimit] resource={}, rlim={:p}", resource, rlim);

    if rlim.is_null() {
        posix::set_errno(errno::EFAULT);
        return u64::MAX;
    }

    if resource < 0 || resource >= RLIMIT_NLIMITS {
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    let pid = current_pid().unwrap_or(0);
    let limits = PROCESS_LIMITS.lock();

    if let Some(limit) = limits[pid as usize].get(resource) {
        unsafe {
            *rlim = *limit;
        }
        posix::set_errno(0);
        0
    } else {
        posix::set_errno(errno::EINVAL);
        u64::MAX
    }
}

/// SYS_SETRLIMIT - Set resource limits
pub fn setrlimit(resource: i32, rlim: *const RLimit) -> u64 {
    ktrace!("[setrlimit] resource={}, rlim={:p}", resource, rlim);

    if rlim.is_null() {
        posix::set_errno(errno::EFAULT);
        return u64::MAX;
    }

    if resource < 0 || resource >= RLIMIT_NLIMITS {
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    let limit = unsafe { *rlim };
    let pid = current_pid().unwrap_or(0);
    let mut limits = PROCESS_LIMITS.lock();

    match limits[pid as usize].set(resource, limit) {
        Ok(()) => {
            posix::set_errno(0);
            0
        }
        Err(e) => {
            kerror!("[setrlimit] Error: {}", e);
            posix::set_errno(errno::EINVAL);
            u64::MAX
        }
    }
}

/// SYS_PRLIMIT64 - Get/set resource limits (combined)
pub fn prlimit64(pid: i64, resource: i32, new_rlim: *const RLimit, old_rlim: *mut RLimit) -> u64 {
    ktrace!(
        "[prlimit64] pid={}, resource={}, new={:p}, old={:p}",
        pid,
        resource,
        new_rlim,
        old_rlim
    );

    if resource < 0 || resource >= RLIMIT_NLIMITS {
        posix::set_errno(errno::EINVAL);
        return u64::MAX;
    }

    let target_pid = if pid == 0 {
        current_pid().unwrap_or(0)
    } else {
        pid as u64
    };

    if target_pid >= MAX_ADDRESS_SPACES as u64 {
        posix::set_errno(errno::ESRCH);
        return u64::MAX;
    }

    let mut limits = PROCESS_LIMITS.lock();

    // Get old limit if requested
    if !old_rlim.is_null() {
        if let Some(limit) = limits[target_pid as usize].get(resource) {
            unsafe {
                *old_rlim = *limit;
            }
        }
    }

    // Set new limit if provided
    if !new_rlim.is_null() {
        let limit = unsafe { *new_rlim };
        if let Err(e) = limits[target_pid as usize].set(resource, limit) {
            kerror!("[prlimit64] Error: {}", e);
            posix::set_errno(errno::EINVAL);
            return u64::MAX;
        }
    }

    posix::set_errno(0);
    0
}

// =============================================================================
// Memory Information Syscall
// =============================================================================

/// Memory info structure (like sysinfo)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MemInfo {
    pub total_ram: u64,
    pub free_ram: u64,
    pub shared_ram: u64,
    pub buffer_ram: u64,
    pub total_swap: u64,
    pub free_swap: u64,
    pub procs: u16,
    pub total_high: u64,
    pub free_high: u64,
    pub mem_unit: u32,
}

/// Get system memory information
pub fn get_meminfo(info: *mut MemInfo) -> u64 {
    if info.is_null() {
        posix::set_errno(errno::EFAULT);
        return u64::MAX;
    }

    // Get kernel heap statistics
    let (_heap_stats, buddy_stats, _slab_stats) = crate::mm::get_memory_stats();

    // Get process count (returns ready, running, sleeping, zombie)
    let (ready, running, sleeping, zombie) = crate::scheduler::get_process_counts();
    let total_procs = ready + running + sleeping + zombie;

    // Calculate total RAM from pages_allocated + pages_free
    let total_pages = buddy_stats.pages_allocated + buddy_stats.pages_free;

    let meminfo = MemInfo {
        total_ram: total_pages * (PAGE_SIZE as u64),
        free_ram: buddy_stats.pages_free * (PAGE_SIZE as u64),
        shared_ram: 0, // TODO: Track shared memory
        buffer_ram: 0,
        total_swap: 0, // No swap support yet
        free_swap: 0,
        procs: total_procs as u16,
        total_high: 0,
        free_high: 0,
        mem_unit: 1,
    };

    unsafe {
        *info = meminfo;
    }

    posix::set_errno(0);
    0
}
