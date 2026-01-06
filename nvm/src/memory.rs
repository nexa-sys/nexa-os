//! Virtual Memory Subsystem for Testing
//!
//! Provides complete memory emulation:
//! - Physical memory (simulated RAM) using single mmap (QEMU-style)
//! - Memory-mapped I/O regions
//! - Page frame allocation
//! - Memory access tracking for debugging

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

// Linux mmap constants
const MAP_PRIVATE: i32 = 0x02;
const MAP_ANONYMOUS: i32 = 0x20;
const PROT_READ: i32 = 0x1;
const PROT_WRITE: i32 = 0x2;

extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
}

/// Page size constants
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: usize = 12;
pub const LARGE_PAGE_SIZE: usize = 2 * 1024 * 1024; // 2MB

/// Physical address type
pub type PhysAddr = u64;
/// Virtual address type  
pub type VirtAddr = u64;

/// Memory region type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    /// Usable RAM
    Ram,
    /// Alias for Ram (compatibility)
    Usable,
    /// Reserved (firmware, etc.)
    Reserved,
    /// ACPI reclaimable
    AcpiReclaimable,
    /// ACPI NVS
    AcpiNvs,
    /// Memory-mapped I/O
    Mmio,
    /// Kernel code/data
    Kernel,
    /// Bootloader data
    Bootloader,
}

impl MemoryType {
    /// Check if memory is usable (Ram or Usable)
    pub fn is_usable(&self) -> bool {
        matches!(self, MemoryType::Ram | MemoryType::Usable)
    }
}

/// A memory region descriptor
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub start: PhysAddr,
    pub size: usize,
    pub region_type: MemoryType,
}

impl MemoryRegion {
    pub fn new(start: PhysAddr, size: usize, region_type: MemoryType) -> Self {
        Self { start, size, region_type }
    }
    
    pub fn end(&self) -> PhysAddr {
        self.start + self.size as u64
    }
    
    pub fn contains(&self, addr: PhysAddr) -> bool {
        addr >= self.start && addr < self.end()
    }
}

/// Physical memory emulation
///
/// This emulates the actual physical RAM that the kernel sees.
/// Uses a single mmap allocation for the entire RAM region (like QEMU).
/// MMIO regions are handled separately by devices.
pub struct PhysicalMemory {
    /// Single mmap'd region for all RAM (like QEMU's RAMBlock)
    ram_base: *mut u8,
    /// Size of the RAM region
    ram_size: usize,
    /// Memory map (like E820)
    regions: RwLock<Vec<MemoryRegion>>,
    /// Total usable RAM size
    total_size: usize,
    /// Statistics
    stats: Mutex<MemoryStats>,
}

#[derive(Debug, Default, Clone)]
pub struct MemoryStats {
    pub reads: u64,
    pub writes: u64,
    pub page_faults: u64,
    pub allocations: u64,
    pub deallocations: u64,
}

// Safety: The ram_base pointer is valid for the lifetime of PhysicalMemory
// and we use proper synchronization for stats
unsafe impl Send for PhysicalMemory {}
unsafe impl Sync for PhysicalMemory {}

