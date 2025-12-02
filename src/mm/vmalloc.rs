/// Virtual Memory Allocator (vmalloc) for NexaOS
///
/// Provides kernel virtual memory allocation similar to Linux vmalloc,
/// allowing non-contiguous physical pages to be mapped to contiguous
/// virtual addresses. This is useful for large allocations where
/// physical contiguity is not required.
///
/// # Features
/// - Virtual address space management
/// - Lazy allocation and mapping
/// - Guard pages for overflow detection
/// - TLB management
/// - Memory-mapped I/O support
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// =============================================================================
// Constants
// =============================================================================

/// Virtual memory area base address (high kernel addresses)
const VMALLOC_START: u64 = 0xFFFF_FF80_0000_0000;
const VMALLOC_END: u64 = 0xFFFF_FF80_4000_0000; // 1GB vmalloc region

/// Guard page size (for detecting overflows)
const GUARD_PAGE_SIZE: u64 = 4096;

/// Page size
const PAGE_SIZE: u64 = 4096;

/// Maximum number of vmalloc regions
const MAX_VMALLOC_REGIONS: usize = 1024;

// =============================================================================
// Virtual Memory Area
// =============================================================================

/// Flags for virtual memory areas
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmFlags {
    bits: u32,
}

impl VmFlags {
    pub const NONE: Self = Self { bits: 0 };
    pub const WRITABLE: Self = Self { bits: 1 << 0 };
    pub const EXECUTABLE: Self = Self { bits: 1 << 1 };
    pub const USER: Self = Self { bits: 1 << 2 };
    pub const UNCACHED: Self = Self { bits: 1 << 3 };
    pub const GUARD: Self = Self { bits: 1 << 4 };
    pub const LAZY: Self = Self { bits: 1 << 5 };

    pub fn contains(&self, other: Self) -> bool {
        self.bits & other.bits == other.bits
    }

    pub fn insert(&mut self, other: Self) {
        self.bits |= other.bits;
    }

    pub fn to_page_flags(&self) -> u64 {
        use x86_64::structures::paging::PageTableFlags;

        let mut flags = PageTableFlags::PRESENT;

        if self.contains(Self::WRITABLE) {
            flags |= PageTableFlags::WRITABLE;
        }

        if !self.contains(Self::EXECUTABLE) {
            flags |= PageTableFlags::NO_EXECUTE;
        }

        if self.contains(Self::USER) {
            flags |= PageTableFlags::USER_ACCESSIBLE;
        }

        if self.contains(Self::UNCACHED) {
            flags |= PageTableFlags::NO_CACHE;
            flags |= PageTableFlags::WRITE_THROUGH;
        }

        flags.bits()
    }
}

/// Virtual memory area descriptor
#[derive(Debug, Clone, Copy)]
struct VmArea {
    /// Virtual start address
    virt_start: u64,
    /// Size in bytes
    size: u64,
    /// Flags
    flags: VmFlags,
    /// Physical pages backing this area (optional, for lazy allocation)
    phys_pages: [Option<u64>; 256],
    /// Number of mapped pages
    mapped_pages: usize,
    /// Allocation tag for debugging
    tag: u64,
}

impl VmArea {
    const fn empty() -> Self {
        Self {
            virt_start: 0,
            size: 0,
            flags: VmFlags::NONE,
            phys_pages: [None; 256],
            mapped_pages: 0,
            tag: 0,
        }
    }
}

// =============================================================================
// Virtual Memory Allocator
// =============================================================================

pub struct VmallocAllocator {
    /// Next virtual address to allocate
    next_vaddr: AtomicU64,
    /// Active virtual memory areas
    areas: [Option<VmArea>; MAX_VMALLOC_REGIONS],
    /// Number of active areas
    area_count: usize,
    /// Statistics
    stats: VmallocStats,
}

#[derive(Debug, Clone, Copy, Default)]
struct VmallocStats {
    allocations: u64,
    frees: u64,
    bytes_allocated: u64,
    bytes_freed: u64,
    lazy_faults: u64,
    tlb_flushes: u64,
}

impl VmallocAllocator {
    const fn new() -> Self {
        const EMPTY_AREA: Option<VmArea> = None;
        Self {
            next_vaddr: AtomicU64::new(VMALLOC_START),
            areas: [EMPTY_AREA; MAX_VMALLOC_REGIONS],
            area_count: 0,
            stats: VmallocStats {
                allocations: 0,
                frees: 0,
                bytes_allocated: 0,
                bytes_freed: 0,
                lazy_faults: 0,
                tlb_flushes: 0,
            },
        }
    }

