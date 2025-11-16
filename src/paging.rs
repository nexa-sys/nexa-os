/// Memory paging setup for x86_64
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use x86_64::structures::paging::{PageTable, PhysFrame};
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

static NEXT_PT_FRAME: AtomicU64 = AtomicU64::new(0x0800_0000);
static NEXT_USER_REGION: AtomicU64 = AtomicU64::new(0x1000_0000);

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

fn phys_to_page_table_mut(addr: PhysAddr) -> &'static mut PageTable {
    unsafe { &mut *(addr.as_u64() as *mut PageTable) }
}

fn phys_to_page_table(addr: PhysAddr) -> &'static PageTable {
    unsafe { &*(addr.as_u64() as *const PageTable) }
}

fn alloc_page_table_frame() -> Result<PhysAddr, &'static str> {
    const FRAME_SIZE: u64 = 0x1000;
    let addr = NEXT_PT_FRAME.fetch_add(FRAME_SIZE, AtomicOrdering::SeqCst);
    if addr.checked_add(FRAME_SIZE).unwrap_or(u64::MAX) > 0x1_0000_0000 {
        return Err("Out of page table frames");
    }

    unsafe {
        core::ptr::write_bytes(addr as *mut u8, 0, FRAME_SIZE as usize);
    }

    Ok(PhysAddr::new(addr))
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

/// Return the physical address of the currently loaded PML4.
pub fn current_pml4_phys() -> u64 {
    use x86_64::registers::control::Cr3;

    let (frame, _) = Cr3::read();
    frame.start_address().as_u64()
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

    // Map VGA buffer for kernel output
    map_vga_buffer();

    // Flush CR3 to ensure the CPU observes updated mappings
    let (pml4_frame_flush, _) = Cr3::read();
    Cr3::write(pml4_frame_flush, Cr3Flags::empty());

    crate::kinfo!("User page tables initialized with USER_ACCESSIBLE permissions");
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
    map_device_region_internal(phys_start, length, false)
}

/// Map a physical device region and mark it user-accessible so Ring3 code can
/// touch the MMIO window (still uncached + non-executable).
pub unsafe fn map_user_device_region(
    phys_start: u64,
    length: usize,
) -> Result<*mut u8, MapDeviceError> {
    map_device_region_internal(phys_start, length, true)
}

unsafe fn map_device_region_internal(
    phys_start: u64,
    length: usize,
    user_accessible: bool,
) -> Result<*mut u8, MapDeviceError> {
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

            if user_accessible {
                flags |= PageTableFlags::USER_ACCESSIBLE;
            }

            if nxe_supported() {
                flags |= PageTableFlags::NO_EXECUTE;
            }

            entry.set_addr(PhysAddr::new(addr), flags);
        } else if user_accessible {
            // Upgrade existing mapping to user accessible if necessary.
            let mut flags = entry.flags() | PageTableFlags::USER_ACCESSIBLE;
            if nxe_supported() {
                flags |= PageTableFlags::NO_EXECUTE;
            }
            entry.set_flags(flags);
        }
    }

    let (flush_frame, _) = Cr3::read();
    Cr3::write(flush_frame, Cr3Flags::empty());

    Ok(phys_start as *mut u8)
}

/// Physical address of the kernel's PML4 root table.
pub fn kernel_pml4_phys() -> u64 {
    KERNEL_PML4.phys_addr().as_u64()
}

fn clone_table(dst: &mut PageTable, src: &PageTable) {
    unsafe {
        core::ptr::copy_nonoverlapping(src as *const PageTable, dst as *mut PageTable, 1);
    }
}

