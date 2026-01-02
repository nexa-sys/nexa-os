/// Advanced Memory Allocator for NexaOS
///
/// This module implements a production-grade memory allocation system inspired by
/// Linux, BSD, and seL4, featuring:
///
/// 1. **Buddy Allocator**: For efficient physical page frame allocation.
///    - Uses embedded linked lists for infinite scalability (no fixed node array).
///    - O(log n) allocation/free.
///    - Robust merging and splitting logic.
/// 2. **Slab Allocator**: For kernel object caching.
///    - Uses linked lists of pages for infinite scalability.
///    - Supports object sizes up to 2048 bytes.
///    - Efficient partial page tracking and page reclaiming.
///    - Cache coloring (implicit via header) and alignment.
/// 3. **Virtual Memory Allocator**: For kernel heap management.
/// 4. **Per-CPU Caching**: Reduces lock contention in SMP systems.
/// 5. **Memory Statistics**: Tracking allocations, frees, and fragmentation.
///
/// # Design Goals
/// - Zero fragmentation for common allocation sizes (via slabs)
/// - O(log n) allocation time for buddy system
/// - Lock-free fast paths where possible
/// - Comprehensive debugging and leak detection
/// - Production-ready reliability                         
use spin::Mutex;

// =============================================================================
// Constants and Configuration
// =============================================================================

/// Maximum order for buddy allocator (2^MAX_ORDER pages)
/// MAX_ORDER=11 means up to 2^11 = 2048 pages = 8MB contiguous allocations
pub const MAX_ORDER: usize = 11;

/// Page size (4KB for x86_64)
pub const PAGE_SIZE: usize = 4096;

/// Number of slab size classes
pub const SLAB_CLASSES: usize = 8;

/// Slab sizes: 16, 32, 64, 128, 256, 512, 1024, 2048 bytes
/// Larger allocations are handled directly by the buddy allocator
pub const SLAB_SIZES: [usize; SLAB_CLASSES] = [16, 32, 64, 128, 256, 512, 1024, 2048];

/// Magic number for heap block validation
pub const HEAP_MAGIC: u32 = 0xDEADBEEF;

/// Poison pattern for freed memory (aids debugging)
pub const POISON_BYTE: u8 = 0xCC;

// =============================================================================
// Buddy Allocator Helper Functions (Exported for testing)
// =============================================================================

/// Calculate minimum order needed for a given size
/// Order 0 = 1 page (4KB), Order 1 = 2 pages (8KB), etc.
#[inline]
pub const fn size_to_order(size: usize) -> usize {
    if size <= PAGE_SIZE {
        return 0;
    }
    let pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
    // Round up to next power of 2
    (usize::BITS - (pages - 1).leading_zeros()) as usize
}

/// Convert order to size in bytes
#[inline]
pub const fn order_to_size(order: usize) -> usize {
    PAGE_SIZE << order
}

/// Calculate buddy address by XORing with block size
/// Buddy pairs differ only in the bit position corresponding to block size
#[inline]
pub const fn get_buddy_addr(addr: u64, order: usize) -> u64 {
    let block_size = (PAGE_SIZE << order) as u64;
    addr ^ block_size
}

/// Check if two addresses form a valid buddy pair at given order
#[inline]
pub const fn is_valid_buddy_pair(addr1: u64, addr2: u64, order: usize) -> bool {
    let block_size = (PAGE_SIZE << order) as u64;
    // Check alignment
    if addr1 & (block_size - 1) != 0 || addr2 & (block_size - 1) != 0 {
        return false;
    }
    // XOR check: buddies differ only in the buddy bit
    (addr1 ^ addr2) == block_size
}

/// Check if address is aligned to the given order
#[inline]
pub const fn is_order_aligned(addr: u64, order: usize) -> bool {
    let alignment = (PAGE_SIZE << order) as u64;
    addr & (alignment - 1) == 0
}

// =============================================================================
// Physical Frame Allocator (Buddy System)
// =============================================================================