    /// Allocate virtual memory
    ///
    /// # Arguments
    /// * `size` - Size in bytes
    /// * `flags` - Memory flags
    ///
    /// # Returns
    /// Virtual address of allocated region, or None if out of address space
    pub fn allocate(&mut self, size: u64, flags: VmFlags) -> Option<u64> {
        if size == 0 || self.area_count >= MAX_VMALLOC_REGIONS {
            return None;
        }

        // Align size to page boundary
        let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        // Reserve virtual address space with guard pages
        let total_size = aligned_size + 2 * GUARD_PAGE_SIZE;
        let virt_addr = self.next_vaddr.fetch_add(total_size, Ordering::SeqCst);

        if virt_addr + total_size > VMALLOC_END {
            crate::kerror!("vmalloc: out of virtual address space");
            return None;
        }

        // Create new area (skip first guard page)
        let area_start = virt_addr + GUARD_PAGE_SIZE;
        let area = VmArea {
            virt_start: area_start,
            size: aligned_size,
            flags,
            phys_pages: [None; 256],
            mapped_pages: 0,
            tag: self.stats.allocations,
        };

        // Find free slot
        for slot in self.areas.iter_mut() {
            if slot.is_none() {
                *slot = Some(area);
                self.area_count += 1;
                break;
            }
        }

        self.stats.allocations += 1;
        self.stats.bytes_allocated += aligned_size;

        // Map pages immediately unless LAZY flag is set
        if !flags.contains(VmFlags::LAZY) {
            if let Err(e) = self.map_area(area_start, aligned_size, flags) {
                crate::kerror!("vmalloc: failed to map area: {:?}", e);
                return None;
            }
        }

        crate::kdebug!(
            "vmalloc: allocated {:#x} bytes at {:#x}",
            aligned_size,
            area_start
        );

        Some(area_start)
    }

    /// Free virtual memory
    pub fn free(&mut self, virt_addr: u64) {
        // Find the area first and collect information we need
        let mut found_idx = None;
        let mut area_size = 0u64;
        let mut area_virt_start = 0u64;
        let mut phys_pages_to_free: [Option<u64>; 256] = [None; 256];

        for (idx, slot) in self.areas.iter().enumerate() {
            if let Some(area) = slot {
                if area.virt_start == virt_addr {
                    found_idx = Some(idx);
                    area_size = area.size;
                    area_virt_start = area.virt_start;
                    phys_pages_to_free.copy_from_slice(&area.phys_pages);
                    break;
                }
            }
        }

        if let Some(idx) = found_idx {
            self.stats.frees += 1;
            self.stats.bytes_freed += area_size;

            // Unmap pages
            self.unmap_area(area_virt_start, area_size);

            // Free physical pages
            for phys_page in phys_pages_to_free.iter() {
                if let Some(phys) = phys_page {
                    self.free_physical_page(*phys);
                }
            }

            self.areas[idx] = None;
            self.area_count -= 1;

            crate::kdebug!("vmalloc: freed {:#x} bytes at {:#x}", area_size, virt_addr);
        } else {
            crate::kerror!(
                "vmalloc: attempted to free invalid address {:#x}",
                virt_addr
            );
        }
    }

    /// Map physical pages to virtual area
    fn map_area(&mut self, virt_start: u64, size: u64, flags: VmFlags) -> Result<(), &'static str> {
        let page_count = (size / PAGE_SIZE) as usize;

        for i in 0..page_count {
            let virt_page = virt_start + (i as u64 * PAGE_SIZE);

            // Allocate physical page
            let phys_page = self.allocate_physical_page()?;

            // Map it
            unsafe {
                self.map_page(virt_page, phys_page, flags)?;
            }

            // Store in area
            if let Some(area) = self.find_area_mut(virt_start) {
                if i < area.phys_pages.len() {
                    area.phys_pages[i] = Some(phys_page);
                    area.mapped_pages += 1;
                }
            }
        }

        // Flush TLB
        self.flush_tlb_range(virt_start, size);

