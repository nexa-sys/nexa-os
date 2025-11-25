//! Safety abstraction layer for unsafe operations.
//!
//! This module provides safe wrappers and abstractions for common unsafe
//! operations in kernel code. The goal is to centralize unsafe code and
//! provide documented, auditable interfaces.
//!
//! # Modules
//!
//! - `arena`: Fixed-size bump arena for boot-time allocations
//! - `raw`: Raw memory access with bounds checking
//! - `volatile`: Volatile memory access for MMIO and hardware registers
//! - `ptr`: Pointer-to-reference conversions and slice creation
//! - `x86`: x86-64 specific operations (port I/O, MSRs, etc.)
//! - `alloc`: Memory allocation wrappers
//! - `static_data`: Static mutable variable access patterns
//! - `packet`: Network packet header casting
//! - `paging`: Page table operations

pub mod alloc;
pub mod arena;
pub mod packet;
pub mod paging;
pub mod ptr;
pub mod raw;
pub mod static_data;
pub mod volatile;
pub mod x86;

// Re-export commonly used types and functions
pub use arena::{ArenaError, StaticArena};
pub use raw::{static_slice_from_raw_parts, RawAccessError, RawReader, StaticBufferAccessor};

// Volatile access
pub use volatile::{volatile_read, volatile_write, MmioRegion, Volatile};

// Pointer operations
pub use ptr::{
    copy_from_user, copy_slice_to_user, copy_to_user, ptr_to_mut, ptr_to_ref, slice_from_ptr,
    slice_from_ptr_mut, static_slice, static_slice_mut, UserSlice, UserSliceMut,
};

// x86 operations
pub use x86::{
    cpuid, cpuid_count, flush_tlb, hlt, inb, inl, inw, invlpg, is_stack_aligned, lfence, memcpy,
    memset, memzero, mfence, outb, outl, outw, pause, pci_config_read32, pci_config_write32,
    read_cr3, read_low_memory, read_phys_u64, read_rsp, rdtsc, serial_debug_byte, serial_debug_hex,
    serial_debug_str, sfence, stack_alignment_offset, write_low_memory, write_phys_u64,
};

// Allocation
pub use alloc::{allocate, allocate_zeroed, deallocate, layout_array, layout_of, TrackedAllocation};

// Static data access
pub use static_data::{StaticArray, StaticMut};

// Packet handling
pub use packet::{cast_header, cast_header_mut, read_header_unaligned, write_header_unaligned, FromBytes, PacketBuffer, PacketBufferMut};

// Page table operations
pub use paging::{
    activate_cr3, current_cr3, entry_is_huge, entry_is_present, entry_phys_addr, flush_tlb_all,
    page_table_at_phys, page_table_at_phys_ref, page_table_indices, translate_virtual,
    validate_cr3, verify_pml4_content,
};