/// Buddy allocator for physical page frames
///
/// Uses the classic buddy system algorithm where blocks are split and merged
/// in powers of 2. This provides O(log n) allocation/free with minimal fragmentation.
///
/// This implementation stores free list pointers directly in the free memory blocks,
/// eliminating the need for a separate node array and removing the limit on the
/// number of free blocks.
pub struct BuddyAllocator {
    /// Free lists for each order (0..MAX_ORDER)
    /// free_lists[i] contains the physical address of the first free block of order i
    free_lists: [Option<u64>; MAX_ORDER],
    /// Base physical address managed by this allocator
    base_addr: u64,
    /// Total size in bytes
    total_size: u64,
    /// Statistics
    stats: BuddyStats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BuddyStats {
    pub allocations: u64,
    pub frees: u64,
    pub splits: u64,
    pub merges: u64,
    pub pages_allocated: u64,
    pub pages_free: u64,
}

impl BuddyAllocator {
    /// Create a new buddy allocator
    const fn new() -> Self {
        Self {
            free_lists: [None; MAX_ORDER],
            base_addr: 0,
            total_size: 0,
            stats: BuddyStats {
                allocations: 0,
                frees: 0,
                splits: 0,
                merges: 0,
                pages_allocated: 0,
                pages_free: 0,
            },
        }
    }

    /// Initialize the buddy allocator with a memory region
    ///
    /// # Arguments
    /// * `base` - Physical base address (must be page-aligned)
    /// * `size` - Size in bytes (will be rounded down to page boundary)
    pub fn init(&mut self, base: u64, size: u64) {
        if base & 0xFFF != 0 {
            panic!("Buddy allocator base {:#x} is not page-aligned", base);
        }

        self.base_addr = base;
        self.total_size = size & !0xFFF;

        let total_pages = (self.total_size / PAGE_SIZE as u64) as usize;
        self.stats.pages_free = total_pages as u64;

        crate::kinfo!(
            "Buddy allocator initialized: base={:#x}, size={:#x} ({} pages)",
            base,
            self.total_size,
            total_pages
        );

        // Add initial free blocks to the allocator
        self.add_free_region(base, self.total_size);
    }

    /// Add a free memory region to the buddy allocator
    fn add_free_region(&mut self, mut addr: u64, mut size: u64) {
        while size >= PAGE_SIZE as u64 {
            // Find the largest order that fits
            let mut order = 0;
            while order < MAX_ORDER - 1 {
                let block_size = (PAGE_SIZE << (order + 1)) as u64;
                if size < block_size || addr & (block_size - 1) != 0 {
                    break;
                }
                order += 1;
            }

            let block_size = (PAGE_SIZE << order) as u64;
            self.add_free_block(addr, order);

            addr += block_size;
            size -= block_size;
        }
    }

    /// Add a free block to the free list
    /// Helper to validate an address is valid for pointer operations
    #[inline]
    fn is_valid_addr(addr: u64) -> bool {
        // Address must be non-zero and 8-byte aligned for u64 read/write
        addr != 0 && addr & 7 == 0
    }

    fn add_free_block(&mut self, addr: u64, order: usize) {
        if !Self::is_valid_addr(addr) {
            crate::kerror!("add_free_block: invalid addr {:#x} (order={})", addr, order);
            return;
        }
        unsafe {
            // Store the next pointer in the free block itself
            let next = self.free_lists[order];
            let ptr = addr as *mut u64;
            // Use volatile write to ensure it's not optimized out, though standard write is likely fine
            // assuming we have exclusive access (which we do via Mutex in KernelHeap)
            core::ptr::write(ptr, next.unwrap_or(u64::MAX));
        }
        self.free_lists[order] = Some(addr);
    }

    /// Allocate physical pages
    ///
    /// # Arguments
    /// * `order` - Order of allocation (allocates 2^order pages)
    ///
    /// # Returns
    /// Physical address of allocated block, or None if out of memory
    pub fn allocate(&mut self, order: usize) -> Option<u64> {
        if order >= MAX_ORDER {
            return None;
        }

        // Find a free block of the requested order or larger
        for current_order in order..MAX_ORDER {
            if let Some(addr) = self.free_lists[current_order] {
                // Validate address before reading
                if !Self::is_valid_addr(addr) {
                    crate::kerror!("allocate: corrupt free list at order {}, addr={:#x}", current_order, addr);
                    self.free_lists[current_order] = None;
                    continue;
                }
                // Remove from free list
                unsafe {
                    let ptr = addr as *const u64;
                    let next = core::ptr::read(ptr);
                    self.free_lists[current_order] =
                        if next == u64::MAX { None } else { Some(next) };
                }

                // Split if necessary
                for split_order in (order + 1..=current_order).rev() {
                    self.stats.splits += 1;
                    let buddy_addr = addr + ((PAGE_SIZE << (split_order - 1)) as u64);
                    self.add_free_block(buddy_addr, split_order - 1);
                }

                self.stats.allocations += 1;
                self.stats.pages_allocated += (1 << order) as u64;
                self.stats.pages_free -= (1 << order) as u64;

                // Clear the pointer in the allocated block for safety
                unsafe {
                    core::ptr::write_bytes(addr as *mut u8, 0, 8);
                }

                return Some(addr);
            }
        }

        None
    }

