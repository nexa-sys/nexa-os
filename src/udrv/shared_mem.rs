//! Shared Memory Regions for User-space Driver Framework
//!
//! Implements shared memory regions for efficient data transfer between
//! kernel and user-space drivers without copying.
//!
//! # Design Philosophy
//!
//! Shared memory enables:
//! - Zero-copy data transfer
//! - Ring buffers for packet/block I/O
//! - Control/data plane communication
//! - DMA buffer sharing

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

/// Shared region ID type
pub type SharedRegionId = u32;

/// Maximum shared regions
pub const MAX_SHARED_REGIONS: usize = 256;

/// Shared memory region
#[derive(Debug, Clone)]
pub struct SharedRegion {
    /// Region ID
    pub id: SharedRegionId,
    /// Physical address
    pub phys_addr: u64,
    /// Size in bytes
    pub size: u64,
    /// Region type
    pub region_type: SharedRegionType,
    /// Access permissions
    pub access: SharedAccess,
    /// Owner (driver/container ID)
    pub owner: u32,
    /// Mapped clients
    pub clients: Vec<SharedClient>,
    /// Flags
    pub flags: u32,
}

/// Shared region types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SharedRegionType {
    /// General purpose shared memory
    General = 0,
    /// Ring buffer for packet I/O
    RingBuffer = 1,
    /// DMA buffer
    DmaBuffer = 2,
    /// Control message buffer
    ControlBuffer = 3,
    /// Frame buffer (display)
    FrameBuffer = 4,
}

/// Access permissions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SharedAccess {
    /// Read-only
    ReadOnly = 0,
    /// Write-only
    WriteOnly = 1,
    /// Read-write
    ReadWrite = 2,
}

/// Shared region client
#[derive(Debug, Clone, Copy)]
pub struct SharedClient {
    /// Client ID (driver/container/process)
    pub client_id: u32,
    /// Virtual address in client's space
    pub virt_addr: u64,
    /// Access granted
    pub access: SharedAccess,
}

/// Shared region flags
pub mod shared_flags {
    /// Region is cacheable
    pub const CACHEABLE: u32 = 1 << 0;
    /// Region supports prefetch
    pub const PREFETCH: u32 = 1 << 1;
    /// Region is DMA coherent
    pub const DMA_COHERENT: u32 = 1 << 2;
    /// Region is locked (no swap)
    pub const LOCKED: u32 = 1 << 3;
    /// Region is contiguous physical memory
    pub const CONTIGUOUS: u32 = 1 << 4;
}

/// Shared memory error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedMemError {
    /// Table full
    TableFull,
    /// Region not found
    NotFound,
    /// Permission denied
    PermissionDenied,
    /// Invalid size
    InvalidSize,
    /// Already mapped
    AlreadyMapped,
    /// Not mapped
    NotMapped,
    /// Allocation failed
    AllocationFailed,
}

// Global state
static REGIONS: Mutex<[Option<SharedRegion>; MAX_SHARED_REGIONS]> =
    Mutex::new([const { None }; MAX_SHARED_REGIONS]);
static NEXT_REGION_ID: AtomicU32 = AtomicU32::new(1);

/// Initialize shared memory subsystem
pub fn init() {
    crate::kinfo!("UDRV/SharedMem: Initializing shared memory subsystem");
    crate::kinfo!("UDRV/SharedMem: {} max shared regions", MAX_SHARED_REGIONS);
}

/// Create a shared memory region
pub fn create(
    size: u64,
    region_type: SharedRegionType,
    access: SharedAccess,
    owner: u32,
    flags: u32,
) -> Result<SharedRegionId, SharedMemError> {
    if size == 0 || size > (1 << 30) {
        // Max 1GB
        return Err(SharedMemError::InvalidSize);
    }

    // Allocate physical memory (would use actual allocator)
    let phys_addr = allocate_physical(size, flags)?;

    let mut regions = REGIONS.lock();

    for slot in regions.iter_mut() {
        if slot.is_none() {
            let id = NEXT_REGION_ID.fetch_add(1, Ordering::SeqCst);

            *slot = Some(SharedRegion {
                id,
                phys_addr,
                size,
                region_type,
                access,
                owner,
                clients: Vec::new(),
                flags,
            });

            crate::kinfo!(
                "UDRV/SharedMem: Created region {} ({} bytes, type {:?})",
                id,
                size,
                region_type
            );

            return Ok(id);
        }
    }

    // Free allocated memory on failure
    free_physical(phys_addr, size);
    Err(SharedMemError::TableFull)
}