impl PhysicalMemory {
    /// Create physical memory with given size
    /// 
    /// Allocates a single contiguous region via mmap (like QEMU)
    pub fn new(size_mb: usize) -> Self {
        let total_size = size_mb * 1024 * 1024;
        
        // Allocate RAM via mmap (without MAP_FIXED - let OS choose address)
        let ram_base = unsafe {
            mmap(
                std::ptr::null_mut(),
                total_size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        
        let map_failed = usize::MAX as *mut u8;
        if ram_base == map_failed || ram_base.is_null() {
            panic!("Failed to mmap {} MB for physical memory", size_mb);
        }
        
        // Zero the memory
        unsafe {
            std::ptr::write_bytes(ram_base, 0, total_size);
        }
        
        let mut regions = Vec::new();
        
        // Create standard x86 memory map
        // 0x0 - 0x9FFFF: Low memory (640KB)
        regions.push(MemoryRegion::new(0x0, 0xA0000, MemoryType::Ram));
        // 0xA0000 - 0xFFFFF: Video memory and ROM (reserved)
        regions.push(MemoryRegion::new(0xA0000, 0x60000, MemoryType::Reserved));
        // 1MB+ : Main memory
        regions.push(MemoryRegion::new(0x100000, total_size - 0x100000, MemoryType::Ram));
        
        Self {
            ram_base,
            ram_size: total_size,
            regions: RwLock::new(regions),
            total_size,
            stats: Mutex::new(MemoryStats::default()),
        }
    }
    
    /// Create with custom memory map
    pub fn with_regions(regions: Vec<MemoryRegion>) -> Self {
        let total_size: usize = regions.iter()
            .filter(|r| r.region_type == MemoryType::Ram)
            .map(|r| r.size)
            .sum();
        
        // Allocate RAM via mmap
        let ram_base = unsafe {
            mmap(
                std::ptr::null_mut(),
                total_size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        
        let map_failed = usize::MAX as *mut u8;
        if ram_base == map_failed || ram_base.is_null() {
            panic!("Failed to mmap {} bytes for physical memory", total_size);
        }
        
        unsafe {
            std::ptr::write_bytes(ram_base, 0, total_size);
        }
            
        Self {
            ram_base,
            ram_size: total_size,
            regions: RwLock::new(regions),
            total_size,
            stats: Mutex::new(MemoryStats::default()),
        }
    }
    
    /// Get memory map (like E820)
    pub fn memory_map(&self) -> Vec<MemoryRegion> {
        self.regions.read().unwrap().clone()
    }
    
    /// Get total RAM size
    pub fn total_size(&self) -> usize {
        self.total_size
    }
    
    /// Get statistics
    pub fn stats(&self) -> MemoryStats {
        self.stats.lock().unwrap().clone()
    }
    
    /// Get pointer to physical address (bounds checked)
    #[inline]
    fn get_ptr(&self, addr: PhysAddr) -> *mut u8 {
        if (addr as usize) >= self.ram_size {
            panic!("Physical address {:#x} out of bounds (max {:#x})", addr, self.ram_size);
        }
        unsafe { self.ram_base.add(addr as usize) }
    }
    
    /// Read a byte from physical memory
    #[inline]
    pub fn read_u8(&self, addr: PhysAddr) -> u8 {
        self.stats.lock().unwrap().reads += 1;
        unsafe { *self.get_ptr(addr) }
    }
    
    /// Write a byte to physical memory
    #[inline]
    pub fn write_u8(&self, addr: PhysAddr, value: u8) {
        self.stats.lock().unwrap().writes += 1;
        unsafe { *self.get_ptr(addr) = value; }
    }
    
    /// Read a 16-bit value
    #[inline]
    pub fn read_u16(&self, addr: PhysAddr) -> u16 {
        let lo = self.read_u8(addr) as u16;
        let hi = self.read_u8(addr + 1) as u16;
        lo | (hi << 8)
    }
    
    /// Write a 16-bit value
    #[inline]
    pub fn write_u16(&self, addr: PhysAddr, value: u16) {
        self.write_u8(addr, value as u8);
        self.write_u8(addr + 1, (value >> 8) as u8);
    }
    
    /// Read a 32-bit value
    pub fn read_u32(&self, addr: PhysAddr) -> u32 {
        let b0 = self.read_u8(addr) as u32;
        let b1 = self.read_u8(addr + 1) as u32;
        let b2 = self.read_u8(addr + 2) as u32;
        let b3 = self.read_u8(addr + 3) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }
    
    /// Write a 32-bit value
    pub fn write_u32(&self, addr: PhysAddr, value: u32) {
        self.write_u8(addr, value as u8);
        self.write_u8(addr + 1, (value >> 8) as u8);
        self.write_u8(addr + 2, (value >> 16) as u8);
        self.write_u8(addr + 3, (value >> 24) as u8);
    }
    
    /// Read a 64-bit value
    pub fn read_u64(&self, addr: PhysAddr) -> u64 {
        let lo = self.read_u32(addr) as u64;
        let hi = self.read_u32(addr + 4) as u64;
        lo | (hi << 32)
    }
    
    /// Write a 64-bit value
    pub fn write_u64(&self, addr: PhysAddr, value: u64) {
        self.write_u32(addr, value as u32);
        self.write_u32(addr + 4, (value >> 32) as u32);
    }
    
    /// Read a slice of bytes (optimized for bulk reads)
    #[inline]
    pub fn read_bytes(&self, addr: PhysAddr, buf: &mut [u8]) {
        if (addr as usize) + buf.len() > self.ram_size {
            panic!("read_bytes: address range {:#x}-{:#x} out of bounds", 
                   addr, addr + buf.len() as u64);
        }
        self.stats.lock().unwrap().reads += buf.len() as u64;
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.ram_base.add(addr as usize),
                buf.as_mut_ptr(),
                buf.len()
            );
        }
    }
    
    /// Write a slice of bytes (optimized for bulk writes)
    #[inline]
    pub fn write_bytes(&self, addr: PhysAddr, data: &[u8]) {
        if (addr as usize) + data.len() > self.ram_size {
            panic!("write_bytes: address range {:#x}-{:#x} out of bounds", 
                   addr, addr + data.len() as u64);
        }
        self.stats.lock().unwrap().writes += data.len() as u64;
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                self.ram_base.add(addr as usize),
                data.len()
            );
        }
    }
    
