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
// Move user region start to 320MB (0x1400_0000) to safely skip over the initramfs
// which appears to be loaded around 240MB-250MB.
// This gives us space from 320MB to 512MB (End of RAM) for user processes.
static NEXT_USER_REGION: AtomicU64 = AtomicU64::new(0x2000_0000); // Start at 512MB, after rootfs (which ends at ~361MB)

// CR3 allocation statistics for debugging and monitoring
static CR3_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static CR3_ACTIVATIONS: AtomicU64 = AtomicU64::new(0);
static CR3_FREES: AtomicU64 = AtomicU64::new(0);

// Demand paging statistics
static DEMAND_PAGE_FAULTS: AtomicU64 = AtomicU64::new(0);
static DEMAND_PAGES_ALLOCATED: AtomicU64 = AtomicU64::new(0);

// Simple free list for user regions
// Each entry is (base_address, size) - we use 0 to indicate an empty slot
const MAX_FREE_REGIONS: usize = 64;
static FREE_USER_REGIONS: spin::Mutex<[(u64, u64); MAX_FREE_REGIONS]> =
    spin::Mutex::new([(0, 0); MAX_FREE_REGIONS]);
// Statistics for user region allocation
static USER_REGIONS_ALLOCATED: AtomicU64 = AtomicU64::new(0);
static USER_REGIONS_FREED: AtomicU64 = AtomicU64::new(0);

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
///
/// # Arguments
/// * `phys_base` - Physical base address for user memory
/// * `size` - Size of user memory region
/// * `demand_paging` - If true, pages are mapped on-demand via page faults.
///                     If false, all pages are mapped immediately (required for fork).
pub fn create_process_address_space(
    phys_base: u64,
    size: u64,
    demand_paging: bool,
) -> Result<u64, &'static str> {
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
        let kernel_pd_addr = pdp[user_pdp_index].addr();
        let kernel_pd = phys_to_page_table(kernel_pd_addr);
        crate::ktrace!(
            "create_process_address_space: cloning from kernel_pd at {:#x} to new pd at {:#x}",
            kernel_pd_addr.as_u64(),
            pd_phys.as_u64()
        );

        clone_table(pd, kernel_pd);
    }

    let mut pdp_flags = pdp[user_pdp_index].flags();
    pdp_flags |=
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    pdp[user_pdp_index].set_addr(pd_phys, pdp_flags);

    let page_count = ((aligned_size + HUGE_PAGE_SIZE - 1) / HUGE_PAGE_SIZE).max(1);

    if demand_paging {
        // DEMAND PAGING MODE (按需分配模式):
        // Don't map pages immediately. Instead, clear PD entries so page faults
        // will trigger on first access. The page fault handler will map pages
        // on-demand via handle_user_demand_fault().
        //
        // Clear user region PD entries to ensure "not present" page faults
        // USER_VIRT_BASE = 0x1000000, PD index = 8
        // Clear entries 8 through 8+page_count to cover the user region
        let user_pd_start = ((USER_VIRT_BASE >> 21) & 0x1FF) as usize; // = 8
        for i in 0..(page_count as usize) {
            let pd_idx = user_pd_start + i;
            if pd_idx < 512 {
                // Clear the entry - no PRESENT flag means page fault on access
                pd[pd_idx].set_addr(PhysAddr::new(0), PageTableFlags::empty());
            }
        }

        crate::ktrace!(
            "create_process_address_space: DEMAND PAGING mode, {} pages will be mapped on-demand (phys_base={:#x})",
            page_count,
            phys_base
        );
    } else {
        // IMMEDIATE MAPPING MODE (立即映射模式):
        // Map all pages immediately. Required for fork() where memory has already
        // been copied to physical addresses.
        for page in 0..page_count {
            let offset = page * HUGE_PAGE_SIZE;
            let virt_addr = USER_VIRT_BASE + offset;
            let phys_addr = phys_base + offset;
            let pd_index = ((virt_addr >> 21) & 0x1FF) as usize;

            let flags = PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE
                | PageTableFlags::HUGE_PAGE;

            pd[pd_index].set_addr(PhysAddr::new(phys_addr), flags);
        }

        crate::ktrace!(
            "create_process_address_space: IMMEDIATE mode, {} pages mapped (phys_base={:#x})",
            page_count,
            phys_base
        );
    }

    // Increment allocation counter for monitoring
    CR3_ALLOCATIONS.fetch_add(1, AtomicOrdering::Relaxed);

    Ok(pml4_phys.as_u64())
}