        Ok(())
    }

    /// Unmap virtual area
    fn unmap_area(&mut self, virt_start: u64, size: u64) {
        let page_count = (size / PAGE_SIZE) as usize;

        for i in 0..page_count {
            let virt_page = virt_start + (i as u64 * PAGE_SIZE);
            unsafe {
                self.unmap_page(virt_page);
            }
        }

        self.flush_tlb_range(virt_start, size);
    }

    /// Map a single page
    unsafe fn map_page(&self, virt: u64, phys: u64, flags: VmFlags) -> Result<(), &'static str> {
        use x86_64::registers::control::Cr3;
        use x86_64::structures::paging::PageTable;
        use x86_64::PhysAddr;

        let (pml4_frame, _) = Cr3::read();
        let pml4_addr = pml4_frame.start_address();
        let pml4 = &mut *(pml4_addr.as_u64() as *mut PageTable);

        // Calculate indices
        let pml4_idx = ((virt >> 39) & 0x1FF) as usize;
        let pdp_idx = ((virt >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt >> 12) & 0x1FF) as usize;

        // Ensure PDP exists
        if pml4[pml4_idx].is_unused() {
            let pdp_phys = self.allocate_physical_page()?;
            crate::safety::memzero(pdp_phys as *mut u8, PAGE_SIZE as usize);
            pml4[pml4_idx].set_addr(
                PhysAddr::new(pdp_phys),
                x86_64::structures::paging::PageTableFlags::PRESENT
                    | x86_64::structures::paging::PageTableFlags::WRITABLE,
            );
        }

        let pdp_addr = pml4[pml4_idx].addr();
        let pdp = &mut *(pdp_addr.as_u64() as *mut PageTable);

        // Ensure PD exists
        if pdp[pdp_idx].is_unused() {
            let pd_phys = self.allocate_physical_page()?;
            crate::safety::memzero(pd_phys as *mut u8, PAGE_SIZE as usize);
            pdp[pdp_idx].set_addr(
                PhysAddr::new(pd_phys),
                x86_64::structures::paging::PageTableFlags::PRESENT
                    | x86_64::structures::paging::PageTableFlags::WRITABLE,
            );
        }

        let pd_addr = pdp[pdp_idx].addr();
        let pd = &mut *(pd_addr.as_u64() as *mut PageTable);

        // Ensure PT exists
        if pd[pd_idx].is_unused() {
            let pt_phys = self.allocate_physical_page()?;
            crate::safety::memzero(pt_phys as *mut u8, PAGE_SIZE as usize);
            pd[pd_idx].set_addr(
                PhysAddr::new(pt_phys),
                x86_64::structures::paging::PageTableFlags::PRESENT
                    | x86_64::structures::paging::PageTableFlags::WRITABLE,
            );
        }

        let pt_addr = pd[pd_idx].addr();
        let pt = &mut *(pt_addr.as_u64() as *mut PageTable);

        // Map the page
        use x86_64::structures::paging::PageTableFlags;
        let page_flags = PageTableFlags::from_bits_truncate(flags.to_page_flags());
        pt[pt_idx].set_addr(PhysAddr::new(phys), page_flags);

        Ok(())
    }

    /// Unmap a single page
    unsafe fn unmap_page(&self, virt: u64) {
        use x86_64::registers::control::Cr3;
        use x86_64::structures::paging::PageTable;

        let (pml4_frame, _) = Cr3::read();
        let pml4_addr = pml4_frame.start_address();
        let pml4 = &mut *(pml4_addr.as_u64() as *mut PageTable);

        let pml4_idx = ((virt >> 39) & 0x1FF) as usize;
        let pdp_idx = ((virt >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt >> 12) & 0x1FF) as usize;

        if pml4[pml4_idx].is_unused() {
            return;
        }

        let pdp = &mut *(pml4[pml4_idx].addr().as_u64() as *mut PageTable);
        if pdp[pdp_idx].is_unused() {
            return;
        }

        let pd = &mut *(pdp[pdp_idx].addr().as_u64() as *mut PageTable);
        if pd[pd_idx].is_unused() {
            return;
        }

        let pt = &mut *(pd[pd_idx].addr().as_u64() as *mut PageTable);
        pt[pt_idx].set_unused();
    }

    /// Flush TLB for a range of addresses
    fn flush_tlb_range(&mut self, virt_start: u64, size: u64) {
        use x86_64::instructions::tlb;
        use x86_64::VirtAddr;

        let page_count = (size + PAGE_SIZE - 1) / PAGE_SIZE;

        for i in 0..page_count {
            let addr = VirtAddr::new(virt_start + i * PAGE_SIZE);
            tlb::flush(addr);
        }

        self.stats.tlb_flushes += page_count;
    }

    /// Find area by virtual address
    fn find_area_mut(&mut self, virt_addr: u64) -> Option<&mut VmArea> {
        for slot in self.areas.iter_mut() {
            if let Some(area) = slot {
                if area.virt_start == virt_addr {
                    return Some(area);
                }
            }
        }
        None
    }

    /// Allocate a physical page (delegates to buddy allocator)
    fn allocate_physical_page(&self) -> Result<u64, &'static str> {
        crate::allocator::kalloc(PAGE_SIZE as usize)
            .map(|ptr| ptr as u64)
            .ok_or("Out of physical memory")
    }

    /// Free a physical page
    fn free_physical_page(&self, phys: u64) {
        crate::allocator::kfree(phys as *mut u8);
    }

    /// Handle page fault for lazy allocation
    pub fn handle_page_fault(&mut self, virt_addr: u64) -> Result<(), &'static str> {
        // First pass: find the area and collect necessary information
        let mut found_idx = None;
        let mut page_idx = 0usize;
        let mut area_flags = VmFlags::NONE;
        let mut is_lazy = false;
        let mut already_mapped = false;
        let mut area_virt_start = 0u64;

        for (idx, slot) in self.areas.iter().enumerate() {
            if let Some(area) = slot {
                if virt_addr >= area.virt_start && virt_addr < area.virt_start + area.size {
                    is_lazy = area.flags.contains(VmFlags::LAZY);
                    if !is_lazy {
                        return Err("Page fault in non-lazy area");
                    }

                    let page_offset = virt_addr - area.virt_start;
                    page_idx = (page_offset / PAGE_SIZE) as usize;

                    if page_idx >= area.phys_pages.len() {
                        return Err("Page index out of bounds");
                    }

                    already_mapped = area.phys_pages[page_idx].is_some();
                    if already_mapped {
                        return Ok(());
                    }

                    found_idx = Some(idx);
                    area_flags = area.flags;
                    area_virt_start = area.virt_start;
                    break;
                }
            }
        }

        let idx = found_idx.ok_or("Address not in any vmalloc area")?;

        // Second pass: allocate and map page
        let phys = self.allocate_physical_page()?;
        let virt_page = area_virt_start + (page_idx as u64 * PAGE_SIZE);

        unsafe {
            self.map_page(virt_page, phys, area_flags)?;
        }

        // Update the area
        if let Some(area) = &mut self.areas[idx] {
            area.phys_pages[page_idx] = Some(phys);
            area.mapped_pages += 1;
        }

        self.stats.lazy_faults += 1;

        crate::kdebug!(
            "vmalloc: lazy fault handled at {:#x}, mapped phys {:#x}",
            virt_addr,
            phys
        );

        Ok(())
    }

    /// Get statistics
    pub fn stats(&self) -> VmallocStats {
        self.stats
    }
}