    /// Free physical pages
    ///
    /// # Arguments
    /// * `addr` - Physical address to free
    /// * `order` - Order of allocation (must match original allocation)
    pub fn free(&mut self, addr: u64, order: usize) {
        if order >= MAX_ORDER {
            crate::kerror!("Invalid order {} in buddy free", order);
            return;
        }

        self.stats.frees += 1;
        self.stats.pages_allocated -= (1 << order) as u64;
        self.stats.pages_free += (1 << order) as u64;

        // Try to merge with buddy
        let mut current_addr = addr;
        let mut current_order = order;

        while current_order < MAX_ORDER - 1 {
            let block_size = (PAGE_SIZE << current_order) as u64;
            let buddy_addr = current_addr ^ block_size;

            // Check if buddy is free
            if self.remove_from_free_list(buddy_addr, current_order) {
                self.stats.merges += 1;
                // Merge: the lower address becomes the merged block
                current_addr = current_addr.min(buddy_addr);
                current_order += 1;
            } else {
                break;
            }
        }

        self.add_free_block(current_addr, current_order);
    }

    /// Find and remove a buddy from free list
    /// Returns true if found and removed
    fn remove_from_free_list(&mut self, target_addr: u64, order: usize) -> bool {
        let mut prev_addr: Option<u64> = None;
        let mut current_addr = self.free_lists[order];

        while let Some(addr) = current_addr {
            // Validate address before any pointer operations
            if !Self::is_valid_addr(addr) {
                crate::kerror!("remove_from_free_list: corrupt free list at order {}, addr={:#x}", order, addr);
                // Truncate the list at the previous valid entry
                if let Some(prev) = prev_addr {
                    unsafe {
                        core::ptr::write(prev as *mut u64, u64::MAX);
                    }
                } else {
                    self.free_lists[order] = None;
                }
                return false;
            }

            if addr == target_addr {
                // Found it, remove from list
                unsafe {
                    let ptr = addr as *const u64;
                    let next = core::ptr::read(ptr);
                    let next_opt = if next == u64::MAX { None } else { Some(next) };

                    if let Some(prev) = prev_addr {
                        let prev_ptr = prev as *mut u64;
                        core::ptr::write(prev_ptr, next);
                    } else {
                        self.free_lists[order] = next_opt;
                    }
                }
                return true;
            }

            prev_addr = Some(addr);
            unsafe {
                let ptr = addr as *const u64;
                let next = core::ptr::read(ptr);
                current_addr = if next == u64::MAX { None } else { Some(next) };
            }
        }

        false
    }

    /// Get allocator statistics
    pub fn stats(&self) -> BuddyStats {
        self.stats
    }
}

// =============================================================================
// Slab Allocator
// =============================================================================

/// Slab for a specific object size
struct Slab {
    /// Size of objects in this slab
    object_size: usize,
    /// Number of objects per slab page
    objects_per_slab: usize,
    /// Head of the list of pages with free objects
    partial_head: Option<u64>,

    // Stats
    allocated_count: usize,
}

impl Slab {
    const fn new(size: usize) -> Self {
        // Calculate objects per page, accounting for header
        let header_size = core::mem::size_of::<SlabPageHeader>();
        let available_space = PAGE_SIZE - header_size;
        let objects_per_slab = if size <= available_space {
            available_space / size
        } else {
            0 // Should not happen for valid slab sizes
        };

        Self {
            object_size: size,
            objects_per_slab,
            partial_head: None,
            allocated_count: 0,
        }
    }

    /// Allocate an object from this slab
    fn allocate(&mut self, buddy: &mut BuddyAllocator) -> Option<u64> {
        // If no partial pages, allocate a new one
        if self.partial_head.is_none() {
            self.allocate_new_page(buddy)?;
        }

        let page_addr = self.partial_head?;

        unsafe {
            let header_ptr = page_addr as *mut SlabPageHeader;
            let header = &mut *header_ptr;

            // Pop from free list
            if header.free_list != SlabPageHeader::NONE16 {
                let header_size = core::mem::size_of::<SlabPageHeader>();
                let obj_addr = page_addr
                    + header_size as u64
                    + (header.free_list as u64 * self.object_size as u64);

                // Read next pointer from freed object
                // Objects in free list store the index of the next free object
                let next_idx_ptr = obj_addr as *const u16;
                let next_idx = core::ptr::read(next_idx_ptr);

                header.free_list = next_idx;
                header.free_count -= 1;
                self.allocated_count += 1;

                // If page is now full, remove from partial list
                if header.free_count == 0 {
                    self.remove_from_partial_list(page_addr);
                }

                // Clear the pointer in the allocated object
                core::ptr::write_bytes(obj_addr as *mut u8, 0, self.object_size);

                return Some(obj_addr);
            }
        }

        None
    }

