//! Page table operation abstractions.
//!
//! This module provides safe wrappers around x86-64 page table operations
//! used for memory management and address space manipulation.

use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::{PageTable, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

// ============================================================================
// Page Table Access
// ============================================================================

/// Read the current CR3 value (page table root physical address).
#[inline]
pub fn current_cr3() -> u64 {
    let (frame, _) = Cr3::read();
    frame.start_address().as_u64()
}

/// Activate an address space by loading CR3.
///
/// # Safety Requirements
/// - `cr3_phys` must be a valid, 4KB-aligned physical address
/// - The page table at `cr3_phys` must be properly initialized
/// - The page table must map the kernel code and stack regions
///
/// # Panics
/// Panics if `cr3_phys` is not 4KB-aligned.
#[inline]
pub fn activate_cr3(cr3_phys: u64) {
    debug_assert!(
        cr3_phys & 0xFFF == 0,
        "CR3 must be 4KB-aligned: {:#x}",
        cr3_phys
    );

    let frame = PhysFrame::<Size4KiB>::from_start_address(PhysAddr::new(cr3_phys))
        .expect("CR3 address must be 4KB-aligned");

    unsafe {
        Cr3::write(frame, Cr3Flags::empty());
    }
}

/// Flush the TLB by reloading CR3 on current CPU and sending IPI to all other CPUs.
/// This is critical for SMP systems where page table modifications must be visible
/// to all CPUs before any process continues execution.
#[inline]
pub fn flush_tlb_all() {
    // First, flush the current CPU's TLB
    let (frame, flags) = Cr3::read();
    unsafe {
        Cr3::write(frame, flags);
    }

    // Then send TLB flush IPI to all other CPUs
    // This ensures that if the process runs on a different CPU after execve/fork,
    // it won't see stale TLB entries pointing to old physical addresses
    crate::smp::send_tlb_flush_ipi_all();
}

/// Invalidate a single TLB entry.
#[inline]
pub fn invlpg(addr: u64) {
    use x86_64::instructions::tlb;
    use x86_64::VirtAddr;
    tlb::flush(VirtAddr::new(addr));
}

// ============================================================================
// Page Table Manipulation
// ============================================================================

/// Get a mutable reference to a page table at a physical address.
///
/// # Safety
/// - `phys_addr` must point to a valid, initialized page table
/// - The physical address must be identity-mapped
/// - Caller must ensure no other references to this page table exist
#[inline]
pub unsafe fn page_table_at_phys(phys_addr: u64) -> &'static mut PageTable {
    &mut *(phys_addr as *mut PageTable)
}

/// Get an immutable reference to a page table at a physical address.
///
/// # Safety
/// - `phys_addr` must point to a valid, initialized page table
/// - The physical address must be identity-mapped
#[inline]
pub unsafe fn page_table_at_phys_ref(phys_addr: u64) -> &'static PageTable {
    &*(phys_addr as *const PageTable)
}

/// Calculate page table indices for a virtual address.
///
/// Returns (pml4_index, pdp_index, pd_index, pt_index).
#[inline]
pub const fn page_table_indices(virt_addr: u64) -> (usize, usize, usize, usize) {
    let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
    let pdp_idx = ((virt_addr >> 30) & 0x1FF) as usize;
    let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
    let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;
    (pml4_idx, pdp_idx, pd_idx, pt_idx)
}

/// Check if a page table entry is present.
#[inline]
pub fn entry_is_present(entry: &x86_64::structures::paging::page_table::PageTableEntry) -> bool {
    entry.flags().contains(PageTableFlags::PRESENT)
}

/// Check if a page table entry is a huge page (2MB or 1GB).
#[inline]
pub fn entry_is_huge(entry: &x86_64::structures::paging::page_table::PageTableEntry) -> bool {
    entry.flags().contains(PageTableFlags::HUGE_PAGE)
}

/// Get the physical address from a page table entry.
#[inline]
pub fn entry_phys_addr(entry: &x86_64::structures::paging::page_table::PageTableEntry) -> u64 {
    entry.addr().as_u64()
}

// ============================================================================
// Page Table Walking
// ============================================================================

/// Walk page tables to find the physical address for a virtual address.
///
/// Returns None if the address is not mapped.
///
/// # Safety
/// - The page tables must be valid and identity-mapped
pub unsafe fn translate_virtual(cr3: u64, virt_addr: u64) -> Option<u64> {
    let (pml4_idx, pdp_idx, pd_idx, pt_idx) = page_table_indices(virt_addr);

    // PML4
    let pml4 = page_table_at_phys_ref(cr3);
    let pml4_entry = &pml4[pml4_idx];
    if !entry_is_present(pml4_entry) {
        return None;
    }

    // PDP
    let pdp = page_table_at_phys_ref(entry_phys_addr(pml4_entry));
    let pdp_entry = &pdp[pdp_idx];
    if !entry_is_present(pdp_entry) {
        return None;
    }
    if entry_is_huge(pdp_entry) {
        // 1GB huge page
        let base = entry_phys_addr(pdp_entry) & !0x3FFFFFFF;
        return Some(base | (virt_addr & 0x3FFFFFFF));
    }

    // PD
    let pd = page_table_at_phys_ref(entry_phys_addr(pdp_entry));
    let pd_entry = &pd[pd_idx];
    if !entry_is_present(pd_entry) {
        return None;
    }
    if entry_is_huge(pd_entry) {
        // 2MB huge page
        let base = entry_phys_addr(pd_entry) & !0x1FFFFF;
        return Some(base | (virt_addr & 0x1FFFFF));
    }

    // PT
    let pt = page_table_at_phys_ref(entry_phys_addr(pd_entry));
    let pt_entry = &pt[pt_idx];
    if !entry_is_present(pt_entry) {
        return None;
    }

    let base = entry_phys_addr(pt_entry) & !0xFFF;
    Some(base | (virt_addr & 0xFFF))
}

// ============================================================================
// CR3 Validation
// ============================================================================

/// Validate a CR3 value before use.
///
/// Returns true if the CR3 value appears to be valid:
/// - 4KB aligned
/// - Within reasonable physical memory bounds
pub fn validate_cr3(cr3: u64) -> bool {
    // Must be 4KB aligned
    if cr3 & 0xFFF != 0 {
        return false;
    }

    // Must not be zero (unless explicitly allowed)
    if cr3 == 0 {
        return false;
    }

    // Should be within reasonable physical memory (< 4GB for typical systems)
    if cr3 >= 0x1_0000_0000 {
        // Warn but don't fail - some systems may have higher addresses
        crate::kwarn!("CR3 {:#x} is in high physical memory range (>4GB)", cr3);
    }

    true
}

/// Read and verify PML4 entries at a CR3 address.
///
/// # Safety
/// - CR3 must point to valid, identity-mapped memory
pub unsafe fn verify_pml4_content(cr3: u64) -> bool {
    let pml4 = page_table_at_phys_ref(cr3);

    // At least one entry should be present for the kernel
    let mut has_present = false;
    for entry in pml4.iter() {
        if entry_is_present(entry) {
            has_present = true;
            break;
        }
    }

    has_present
}
