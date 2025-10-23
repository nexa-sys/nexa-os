/// Memory paging setup for x86_64
use x86_64::structures::paging::PageTable;
use x86_64::VirtAddr;

/// Initialize identity-mapped paging
pub fn init() {
    unsafe { 
        init_user_page_tables();
        ensure_paging_enabled();
    };
    crate::kinfo!("Paging initialized with user page tables");
}

/// Initialize page tables with user space mapping
/// SAFETY: This function must be called only once during kernel initialization
unsafe fn init_user_page_tables() {
    use x86_64::registers::control::Cr3;
    use x86_64::structures::paging::{PageTable, PageTableFlags, PhysFrame, Size4KiB};
    use x86_64::PhysAddr;

    crate::kinfo!("Setting up user-accessible pages for user space (0x400000-0x800000)");
    
    // For now, let's try a simpler approach - set USER_ACCESSIBLE on identity-mapped pages
    // We'll iterate through the expected page table structure for identity mapping
    
    // Get current page table root (PML4)
    let (pml4_frame, _) = Cr3::read();
    let pml4_addr = pml4_frame.start_address();
    let pml4 = &mut *(pml4_addr.as_u64() as *mut PageTable);
    
    crate::kinfo!("PML4 at {:#x}", pml4_addr.as_u64());
    
    // For identity mapping, we expect:
    // - PML4[0] -> PDP (covers 0-512GB)
    // - PDP[0] -> PD (covers 0-1GB) 
    // - PD[0-3] -> PTs (covers 0-4MB, but we need up to 2MB)
    
    // Check if we have the expected structure
    if !pml4[0].is_unused() {
        let pdp_addr = pml4[0].addr();
        let pdp = &mut *(pdp_addr.as_u64() as *mut PageTable);
        crate::kinfo!("PDP at {:#x}", pdp_addr.as_u64());
        
        if !pdp[0].is_unused() {
            let pdp_flags = pdp[0].flags();
            crate::kinfo!("PDP[0] flags: {:#x}", pdp_flags.bits());
            
            if pdp_flags.contains(PageTableFlags::HUGE_PAGE) {
                // This is a 2MB huge page, set USER_ACCESSIBLE directly on PDP entry
                crate::kinfo!("Detected 2MB huge page at PDP[0], setting USER_ACCESSIBLE");
                if !pdp_flags.contains(PageTableFlags::USER_ACCESSIBLE) {
                    pdp[0].set_flags(pdp_flags | PageTableFlags::USER_ACCESSIBLE);
                    crate::kinfo!("Set USER_ACCESSIBLE on 2MB huge page");
                } else {
                    crate::kinfo!("2MB huge page already has USER_ACCESSIBLE");
                }
            } else {
                // Normal 4KB pages, traverse to PD and PT
                let pd_addr = pdp[0].addr();
                let pd = &mut *(pd_addr.as_u64() as *mut PageTable);
                crate::kinfo!("PD at {:#x}", pd_addr.as_u64());
                
                // PD entries 0-3 cover 0-4MB, we need 0-2MB (pages 0-511)
                for pd_idx in 0..4 {
                    if !pd[pd_idx].is_unused() {
                        let pd_flags = pd[pd_idx].flags();
                        crate::kinfo!("PD[{}] flags: {:#x}", pd_idx, pd_flags.bits());
                        
                        let pt_addr = pd[pd_idx].addr();
                        let pt = &mut *(pt_addr.as_u64() as *mut PageTable);
                        crate::kinfo!("PT[{}] at {:#x}", pd_idx, pt_addr.as_u64());
                        
                        // Each PT has 512 entries, 4 PTs cover 2048 pages (8MB)
                        let start_pt_entry = if pd_idx == 0 { 256 } else { 0 }; // Start from 0x400000 (page 256)
                        let end_pt_entry = if pd_idx == 1 { 256 } else { 512 }; // End at 0x800000 (page 512)
                        
                        for pt_idx in start_pt_entry..end_pt_entry {
                            let flags = pt[pt_idx].flags();
                            if !flags.contains(PageTableFlags::USER_ACCESSIBLE) {
                                pt[pt_idx].set_flags(flags | PageTableFlags::USER_ACCESSIBLE);
                                crate::kdebug!("Set USER_ACCESSIBLE for page {:#x} (PT[{}][{}])", 
                                    (pd_idx * 512 + pt_idx) * 4096, pd_idx, pt_idx);
                            }
                        }
                    }
                }
            }
        }
    }

    crate::kinfo!("User page tables initialized with USER_ACCESSIBLE permissions");
}

/// Ensure paging is enabled (required for user mode)
unsafe fn ensure_paging_enabled() {
    use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
    
    // Ensure PAE is enabled (required for x86_64)
    let mut cr4 = Cr4::read();
    if !cr4.contains(Cr4Flags::PHYSICAL_ADDRESS_EXTENSION) {
        cr4.insert(Cr4Flags::PHYSICAL_ADDRESS_EXTENSION);
        Cr4::write(cr4);
        crate::kinfo!("PAE enabled");
    }
    
    let mut cr0 = Cr0::read();
    if !cr0.contains(Cr0Flags::PAGING) {
        cr0.insert(Cr0Flags::PAGING);
        Cr0::write(cr0);
        crate::kinfo!("Paging enabled");
    } else {
        crate::kdebug!("Paging already enabled");
    }
}