    /// Get a mutable slice of physical memory
    /// 
    /// # Safety
    /// This provides direct access to memory. Caller must ensure:
    /// - No concurrent access to the same region
    /// - Proper bounds checking
    #[inline]
    pub fn as_slice_mut(&self, addr: usize, len: usize) -> &mut [u8] {
        if addr + len > self.ram_size {
            panic!("as_slice_mut: address range {:#x}-{:#x} out of bounds", 
                   addr, addr + len);
        }
        unsafe {
            std::slice::from_raw_parts_mut(self.ram_base.add(addr), len)
        }
    }
    
    /// Get raw pointer to physical address (for direct access)
    /// 
    /// # Safety
    /// Caller must ensure proper synchronization and bounds checking
    #[inline]
    pub unsafe fn raw_ptr(&self, addr: PhysAddr) -> *mut u8 {
        self.get_ptr(addr)
    }
    
    /// Get the base pointer and size (for advanced usage like snapshotting)
    pub fn ram_region(&self) -> (*mut u8, usize) {
        (self.ram_base, self.ram_size)
    }
}

impl Drop for PhysicalMemory {
    fn drop(&mut self) {
        // Free the mmap'd region
        if !self.ram_base.is_null() {
            unsafe { munmap(self.ram_base, self.ram_size); }
        }
    }
}

/// Virtual memory translation
/// 
/// Provides 4-level page table walking for virtual address translation.
pub struct VirtualMemory {
    phys_mem: Arc<PhysicalMemory>,
    cr3: Mutex<u64>,
}

impl VirtualMemory {
    pub fn new(phys_mem: Arc<PhysicalMemory>) -> Self {
        Self {
            phys_mem,
            cr3: Mutex::new(0),
        }
    }
    
    /// Set CR3 (page table base)
    pub fn set_cr3(&self, cr3: u64) {
        *self.cr3.lock().unwrap() = cr3;
    }
    
    /// Get current CR3
    pub fn get_cr3(&self) -> u64 {
        *self.cr3.lock().unwrap()
    }
    