/// Activate the address space represented by the supplied CR3 physical address.
/// Passing 0 selects the kernel's bootstrap page tables.
///
/// # Safety and Validation
/// - CR3 must be 4KB-aligned (bits 0-11 must be zero)
/// - CR3 must point to a valid PML4 table in physical memory
/// - This function validates CR3 before activation to prevent GP faults
///
/// # Context Switching
/// This function is called during:
/// 1. Process execution (Process::execute)
/// 2. Context switches (scheduler::do_schedule)
/// 3. Returning to kernel space (scheduler::set_current_pid(None))
pub extern "C" fn activate_address_space(cr3_phys: u64) {
    use x86_64::registers::control::{Cr3, Cr3Flags};
    use x86_64::structures::paging::Size4KiB;

    crate::kdebug!("[activate_address_space] ENTRY: cr3_phys={:#x}", cr3_phys);

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
            crate::kwarn!(
                "[WARN] CR3 {:#x} is in very high physical address range",
                cr3_phys
            );
        }

        // CRITICAL: Verify page table content is valid before activating
        // Read the first few entries from the PML4 to ensure they're sensible
        let pml4_ptr = cr3_phys as *const u64;
        unsafe {
            let entry0 = core::ptr::read_volatile(pml4_ptr);
            let entry1 = core::ptr::read_volatile(pml4_ptr.add(1));
            crate::kdebug!(
                "[activate_address_space] PML4 content check: entry[0]={:#x}, entry[1]={:#x}",
                entry0,
                entry1
            );
            // Check if entries look valid (present bit should be set for some entries)
            if entry0 == 0 && entry1 == 0 {
                crate::kerror!(
                    "[CRITICAL] CR3 {:#x} points to all-zero page table!",
                    cr3_phys
                );
                crate::kfatal!("Page table corrupted - all entries are zero");
            }
        }
    }

    let target = if cr3_phys == 0 {
        kernel_pml4_phys()
    } else {
        cr3_phys
    };

    crate::kdebug!("[activate_address_space] Target CR3={:#x}", target);

    let (current, _) = Cr3::read();
    crate::kdebug!(
        "[activate_address_space] Current CR3={:#x}, Target CR3={:#x}",
        current.start_address().as_u64(),
        target
    );

    if current.start_address().as_u64() == target {
        crate::kdebug!("[activate_address_space] CR3 already active, returning");
        return; // Short-circuit: CR3 already active, no need to reload
    }

    // Validate the frame creation - convert physical address to PhysFrame
    crate::kdebug!(
        "[activate_address_space] Creating PhysFrame from target={:#x}",
        target
    );
    let frame_result: Result<PhysFrame<Size4KiB>, _> =
        PhysFrame::from_start_address(PhysAddr::new(target));
    if frame_result.is_err() {
        crate::kerror!("[CRITICAL] Cannot create PhysFrame from CR3 {:#x}", target);
        crate::kfatal!("PhysFrame creation failed - possible invalid CR3");
    }

    let frame: PhysFrame<Size4KiB> = frame_result.expect("PhysFrame validation already checked");

    // Write CR3 using x86_64 crate
    unsafe {
        Cr3::write(frame, Cr3Flags::empty());
    }

    // Increment activation counter for monitoring
    CR3_ACTIVATIONS.fetch_add(1, AtomicOrdering::Relaxed);
}

/// Read the current CR3 value from the CPU.
/// Returns the physical address of the currently active PML4 table.
///
/// # Usage
/// This function is useful for:
/// - Debugging page table issues
/// - Verifying context switches completed correctly
/// - Auditing which address space is currently active
pub fn read_current_cr3() -> u64 {
    use x86_64::registers::control::Cr3;
    let (frame, _) = Cr3::read();
    frame.start_address().as_u64()
}

