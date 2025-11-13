/// Memory paging setup for x86_64
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};
use x86_64::structures::paging::PageTable;
use x86_64::PhysAddr;

#[repr(align(4096))]
struct PageTableHolder(UnsafeCell<PageTable>);

unsafe impl Sync for PageTableHolder {}

impl PageTableHolder {
    const fn new() -> Self {
        Self(UnsafeCell::new(PageTable::new()))
    }

    fn reset(&self) {
        crate::kinfo!("paging::reset table @ {:#x}\n", self.as_ptr() as u64);
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
static KERNEL_PML4: PageTableHolder = PageTableHolder::new();

const EXTRA_TABLE_COUNT: usize = 32;

static EXTRA_TABLES: [PageTableHolder; EXTRA_TABLE_COUNT] = [
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
    PageTableHolder::new(),
];

static EXTRA_TABLE_INDEX: AtomicUsize = AtomicUsize::new(0);
static NXE_CHECKED: AtomicBool = AtomicBool::new(false);
static NXE_AVAILABLE: AtomicBool = AtomicBool::new(false);

fn allocate_extra_table() -> Option<&'static PageTableHolder> {
    let idx = EXTRA_TABLE_INDEX.fetch_add(1, AtomicOrdering::SeqCst);
    if idx >= EXTRA_TABLES.len() {
        None
    } else {
        let holder = &EXTRA_TABLES[idx];
        holder.reset();
        Some(holder)
    }
}

#[derive(Debug)]
pub enum MapDeviceError {
    OutOfTableSpace,
}

/// Ensure the CPU's NX bit is set before we rely on non-executable mappings.
pub fn ensure_nxe_enabled() {
    if NXE_CHECKED.load(AtomicOrdering::Relaxed) {
        return;
    }

    if !cpu_supports_nx() {
        // Hardware does not expose NX; remember that NO_EXECUTE must remain unset.
        crate::kwarn!("CPU does not report NX support; skipping NXE enable");
        NXE_CHECKED.store(true, AtomicOrdering::Relaxed);
        NXE_AVAILABLE.store(false, AtomicOrdering::Relaxed);
        return;
    }

    unsafe {
        enable_nxe();
    }
}

fn cpu_supports_nx() -> bool {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let result = core::arch::x86_64::__cpuid(0x8000_0001);
        (result.edx & (1 << 20)) != 0
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

unsafe fn enable_nxe() {
    use x86_64::registers::model_specific::Msr;

    const IA32_EFER: u32 = 0xC000_0080;
    const EFER_NXE: u64 = 1 << 11;

    let mut msr = Msr::new(IA32_EFER);
    let mut value = msr.read();

    if (value & EFER_NXE) == 0 {
        value |= EFER_NXE;
        msr.write(value);
        crate::kinfo!("Enabled NXE bit in IA32_EFER");
    } else {
        crate::kdebug!("IA32_EFER.NXE already set");
    }

    NXE_CHECKED.store(true, AtomicOrdering::Relaxed);
    NXE_AVAILABLE.store(true, AtomicOrdering::Relaxed);
}

fn nxe_supported() -> bool {
    NXE_AVAILABLE.load(AtomicOrdering::Relaxed)
}

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
    use x86_64::structures::paging::{PageTableFlags, PhysFrame};

    crate::kinfo!("Setting up fresh identity-mapped paging structures");

    const IDENTITY_MAP_GB: usize = 4;

    // Prepare a clean PML4 that we fully control.
    KERNEL_PML4.reset();
    let new_pml4 = &mut *KERNEL_PML4.as_mut_ptr();
    for entry in new_pml4.iter_mut() {
        entry.set_unused();
    }

    // Allocate a PDP table for the lower canonical region.
    let identity_pdp_holder = allocate_extra_table().expect("out of paging tables for PDP");
    let identity_pdp = &mut *identity_pdp_holder.as_mut_ptr();
    identity_pdp_holder.reset();

    let pd_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    new_pml4[0].set_addr(identity_pdp_holder.phys_addr(), pd_flags);

    // Map the first N gigabytes using 2 MiB huge pages.
    for gb in 0..IDENTITY_MAP_GB {
        let pd_holder = allocate_extra_table().expect("out of paging tables for PD");
        pd_holder.reset();
        let pd = &mut *pd_holder.as_mut_ptr();

        for entry in 0..512 {
            let phys = ((gb * 512 + entry) as u64) * 0x200000u64;
            pd[entry].set_addr(
                PhysAddr::new(phys),
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::HUGE_PAGE
                    | PageTableFlags::GLOBAL,
            );
        }

        identity_pdp[gb].set_addr(pd_holder.phys_addr(), pd_flags);
    }

    // Provide a recursive mapping slot near the top of the address space for diagnostics.
    let recursive_index = 510usize;
    new_pml4[recursive_index].set_addr(KERNEL_PML4.phys_addr(), pd_flags);

    // Switch to the freshly prepared paging structures.
    let new_frame =
        PhysFrame::from_start_address(KERNEL_PML4.phys_addr()).expect("aligned PML4 frame");
    Cr3::write(new_frame, Cr3Flags::empty());
    crate::kinfo!(
        "Switched CR3 to new identity root at {:#x}",
        new_frame.start_address().as_u64()
    );

    // Map user program virtual addresses (code, heap, stack) to physical addresses
    // User program expects to run at virtual address 0x200000, with heap and stack
    // residing up to 0x700000. We map the region with 2 MiB huge pages so the
    // stack (at 0x600000-0x700000) has backing memory and retains user access.
    map_user_program();

    // Map VGA buffer for kernel output
    map_vga_buffer();

    // Flush CR3 to ensure the CPU observes updated mappings
    let (pml4_frame_flush, _) = Cr3::read();
    Cr3::write(pml4_frame_flush, Cr3Flags::empty());

    crate::kinfo!("User page tables initialized with USER_ACCESSIBLE permissions");
}

/// Map user program virtual addresses to physical addresses
unsafe fn map_user_program() {
    use crate::process::{USER_PHYS_BASE, USER_REGION_SIZE, USER_VIRT_BASE};
    use x86_64::registers::control::Cr3;
    use x86_64::structures::paging::{PageTable, PageTableFlags};
    use x86_64::PhysAddr;

    const HUGE_PAGE_SIZE: u64 = 0x200000; // 2 MiB

    crate::kinfo!(
        "Mapping user region: virtual {:#x}-{:#x} -> physical {:#x}-{:#x}",
        USER_VIRT_BASE,
        USER_VIRT_BASE + USER_REGION_SIZE,
        USER_PHYS_BASE,
        USER_PHYS_BASE + USER_REGION_SIZE
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

    for offset in (0..USER_REGION_SIZE).step_by(HUGE_PAGE_SIZE as usize) {
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
        crate::kinfo!("VGA mapped and marked ready (huge page)");
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
        crate::kinfo!("VGA mapped and marked ready (PD huge page)");
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
    crate::kinfo!("VGA mapped and marked ready (page table)");
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

/// Map a physical device region into the kernel's virtual address space using
/// identity mapping and 2 MiB huge pages. Returns the virtual address (which
/// equals the physical start on success).
pub unsafe fn map_device_region(phys_start: u64, length: usize) -> Result<*mut u8, MapDeviceError> {
    use x86_64::registers::control::{Cr3, Cr3Flags};
    use x86_64::structures::paging::PageTableFlags;

    ensure_nxe_enabled();

    if length == 0 {
        return Ok(phys_start as *mut u8);
    }

    let (pml4_frame, _) = Cr3::read();
    let pml4_addr = pml4_frame.start_address();
    let pml4 = &mut *(pml4_addr.as_u64() as *mut PageTable);

    let huge_page = 0x200000u64;
    let start = phys_start & !(huge_page - 1);
    let end = (phys_start + length as u64 + huge_page - 1) & !(huge_page - 1);

    for addr in (start..end).step_by(huge_page as usize) {
        let virt = addr;
        let pml4_index = ((virt >> 39) & 0x1FF) as usize;
        let pdp_index = ((virt >> 30) & 0x1FF) as usize;
        let pd_index = ((virt >> 21) & 0x1FF) as usize;

        if pml4[pml4_index].is_unused() {
            let table = allocate_extra_table().ok_or(MapDeviceError::OutOfTableSpace)?;
            pml4[pml4_index].set_addr(
                table.phys_addr(),
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            );
        }

        let pdp_ptr = pml4[pml4_index].addr().as_u64() as *mut PageTable;
        let pdp = &mut *pdp_ptr;

        if pdp[pdp_index].flags().contains(PageTableFlags::HUGE_PAGE) {
            // Already covered by a 1 GiB huge page, nothing else to do.
            continue;
        }

        if pdp[pdp_index].is_unused() {
            let table = allocate_extra_table().ok_or(MapDeviceError::OutOfTableSpace)?;
            pdp[pdp_index].set_addr(
                table.phys_addr(),
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            );
        }

        let pd_ptr = pdp[pdp_index].addr().as_u64() as *mut PageTable;
        let pd = &mut *pd_ptr;

        let entry = &mut pd[pd_index];
        if entry.is_unused() {
            let mut flags = PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::HUGE_PAGE
                | PageTableFlags::WRITE_THROUGH
                | PageTableFlags::NO_CACHE;

            if nxe_supported() {
                flags |= PageTableFlags::NO_EXECUTE;
            }

            entry.set_addr(PhysAddr::new(addr), flags);
        }
    }

    let (flush_frame, _) = Cr3::read();
    Cr3::write(flush_frame, Cr3Flags::empty());

    Ok(phys_start as *mut u8)
}
