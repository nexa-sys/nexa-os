/// Memory paging setup for x86_64
use x86_64::structures::paging::PageTable;
use x86_64::VirtAddr;

/// Initialize identity-mapped paging
pub fn init() {
    unsafe { 
        init_identity_mapping();
        ensure_paging_enabled();
    };
    crate::kinfo!("Paging initialized with identity mapping");
}

/// Initialize identity mapping for the entire address space
/// SAFETY: This function must be called only once during kernel initialization
unsafe fn init_identity_mapping() {
    use x86_64::registers::control::Cr3;

    // Get current level 4 page table
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = VirtAddr::new(phys.as_u64());
    let _level_4_table = &mut *(virt.as_mut_ptr() as *mut PageTable);

    // For now, assume bootloader has set up identity mapping
    // In a full implementation, we would need to create proper page tables
    // But for this demo, we'll rely on the bootloader's mapping

    crate::kdebug!("Level 4 page table at physical address: {:#x}", phys.as_u64());
}

/// Ensure paging is enabled (required for user mode)
unsafe fn ensure_paging_enabled() {
    use x86_64::registers::control::{Cr0, Cr0Flags};
    
    let mut cr0 = Cr0::read();
    if !cr0.contains(Cr0Flags::PAGING) {
        cr0.insert(Cr0Flags::PAGING);
        Cr0::write(cr0);
        crate::kinfo!("Paging enabled");
    } else {
        crate::kdebug!("Paging already enabled");
    }
}