// =============================================================================
// Global Vmalloc Instance
// =============================================================================

static VMALLOC: Mutex<VmallocAllocator> = Mutex::new(VmallocAllocator::new());

/// Allocate virtual memory
pub fn vmalloc(size: u64) -> Option<*mut u8> {
    let mut allocator = VMALLOC.lock();
    let mut flags = VmFlags::WRITABLE;
    flags.insert(VmFlags::NONE);
    allocator.allocate(size, flags).map(|addr| addr as *mut u8)
}

/// Allocate virtual memory with custom flags
pub fn vmalloc_flags(size: u64, flags: VmFlags) -> Option<*mut u8> {
    let mut allocator = VMALLOC.lock();
    allocator.allocate(size, flags).map(|addr| addr as *mut u8)
}

/// Free virtual memory
pub fn vfree(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }

    let mut allocator = VMALLOC.lock();
    allocator.free(ptr as u64);
}

/// Handle vmalloc page fault
pub fn vmalloc_handle_fault(virt_addr: u64) -> Result<(), &'static str> {
    let mut allocator = VMALLOC.lock();
    allocator.handle_page_fault(virt_addr)
}

/// Print vmalloc statistics
pub fn print_vmalloc_stats() {
    let allocator = VMALLOC.lock();
    let stats = allocator.stats();

    crate::kinfo!("=== Vmalloc Statistics ===");
    crate::kinfo!("  Allocations: {}", stats.allocations);
    crate::kinfo!("  Frees: {}", stats.frees);
    crate::kinfo!("  Active regions: {}", stats.allocations - stats.frees);
    crate::kinfo!("  Bytes allocated: {} KB", stats.bytes_allocated / 1024);
    crate::kinfo!("  Bytes freed: {} KB", stats.bytes_freed / 1024);
    crate::kinfo!(
        "  Current usage: {} KB",
        (stats.bytes_allocated - stats.bytes_freed) / 1024
    );
    crate::kinfo!("  Lazy faults: {}", stats.lazy_faults);
    crate::kinfo!("  TLB flushes: {}", stats.tlb_flushes);
    crate::kinfo!("=== End Vmalloc Statistics ===");
}
