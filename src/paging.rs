/// Memory paging setup for x86_64

use core::cell::UnsafeCell;
use x86_64::PhysAddr;
use x86_64::structures::paging::PageTable;

#[repr(align(4096))]
struct PageTableHolder(UnsafeCell<PageTable>);

unsafe impl Sync for PageTableHolder {}

impl PageTableHolder {
    const fn new() -> Self {
        Self(UnsafeCell::new(PageTable::new()))
    }

    fn reset(&self) {
        crate::serial::_print(format_args!(
            "paging::reset table @ {:#x}\n",
            self.as_ptr() as u64
        ));
        let table = unsafe { &mut *self.as_mut_ptr() };
        for entry in table.iter_mut() {
            entry.set_unused();
        }
    }

    fn as_ptr(&self) -> *const PageTable {
        self.0.get() as *const PageTable
    }

    fn as_mut_ptr(&self) -> *mut PageTable {
        self.0.get()
    }

    fn phys_addr(&self) -> PhysAddr {
        PhysAddr::new(self.as_ptr() as u64)
    }
}

static USER_PDP: PageTableHolder = PageTableHolder::new();
static USER_PD: PageTableHolder = PageTableHolder::new();
static KERNEL_IDENTITY_PD: PageTableHolder = PageTableHolder::new();

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
    use x86_64::registers::control::{Cr3, Cr3Flags};
    use x86_64::structures::paging::PageTableFlags;

    crate::kinfo!("Setting up user-accessible pages for user space (0x400000-0x800000)");

    // Get current page table root (PML4)
    let (pml4_frame, _) = Cr3::read();
    let pml4_addr = pml4_frame.start_address();
    let pml4 = &mut *(pml4_addr.as_u64() as *mut PageTable);

    crate::serial::_print(format_args!(
        "init_user_page_tables: CR3 frame={:#x}\n",
        pml4_addr.as_u64()
    ));

    crate::kinfo!("PML4 at {:#x}", pml4_addr.as_u64());

    // For identity mapping, we expect:
    // - PML4[0] -> PDP (covers 0-512GB)
    // - PDP[0] -> PD (covers 0-1GB)
    // - PD[0-3] -> PTs (covers 0-4MB, but we need up to 2MB)

    let mut should_map_user = true;

    // Check if we have the expected structure
    if !pml4[0].is_unused() {
        let current_flags = pml4[0].flags();
        if !current_flags.contains(PageTableFlags::USER_ACCESSIBLE) {
            pml4[0].set_flags(
                current_flags
                    | PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE,
            );
            crate::kinfo!("Enabled USER_ACCESSIBLE on PML4[0]");
        }

        let pdp_addr = pml4[0].addr();
        let pdp = &mut *(pdp_addr.as_u64() as *mut PageTable);
        crate::kinfo!("PDP at {:#x}", pdp_addr.as_u64());
        crate::serial::_print(format_args!(
            "PDP[0] initial flags={:#x}, addr={:#x}\n",
            pdp[0].flags().bits(),
            pdp[0].addr().as_u64()
        ));

        if !pdp[0].is_unused() {
            crate::kinfo!("PDP[0] is used, getting addr");

            // Check if it's a huge page (PS bit set)
            let flags = pdp[0].flags();
            if flags.contains(PageTableFlags::HUGE_PAGE) {
                // Convert the 1 GiB huge page into a directory of 2 MiB pages so
                // we can retain identity mapping while granting user access to the
                // low-memory region needed by the userspace binary.
                KERNEL_IDENTITY_PD.reset();
                let pd_ptr = KERNEL_IDENTITY_PD.as_mut_ptr();
                let pd = unsafe { &mut *pd_ptr };

                for (index, entry) in pd.iter_mut().enumerate() {
                    let phys = (index as u64) * 0x200000;
                    entry.set_addr(
                        PhysAddr::new(phys),
                        PageTableFlags::PRESENT
                            | PageTableFlags::WRITABLE
                            | PageTableFlags::USER_ACCESSIBLE
                            | PageTableFlags::HUGE_PAGE,
                    );
                }

                pdp[0].set_addr(
                    KERNEL_IDENTITY_PD.phys_addr(),
                    PageTableFlags::PRESENT
                        | PageTableFlags::WRITABLE
                        | PageTableFlags::USER_ACCESSIBLE,
                );
                crate::kinfo!(
                    "Converted PDP[0] 1GiB huge page into 2MiB identity directory"
                );
                crate::vga_buffer::set_vga_ready();
                should_map_user = false;
            } else {
                // Ensure standard page table entry carries correct permissions
                pdp[0].set_flags(
                    PageTableFlags::PRESENT
                        | PageTableFlags::WRITABLE
                        | PageTableFlags::USER_ACCESSIBLE,
                );

                let mut pd_addr = pdp[0].addr();
                if pd_addr.as_u64() == 0 {
                    KERNEL_IDENTITY_PD.reset();
                    let phys = KERNEL_IDENTITY_PD.phys_addr();
                    pdp[0].set_addr(
                        phys,
                        PageTableFlags::PRESENT
                            | PageTableFlags::WRITABLE
                            | PageTableFlags::USER_ACCESSIBLE,
                    );
                    crate::kinfo!("Allocated new PD at {:#x}", phys.as_u64());
                    pd_addr = phys;
                }

                crate::kinfo!("PD addr: {:#x}", pd_addr.as_u64());
                let pd = unsafe { &mut *(pd_addr.as_u64() as *mut PageTable) };
                crate::kinfo!("PD pointer created");
                crate::kinfo!("PD at {:#x}", pd_addr.as_u64());
                crate::serial::_print(format_args!(
                    "PD[0] before mapping: flags={:#x}, addr={:#x}\n",
                    pd[0].flags().bits(),
                    pd[0].addr().as_u64()
                ));

                // Ensure the first 2 MiB (kernel identity region) is mapped so the kernel stack
                // and other low memory data remain accessible even after we adjust permissions.
                if pd[0].is_unused() {
                    pd[0].set_addr(
                        PhysAddr::new(0),
                        PageTableFlags::PRESENT
                            | PageTableFlags::WRITABLE
                            | PageTableFlags::HUGE_PAGE,
                    );
                    crate::kinfo!(
                        "Mapped kernel identity huge page: virtual 0x0 -> physical 0x0"
                    );
                }
                crate::serial::_print(format_args!(
                    "PD[0] after mapping: flags={:#x}, addr={:#x}\n",
                    pd[0].flags().bits(),
                    pd[0].addr().as_u64()
                ));

                // Set USER_ACCESSIBLE on existing PD entries
                for pd_idx in 0..4 {
                    // Check PD[0-3] for 0-4MB range
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
                                crate::kdebug!(
                                    "Set USER_ACCESSIBLE for PT[{}][{}]",
                                    pd_idx,
                                    pt_idx
                                );
                            }
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

    // Map user program virtual addresses (code, heap, stack) to physical addresses
    // User program expects to run at virtual address 0x200000, with heap and stack
    // residing up to 0x700000. We map the region with 2 MiB huge pages so the
    // stack (at 0x600000-0x700000) has backing memory and retains user access.
    if should_map_user {
        map_user_program();
    } else {
        crate::kinfo!("User region reuses converted identity mappings; skipping extra map");
    }

    // Map VGA buffer for kernel output
    map_vga_buffer();

    // Flush CR3 to ensure the CPU observes updated mappings
    let (pml4_frame_flush, _) = Cr3::read();
    Cr3::write(pml4_frame_flush, Cr3Flags::empty());

    crate::kinfo!("User page tables initialized with USER_ACCESSIBLE permissions");
}

/// Map user program virtual addresses to physical addresses
unsafe fn map_user_program() {
    use x86_64::registers::control::Cr3;
    use x86_64::structures::paging::{PageTable, PageTableFlags};
    use x86_64::PhysAddr;

    // Keep these constants aligned with the values used in `process::from_elf`
    const USER_VIRT_BASE: u64 = 0x200000; // Virtual base expected by the userspace binary
    const USER_PHYS_BASE: u64 = 0x200000; // Physical base where we load the ELF segments
    const USER_TOTAL_SIZE: u64 = 0x500000; // Cover code, heap, and stack (up to 0x700000)
    const HUGE_PAGE_SIZE: u64 = 0x200000; // 2 MiB

    crate::kinfo!(
        "Mapping user region: virtual {:#x}-{:#x} -> physical {:#x}-{:#x}",
        USER_VIRT_BASE,
        USER_VIRT_BASE + USER_TOTAL_SIZE,
        USER_PHYS_BASE,
        USER_PHYS_BASE + USER_TOTAL_SIZE
    );

    // Get current page table root (PML4)
    let (pml4_frame, _) = Cr3::read();
    let pml4_addr = pml4_frame.start_address();
    let pml4 = &mut *(pml4_addr.as_u64() as *mut PageTable);

    // Calculate indices for the virtual base address
    let pml4_index_val = ((USER_VIRT_BASE >> 39) & 0x1FF) as usize;
    let pdp_index_val = ((USER_VIRT_BASE >> 30) & 0x1FF) as usize;

    // Ensure PDP exists
    if pml4[pml4_index_val].is_unused() {
        USER_PDP.reset();
        let pdp_phys = USER_PDP.phys_addr();
        pml4[pml4_index_val].set_addr(
            pdp_phys,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
        );
        crate::kinfo!("Allocated PDP at {:#x}", pdp_phys.as_u64());
    }

    let pdp_addr = pml4[pml4_index_val].addr();
    let pdp = &mut *(pdp_addr.as_u64() as *mut PageTable);

    // Ensure PD exists
    if pdp[pdp_index_val].is_unused() {
        USER_PD.reset();
        let pd_phys = USER_PD.phys_addr();
        pdp[pdp_index_val].set_addr(
            pd_phys,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
        );
        crate::kinfo!("Allocated PD at {:#x}", pd_phys.as_u64());
    }

    let pd_addr = pdp[pdp_index_val].addr();
    let pd = &mut *(pd_addr.as_u64() as *mut PageTable);

    for offset in (0..USER_TOTAL_SIZE).step_by(HUGE_PAGE_SIZE as usize) {
        let virt_addr = USER_VIRT_BASE + offset;
        let phys_addr = USER_PHYS_BASE + offset;
        let pd_index = ((virt_addr >> 21) & 0x1FF) as usize;

        pd[pd_index].set_addr(
            PhysAddr::new(phys_addr),
            PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE
                | PageTableFlags::HUGE_PAGE,
        );

        crate::kinfo!(
            "Mapped user huge page: virtual {:#x} -> physical {:#x}",
            virt_addr,
            phys_addr
        );
    }

    crate::kinfo!("User program mapping completed using huge pages");
}

/// Map VGA buffer for kernel output
unsafe fn map_vga_buffer() {
    use x86_64::registers::control::Cr3;
    use x86_64::structures::paging::{PageTable, PageTableFlags, PhysFrame, Size4KiB};
    use x86_64::PhysAddr;

    const VGA_VIRT_ADDR: u64 = 0xb8000; // Virtual address for VGA buffer
    const VGA_PHYS_ADDR: u64 = 0xb8000; // Physical address for VGA buffer

    crate::kdebug!(
        "Mapping VGA buffer: virtual {:#x} -> physical {:#x}",
        VGA_VIRT_ADDR,
        VGA_PHYS_ADDR
    );

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
        return;
    }

    // Normal page table structure exists (PD entry should not be a huge page)
    let pd_addr = pdp[pdp_index].addr();
    let pd = &mut *(pd_addr.as_u64() as *mut PageTable);
    let pd_entry = &mut pd[pd_index];

    if pd_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
        // The VGA page resides within an identity-mapped huge page; no further
        // action is needed beyond ensuring permissions are correct.
        let flags = pd_entry.flags();
        pd_entry.set_flags(flags | PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
        crate::kdebug!("VGA buffer covered by PD huge page, updated permissions");
        crate::vga_buffer::set_vga_ready();
        crate::serial_println!("VGA mapped and marked ready (PD huge page)");
        return;
    }

    let pt_addr = pd_entry.addr();
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