/// Validate that a CR3 value is well-formed and safe to use.
/// Returns Ok(()) if valid, Err with description if invalid.
///
/// # Validation Checks
/// 1. Page alignment (4KB boundary, bits 0-11 zero)
/// 2. Physical address sanity (not beyond reasonable RAM limit)
/// 3. Not zero (unless explicitly allowed for kernel PT)
pub fn validate_cr3(cr3: u64, allow_zero: bool) -> Result<(), &'static str> {
    if cr3 == 0 {
        if allow_zero {
            return Ok(());
        } else {
            return Err("CR3 is zero but zero not allowed in this context");
        }
    }

    // Check 4KB alignment
    if cr3 & 0xFFF != 0 {
        return Err("CR3 is not 4KB-aligned");
    }

    // Sanity check: CR3 should be in reasonable physical RAM range
    // We allow up to 4GB here, but warn if beyond 1GB
    if cr3 >= 0x1_0000_0000 {
        return Err("CR3 exceeds 4GB physical address limit");
    }

    Ok(())
}

/// Free a process-specific address space.
/// This should be called when a process exits to reclaim page table memory.
///
/// # Safety
/// - Must not free the kernel's page tables (cr3 == 0 or kernel_pml4_phys())
/// - Must not free the currently active CR3 (switch to kernel PT first)
/// - Only frees the PML4, PDP, and PD; assumes pages are managed separately
///
/// # Current Limitations
/// Currently this is a placeholder. Full implementation requires:
/// 1. Walking the page table hierarchy
/// 2. Freeing all allocated page table pages (PML4, PDP, PD)
/// 3. Ensuring no dangling references exist
///
/// # TODO
/// Implement proper page table deallocation to prevent memory leaks
pub fn free_process_address_space(cr3: u64) {
    if cr3 == 0 || cr3 == kernel_pml4_phys() {
        crate::kwarn!("Attempted to free kernel page tables, ignoring");
        return;
    }

    let current_cr3 = read_current_cr3();
    if cr3 == current_cr3 {
        crate::kerror!("Cannot free currently active CR3 {:#x}", cr3);
        crate::kfatal!("Attempted to free active page tables");
    }

    // Increment free counter for monitoring
    CR3_FREES.fetch_add(1, AtomicOrdering::Relaxed);

    // TODO: Implement proper page table deallocation
    // For now, just log that we should free this
    crate::kdebug!("TODO: Free page tables at CR3 {:#x}", cr3);

    // In a full implementation, we would:
    // 1. Walk the PML4 entries for user space (lower half)
    // 2. For each present PDP, walk its entries
    // 3. For each present PD, walk its entries
    // 4. Free PT pages (if using 4KB pages)
    // 5. Free PD pages
    // 6. Free PDP pages
    // 7. Free PML4 page
    // 8. Return freed frames to the frame allocator
}

/// Debug helper: print information about a CR3 value and its page table structure.
/// This is useful for diagnosing page table corruption or misconfigurations.
///
/// # Output
/// Prints to kernel log:
/// - CR3 physical address and validation status
/// - Whether this is kernel or user space PT
/// - PML4 entry count for user space (if accessible)
pub fn debug_cr3_info(cr3: u64, label: &str) {
    crate::kinfo!("=== CR3 Debug Info: {} ===", label);
    crate::kinfo!("  CR3 Physical Address: {:#x}", cr3);

    match validate_cr3(cr3, true) {
        Ok(()) => crate::kinfo!("  Validation: OK"),
        Err(e) => crate::kerror!("  Validation: FAILED - {}", e),
    }

    if cr3 == 0 {
        crate::kinfo!("  Type: Zero (will use kernel PT)");
    } else if cr3 == kernel_pml4_phys() {
        crate::kinfo!("  Type: Kernel PML4");
    } else {
        crate::kinfo!("  Type: User process PT");

        // Try to read PML4 entry count (careful, may not be mapped)
        let pml4 = phys_to_page_table(PhysAddr::new(cr3));
        let mut present_count = 0;
        let mut user_entries = 0;

        for (i, entry) in pml4.iter().enumerate() {
            if !entry.is_unused() {
                present_count += 1;
                if i < 256 {
                    // Lower half = user space
                    user_entries += 1;
                }
            }
        }

        crate::kinfo!(
            "  PML4 entries: {} total, {} user space",
            present_count,
            user_entries
        );
    }

    let current = read_current_cr3();
    if cr3 == current {
        crate::kinfo!("  Status: ACTIVE (currently loaded in CPU)");
    } else {
        crate::kinfo!("  Status: Inactive (current CR3={:#x})", current);
    }

    crate::kinfo!("=== End CR3 Debug Info ===");
}