/// Map region to a client
pub fn map_to_client(
    id: SharedRegionId,
    client_id: u32,
    access: SharedAccess,
) -> Result<u64, SharedMemError> {
    let mut regions = REGIONS.lock();
    let region = regions
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|r| r.id == id))
        .ok_or(SharedMemError::NotFound)?;

    // Check access permissions
    if !can_grant_access(region.access, access) {
        return Err(SharedMemError::PermissionDenied);
    }

    // Check if already mapped
    if region.clients.iter().any(|c| c.client_id == client_id) {
        return Err(SharedMemError::AlreadyMapped);
    }

    // Map to client's address space (would use actual mapping)
    let virt_addr = map_to_address_space(client_id, region.phys_addr, region.size, access)?;

    region.clients.push(SharedClient {
        client_id,
        virt_addr,
        access,
    });

    crate::kinfo!(
        "UDRV/SharedMem: Mapped region {} to client {} at {:#x}",
        id,
        client_id,
        virt_addr
    );

    Ok(virt_addr)
}

/// Unmap region from a client
pub fn unmap_from_client(id: SharedRegionId, client_id: u32) -> Result<(), SharedMemError> {
    let mut regions = REGIONS.lock();
    let region = regions
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|r| r.id == id))
        .ok_or(SharedMemError::NotFound)?;

    let idx = region
        .clients
        .iter()
        .position(|c| c.client_id == client_id)
        .ok_or(SharedMemError::NotMapped)?;

    let client = region.clients.remove(idx);

    // Unmap from address space (would use actual unmapping)
    unmap_from_address_space(client_id, client.virt_addr, region.size)?;

    crate::kinfo!(
        "UDRV/SharedMem: Unmapped region {} from client {}",
        id,
        client_id
    );

    Ok(())
}

/// Destroy a shared region
pub fn destroy(id: SharedRegionId) -> Result<(), SharedMemError> {
    let mut regions = REGIONS.lock();

    for slot in regions.iter_mut() {
        if let Some(region) = slot {
            if region.id == id {
                // Unmap from all clients
                for client in &region.clients {
                    let _ =
                        unmap_from_address_space(client.client_id, client.virt_addr, region.size);
                }

                // Free physical memory
                free_physical(region.phys_addr, region.size);

                crate::kinfo!("UDRV/SharedMem: Destroyed region {}", id);

                *slot = None;
                return Ok(());
            }
        }
    }

    Err(SharedMemError::NotFound)
}

/// Get region info
pub fn get_info(id: SharedRegionId) -> Option<SharedRegionInfo> {
    let regions = REGIONS.lock();
    let region = regions
        .iter()
        .find_map(|slot| slot.as_ref().filter(|r| r.id == id))?;

    Some(SharedRegionInfo {
        id: region.id,
        phys_addr: region.phys_addr,
        size: region.size,
        region_type: region.region_type,
        access: region.access,
        owner: region.owner,
        client_count: region.clients.len(),
        flags: region.flags,
    })
}

/// Shared region info (read-only view)
#[derive(Debug, Clone)]
pub struct SharedRegionInfo {
    pub id: SharedRegionId,
    pub phys_addr: u64,
    pub size: u64,
    pub region_type: SharedRegionType,
    pub access: SharedAccess,
    pub owner: u32,
    pub client_count: usize,
    pub flags: u32,
}

/// List all regions
pub fn list_regions() -> Vec<SharedRegionId> {
    let regions = REGIONS.lock();
    regions
        .iter()
        .filter_map(|slot| slot.as_ref().map(|r| r.id))
        .collect()
}

/// List regions for owner
pub fn list_for_owner(owner: u32) -> Vec<SharedRegionId> {
    let regions = REGIONS.lock();
    regions
        .iter()
        .filter_map(|slot| {
            slot.as_ref()
                .and_then(|r| if r.owner == owner { Some(r.id) } else { None })
        })
        .collect()
}

// ---- Helper functions ----

