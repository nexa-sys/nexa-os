/// Memory paging setup for x86_64
use x86_64::structures::paging::{PageTable, PageTableFlags, PhysFrame, Size4KiB, PageTableIndex};
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
            crate::kinfo!("PDP[0] is used, getting addr");
            let pd_addr = pdp[0].addr();
            crate::kinfo!("PD addr: {:#x}", pd_addr.as_u64());
            
            // Check if it's a huge page (PS bit set)
            let flags = pdp[0].flags();
            if flags.contains(PageTableFlags::HUGE_PAGE) {
                // For huge pages, set USER_ACCESSIBLE directly on PDP entry
                pdp[0].set_flags(flags | PageTableFlags::USER_ACCESSIBLE);
                crate::kinfo!("Set USER_ACCESSIBLE for huge page at PDP[0]");
                return;
            }
            // Set PDP[0] to not huge page
            pdp[0].set_flags(PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
            
            let pdp_flags = pdp[0].flags();
            crate::kinfo!("PDP[0] flags: {:#x}", pdp_flags.bits());
            
            // Always treat as 4KB pages for simplicity
            let pd_addr = pdp[0].addr();
            if pd_addr.as_u64() == 0 {
                // PD not allocated, allocate at fixed address 0x12a000
                let new_pd_addr = PhysAddr::new(0x12a000);
                // Clear the PD memory first
                unsafe {
                    core::ptr::write_bytes(new_pd_addr.as_u64() as *mut u8, 0, 4096);
                }
                pdp[0].set_addr(new_pd_addr, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
                crate::kinfo!("Allocated new PD at {:#x}", new_pd_addr.as_u64());
            }
            let pd_addr = pdp[0].addr();
            crate::kinfo!("PD addr: {:#x}", pd_addr.as_u64());
            let pd = unsafe { &mut *(pd_addr.as_u64() as *mut PageTable) };
            crate::kinfo!("PD pointer created");
            crate::kinfo!("PD at {:#x}", pd_addr.as_u64());
            
            // Initialize PD to all zeros first
            crate::kinfo!("Initializing PD to zeros");
            for i in 0..512 {
                pd[i].set_unused();
            }
            crate::kinfo!("PD initialized to zeros");
            
            // Set USER_ACCESSIBLE on existing PD entries
            for pd_idx in 0..4 { // Check PD[0-3] for 0-4MB range
                crate::kinfo!("Checking PD[{}]", pd_idx);
                
                if pd[pd_idx].is_unused() {
                    crate::kinfo!("PD[{}] is unused", pd_idx);
                    continue;
                }
                
                crate::kinfo!("PD[{}] is used, getting flags", pd_idx);
                let pd_flags = pd[pd_idx].flags();
                crate::kinfo!("PD[{}] flags: {:#x}", pd_idx, pd_flags.bits());
                if pd_flags.contains(PageTableFlags::HUGE_PAGE) {
                    // 2MB huge page
                    pd[pd_idx].set_flags(pd_flags | PageTableFlags::USER_ACCESSIBLE);
                    crate::kdebug!("Set USER_ACCESSIBLE for 2MB page at PD[{}]", pd_idx);
                } else {
                    // 4KB pages - set USER_ACCESSIBLE on PT entries
                    let pt_addr = pd[pd_idx].addr();
                    crate::kinfo!("PT[{}] at {:#x}", pd_idx, pt_addr.as_u64());
                    let pt = unsafe { &mut *(pt_addr.as_u64() as *mut PageTable) };
                    
                    // Set USER_ACCESSIBLE on all PT entries
                    for pt_idx in 0..512 {
                        if !pt[pt_idx].is_unused() {
                            let pt_flags = pt[pt_idx].flags();
                            pt[pt_idx].set_flags(pt_flags | PageTableFlags::USER_ACCESSIBLE);
                            crate::kdebug!("Set USER_ACCESSIBLE for PT[{}][{}]", pd_idx, pt_idx);
                        }
                    }
                }
            }
        } else {
            crate::kinfo!("PDP[0] is unused - no identity mapping found");
        }
    } else {
        crate::kinfo!("PML4[0] is unused - no page tables found");
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