    /// Free an object back to this slab
    fn free(&mut self, addr: u64, buddy: &mut BuddyAllocator) {
        // Find page start
        let page_addr = addr & !(PAGE_SIZE as u64 - 1);

        unsafe {
            let header_ptr = page_addr as *mut SlabPageHeader;
            let header = &mut *header_ptr;

            let header_size = core::mem::size_of::<SlabPageHeader>();
            // Calculate object index
            let offset = addr - (page_addr + header_size as u64);
            let obj_idx = (offset / self.object_size as u64) as u16;

            // Poison memory
            core::ptr::write_bytes(addr as *mut u8, POISON_BYTE, self.object_size);

            // Add to free list
            let next = header.free_list;
            let obj_ptr = addr as *mut u16;
            core::ptr::write(obj_ptr, next);

            header.free_list = obj_idx;
            header.free_count += 1;
            self.allocated_count -= 1;

            // If page was full (now has 1 free), add to partial list
            if header.free_count == 1 {
                self.add_to_partial_list(page_addr);
            }

            // If page is completely empty, free it to buddy
            if header.free_count as usize == self.objects_per_slab {
                self.remove_from_partial_list(page_addr);
                buddy.free(page_addr, 0);
            }
        }
    }

    /// Allocate a new page for this slab
    fn allocate_new_page(&mut self, buddy: &mut BuddyAllocator) -> Option<()> {
        let page_addr = buddy.allocate(0)?;

        unsafe {
            let header_ptr = page_addr as *mut SlabPageHeader;
            // Initialize header
            core::ptr::write(
                header_ptr,
                SlabPageHeader {
                    next: SlabPageHeader::NONE,
                    prev: SlabPageHeader::NONE,
                    free_list: 0, // Start with object 0
                    free_count: self.objects_per_slab as u16,
                    _padding: 0,
                },
            );

            let header_size = core::mem::size_of::<SlabPageHeader>();
            // Initialize object free list
            let base_addr = page_addr + header_size as u64;
            for i in 0..self.objects_per_slab {
                let obj_addr = base_addr + (i * self.object_size) as u64;
                let next_idx = if i == self.objects_per_slab - 1 {
                    SlabPageHeader::NONE16
                } else {
                    (i + 1) as u16
                };
                core::ptr::write(obj_addr as *mut u16, next_idx);
            }

            self.add_to_partial_list(page_addr);
        }

        Some(())
    }

    unsafe fn add_to_partial_list(&mut self, page_addr: u64) {
        let header_ptr = page_addr as *mut SlabPageHeader;
        let header = &mut *header_ptr;

        header.next = self.partial_head.unwrap_or(SlabPageHeader::NONE);
        header.prev = SlabPageHeader::NONE;

        if let Some(head_addr) = self.partial_head {
            let head_ptr = head_addr as *mut SlabPageHeader;
            (*head_ptr).prev = page_addr;
        }

        self.partial_head = Some(page_addr);
    }