fn can_grant_access(region_access: SharedAccess, requested: SharedAccess) -> bool {
    match region_access {
        SharedAccess::ReadWrite => true, // Can grant any access
        SharedAccess::ReadOnly => requested == SharedAccess::ReadOnly,
        SharedAccess::WriteOnly => requested == SharedAccess::WriteOnly,
    }
}

/// Allocate physical memory for shared region
fn allocate_physical(size: u64, flags: u32) -> Result<u64, SharedMemError> {
    // In real implementation, this would use the physical memory allocator
    // For now, use a simple bump allocator simulation
    static NEXT_PHYS: AtomicU32 = AtomicU32::new(0x2000_0000); // Start at 512MB

    let aligned_size = (size + 0xFFF) & !0xFFF; // Page align
    let addr = NEXT_PHYS.fetch_add(aligned_size as u32, Ordering::SeqCst) as u64;

    // Check if we exceeded available memory (simplified check)
    if addr + aligned_size > 0x4000_0000 {
        // 1GB limit
        return Err(SharedMemError::AllocationFailed);
    }

    Ok(addr)
}

/// Free physical memory
fn free_physical(_addr: u64, _size: u64) {
    // In real implementation, return to allocator
}

/// Map physical memory to client's address space
fn map_to_address_space(
    _client_id: u32,
    phys_addr: u64,
    _size: u64,
    _access: SharedAccess,
) -> Result<u64, SharedMemError> {
    // In real implementation, this would:
    // 1. Get client's page table
    // 2. Find free virtual address range
    // 3. Map physical pages with appropriate permissions

    // For now, use direct mapping
    Ok(phys_addr + 0xFFFF_8000_0000_0000) // Direct map offset
}

/// Unmap from client's address space
fn unmap_from_address_space(
    _client_id: u32,
    _virt_addr: u64,
    _size: u64,
) -> Result<(), SharedMemError> {
    // In real implementation, this would:
    // 1. Get client's page table
    // 2. Unmap the virtual address range
    // 3. Flush TLB

    Ok(())
}

// ---- Ring buffer helpers ----

/// Ring buffer header structure (placed at start of shared region)
#[repr(C)]
pub struct RingBufferHeader {
    /// Magic number for validation
    pub magic: u32,
    /// Head index (consumer)
    pub head: u32,
    /// Tail index (producer)
    pub tail: u32,
    /// Number of entries
    pub entries: u32,
    /// Entry size in bytes
    pub entry_size: u32,
    /// Flags
    pub flags: u32,
    /// Reserved for alignment
    pub _reserved: [u32; 2],
}

impl RingBufferHeader {
    pub const MAGIC: u32 = 0x52494E47; // "RING"

    pub fn init(&mut self, entries: u32, entry_size: u32) {
        self.magic = Self::MAGIC;
        self.head = 0;
        self.tail = 0;
        self.entries = entries;
        self.entry_size = entry_size;
        self.flags = 0;
        self._reserved = [0; 2];
    }

    pub fn is_valid(&self) -> bool {
        self.magic == Self::MAGIC
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    pub fn is_full(&self) -> bool {
        ((self.tail + 1) % self.entries) == self.head
    }

    pub fn len(&self) -> u32 {
        (self.tail + self.entries - self.head) % self.entries
    }

    pub fn data_offset(&self) -> u32 {
        core::mem::size_of::<Self>() as u32
    }
}

/// Create a ring buffer shared region
pub fn create_ring_buffer(
    entries: u32,
    entry_size: u32,
    owner: u32,
) -> Result<SharedRegionId, SharedMemError> {
    let header_size = core::mem::size_of::<RingBufferHeader>() as u64;
    let data_size = entries as u64 * entry_size as u64;
    let total_size = header_size + data_size;

    let id = create(
        total_size,
        SharedRegionType::RingBuffer,
        SharedAccess::ReadWrite,
        owner,
        shared_flags::LOCKED | shared_flags::DMA_COHERENT,
    )?;

    // Initialize header
    let regions = REGIONS.lock();
    if let Some(region) = regions
        .iter()
        .find_map(|slot| slot.as_ref().filter(|r| r.id == id))
    {
        let header_ptr = (region.phys_addr + 0xFFFF_8000_0000_0000) as *mut RingBufferHeader;
        unsafe {
            (*header_ptr).init(entries, entry_size);
        }
    }

    Ok(id)
}