/// Print CR3 allocation statistics.
/// Useful for monitoring memory usage and detecting leaks.
pub fn print_cr3_statistics() {
    let allocs = CR3_ALLOCATIONS.load(AtomicOrdering::Relaxed);
    let activations = CR3_ACTIVATIONS.load(AtomicOrdering::Relaxed);
    let frees = CR3_FREES.load(AtomicOrdering::Relaxed);

    crate::kinfo!("=== CR3 Statistics ===");
    crate::kinfo!("  Total CR3 allocations: {}", allocs);
    crate::kinfo!("  Total CR3 activations: {}", activations);
    crate::kinfo!("  Total CR3 frees: {}", frees);
    crate::kinfo!(
        "  Active CR3s (allocs - frees): {}",
        allocs.saturating_sub(frees)
    );

    if allocs > frees {
        let leaked = allocs - frees;
        crate::kwarn!("  WARNING: {} CR3(s) may be leaked!", leaked);
    }

    crate::kinfo!("=== End CR3 Statistics ===");
}

/// Simple bump allocator for user-visible physical regions.
/// First checks the free list for a suitable region, then falls back to bump allocation.
pub fn allocate_user_region(size: u64) -> Option<u64> {
    if size == 0 {
        return None;
    }

    const ALIGN: u64 = 0x200000; // 2 MiB pages
    let aligned_size = (size + ALIGN - 1) & !(ALIGN - 1);

    // First, try to find a suitable region in the free list
    {
        let mut free_list = FREE_USER_REGIONS.lock();
        for slot in free_list.iter_mut() {
            if slot.0 != 0 && slot.1 >= aligned_size {
                let base = slot.0;
                let old_size = slot.1;

                if old_size == aligned_size {
                    // Exact match - remove from free list
                    *slot = (0, 0);
                } else {
                    // Partial match - shrink the free region
                    slot.0 += aligned_size;
                    slot.1 -= aligned_size;
                }

                // Zero the memory before returning
                unsafe {
                    core::ptr::write_bytes(base as *mut u8, 0, aligned_size as usize);
                }

                USER_REGIONS_ALLOCATED.fetch_add(1, AtomicOrdering::Relaxed);
                crate::kinfo!(
                    "allocate_user_region: reused {} bytes at {:#x} from free list",
                    aligned_size,
                    base
                );
                return Some(base);
            }
        }
    }

    // No suitable free region found, use bump allocator
    let base = NEXT_USER_REGION.fetch_add(aligned_size, AtomicOrdering::SeqCst);
    if base.checked_add(aligned_size).unwrap_or(u64::MAX) > 0x1_0000_0000 {
        crate::kerror!("allocate_user_region: out of physical memory");
        return None;
    }

    unsafe {
        core::ptr::write_bytes(base as *mut u8, 0, aligned_size as usize);
    }

    USER_REGIONS_ALLOCATED.fetch_add(1, AtomicOrdering::Relaxed);
    crate::kdebug!(
        "allocate_user_region: allocated {} bytes at {:#x}",
        aligned_size,
        base
    );

    Some(base)
}

/// Free a user region back to the free list for reuse.
/// The memory should no longer be in use by any process.
pub fn free_user_region(base: u64, size: u64) {
    if base == 0 || size == 0 {
        return;
    }

    // Don't free the initial shared USER_PHYS_BASE region (used by first process)
    if base == crate::process::USER_PHYS_BASE {
        crate::kdebug!(
            "free_user_region: skipping initial USER_PHYS_BASE {:#x}",
            base
        );
        return;
    }

    const ALIGN: u64 = 0x200000; // 2 MiB pages
    let aligned_size = (size + ALIGN - 1) & !(ALIGN - 1);

    let mut free_list = FREE_USER_REGIONS.lock();

    // Try to find an empty slot in the free list
    for slot in free_list.iter_mut() {
        if slot.0 == 0 {
            *slot = (base, aligned_size);
            USER_REGIONS_FREED.fetch_add(1, AtomicOrdering::Relaxed);
            crate::kinfo!(
                "free_user_region: freed {} bytes at {:#x}",
                aligned_size,
                base
            );
            return;
        }
    }

    // Free list is full - log a warning but don't leak memory tracking
    // The memory is still "freed" in the sense that we won't use it,
    // but we can't reuse it until a slot opens up
    crate::kwarn!(
        "free_user_region: free list full, cannot track freed region at {:#x} ({} bytes)",
        base,
        aligned_size
    );
}

