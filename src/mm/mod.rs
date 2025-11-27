//! Memory Management subsystem for NexaOS
//!
//! This module contains all memory-related functionality including:
//! - Physical and virtual memory allocation (buddy + slab allocators)
//! - Page table management and mapping
//! - Virtual memory regions (vmalloc)
//! - NUMA topology support
//! - Memory region detection

pub mod allocator;
pub mod memory;
pub mod numa;
pub mod paging;
pub mod vmalloc;

// Re-export commonly used items from allocator
pub use allocator::{
    get_memory_stats, init_kernel_heap, init_numa_allocator, kalloc, kfree, numa_alloc_local, numa_alloc_on_node,
    numa_alloc_policy, numa_free, print_memory_stats, zalloc, BuddyAllocator, BuddyStats,
    GlobalAllocator, HeapStats, KernelHeap, MemoryZone, NumaAllocator, NumaNodeAllocator,
    SlabAllocator, SlabStats, ZoneAllocator,
};

// Re-export from memory
pub use memory::{find_heap_region, log_memory_overview};

// Re-export from numa
pub use numa::{
    addr_to_node, best_node_for_policy, cpu_to_node, cpus_on_node, current_node, get_node,
    init as init_numa, is_initialized as numa_is_initialized, memory_affinity_entries,
    node_count, node_distance, online_nodes, CpuNumaMapping, MemoryNumaMapping, NumaNode,
    NumaPolicy, LOCAL_DISTANCE, MAX_NUMA_NODES, NUMA_NO_NODE, REMOTE_DISTANCE,
    UNREACHABLE_DISTANCE,
};

// Re-export from paging
pub use paging::{
    activate_address_space, allocate_user_region, create_process_address_space, current_pml4_phys,
    debug_cr3_info, ensure_nxe_enabled, free_process_address_space, init, kernel_pml4_phys,
    print_cr3_statistics, read_current_cr3, validate_cr3, MapDeviceError,
};

// Re-export from vmalloc
pub use vmalloc::{
    print_vmalloc_stats, vfree, vmalloc, vmalloc_flags, vmalloc_handle_fault, VmFlags,
    VmallocAllocator,
};