    /// Translate virtual to physical address using 4-level paging
    pub fn translate(&self, virt: VirtAddr) -> Option<PhysAddr> {
        let cr3 = self.get_cr3();
        if cr3 == 0 {
            // Identity mapping if paging not set up
            return Some(virt);
        }
        
        // 4-level page table indices
        let pml4_idx = ((virt >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virt >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt >> 12) & 0x1FF) as usize;
        let offset = (virt & 0xFFF) as usize;
        
        // Walk page tables
        let pml4_addr = cr3 & !0xFFF;
        let pml4e = self.phys_mem.read_u64(pml4_addr + (pml4_idx * 8) as u64);
        if pml4e & 1 == 0 { return None; } // Not present
        
        let pdpt_addr = pml4e & !0xFFF;
        let pdpte = self.phys_mem.read_u64(pdpt_addr + (pdpt_idx * 8) as u64);
        if pdpte & 1 == 0 { return None; }
        if pdpte & 0x80 != 0 {
            // 1GB page
            return Some((pdpte & !0x3FFFFFFF) | (virt & 0x3FFFFFFF));
        }
        
        let pd_addr = pdpte & !0xFFF;
        let pde = self.phys_mem.read_u64(pd_addr + (pd_idx * 8) as u64);
        if pde & 1 == 0 { return None; }
        if pde & 0x80 != 0 {
            // 2MB page
            return Some((pde & !0x1FFFFF) | (virt & 0x1FFFFF));
        }
        
        let pt_addr = pde & !0xFFF;
        let pte = self.phys_mem.read_u64(pt_addr + (pt_idx * 8) as u64);
        if pte & 1 == 0 { return None; }
        
        Some((pte & !0xFFF) | offset as u64)
    }
    
    /// Read from virtual address
    pub fn read_u8(&self, virt: VirtAddr) -> Option<u8> {
        self.translate(virt).map(|phys| self.phys_mem.read_u8(phys))
    }
    
    /// Write to virtual address
    pub fn write_u8(&self, virt: VirtAddr, value: u8) -> bool {
        if let Some(phys) = self.translate(virt) {
            self.phys_mem.write_u8(phys, value);
            true
        } else {
            false
        }
    }
}

/// Mock page frame allocator
/// 
/// This allocates page-aligned memory from host system and tracks allocations.
/// Unlike the kernel's allocator, this doesn't manage physical memory map - 
/// it just provides page-aligned allocations.
pub struct MockPageAllocator {
    allocations: Mutex<HashMap<usize, usize>>, // address -> size
    total_allocated: Mutex<usize>,
    page_size: usize,
}

impl MockPageAllocator {
    pub const DEFAULT_PAGE_SIZE: usize = 4096;

    pub fn new() -> Self {
        Self::with_page_size(Self::DEFAULT_PAGE_SIZE)
    }

    pub fn with_page_size(page_size: usize) -> Self {
        Self {
            allocations: Mutex::new(HashMap::new()),
            total_allocated: Mutex::new(0),
            page_size,
        }
    }

    /// Allocate pages
    pub fn alloc_pages(&self, count: usize) -> Option<*mut u8> {
        let size = count * self.page_size;
        let layout = Layout::from_size_align(size, self.page_size).ok()?;
        
        let ptr = unsafe { alloc_zeroed(layout) };
        if ptr.is_null() {
            return None;
        }
        
        let addr = ptr as usize;
        let mut allocations = self.allocations.lock().unwrap();
        allocations.insert(addr, size);
        
        let mut total = self.total_allocated.lock().unwrap();
        *total += size;
        
        Some(ptr)
    }

    /// Free pages
    pub fn free_pages(&self, ptr: *mut u8, count: usize) {
        let addr = ptr as usize;
        let size = count * self.page_size;
        
        let mut allocations = self.allocations.lock().unwrap();
        if allocations.remove(&addr).is_some() {
            let layout = Layout::from_size_align(size, self.page_size).unwrap();
            unsafe { dealloc(ptr, layout) };
            
            let mut total = self.total_allocated.lock().unwrap();
            *total = total.saturating_sub(size);
        }
    }