/// Print user region allocation statistics
pub fn print_user_region_statistics() {
    let allocated = USER_REGIONS_ALLOCATED.load(AtomicOrdering::Relaxed);
    let freed = USER_REGIONS_FREED.load(AtomicOrdering::Relaxed);

    crate::kinfo!("=== User Region Statistics ===");
    crate::kinfo!("  Total allocations: {}", allocated);
    crate::kinfo!("  Total frees: {}", freed);
    crate::kinfo!("  Active regions: {}", allocated.saturating_sub(freed));

    let free_list = FREE_USER_REGIONS.lock();
    let mut free_count = 0;
    let mut free_bytes = 0u64;
    for slot in free_list.iter() {
        if slot.0 != 0 {
            free_count += 1;
            free_bytes += slot.1;
        }
    }
    crate::kinfo!(
        "  Free list: {} entries, {} bytes available",
        free_count,
        free_bytes
    );
    crate::kinfo!("=== End User Region Statistics ===");
}

// =============================================================================
// Demand Paging (按需分配) Implementation
// =============================================================================

/// Check if a virtual address is within the user space region that supports demand paging.
/// Clear all user-space page table mappings in the specified CR3.
/// This is used by execve to ensure that demand paging will create fresh mappings
/// for the newly loaded program.
///
/// # Safety
/// - cr3 must point to a valid PML4 table
/// - This function modifies page tables and must be called with appropriate locks held
pub unsafe fn clear_user_mappings(cr3: u64) {
    use crate::process::{USER_VIRT_BASE, INTERP_BASE, INTERP_REGION_SIZE};
    use x86_64::structures::paging::{PageTable, PageTableFlags};

    const HUGE_PAGE_SIZE: u64 = 0x200000; // 2 MiB

    // Get the PML4 table
    let pml4 = &mut *(cr3 as *mut PageTable);

    // Calculate the range of virtual addresses to clear
    let user_start = USER_VIRT_BASE;
    let user_end = INTERP_BASE + INTERP_REGION_SIZE;

    // Iterate through all huge pages in the user region and clear their mappings
    let mut virt = user_start;
    while virt < user_end {
        // Get PML4 index (bits 39-47)
        let pml4_idx = ((virt >> 39) & 0x1FF) as usize;

        // Check if PML4 entry is present
        if !pml4[pml4_idx].flags().contains(PageTableFlags::PRESENT) {
            virt += HUGE_PAGE_SIZE;
            continue;
        }

        // Get PDP table
        let pdp_addr = pml4[pml4_idx].addr().as_u64();
        let pdp = &mut *(pdp_addr as *mut PageTable);

        // Get PDP index (bits 30-38)
        let pdp_idx = ((virt >> 30) & 0x1FF) as usize;

        // Check if PDP entry is present
        if !pdp[pdp_idx].flags().contains(PageTableFlags::PRESENT) {
            virt += HUGE_PAGE_SIZE;
            continue;
        }

        // Get PD table
        let pd_addr = pdp[pdp_idx].addr().as_u64();
        let pd = &mut *(pd_addr as *mut PageTable);

        // Get PD index (bits 21-29)
        let pd_idx = ((virt >> 21) & 0x1FF) as usize;

        // Check if PD entry is present
        if pd[pd_idx].flags().contains(PageTableFlags::PRESENT) {
            // Clear the PD entry
            pd[pd_idx].set_unused();
        }

        virt += HUGE_PAGE_SIZE;
    }

    // Flush TLB to ensure the cleared mappings take effect
    use x86_64::instructions::tlb;
    tlb::flush_all();
}

/// Returns true if the address is in the user region (USER_VIRT_BASE to USER_VIRT_BASE + USER_REGION_SIZE).
pub fn is_user_demand_page_address(virt_addr: u64) -> bool {
    use crate::process::{USER_VIRT_BASE, USER_REGION_SIZE};
    virt_addr >= USER_VIRT_BASE && virt_addr < USER_VIRT_BASE + USER_REGION_SIZE
}