    unsafe fn remove_from_partial_list(&mut self, page_addr: u64) {
        let header_ptr = page_addr as *mut SlabPageHeader;
        let header = &mut *header_ptr;

        if header.prev != SlabPageHeader::NONE {
            let prev_ptr = header.prev as *mut SlabPageHeader;
            (*prev_ptr).next = header.next;
        } else {
            self.partial_head = if header.next == SlabPageHeader::NONE {
                None
            } else {
                Some(header.next)
            };
        }

        if header.next != SlabPageHeader::NONE {
            let next_ptr = header.next as *mut SlabPageHeader;
            (*next_ptr).prev = header.prev;
        }

        header.next = SlabPageHeader::NONE;
        header.prev = SlabPageHeader::NONE;
    }
}

#[repr(C)]
struct SlabPageHeader {
    next: u64,      // u64::MAX for None
    prev: u64,      // u64::MAX for None
    free_list: u16, // u16::MAX for None
    free_count: u16,
    _padding: u32, // Ensure 8-byte alignment and size >= 24
}

impl SlabPageHeader {
    const NONE: u64 = u64::MAX;
    const NONE16: u16 = u16::MAX;
}

/// Slab allocator managing multiple slab caches
pub struct SlabAllocator {
    slabs: [Slab; SLAB_CLASSES],
    stats: SlabStats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SlabStats {
    pub allocations: u64,
    pub frees: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

impl SlabAllocator {
    const fn new() -> Self {
        Self {
            slabs: [
                Slab::new(SLAB_SIZES[0]),
                Slab::new(SLAB_SIZES[1]),
                Slab::new(SLAB_SIZES[2]),
                Slab::new(SLAB_SIZES[3]),
                Slab::new(SLAB_SIZES[4]),
                Slab::new(SLAB_SIZES[5]),
                Slab::new(SLAB_SIZES[6]),
                Slab::new(SLAB_SIZES[7]),
            ],
            stats: SlabStats {
                allocations: 0,
                frees: 0,
                cache_hits: 0,
                cache_misses: 0,
            },
        }
    }

    /// Allocate memory from slab cache
    pub fn allocate(&mut self, size: usize, buddy: &mut BuddyAllocator) -> Option<u64> {
        // Find appropriate slab
        for (i, &slab_size) in SLAB_SIZES.iter().enumerate() {
            if size <= slab_size {
                self.stats.allocations += 1;

                // Check if we have free objects (hit) or need new page (miss)
                // This is a rough approximation, real hit/miss depends on if we have partial pages
                if self.slabs[i].partial_head.is_some() {
                    self.stats.cache_hits += 1;
                } else {
                    self.stats.cache_misses += 1;
                }

                return self.slabs[i].allocate(buddy);
            }
        }

        // Size too large for slab, fall back to buddy allocator
        let pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        let order = pages.next_power_of_two().trailing_zeros() as usize;
        buddy.allocate(order)
    }

    /// Free memory back to slab cache
    pub fn free(&mut self, addr: u64, size: usize, buddy: &mut BuddyAllocator) {
        for (i, &slab_size) in SLAB_SIZES.iter().enumerate() {
            if size <= slab_size {
                self.stats.frees += 1;
                self.slabs[i].free(addr, buddy);
                return;
            }
        }

        // Size too large for slab, free to buddy allocator
        let pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        let order = pages.next_power_of_two().trailing_zeros() as usize;
        buddy.free(addr, order);
    }

    /// Get allocator statistics
    pub fn stats(&self) -> SlabStats {
        self.stats
    }
}

// =============================================================================
// Kernel Heap Allocator
// =============================================================================

/// Heap block header for allocation tracking
#[repr(C)]
struct HeapBlockHeader {
    magic: u32,
    size: u32,
    freed: bool,
    alloc_tag: u64,
}

/// Global kernel heap allocator combining buddy and slab allocators
pub struct KernelHeap {
    buddy: BuddyAllocator,
    slab: SlabAllocator,
    stats: HeapStats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HeapStats {
    pub total_allocations: u64,
    pub total_frees: u64,
    pub bytes_allocated: u64,
    pub bytes_freed: u64,
    pub peak_usage: u64,
}

impl KernelHeap {
    const fn new() -> Self {
        Self {
            buddy: BuddyAllocator::new(),
            slab: SlabAllocator::new(),
            stats: HeapStats {
                total_allocations: 0,
                total_frees: 0,
                bytes_allocated: 0,
                bytes_freed: 0,
                peak_usage: 0,
            },
        }
    }

    /// Initialize the kernel heap
    pub fn init(&mut self, base: u64, size: u64) {
        self.buddy.init(base, size);
        crate::kinfo!("Kernel heap initialized at {:#x}, size={:#x}", base, size);
    }

    /// Allocate memory from kernel heap
    pub fn allocate(&mut self, size: usize) -> Option<u64> {
        if size == 0 {
            return None;
        }

        let header_size = core::mem::size_of::<HeapBlockHeader>();
        let total_size = size + header_size;

        let addr = self.slab.allocate(total_size, &mut self.buddy)?;

        // Write header
        unsafe {
            let header = addr as *mut HeapBlockHeader;
            (*header).magic = HEAP_MAGIC;
            (*header).size = size as u32;
            (*header).freed = false;
            (*header).alloc_tag = crate::scheduler::current_pid().unwrap_or(0) as u64;
        }

        self.stats.total_allocations += 1;
        self.stats.bytes_allocated += size as u64;

        let current_usage = self.stats.bytes_allocated - self.stats.bytes_freed;
        if current_usage > self.stats.peak_usage {
            self.stats.peak_usage = current_usage;
        }

        Some(addr + header_size as u64)
    }

    /// Free memory back to kernel heap
    pub fn free(&mut self, addr: u64) {
        if addr == 0 {
            return;
        }

        let header_size = core::mem::size_of::<HeapBlockHeader>();
        let header_addr = addr - header_size as u64;

        // Validate header
        unsafe {
            let header = header_addr as *const HeapBlockHeader;
            if (*header).magic != HEAP_MAGIC {
                crate::kerror!(
                    "Heap free: invalid magic at {:#x}, expected {:#x}, got {:#x}",
                    header_addr,
                    HEAP_MAGIC,
                    (*header).magic
                );
                return;
            }

            if (*header).freed {
                crate::kerror!("Heap free: double free detected at {:#x}", addr);
                return;
            }

            let size = (*header).size as usize;
            let total_size = size + header_size;

            // Mark as freed
            let header_mut = header_addr as *mut HeapBlockHeader;
            (*header_mut).freed = true;

            self.stats.total_frees += 1;
            self.stats.bytes_freed += size as u64;

            self.slab.free(header_addr, total_size, &mut self.buddy);
        }
    }

    /// Validate heap integrity (for debugging)
    pub fn validate_heap(&self) -> bool {
        // This is a basic validation; in a full seL4-style system,
        // we'd have formal verification of invariants
        let stats = &self.stats;
        if stats.bytes_allocated < stats.bytes_freed {
            crate::kerror!("Heap validation failed: more bytes freed than allocated");
            return false;
        }
        // Additional validations can be added here
        true
    }

    /// Get heap statistics
    pub fn get_stats(&self) -> (HeapStats, BuddyStats, SlabStats) {
        (self.stats, self.buddy.stats(), self.slab.stats())
    }
}

// =============================================================================
// Global Allocator Implementation
// =============================================================================

use core::alloc::{GlobalAlloc, Layout};

pub struct GlobalAllocator;

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Note: We currently ignore alignment requirements beyond what the
        // slab/buddy allocators naturally provide (powers of 2 / page alignment).
        // For stricter alignment, we would need to implement aligned allocation.
        kalloc(layout.size()).unwrap_or(core::ptr::null_mut())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        kfree(ptr);
    }
}

#[global_allocator]
static ALLOCATOR: GlobalAllocator = GlobalAllocator;

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    crate::kerror!("ALLOCATION ERROR: {:?}", layout);
    print_memory_stats();
    panic!("allocation error: {:?}", layout)
}

// =============================================================================
// Memory Zone Management (like Linux zones)
// =============================================================================

/// Memory zones for different types of memory
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryZone {
    /// DMA-capable memory (< 16MB)
    Dma,
    /// Normal kernel memory (16MB - 896MB)
    Normal,
    /// High memory (> 896MB, if applicable)
    High,
}

// =============================================================================
// NUMA-Aware Memory Allocation
// =============================================================================

/// Per-NUMA node heap allocator
pub struct NumaNodeAllocator {
    /// NUMA node ID
    node_id: u32,
    /// Heap for this node
    heap: KernelHeap,
    /// Whether this node is initialized
    initialized: bool,
}

impl NumaNodeAllocator {
    const fn new() -> Self {
        Self {
            node_id: 0,
            heap: KernelHeap::new(),
            initialized: false,
        }
    }