    /// Get total allocated memory
    pub fn total_allocated(&self) -> usize {
        *self.total_allocated.lock().unwrap()
    }

    /// Get number of allocations
    pub fn allocation_count(&self) -> usize {
        self.allocations.lock().unwrap().len()
    }

    /// Check for memory leaks
    pub fn check_leaks(&self) -> bool {
        self.allocation_count() == 0
    }
}

impl Default for MockPageAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MockPageAllocator {
    fn drop(&mut self) {
        // Clean up any remaining allocations
        let allocations = self.allocations.get_mut().unwrap();
        for (&addr, &size) in allocations.iter() {
            let layout = Layout::from_size_align(size, self.page_size).unwrap();
            unsafe { dealloc(addr as *mut u8, layout) };
        }
    }
}

// =============================================================================
// AddressSpace - Memory bus with MMIO routing
// =============================================================================

/// MMIO handler trait for devices that respond to memory-mapped I/O
pub trait MmioHandler: Send + Sync {
    fn read(&self, offset: usize, size: u8) -> u64;
    fn write(&self, offset: usize, size: u8, value: u64);
    
    /// Bulk fill operation for REP STOS optimization
    /// Default implementation falls back to individual writes
    /// Returns number of units actually written
    fn fill(&self, offset: usize, value: u64, count: usize, unit_size: u8) -> usize {
        for i in 0..count {
            self.write(offset + i * (unit_size as usize), unit_size, value);
        }
        count
    }
}

/// MMIO region registration
struct MmioRegion {
    start: PhysAddr,
    size: usize,
    handler: Arc<dyn MmioHandler>,
}

/// Address space that routes memory accesses to RAM or MMIO devices
/// 
/// This is the memory bus - it receives all memory accesses and routes them
/// to either physical RAM or the appropriate MMIO device based on address.
pub struct AddressSpace {
    /// Physical RAM
    ram: Arc<PhysicalMemory>,
    /// Registered MMIO regions (sorted by start address)
    mmio_regions: RwLock<Vec<MmioRegion>>,
}

impl AddressSpace {
    /// Create a new address space with the given physical memory
    pub fn new(ram: Arc<PhysicalMemory>) -> Self {
        Self {
            ram,
            mmio_regions: RwLock::new(Vec::new()),
        }
    }
    
    /// Register an MMIO region
    pub fn register_mmio(&self, start: PhysAddr, size: usize, handler: Arc<dyn MmioHandler>) {
        let mut regions = self.mmio_regions.write().unwrap();
        regions.push(MmioRegion { start, size, handler });
        // Keep sorted for binary search
        regions.sort_by_key(|r| r.start);
    }
    
    /// Find MMIO handler for address, returns (handler, offset within region)
    fn find_mmio(&self, addr: PhysAddr) -> Option<(Arc<dyn MmioHandler>, usize)> {
        let regions = self.mmio_regions.read().unwrap();
        for region in regions.iter() {
            if addr >= region.start && addr < region.start + region.size as u64 {
                let offset = (addr - region.start) as usize;
                return Some((region.handler.clone(), offset));
            }
        }
        None
    }
    
    /// Read a byte
    #[inline]
    pub fn read_u8(&self, addr: PhysAddr) -> u8 {
        if let Some((handler, offset)) = self.find_mmio(addr) {
            handler.read(offset, 1) as u8
        } else {
            self.ram.read_u8(addr)
        }
    }
    
    /// Write a byte
    #[inline]
    pub fn write_u8(&self, addr: PhysAddr, value: u8) {
        if let Some((handler, offset)) = self.find_mmio(addr) {
            handler.write(offset, 1, value as u64);
        } else {
            self.ram.write_u8(addr, value);
        }
    }
    