/// Handle a user-space page fault for demand paging.
/// This function is called from the page fault handler when a user-mode process
/// accesses an unmapped page in its address space.
///
/// # Arguments
/// * `fault_addr` - The virtual address that caused the page fault
/// * `pid` - The process ID of the faulting process
/// * `cr3` - The CR3 (page table root) of the process
/// * `memory_base` - The physical base address of the process's pre-allocated memory region
///
/// # Returns
/// * `Ok(())` - If the page was successfully mapped
/// * `Err(&'static str)` - If the page could not be mapped (e.g., out of bounds)
///
/// # Safety
/// This function modifies page tables and must only be called from the page fault handler
/// in an appropriate context (interrupts may be disabled).
pub fn handle_user_demand_fault(
    fault_addr: u64,
    _pid: u64,
    cr3: u64,
    memory_base: u64,
) -> Result<(), &'static str> {
    use crate::process::{USER_VIRT_BASE, USER_REGION_SIZE};

    // Validate the fault address is within user space
    if !is_user_demand_page_address(fault_addr) {
        return Err("Fault address not in user demand page region");
    }

    // Calculate the page-aligned virtual address
    const HUGE_PAGE_SIZE: u64 = 0x200000; // 2 MiB
    let page_virt = fault_addr & !(HUGE_PAGE_SIZE - 1);

    // Calculate the offset from USER_VIRT_BASE
    let offset = page_virt - USER_VIRT_BASE;

    // Ensure the offset is within the allocated region
    if offset >= USER_REGION_SIZE {
        return Err("Demand fault offset exceeds user region size");
    }

    // Calculate the corresponding physical address
    // The physical memory is pre-allocated at memory_base, so we just need
    // to map the virtual page to the corresponding physical page
    let page_phys = memory_base + offset;

    // Map the page in the process's page table
    unsafe {
        map_user_page_in_cr3(cr3, page_virt, page_phys)?;
    }

    // Update statistics
    DEMAND_PAGE_FAULTS.fetch_add(1, AtomicOrdering::Relaxed);
    DEMAND_PAGES_ALLOCATED.fetch_add(1, AtomicOrdering::Relaxed);

    // Flush the TLB entry for this address
    use x86_64::instructions::tlb;
    use x86_64::VirtAddr;
    tlb::flush(VirtAddr::new(page_virt));

    Ok(())
}

/// Map a single 2 MiB huge page in the specified CR3's page table.
/// This is used for demand paging to map pages on-demand.
///
/// # Safety
/// - cr3 must point to a valid PML4 table
/// - The page table structure for the user region must already exist (PML4 -> PDP -> PD)
unsafe fn map_user_page_in_cr3(cr3: u64, virt_addr: u64, phys_addr: u64) -> Result<(), &'static str> {
    use x86_64::structures::paging::{PageTable, PageTableFlags};

    let pml4 = &mut *(cr3 as *mut PageTable);

    // Calculate page table indices
    let pml4_index = ((virt_addr >> 39) & 0x1FF) as usize;
    let pdp_index = ((virt_addr >> 30) & 0x1FF) as usize;
    let pd_index = ((virt_addr >> 21) & 0x1FF) as usize;

    // Navigate to PDP
    if pml4[pml4_index].is_unused() {
        return Err("PML4 entry for user region is not present");
    }
    let pdp = &mut *(pml4[pml4_index].addr().as_u64() as *mut PageTable);

    // Navigate to PD
    if pdp[pdp_index].is_unused() {
        return Err("PDP entry for user region is not present");
    }
    let pd = &mut *(pdp[pdp_index].addr().as_u64() as *mut PageTable);

    // Check if already mapped (should not happen for demand paging)
    if !pd[pd_index].is_unused() {
        crate::kwarn!(
            "demand_fault: page at {:#x} already mapped to {:#x}",
            virt_addr,
            pd[pd_index].addr().as_u64()
        );
        return Ok(()); // Already mapped, nothing to do
    }

    // Set up the page table entry with user-accessible huge page
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::HUGE_PAGE;

    pd[pd_index].set_addr(PhysAddr::new(phys_addr), flags);

    crate::kdebug!(
        "demand_fault: mapped PD[{}] = virt {:#x} -> phys {:#x}",
        pd_index,
        virt_addr,
        phys_addr
    );

    Ok(())
}

/// Print demand paging statistics
pub fn print_demand_paging_statistics() {
    let faults = DEMAND_PAGE_FAULTS.load(AtomicOrdering::Relaxed);
    let allocated = DEMAND_PAGES_ALLOCATED.load(AtomicOrdering::Relaxed);

    crate::kinfo!("=== Demand Paging Statistics ===");
    crate::kinfo!("  Total page faults handled: {}", faults);
    crate::kinfo!("  Total pages allocated on-demand: {}", allocated);
    crate::kinfo!("=== End Demand Paging Statistics ===");
}