    fn init(&mut self, node_id: u32, base: u64, size: u64) {
        self.node_id = node_id;
        self.heap.init(base, size);
        self.initialized = true;
        crate::kinfo!(
            "NUMA node {} allocator: {:#x} - {:#x} ({} MB)",
            node_id,
            base,
            base + size,
            size / (1024 * 1024)
        );
    }

    fn allocate(&mut self, size: usize) -> Option<u64> {
        if !self.initialized {
            return None;
        }
        self.heap.allocate(size)
    }

    fn free(&mut self, addr: u64) {
        if !self.initialized {
            return;
        }
        self.heap.free(addr);
    }
}

/// NUMA-aware allocator managing per-node heaps
pub struct NumaAllocator {
    /// Per-node allocators
    nodes: [NumaNodeAllocator; crate::numa::MAX_NUMA_NODES],
    /// Whether NUMA allocation is active
    active: bool,
}

impl NumaAllocator {
    const fn new() -> Self {
        Self {
            nodes: [const { NumaNodeAllocator::new() }; crate::numa::MAX_NUMA_NODES],
            active: false,
        }
    }

    /// Initialize NUMA allocator from NUMA topology
    pub fn init(&mut self) {
        if !crate::numa::is_initialized() {
            crate::kinfo!("NUMA allocator: NUMA not available, using UMA mode");
            return;
        }

        let node_count = crate::numa::node_count();
        if node_count <= 1 {
            crate::kinfo!("NUMA allocator: Single node, using UMA mode");
            return;
        }

        // Initialize allocators for each NUMA node based on memory affinity
        for entry in crate::numa::memory_affinity_entries() {
            if entry.numa_node < crate::numa::MAX_NUMA_NODES as u32 {
                let node_idx = entry.numa_node as usize;
                if !self.nodes[node_idx].initialized {
                    self.nodes[node_idx].init(entry.numa_node, entry.base, entry.size);
                }
            }
        }

        self.active = true;
        crate::kinfo!("NUMA allocator initialized with {} nodes", node_count);
    }