    /// Read 16-bit value
    #[inline]
    pub fn read_u16(&self, addr: PhysAddr) -> u16 {
        if let Some((handler, offset)) = self.find_mmio(addr) {
            handler.read(offset, 2) as u16
        } else {
            self.ram.read_u16(addr)
        }
    }
    
    /// Write 16-bit value
    #[inline]
    pub fn write_u16(&self, addr: PhysAddr, value: u16) {
        if let Some((handler, offset)) = self.find_mmio(addr) {
            handler.write(offset, 2, value as u64);
        } else {
            self.ram.write_u16(addr, value);
        }
    }
    
    /// Read 32-bit value
    #[inline]
    pub fn read_u32(&self, addr: PhysAddr) -> u32 {
        if let Some((handler, offset)) = self.find_mmio(addr) {
            handler.read(offset, 4) as u32
        } else {
            self.ram.read_u32(addr)
        }
    }
    
    /// Write 32-bit value
    #[inline]
    pub fn write_u32(&self, addr: PhysAddr, value: u32) {
        if let Some((handler, offset)) = self.find_mmio(addr) {
            handler.write(offset, 4, value as u64);
        } else {
            self.ram.write_u32(addr, value);
        }
    }
    
    /// Read 64-bit value
    #[inline]
    pub fn read_u64(&self, addr: PhysAddr) -> u64 {
        if let Some((handler, offset)) = self.find_mmio(addr) {
            handler.read(offset, 8)
        } else {
            self.ram.read_u64(addr)
        }
    }
    
    /// Write 64-bit value
    #[inline]
    pub fn write_u64(&self, addr: PhysAddr, value: u64) {
        if let Some((handler, offset)) = self.find_mmio(addr) {
            handler.write(offset, 8, value);
        } else {
            self.ram.write_u64(addr, value);
        }
    }
    
    /// Bulk fill memory with a repeated value (optimized for REP STOS)
    /// 
    /// This is significantly faster than individual writes because:
    /// 1. Single MMIO region lookup instead of per-write lookup
    /// 2. Uses MMIO handler's optimized fill() method if available
    /// 3. For RAM, uses memset-style bulk fill
    /// 
    /// Returns number of units written
    #[inline]
    pub fn fill(&self, start_addr: PhysAddr, value: u64, count: usize, unit_size: u8) -> usize {
        if count == 0 {
            return 0;
        }
        
        let total_bytes = count * (unit_size as usize);
        let end_addr = start_addr + total_bytes as u64;
        
        // Check if entire range falls within a single MMIO region
        if let Some((handler, offset)) = self.find_mmio(start_addr) {
            // Verify the entire range is within this MMIO region
            // (check end - 1 to handle exact boundary case)
            if let Some((handler2, _)) = self.find_mmio(end_addr.saturating_sub(1)) {
                if Arc::ptr_eq(&handler, &handler2) {
                    // Entire range is in same MMIO region - use bulk fill
                    return handler.fill(offset, value, count, unit_size);
                }
            }
            // Range crosses regions - fall back to individual writes
            for i in 0..count {
                let addr = start_addr + (i * unit_size as usize) as u64;
                if let Some((h, off)) = self.find_mmio(addr) {
                    h.write(off, unit_size, value);
                } else {
                    match unit_size {
                        1 => self.ram.write_u8(addr, value as u8),
                        2 => self.ram.write_u16(addr, value as u16),
                        4 => self.ram.write_u32(addr, value as u32),
                        8 => self.ram.write_u64(addr, value),
                        _ => {}
                    }
                }
            }
            return count;
        }
        
        // No MMIO - fast path for RAM
        // Use optimized bulk fill based on unit size
        let slice = self.ram.as_slice_mut(start_addr as usize, total_bytes);
        match unit_size {
            1 => {
                // Simple memset
                slice.fill(value as u8);
            }
            2 => {
                let val = value as u16;
                for chunk in slice.chunks_exact_mut(2) {
                    chunk.copy_from_slice(&val.to_le_bytes());
                }
            }
            4 => {
                let val = value as u32;
                for chunk in slice.chunks_exact_mut(4) {
                    chunk.copy_from_slice(&val.to_le_bytes());
                }
            }
            8 => {
                for chunk in slice.chunks_exact_mut(8) {
                    chunk.copy_from_slice(&value.to_le_bytes());
                }
            }
            _ => {
                // Fallback for unusual sizes
                for i in 0..count {
                    let offset = i * (unit_size as usize);
                    let bytes = &value.to_le_bytes()[..unit_size as usize];
                    slice[offset..offset + unit_size as usize].copy_from_slice(bytes);
                }
            }
        }
        count
    }
    
