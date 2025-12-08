//! Memory Management subsystem for NexaOS
//!
//! This module contains all memory-related functionality including:
//! - Physical and virtual memory allocation (buddy + slab allocators)
//! - Page table management and mapping
//! - Virtual memory regions (vmalloc)
//! - Virtual Memory Area (VMA) management for process address spaces
//! - NUMA topology support (optional, enabled with `numa` feature)
//! - Memory region detection
//! - Swap subsystem support

pub mod allocator;
pub mod memory;
#[cfg(feature = "numa")]
pub mod numa;
#[cfg(not(feature = "numa"))]
pub mod numa {
    //! NUMA stub module (feature disabled)
    //! Provides no-op implementations when NUMA support is disabled.
    
    pub const MAX_NUMA_NODES: usize = 1;
    pub const NUMA_NO_NODE: u32 = 0xFFFFFFFF;
    pub const LOCAL_DISTANCE: u8 = 10;
    pub const REMOTE_DISTANCE: u8 = 20;
    pub const UNREACHABLE_DISTANCE: u8 = 255;
    
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum NumaPolicy { 
        #[default] 
        Default, 
        Local,  // Prefer local node
        Bind, 
        Interleave, 
        Preferred 
    }
    
    #[derive(Debug, Clone, Copy)]
    pub struct NumaNode {
        pub id: u32,
        pub online: bool,
    }
    
    #[derive(Debug, Clone, Copy)]
    pub struct CpuNumaMapping {
        pub cpu_id: u32,
        pub numa_node: u32,
    }
    
    #[derive(Debug, Clone, Copy)]
    pub struct MemoryNumaMapping {
        pub base: u64,
        pub size: u64,
        pub numa_node: u32,
    }
    
    pub fn init() -> Result<(), &'static str> { 
        crate::kinfo!("NUMA support disabled (numa feature not enabled)");
        Ok(()) 
    }
    pub fn is_initialized() -> bool { false }
    pub fn node_count() -> u32 { 1 }
    pub fn online_nodes() -> &'static [u32] { &[0] }
    pub fn get_node(_id: u32) -> Option<&'static NumaNode> { None }
    pub fn cpu_to_node(_cpu: u32) -> u32 { 0 }
    pub fn cpus_on_node(_node: u32) -> &'static [u32] { &[] }
    pub fn addr_to_node(_addr: u64) -> u32 { 0 }
    pub fn current_node() -> u32 { 0 }
    pub fn node_distance(_from: u32, _to: u32) -> u8 { LOCAL_DISTANCE }
    pub fn best_node_for_policy(_policy: NumaPolicy) -> u32 { 0 }
    pub fn memory_affinity_entries() -> &'static [MemoryNumaMapping] { &[] }
}
pub mod paging;
pub mod swap;
pub mod vma;
pub mod vmalloc;

// Re-export commonly used items from allocator
pub use allocator::{
    get_memory_stats, init_kernel_heap, init_numa_allocator, kalloc, kfree, numa_alloc_local,
    numa_alloc_on_node, numa_alloc_policy, numa_free, print_memory_stats, zalloc, BuddyAllocator,
    BuddyStats, GlobalAllocator, HeapStats, KernelHeap, MemoryZone, NumaAllocator,
    NumaNodeAllocator, SlabAllocator, SlabStats, ZoneAllocator,
};

// Re-export from memory
pub use memory::{find_heap_region, get_total_physical_memory, log_memory_overview};

// Re-export from numa
pub use numa::{
    addr_to_node, best_node_for_policy, cpu_to_node, cpus_on_node, current_node, get_node,
    init as init_numa, is_initialized as numa_is_initialized, memory_affinity_entries, node_count,
    node_distance, online_nodes, CpuNumaMapping, MemoryNumaMapping, NumaNode, NumaPolicy,
    LOCAL_DISTANCE, MAX_NUMA_NODES, NUMA_NO_NODE, REMOTE_DISTANCE, UNREACHABLE_DISTANCE,
};

// Re-export from paging
pub use paging::{
    activate_address_space, allocate_user_region, create_process_address_space, current_pml4_phys,
    debug_cr3_info, ensure_nxe_enabled, free_process_address_space, free_user_region,
    handle_user_demand_fault, init, is_user_demand_page_address, kernel_pml4_phys,
    print_cr3_statistics, print_demand_paging_statistics, print_user_region_statistics,
    read_current_cr3, validate_cr3, MapDeviceError,
};

// Re-export from vmalloc
pub use vmalloc::{
    print_vmalloc_stats, vfree, vmalloc, vmalloc_flags, vmalloc_handle_fault, VmFlags,
    VmallocAllocator,
};

// Re-export from vma
pub use vma::{
    free_address_space, get_address_space, init_address_space, AddressSpace, VMA, VMABacking,
    VMAFlags, VMAManager, VMAPermissions, VMAStats, MAX_ADDRESS_SPACES, MAX_VMAS,
    PAGE_SIZE as VMA_PAGE_SIZE,
};

// Re-export from swap
pub use swap::{
    get_swap_info, get_swap_stats, is_swap_available, kmod_swap_register, kmod_swap_unregister,
    make_swap_pte, print_swap_stats, pte_is_swap, pte_to_swap_entry, swap_free, swap_in, swap_out,
    SwapEntry, SwapInfo, SwapModuleOps, SWAP_FLAG_DISCARD, SWAP_FLAG_PREFER, SWAP_FLAG_PRIO_MASK,
};