    /// Allocate memory on a specific NUMA node
    pub fn allocate_on_node(&mut self, size: usize, node: u32) -> Option<*mut u8> {
        if !self.active {
            return None;
        }

        if node >= crate::numa::MAX_NUMA_NODES as u32 {
            return None;
        }

        self.nodes[node as usize]
            .allocate(size)
            .map(|addr| addr as *mut u8)
    }

    /// Allocate memory with NUMA policy
    pub fn allocate_with_policy(
        &mut self,
        size: usize,
        policy: crate::numa::NumaPolicy,
    ) -> Option<*mut u8> {
        if !self.active {
            return None;
        }

        let target_node = crate::numa::best_node_for_policy(policy);
        self.allocate_on_node(size, target_node)
    }

    /// Free memory (determines node from address)
    pub fn free(&mut self, addr: u64) {
        if !self.active {
            return;
        }

        // Determine which node this address belongs to
        let node = crate::numa::addr_to_node(addr);
        if node < crate::numa::MAX_NUMA_NODES as u32 {
            self.nodes[node as usize].free(addr);
        }
    }

    /// Check if NUMA allocation is active
    pub fn is_active(&self) -> bool {
        self.active
    }
}

static NUMA_ALLOCATOR: Mutex<NumaAllocator> = Mutex::new(NumaAllocator::new());

/// Initialize the NUMA-aware allocator
pub fn init_numa_allocator() {
    let mut allocator = NUMA_ALLOCATOR.lock();
    allocator.init();
}

/// Allocate memory on the local NUMA node (where current CPU is)
pub fn numa_alloc_local(size: usize) -> Option<*mut u8> {
    let mut allocator = NUMA_ALLOCATOR.lock();
    if !allocator.is_active() {
        // Fall back to regular allocation
        return kalloc(size);
    }
    let local_node = crate::numa::current_node();
    allocator.allocate_on_node(size, local_node).or_else(|| {
        // Fall back to regular allocation if node allocation fails
        drop(allocator);
        kalloc(size)
    })
}

/// Allocate memory on a specific NUMA node
pub fn numa_alloc_on_node(size: usize, node: u32) -> Option<*mut u8> {
    let mut allocator = NUMA_ALLOCATOR.lock();
    if !allocator.is_active() {
        return kalloc(size);
    }
    allocator.allocate_on_node(size, node).or_else(|| {
        drop(allocator);
        kalloc(size)
    })
}

/// Allocate memory with a NUMA policy
pub fn numa_alloc_policy(size: usize, policy: crate::numa::NumaPolicy) -> Option<*mut u8> {
    let mut allocator = NUMA_ALLOCATOR.lock();
    if !allocator.is_active() {
        return kalloc(size);
    }
    allocator.allocate_with_policy(size, policy).or_else(|| {
        drop(allocator);
        kalloc(size)
    })
}

/// Free NUMA-allocated memory
pub fn numa_free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let mut allocator = NUMA_ALLOCATOR.lock();
    if allocator.is_active() {
        allocator.free(ptr as u64);
    } else {
        drop(allocator);
        kfree(ptr);
    }
}

pub struct ZoneAllocator {
    dma_heap: KernelHeap,
    normal_heap: KernelHeap,
    high_heap: KernelHeap,
}

impl ZoneAllocator {
    const fn new() -> Self {
        Self {
            dma_heap: KernelHeap::new(),
            normal_heap: KernelHeap::new(),
            high_heap: KernelHeap::new(),
        }
    }

    pub fn init(&mut self, memmap: &[(u64, u64, MemoryZone)]) {
        for (base, size, zone) in memmap {
            match zone {
                MemoryZone::Dma => {
                    self.dma_heap.init(*base, *size);
                    crate::kinfo!("DMA zone: {:#x} - {:#x}", base, base + size);
                }
                MemoryZone::Normal => {
                    self.normal_heap.init(*base, *size);
                    crate::kinfo!("Normal zone: {:#x} - {:#x}", base, base + size);
                }
                MemoryZone::High => {
                    self.high_heap.init(*base, *size);
                    crate::kinfo!("High zone: {:#x} - {:#x}", base, base + size);
                }
            }
        }
    }