/// Create a process-specific address space rooted at a new PML4.
pub fn create_process_address_space(phys_base: u64, size: u64) -> Result<u64, &'static str> {
    use crate::process::USER_VIRT_BASE;
    use x86_64::structures::paging::PageTableFlags;

    if size == 0 {
        return Err("Process region size must be non-zero");
    }

    const HUGE_PAGE_SIZE: u64 = 0x200000;
    let aligned_size = (size + HUGE_PAGE_SIZE - 1) & !(HUGE_PAGE_SIZE - 1);

    let kernel_root = unsafe { &*KERNEL_PML4.as_ptr() };
    let pml4_phys = alloc_page_table_frame()?;
    let pml4 = phys_to_page_table_mut(pml4_phys);

    clone_table(pml4, kernel_root);

    let user_pml4_index = ((USER_VIRT_BASE >> 39) & 0x1FF) as usize;
    let kernel_pml4_entry = kernel_root[user_pml4_index].clone();

    // Clone the PDP that covers the lower canonical address range so we can
    // customize user-accessible regions without mutating the kernel template.
    let pdp_phys = alloc_page_table_frame()?;
    let pdp = phys_to_page_table_mut(pdp_phys);

    if !kernel_pml4_entry.is_unused() {
        let kernel_pdp = phys_to_page_table(kernel_pml4_entry.addr());
        clone_table(pdp, kernel_pdp);
    }

    let mut pml4_flags = kernel_pml4_entry.flags();
    pml4_flags |=
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    pml4[user_pml4_index].set_addr(pdp_phys, pml4_flags);

    let user_pdp_index = ((USER_VIRT_BASE >> 30) & 0x1FF) as usize;

    // Allocate a private PD for the 1 GiB slice that contains user mappings.
    let pd_phys = alloc_page_table_frame()?;
    let pd = phys_to_page_table_mut(pd_phys);

    if !pdp[user_pdp_index].is_unused() {
        let kernel_pd = phys_to_page_table(pdp[user_pdp_index].addr());
        clone_table(pd, kernel_pd);
    }

    let mut pdp_flags = pdp[user_pdp_index].flags();
    pdp_flags |=
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    pdp[user_pdp_index].set_addr(pd_phys, pdp_flags);

    let page_count = ((aligned_size + HUGE_PAGE_SIZE - 1) / HUGE_PAGE_SIZE).max(1);

    for page in 0..page_count {
        let offset = page * HUGE_PAGE_SIZE;
        let virt_addr = USER_VIRT_BASE + offset;
        let phys_addr = phys_base + offset;
        let pd_index = ((virt_addr >> 21) & 0x1FF) as usize;

        let flags = PageTableFlags::PRESENT
            | PageTableFlags::WRITABLE
            | PageTableFlags::USER_ACCESSIBLE
            | PageTableFlags::HUGE_PAGE;

        if nxe_supported() {
            // Leave executable for now; future work can toggle NO_EXECUTE per segment.
        }

        pd[pd_index].set_addr(PhysAddr::new(phys_addr), flags);
    }

    Ok(pml4_phys.as_u64())
}

/// Activate the address space represented by the supplied CR3 physical address.
/// Passing 0 selects the kernel's bootstrap page tables.
pub fn activate_address_space(cr3_phys: u64) {
    use x86_64::registers::control::{Cr3, Cr3Flags};

    // CRITICAL FIX: Validate CR3 before use to catch fork-related page table errors
    // This prevents GP faults from invalid page table structures
    if cr3_phys != 0 {
        // Ensure CR3 is page-aligned (4KB boundary)
        if cr3_phys & 0xFFF != 0 {
            crate::kerror!("[CRITICAL] CR3 {:#x} is not page-aligned!", cr3_phys);
            crate::kfatal!("CR3 alignment check failed - possible fork page table corruption");
        }
        
        // Sanity check: CR3 should be in physical RAM, not beyond 4GB
        if cr3_phys >= 0x1_0000_0000 {
            crate::kwarn!("[WARN] CR3 {:#x} is in very high physical address range", cr3_phys);
        }
    }

    let target = if cr3_phys == 0 {
        kernel_pml4_phys()
    } else {
        cr3_phys
    };

    let (current, _) = Cr3::read();
    if current.start_address().as_u64() == target {
        return;  // Short-circuit: CR3 already active, no need to reload
    }

    // Validate the frame creation - convert physical address to PhysFrame
    let frame_result = PhysFrame::from_start_address(PhysAddr::new(target));
    if frame_result.is_err() {
        crate::kerror!("[CRITICAL] Cannot create PhysFrame from CR3 {:#x}", target);
        crate::kfatal!("PhysFrame creation failed - possible invalid CR3");
    }
    
    let frame = frame_result.expect("PhysFrame validation already checked");

    unsafe {
        Cr3::write(frame, Cr3Flags::empty());
    }

    crate::serial::_print(format_args!(
        "[activate_address_space] Activated CR3={:#x}\n",
        target
    ));
}

/// Simple bump allocator for user-visible physical regions.
pub fn allocate_user_region(size: u64) -> Option<u64> {
    if size == 0 {
        return None;
    }

    const ALIGN: u64 = 0x200000; // 2 MiB pages
    let aligned_size = (size + ALIGN - 1) & !(ALIGN - 1);
    let base = NEXT_USER_REGION.fetch_add(aligned_size, AtomicOrdering::SeqCst);
    if base.checked_add(aligned_size).unwrap_or(u64::MAX) > 0x1_0000_0000 {
        crate::kerror!("allocate_user_region: out of physical memory");
        return None;
    }

    unsafe {
        core::ptr::write_bytes(base as *mut u8, 0, aligned_size as usize);
    }

    crate::kdebug!(
        "allocate_user_region: allocated {} bytes at {:#x}",
        aligned_size,
        base
    );

    Some(base)
}
