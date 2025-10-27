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
                // Since VGA (0xb8000) is identity-mapped by the huge page,
                // it's now safe to write to the VGA buffer. Mark it ready so
                // higher-level logging will emit to VGA again.
                crate::vga_buffer::set_vga_ready();
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

    // Map user program virtual addresses to physical addresses
    // User program expects to run at virtual address 0x200000, but is loaded at physical 0x600000
    map_user_program();

    // Map VGA buffer for kernel output
    map_vga_buffer();

    crate::kinfo!("User page tables initialized with USER_ACCESSIBLE permissions");
}

/// Map user program virtual addresses to physical addresses
unsafe fn map_user_program() {
    use x86_64::registers::control::Cr3;
    use x86_64::structures::paging::{PageTable, PageTableFlags, PhysFrame, Size4KiB};
    use x86_64::PhysAddr;

    const USER_VIRT_BASE: u64 = 0x200000; // Virtual address where user program expects to run
    const USER_PHYS_BASE: u64 = 0x600000; // Physical address where user program is loaded
    const USER_SIZE: u64 = 0x200000; // 2MB user space

    crate::kinfo!("Mapping user program: virtual {:#x} -> physical {:#x}, size {:#x}", 
        USER_VIRT_BASE, USER_PHYS_BASE, USER_SIZE);

    // Get current page table root (PML4)
    let (pml4_frame, _) = Cr3::read();
    let pml4_addr = pml4_frame.start_address();
    let pml4 = &mut *(pml4_addr.as_u64() as *mut PageTable);

    // Calculate indices for virtual address 0x200000
    // 0x200000 = 2MB, so PDP index = 0, PD index = 0, PT index = 0
    let pdp_index = x86_64::structures::paging::PageTableIndex::new(0);
    let pd_index = x86_64::structures::paging::PageTableIndex::new(0);

    // Ensure PDP exists
    if pml4[pdp_index].is_unused() {
        // Allocate PDP
        let pdp_addr = PhysAddr::new(0x120000); // Fixed address for PDP
        unsafe { core::ptr::write_bytes(pdp_addr.as_u64() as *mut u8, 0, 4096); }
        pml4[pdp_index].set_addr(pdp_addr, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
        crate::kinfo!("Allocated PDP at {:#x}", pdp_addr.as_u64());
    }

    let pdp_addr = pml4[pdp_index].addr();
    let pdp = &mut *(pdp_addr.as_u64() as *mut PageTable);

    // Ensure PD exists
    if pdp[pd_index].is_unused() {
        // Allocate PD
        let pd_addr = PhysAddr::new(0x121000); // Fixed address for PD
        unsafe { core::ptr::write_bytes(pd_addr.as_u64() as *mut u8, 0, 4096); }
        pdp[pd_index].set_addr(pd_addr, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
        crate::kinfo!("Allocated PD at {:#x}", pd_addr.as_u64());
    }

    let pd_addr = pdp[pd_index].addr();
    let pd = &mut *(pd_addr.as_u64() as *mut PageTable);

    // Map 2MB of user space (0x200000 -> 0x600000)
    let num_pages = USER_SIZE / 4096;
    let pt_addr_fixed = PhysAddr::new(0x122000); // Fixed PT address
    
    // Allocate PT if needed
    if pd[0].is_unused() {
        unsafe { core::ptr::write_bytes(pt_addr_fixed.as_u64() as *mut u8, 0, 4096); }
        pd[0].set_addr(pt_addr_fixed, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
        crate::kinfo!("Allocated PT at {:#x}", pt_addr_fixed.as_u64());
    }
    
    let pt_addr = pd[0].addr();
    let pt = &mut *(pt_addr.as_u64() as *mut PageTable);
    
    for i in 0..num_pages {
        let virt_addr = USER_VIRT_BASE + i * 4096;
        let phys_addr = USER_PHYS_BASE + i * 4096;
        
        let page_index = x86_64::structures::paging::PageTableIndex::new(((virt_addr / 4096) % 512) as u16);
        let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(phys_addr));
        
        pt[page_index].set_frame(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
        if i == 0 { // Only log the first mapping
            crate::kinfo!("Mapped first page: virtual {:#x} -> physical {:#x}", USER_VIRT_BASE, USER_PHYS_BASE);
        }
    }

    crate::kinfo!("User program mapping completed: {} pages mapped", num_pages);

    // Map user ELF memory (0x400000 virtual -> 0x400000 physical)
    const ELF_VIRT_BASE: u64 = 0x400000;
    const ELF_PHYS_BASE: u64 = 0x400000;
    const ELF_SIZE: u64 = 0x200000; // 2MB

    crate::kinfo!("Mapping ELF program: virtual {:#x} -> physical {:#x}, size {:#x}",
        ELF_VIRT_BASE, ELF_PHYS_BASE, ELF_SIZE);

    let num_pages = ELF_SIZE / 4096;
    for i in 0..num_pages {
        let virt_addr = ELF_VIRT_BASE + i * 4096;
        let phys_addr = ELF_PHYS_BASE + i * 4096;

        // Calculate page table indices
        let pml4_index = ((virt_addr >> 39) & 0x1FF) as usize;
        let pdp_index = ((virt_addr >> 30) & 0x1FF) as usize;
        let pd_index = ((virt_addr >> 21) & 0x1FF) as usize;
        let pt_index = ((virt_addr >> 12) & 0x1FF) as usize;

        // Ensure PDP exists
        if pml4[pml4_index].is_unused() {
            let pdp_addr = PhysAddr::new(0x123000 + (pml4_index as u64) * 4096);
            unsafe { core::ptr::write_bytes(pdp_addr.as_u64() as *mut u8, 0, 4096); }
            pml4[pml4_index].set_addr(pdp_addr, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
        }

        let pdp_addr = pml4[pml4_index].addr();
        let pdp = &mut *(pdp_addr.as_u64() as *mut PageTable);

        // Ensure PD exists
        if pdp[pdp_index].is_unused() {
            let pd_addr = PhysAddr::new(0x124000 + (pdp_index as u64) * 4096);
            unsafe { core::ptr::write_bytes(pd_addr.as_u64() as *mut u8, 0, 4096); }
            pdp[pdp_index].set_addr(pd_addr, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
        }

        let pd_addr = pdp[pdp_index].addr();
        let pd = &mut *(pd_addr.as_u64() as *mut PageTable);

        // Ensure PT exists
        if pd[pd_index].is_unused() {
            let pt_addr = PhysAddr::new(0x125000 + (pd_index as u64) * 4096);
            unsafe { core::ptr::write_bytes(pt_addr.as_u64() as *mut u8, 0, 4096); }
            pd[pd_index].set_addr(pt_addr, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
        }

        let pt_addr = pd[pd_index].addr();
        let pt = &mut *(pt_addr.as_u64() as *mut PageTable);

        let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(phys_addr));
        pt[pt_index].set_frame(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
    }

    crate::kinfo!("ELF program mapping completed: {} pages mapped", num_pages);
}

/// Map VGA buffer for kernel output
unsafe fn map_vga_buffer() {
    use x86_64::registers::control::Cr3;
    use x86_64::structures::paging::{PageTable, PageTableFlags, PhysFrame, Size4KiB};
    use x86_64::PhysAddr;

    const VGA_VIRT_ADDR: u64 = 0xb8000; // Virtual address for VGA buffer
    const VGA_PHYS_ADDR: u64 = 0xb8000; // Physical address for VGA buffer

    crate::kdebug!("Mapping VGA buffer: virtual {:#x} -> physical {:#x}",
        VGA_VIRT_ADDR, VGA_PHYS_ADDR);

    // Get current page table root (PML4)
    let (pml4_frame, _) = Cr3::read();
    let pml4_addr = pml4_frame.start_address();
    let pml4 = &mut *(pml4_addr.as_u64() as *mut PageTable);

    // Calculate indices for virtual address 0xb8000
    let pml4_index = ((VGA_VIRT_ADDR >> 39) & 0x1FF) as usize;
    let pdp_index = ((VGA_VIRT_ADDR >> 30) & 0x1FF) as usize;
    let pd_index = ((VGA_VIRT_ADDR >> 21) & 0x1FF) as usize;
    let pt_index = ((VGA_VIRT_ADDR >> 12) & 0x1FF) as usize;

    // Check if PDP[0] is a huge page
    let pdp_addr = pml4[pml4_index].addr();
    let pdp = &mut *(pdp_addr.as_u64() as *mut PageTable);
    
    if pdp[pdp_index].flags().contains(PageTableFlags::HUGE_PAGE) {
        // PDP[0] is a huge page, VGA buffer is already identity mapped
        // Ensure it has correct permissions
        let flags = pdp[pdp_index].flags();
        pdp[pdp_index].set_flags(flags | PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
        crate::kdebug!("VGA buffer already mapped via huge page, updated permissions");
        // Indicate VGA is ready for higher-level writes
    crate::vga_buffer::set_vga_ready();
    // Transient serial-only confirmation so we can verify at runtime that
    // this branch executed and VGA was marked ready. Serial is always
    // available early in boot, so this will appear in serial logs even if
    // VGA output is not visible.
    crate::serial_println!("VGA mapped and marked ready (huge page)");
    } else {
        // Normal page table structure exists
        let pd_addr = pdp[pdp_index].addr();
        let pd = &mut *(pd_addr.as_u64() as *mut PageTable);

        let pt_addr = pd[pd_index].addr();
        let pt = &mut *(pt_addr.as_u64() as *mut PageTable);

        // Map the page
        let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(VGA_PHYS_ADDR));
        pt[pt_index].set_frame(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE);

        crate::kdebug!("VGA buffer mapping completed via page tables");
        // Indicate VGA is ready for higher-level writes
    crate::vga_buffer::set_vga_ready();
    // Confirm via serial so we can see in the run logs whether this path
    // executed.
    crate::serial_println!("VGA mapped and marked ready (page table)");
    }
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