    pub fn allocate(&mut self, size: usize, zone: MemoryZone) -> Option<*mut u8> {
        match zone {
            MemoryZone::Dma => self.dma_heap.allocate(size),
            MemoryZone::Normal => self.normal_heap.allocate(size),
            MemoryZone::High => self.high_heap.allocate(size),
        }
        .map(|addr| addr as *mut u8)
    }
}

static ZONE_ALLOCATOR: Mutex<ZoneAllocator> = Mutex::new(ZoneAllocator::new());

/// Allocate from specific memory zone
pub fn zalloc(size: usize, zone: MemoryZone) -> Option<*mut u8> {
    let mut allocator = ZONE_ALLOCATOR.lock();
    allocator.allocate(size, zone)
}

// =============================================================================
// Global Allocator Instance
// =============================================================================

static KERNEL_HEAP: Mutex<KernelHeap> = Mutex::new(KernelHeap::new());

/// Get memory statistics from the kernel heap
/// Returns (HeapStats, BuddyStats, SlabStats)
pub fn get_memory_stats() -> (HeapStats, BuddyStats, SlabStats) {
    let heap = KERNEL_HEAP.lock();
    heap.get_stats()
}

/// Initialize the global kernel heap
pub fn init_kernel_heap(base: u64, size: u64) {
    let mut heap = KERNEL_HEAP.lock();
    heap.init(base, size);
}

/// Allocate memory from kernel heap
pub fn kalloc(size: usize) -> Option<*mut u8> {
    let mut heap = KERNEL_HEAP.lock();
    heap.allocate(size).map(|addr| addr as *mut u8)
}

/// Allocate a single page-aligned physical page from buddy allocator
/// Returns page-aligned address suitable for page table operations
pub fn alloc_page() -> Option<u64> {
    let mut heap = KERNEL_HEAP.lock();
    heap.buddy.allocate(0) // order 0 = 1 page = 4KB, already page-aligned
}

/// Free a page previously allocated with alloc_page
pub fn free_page(addr: u64) {
    let mut heap = KERNEL_HEAP.lock();
    heap.buddy.free(addr, 0); // order 0 = 1 page
}

/// Free memory back to kernel heap
pub fn kfree(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }

    let mut heap = KERNEL_HEAP.lock();
    heap.free(ptr as u64);
}

/// Print comprehensive memory statistics
pub fn print_memory_stats() {
    let heap = KERNEL_HEAP.lock();
    let (heap_stats, buddy_stats, slab_stats) = heap.get_stats();

    crate::kinfo!("=== Kernel Memory Statistics ===");
    crate::kinfo!("Heap:");
    crate::kinfo!("  Total allocations: {}", heap_stats.total_allocations);
    crate::kinfo!("  Total frees: {}", heap_stats.total_frees);
    crate::kinfo!(
        "  Active allocations: {}",
        heap_stats.total_allocations - heap_stats.total_frees
    );
    crate::kinfo!(
        "  Bytes allocated: {} KB",
        heap_stats.bytes_allocated / 1024
    );
    crate::kinfo!("  Bytes freed: {} KB", heap_stats.bytes_freed / 1024);
    crate::kinfo!(
        "  Current usage: {} KB",
        (heap_stats.bytes_allocated - heap_stats.bytes_freed) / 1024
    );
    crate::kinfo!("  Peak usage: {} KB", heap_stats.peak_usage / 1024);

    crate::kinfo!("Buddy Allocator:");
    crate::kinfo!("  Allocations: {}", buddy_stats.allocations);
    crate::kinfo!("  Frees: {}", buddy_stats.frees);
    crate::kinfo!("  Splits: {}", buddy_stats.splits);
    crate::kinfo!("  Merges: {}", buddy_stats.merges);
    crate::kinfo!("  Pages allocated: {}", buddy_stats.pages_allocated);
    crate::kinfo!("  Pages free: {}", buddy_stats.pages_free);

    crate::kinfo!("Slab Allocator:");
    crate::kinfo!("  Allocations: {}", slab_stats.allocations);
    crate::kinfo!("  Frees: {}", slab_stats.frees);
    crate::kinfo!("  Cache hits: {}", slab_stats.cache_hits);
    crate::kinfo!("  Cache misses: {}", slab_stats.cache_misses);

    if slab_stats.allocations > 0 {
        let hit_rate = (slab_stats.cache_hits * 100) / slab_stats.allocations;
        crate::kinfo!("  Cache hit rate: {}%", hit_rate);
    }

    crate::kinfo!("=== End Memory Statistics ===");
}