    /// Get underlying physical memory (for firmware loading, etc.)
    pub fn ram(&self) -> &Arc<PhysicalMemory> {
        &self.ram
    }
    
    /// Get raw RAM pointer for JIT native code execution
    /// 
    /// # Safety
    /// This bypasses MMIO routing - native code using this pointer
    /// will NOT trigger MMIO handlers. Use only when you're sure
    /// the accessed addresses are not MMIO regions.
    pub fn ram_ptr(&self) -> *mut PhysicalMemory {
        Arc::as_ptr(&self.ram) as *mut PhysicalMemory
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_physical_memory_basic() {
        let mem = PhysicalMemory::new(64); // 64MB
        
        mem.write_u32(0x1000, 0xDEADBEEF);
        assert_eq!(mem.read_u32(0x1000), 0xDEADBEEF);
        
        mem.write_u64(0x2000, 0x123456789ABCDEF0);
        assert_eq!(mem.read_u64(0x2000), 0x123456789ABCDEF0);
    }
    
    #[test]
    fn test_physical_memory_cross_page() {
        let mem = PhysicalMemory::new(64);
        
        // Write at page boundary - 2 bytes before, 2 bytes after
        mem.write_u32(0xFFE, 0xAABBCCDD);
        assert_eq!(mem.read_u32(0xFFE), 0xAABBCCDD);
    }
    
    #[test]
    fn test_physical_memory_bytes() {
        let mem = PhysicalMemory::new(64);
        
        let data = b"Hello, NexaOS!";
        mem.write_bytes(0x5000, data);
        
        let mut buf = [0u8; 14];
        mem.read_bytes(0x5000, &mut buf);
        assert_eq!(&buf, data);
    }
    
    #[test]
    fn test_memory_map() {
        let mem = PhysicalMemory::new(128);
        let map = mem.memory_map();
        
        // Should have low memory, reserved, and main memory regions
        assert!(map.len() >= 3);
        assert_eq!(map[0].region_type, MemoryType::Ram);
    }
    
    #[test]
    fn test_mock_allocator_basic() {
        let allocator = MockPageAllocator::new();
        
        let ptr = allocator.alloc_pages(1);
        assert!(ptr.is_some());
        assert_eq!(allocator.allocation_count(), 1);
        assert_eq!(allocator.total_allocated(), 4096);
        
        allocator.free_pages(ptr.unwrap(), 1);
        assert_eq!(allocator.allocation_count(), 0);
        assert_eq!(allocator.total_allocated(), 0);
    }

    #[test]
    fn test_mock_allocator_multiple() {
        let allocator = MockPageAllocator::new();
        
        let ptr1 = allocator.alloc_pages(2);
        let ptr2 = allocator.alloc_pages(3);
        
        assert!(ptr1.is_some());
        assert!(ptr2.is_some());
        assert_eq!(allocator.allocation_count(), 2);
        assert_eq!(allocator.total_allocated(), 5 * 4096);
        
        allocator.free_pages(ptr1.unwrap(), 2);
        allocator.free_pages(ptr2.unwrap(), 3);
        
        assert!(allocator.check_leaks());
    }
}
