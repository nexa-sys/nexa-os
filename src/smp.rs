use core::mem::{self, MaybeUninit};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use x86_64::instructions::tables::sgdt;
use x86_64::instructions::{hlt as cpu_hlt, interrupts};

use crate::{acpi, bootinfo, gdt, lapic, paging};

// Per-CPU GS data (same layout as initramfs::GsData)
#[repr(C, align(64))]
#[derive(Copy, Clone)]
struct PerCpuGsData([u64; 32]);

impl PerCpuGsData {
    const fn new() -> Self {
        Self([0; 32])
    }
}

// GS data for each CPU (BSP uses initramfs::GS_DATA, APs use these)
static mut AP_GS_DATA: [PerCpuGsData; MAX_CPUS] = [PerCpuGsData::new(); MAX_CPUS];

// Debug: AP arrival flags (non-zero = arrived)
static AP_ARRIVED: [AtomicU32; MAX_CPUS] = [
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
];

pub const MAX_CPUS: usize = acpi::MAX_CPUS;
const TRAMPOLINE_BASE: u64 = 0x8000;
const TRAMPOLINE_MAX_SIZE: usize = 4096;
const AP_STACK_SIZE: usize = 16 * 4096;
const TRAMPOLINE_VECTOR: u8 = (TRAMPOLINE_BASE >> 12) as u8;
const STARTUP_WAIT_LOOPS: u64 = 50_000_000;  // Increased for reliability
const STARTUP_RETRY_MAX: u32 = 3;

// IPI vectors
pub const IPI_RESCHEDULE: u8 = 0xF0;
pub const IPI_TLB_FLUSH: u8 = 0xF1;
pub const IPI_CALL_FUNCTION: u8 = 0xF2;
pub const IPI_HALT: u8 = 0xF3;

// Control AP startup for debugging  
// FIXED: IPI vector mismatch - was using 0xF9 (no handler) instead of 0xF0 (reschedule handler)
// The crash was caused by sending IPI with unregistered vector, causing GP fault!
// Now using correct IPI_RESCHEDULE (0xF0) which has handler in interrupts.rs
/// Configuration: Enable AP startup (set to false to test BSP-only mode)
const ENABLE_AP_STARTUP: bool = true;  // Re-enabled after disabling ALGN check

static SMP_READY: AtomicBool = AtomicBool::new(false);
static TRAMPOLINE_READY: AtomicBool = AtomicBool::new(false);
static CPU_TOTAL: AtomicUsize = AtomicUsize::new(1);
static ONLINE_CPUS: AtomicUsize = AtomicUsize::new(1);

#[repr(C, align(16))]  // x86_64 ABI requires 16-byte stack alignment
struct AlignedApStack([u8; AP_STACK_SIZE]);

static mut AP_STACKS: [AlignedApStack; MAX_CPUS - 1] = 
    unsafe { core::mem::MaybeUninit::<[AlignedApStack; MAX_CPUS - 1]>::zeroed().assume_init() };

/// Per-CPU runtime data - isolated to each CPU to avoid cache line contention
#[repr(C, align(64))]  // Cache line aligned to prevent false sharing
pub struct CpuData {
    pub cpu_id: u8,
    pub apic_id: u32,
    pub current_pid: AtomicU32,  // Currently running process
    pub idle_time: AtomicU64,    // Idle time in ticks
    pub busy_time: AtomicU64,    // Busy time in ticks
    pub reschedule_pending: AtomicBool,
    pub tlb_flush_pending: AtomicBool,
    pub context_switches: AtomicU64,
    pub interrupts_handled: AtomicU64,
}

impl CpuData {
    fn new(cpu_id: u8, apic_id: u32) -> Self {
        Self {
            cpu_id,
            apic_id,
            current_pid: AtomicU32::new(0),
            idle_time: AtomicU64::new(0),
            busy_time: AtomicU64::new(0),
            reschedule_pending: AtomicBool::new(false),
            tlb_flush_pending: AtomicBool::new(false),
            context_switches: AtomicU64::new(0),
            interrupts_handled: AtomicU64::new(0),
        }
    }
}

static mut CPU_DATA: [MaybeUninit<CpuData>; MAX_CPUS] = 
    unsafe { MaybeUninit::<[MaybeUninit<CpuData>; MAX_CPUS]>::uninit().assume_init() };

unsafe fn cpu_data(idx: usize) -> &'static CpuData {
    CPU_DATA[idx].assume_init_ref()
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum CpuStatus {
    Offline = 0,
    Booting = 1,
    Online = 2,
}

impl CpuStatus {
    fn from_atomic(val: u8) -> Self {
        match val {
            1 => CpuStatus::Booting,
            2 => CpuStatus::Online,
            _ => CpuStatus::Offline,
        }
    }
}

#[allow(dead_code)]
struct CpuInfo {
    apic_id: u32,
    acpi_id: u8,
    is_bsp: bool,
    status: AtomicU8,
    startup_attempts: AtomicU32,
    last_error: AtomicU32,  // Error code from last startup attempt
}

impl CpuInfo {
    fn new(apic_id: u32, acpi_id: u8, is_bsp: bool) -> Self {
        let initial = if is_bsp {
            CpuStatus::Online
        } else {
            CpuStatus::Offline
        } as u8;
        Self {
            apic_id,
            acpi_id,
            is_bsp,
            status: AtomicU8::new(initial),
            startup_attempts: AtomicU32::new(0),
            last_error: AtomicU32::new(0),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ApBootArgs {
    cpu_index: u32,
    apic_id: u32,
}

impl ApBootArgs {
    const fn new() -> Self {
        Self {
            cpu_index: 0,
            apic_id: 0,
        }
    }
}

static mut CPU_INFOS: [MaybeUninit<CpuInfo>; MAX_CPUS] =
    unsafe { MaybeUninit::<[MaybeUninit<CpuInfo>; MAX_CPUS]>::uninit().assume_init() };
static mut AP_BOOT_ARGS: [ApBootArgs; MAX_CPUS] = [ApBootArgs::new(); MAX_CPUS];
static mut BSP_APIC_ID: u32 = 0;

/// Get kernel relocation offset using multiple fallback methods.
/// Returns None if kernel was not relocated or offset cannot be determined.
fn get_kernel_relocation_offset() -> Option<i64> {
    // First try the direct kernel_load_offset from boot info
    if let Some(offset) = bootinfo::kernel_load_offset() {
        return Some(offset);
    }
    
    // Fallback: calculate from entry points if available
    if let Some((expected, actual)) = bootinfo::kernel_entry_points() {
        if expected != 0 && actual != 0 && expected != actual {
            let offset = actual as i64 - expected as i64;
            crate::kinfo!("SMP: Calculated relocation offset from entry points: {:#x}", offset);
            return Some(offset);
        }
    }
    
    None
}

unsafe fn cpu_info(idx: usize) -> &'static CpuInfo {
    CPU_INFOS[idx].assume_init_ref()
}

pub fn init() {
    if SMP_READY.load(Ordering::SeqCst) {
        return;
    }

    match unsafe { init_inner() } {
        Ok(()) => SMP_READY.store(true, Ordering::SeqCst),
        Err(err) => crate::kwarn!("SMP initialization skipped: {}", err),
    }
}

pub fn cpu_count() -> usize {
    CPU_TOTAL.load(Ordering::SeqCst)
}

pub fn online_cpus() -> usize {
    ONLINE_CPUS.load(Ordering::Acquire)
}

/// Get current CPU ID from LAPIC
pub fn current_cpu_id() -> u8 {
    if !SMP_READY.load(Ordering::Acquire) {
        return 0;
    }
    let apic_id = lapic::current_apic_id();
    unsafe {
        for i in 0..CPU_TOTAL.load(Ordering::Relaxed) {
            let info = cpu_info(i);
            if info.apic_id == apic_id {
                return i as u8;
            }
        }
    }
    0
}

/// Get per-CPU data for current CPU
pub fn current_cpu_data() -> Option<&'static CpuData> {
    if !SMP_READY.load(Ordering::Acquire) {
        return None;
    }
    let cpu_id = current_cpu_id() as usize;
    if cpu_id < CPU_TOTAL.load(Ordering::Relaxed) {
        unsafe { Some(cpu_data(cpu_id)) }
    } else {
        None
    }
}

/// Send reschedule IPI to a specific CPU
pub fn send_reschedule_ipi(cpu_id: u8) {
    if !SMP_READY.load(Ordering::Acquire) {
        return;
    }
    let cpu_id = cpu_id as usize;
    if cpu_id >= CPU_TOTAL.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        let info = cpu_info(cpu_id);
        lapic::send_ipi(info.apic_id, IPI_RESCHEDULE);
    }
}

/// Send TLB flush IPI to all CPUs except current
pub fn send_tlb_flush_ipi_all() {
    if !SMP_READY.load(Ordering::Acquire) {
        return;
    }
    let current = current_cpu_id();
    let total = CPU_TOTAL.load(Ordering::Relaxed);
    unsafe {
        for i in 0..total {
            if i == current as usize {
                continue;
            }
            let info = cpu_info(i);
            if CpuStatus::from_atomic(info.status.load(Ordering::Acquire)) == CpuStatus::Online {
                lapic::send_ipi(info.apic_id, IPI_TLB_FLUSH);
            }
        }
    }
}

/// Broadcast IPI to all online CPUs except current
pub fn send_ipi_broadcast(vector: u8) {
    if !SMP_READY.load(Ordering::Acquire) {
        return;
    }
    let current = current_cpu_id();
    let total = CPU_TOTAL.load(Ordering::Relaxed);
    unsafe {
        for i in 0..total {
            if i == current as usize {
                continue;
            }
            let info = cpu_info(i);
            if CpuStatus::from_atomic(info.status.load(Ordering::Acquire)) == CpuStatus::Online {
                lapic::send_ipi(info.apic_id, vector);
            }
        }
    }
}

fn current_online() -> usize {
    unsafe {
        let total = CPU_TOTAL.load(Ordering::SeqCst);
        let mut online = 0;
        for idx in 0..total {
            let status = CpuStatus::from_atomic(cpu_info(idx).status.load(Ordering::SeqCst));
            if status == CpuStatus::Online {
                online += 1;
            }
        }
        online
    }
}

unsafe fn init_inner() -> Result<(), &'static str> {
    acpi::init()?;
    let cpus = acpi::cpus();
    if cpus.is_empty() {
        return Err("ACPI reported zero processors");
    }

    let lapic_base = acpi::lapic_base().unwrap_or(0xFEE0_0000);
    lapic::init(lapic_base);

    install_trampoline()?;
    patch_gdt_descriptors()?;

    let bsp_apic = lapic::bsp_apic_id();
    BSP_APIC_ID = bsp_apic;

    let mut count = 0usize;
    for desc in cpus.iter() {
        if count >= MAX_CPUS {
            crate::kwarn!(
                "SMP: Limiting CPU count to {} (hardware reports {})",
                MAX_CPUS,
                cpus.len()
            );
            break;
        }
        CPU_INFOS[count].as_mut_ptr().write(CpuInfo::new(
            desc.apic_id as u32,
            desc.acpi_processor_id,
            desc.apic_id as u32 == bsp_apic,
        ));
        count += 1;
    }
    CPU_TOTAL.store(count, Ordering::SeqCst);
    
    // Initialize BSP CPU data
    for i in 0..count {
        let info = cpu_info(i);
        if info.is_bsp {
            CPU_DATA[i].as_mut_ptr().write(CpuData::new(i as u8, info.apic_id));
            break;
        }
    }

    crate::kinfo!(
        "SMP: Detected {} logical CPUs (BSP APIC {:#x})",
        count,
        bsp_apic
    );
    
    // Stage 2: Verify trampoline installation details
    crate::kinfo!("SMP: Stage 2 - Verifying trampoline setup");
    crate::kinfo!("  Trampoline installed at: {:#x}", TRAMPOLINE_BASE);
    crate::kinfo!("  Trampoline vector: {:#x}", TRAMPOLINE_VECTOR);
    crate::kinfo!("  PML4 physical address: {:#x}", paging::current_pml4_phys());
    
    // Verify GDT descriptor patching
    let descriptor = sgdt();
    crate::kinfo!("  GDT base for APs: {:#x}, limit: {:#x}", 
        descriptor.base.as_u64(), descriptor.limit);
    
    // Verify AP stacks are available
    for i in 1..count.min(3) {  // Log first 2 APs
        match stack_for(i) {
            Ok(stack) => crate::kinfo!("  AP {} stack top: {:#x}", i, stack),
            Err(e) => crate::kwarn!("  AP {} stack error: {}", i, e),
        }
    }
    
    // Stage 3: Try starting a single AP core for testing
    crate::kinfo!("SMP: Stage 3 - Attempting to start single AP core (CPU 1)");
    
    if !ENABLE_AP_STARTUP {
        crate::kwarn!("SMP: AP startup disabled by ENABLE_AP_STARTUP flag");
        
        // DISABLED: IPI test causes crashes
        // crate::kinfo!("SMP: Testing IPI mechanism...");
        // test_ipi_mechanism();
        
        crate::kinfo!(
            "SMP: {} / {} cores online (BSP only, APs not started)",
            current_online(),
            CPU_TOTAL.load(Ordering::SeqCst)
        );
        return Ok(());
    }
    
    if count > 1 {
        let test_cpu_idx = 1;  // Start only CPU 1
        let info = cpu_info(test_cpu_idx);
        
        crate::kinfo!("SMP: Starting test AP core {} (APIC ID {:#x})...", 
            test_cpu_idx, info.apic_id);
        
        match start_ap(test_cpu_idx) {
            Ok(()) => {
                crate::kinfo!("SMP: ✓ Test AP core {} started successfully!", test_cpu_idx);
                ONLINE_CPUS.fetch_add(1, Ordering::SeqCst);
            }
            Err(err) => {
                crate::kwarn!("SMP: ✗ Failed to start test AP core {}: {}", test_cpu_idx, err);
                crate::kwarn!("SMP: Continuing with BSP only");
            }
        }
    }
    
    crate::kinfo!(
        "SMP: {} / {} cores online",
        current_online(),
        CPU_TOTAL.load(Ordering::SeqCst)
    );
    
    return Ok(());
    
    // Try to start AP cores (DISABLED FOR NOW)
    #[allow(unreachable_code)]
    #[allow(unused_variables)]
    {
    let mut started = 0usize;
    
    for idx in 0..count {
        let info = cpu_info(idx);
        if info.is_bsp {
            continue;
        }
        
        crate::kinfo!("SMP: Starting AP core {} (APIC ID {:#x})...", idx, info.apic_id);
        
        match start_ap(idx) {
            Ok(()) => {
                started += 1;
                crate::kinfo!("SMP: AP core {} started successfully", idx);
            }
            Err(err) => {
                crate::kwarn!(
                    "SMP: Failed to start APIC {:#x} (index {}): {}",
                    info.apic_id,
                    idx,
                    err
                );
            }
        }
    }

    crate::kinfo!(
        "SMP: {} / {} cores online (BSP + {} APs)",
        current_online(),
        CPU_TOTAL.load(Ordering::SeqCst),
        started
    );
    }  // End of unreachable AP startup code

    Ok(())
}

/// Test IPI mechanism before attempting AP startup
#[allow(dead_code)]
unsafe fn test_ipi_mechanism() {
    crate::kinfo!("SMP: [IPI Test] Starting IPI self-test on BSP...");
    
    // Read LAPIC error status before test
    let error_before = lapic::read_error();
    crate::kinfo!("SMP: [IPI Test] LAPIC error status before: {:#x}", error_before);
    
    // Get BSP APIC ID
    let bsp_apic_id = lapic::bsp_apic_id();
    crate::kinfo!("SMP: [IPI Test] BSP APIC ID: {:#x}", bsp_apic_id);
    
    // Test: Read LAPIC base address
    if let Some(base) = lapic::base() {
        crate::kinfo!("SMP: [IPI Test] LAPIC base address: {:#x}", base);
    }
    
    crate::kinfo!("SMP: [IPI Test] Attempting simplified IPI send to BSP...");
    
    // Disable interrupts during IPI send
    x86_64::instructions::interrupts::disable();
    
    // Use 0xF0 (IPI_RESCHEDULE) which has a registered handler in interrupts.rs (line 745)
    // Previous value 0xF9 had NO handler, causing GP fault!
    lapic::send_ipi(bsp_apic_id, 0xF0);
    
    // Re-enable interrupts
    x86_64::instructions::interrupts::enable();
    
    crate::kinfo!("SMP: [IPI Test] IPI send completed without crash!");
    
    // Read LAPIC error status after test
    let error_after = lapic::read_error();
    crate::kinfo!("SMP: [IPI Test] LAPIC error status after: {:#x}", error_after);
    
    crate::kinfo!("SMP: [IPI Test] Completed");
}

unsafe fn install_trampoline() -> Result<(), &'static str> {
    if TRAMPOLINE_READY.load(Ordering::SeqCst) {
        return Ok(());
    }

    extern "C" {
        static __ap_trampoline_start: u8;
        static __ap_trampoline_end: u8;
    }

    // Get link-time addresses (these are the symbols from the linker)
    let link_start = &__ap_trampoline_start as *const u8 as usize;
    let link_end = &__ap_trampoline_end as *const u8 as usize;
    let size = link_end - link_start;
    
    // Apply kernel relocation offset to get the actual runtime address
    // The trampoline code is embedded in the kernel, so it moved with the kernel
    let start = if let Some(offset) = get_kernel_relocation_offset() {
        let relocated = (link_start as i64 + offset) as usize;
        crate::kinfo!("SMP: Trampoline source: link={:#x}, offset={:#x}, relocated={:#x}", 
                      link_start, offset, relocated);
        relocated
    } else {
        crate::kinfo!("SMP: Trampoline source: {:#x} (no relocation)", link_start);
        link_start
    };
    
    if size == 0 {
        return Err("AP trampoline size is zero");
    }
    
    if size > TRAMPOLINE_MAX_SIZE {
        return Err("AP trampoline exceeds reserved space");
    }
    
    crate::kinfo!("SMP: Installing trampoline at {:#x} (size {} bytes)", TRAMPOLINE_BASE, size);

    // Ensure low memory is accessible by checking if it's identity-mapped
    // The trampoline needs to be in low memory for AP startup
    ptr::copy_nonoverlapping(start as *const u8, TRAMPOLINE_BASE as *mut u8, size);
    if size < TRAMPOLINE_MAX_SIZE {
        ptr::write_bytes(
            (TRAMPOLINE_BASE as usize + size) as *mut u8,
            0,
            TRAMPOLINE_MAX_SIZE - size,
        );
    }
    
    // DEBUG: Dump the code that reads ap_entry_ptr
    // The mov (%r9),%rax instruction should be at offset 0x181 relative to trampoline start
    let code_offset = 0x140;  // Around where entry reading code should be
    let code_at = (TRAMPOLINE_BASE as usize + code_offset) as *const u8;
    let mut dump_code = [0u8; 64];
    for i in 0..64 {
        dump_code[i] = ptr::read_volatile(code_at.add(i));
    }
    crate::kinfo!("SMP: Code at 0x{:x}:", TRAMPOLINE_BASE as usize + code_offset);
    crate::kinfo!("  {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
        dump_code[0], dump_code[1], dump_code[2], dump_code[3],
        dump_code[4], dump_code[5], dump_code[6], dump_code[7],
        dump_code[8], dump_code[9], dump_code[10], dump_code[11],
        dump_code[12], dump_code[13], dump_code[14], dump_code[15]);
    crate::kinfo!("  {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
        dump_code[16], dump_code[17], dump_code[18], dump_code[19],
        dump_code[20], dump_code[21], dump_code[22], dump_code[23],
        dump_code[24], dump_code[25], dump_code[26], dump_code[27],
        dump_code[28], dump_code[29], dump_code[30], dump_code[31]);
    crate::kinfo!("  {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
        dump_code[32], dump_code[33], dump_code[34], dump_code[35],
        dump_code[36], dump_code[37], dump_code[38], dump_code[39],
        dump_code[40], dump_code[41], dump_code[42], dump_code[43],
        dump_code[44], dump_code[45], dump_code[46], dump_code[47]);
    crate::kinfo!("  {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
        dump_code[48], dump_code[49], dump_code[50], dump_code[51],
        dump_code[52], dump_code[53], dump_code[54], dump_code[55],
        dump_code[56], dump_code[57], dump_code[58], dump_code[59],
        dump_code[60], dump_code[61], dump_code[62], dump_code[63]);

    TRAMPOLINE_READY.store(true, Ordering::SeqCst);
    Ok(())
}

unsafe fn patch_gdt_descriptors() -> Result<(), &'static str> {
    extern "C" {
        static __ap_trampoline_start: u8;
        static ap_gdt16_ptr: u8;
        static ap_gdt64_ptr: u8;
        static ap_idt_ptr: u8;
    }

    #[repr(C, packed)]
    struct GdtPtr16 {
        limit: u16,
        base: u32,
    }

    #[repr(C, packed)]
    struct GdtPtr64 {
        limit: u16,
        base: u64,
    }

    #[repr(C, packed)]
    struct IdtPtr {
        limit: u16,
        base: u64,
    }

    let descriptor = sgdt();
    let base = descriptor.base.as_u64();
    if base >= (1u64 << 32) {
        return Err("Kernel GDT base exceeds xAPIC addressing");
    }

    let gdt16 = GdtPtr16 {
        limit: descriptor.limit,
        base: base as u32,
    };
    let gdt64 = GdtPtr64 {
        limit: descriptor.limit,
        base,
    };

    write_trampoline_bytes(
        &__ap_trampoline_start,
        &ap_gdt16_ptr,
        gdt16_as_bytes(&gdt16),
    )?;
    write_trampoline_bytes(
        &__ap_trampoline_start,
        &ap_gdt64_ptr,
        gdt64_as_bytes(&gdt64),
    )?;
    
    // Set up IDT pointer for AP cores
    use x86_64::instructions::tables::sidt;
    let idt_descriptor = sidt();
    let idt_ptr = IdtPtr {
        limit: idt_descriptor.limit,
        base: idt_descriptor.base.as_u64(),
    };
    write_trampoline_bytes(
        &__ap_trampoline_start,
        &ap_idt_ptr,
        idt_ptr_as_bytes(&idt_ptr),
    )?;
    
    let idt_base = idt_descriptor.base.as_u64();
    let idt_limit = idt_descriptor.limit;
    crate::kinfo!("SMP: IDT configured for AP cores (base={:#x}, limit={:#x})", 
        idt_base, idt_limit);
    
    Ok(())
}

fn gdt16_as_bytes(ptr: &impl Sized) -> &[u8] {
    unsafe { core::slice::from_raw_parts(ptr as *const _ as *const u8, mem::size_of_val(ptr)) }
}

fn gdt64_as_bytes(ptr: &impl Sized) -> &[u8] {
    unsafe { core::slice::from_raw_parts(ptr as *const _ as *const u8, mem::size_of_val(ptr)) }
}

fn idt_ptr_as_bytes(ptr: &impl Sized) -> &[u8] {
    unsafe { core::slice::from_raw_parts(ptr as *const _ as *const u8, mem::size_of_val(ptr)) }
}

unsafe fn start_ap(index: usize) -> Result<(), &'static str> {
    let info = cpu_info(index);
    let apic_id = info.apic_id;
    
    for attempt in 0..STARTUP_RETRY_MAX {
        crate::kinfo!(
            "SMP: Starting AP core {} (APIC {:#x}), attempt {}/{}",
            index, apic_id, attempt + 1, STARTUP_RETRY_MAX
        );
        
        crate::kinfo!("SMP: [{}] Preparing launch parameters...", index);
        prepare_ap_launch(index)?;
        
        crate::kinfo!("SMP: [{}] Setting Booting state", index);
        info.status.store(CpuStatus::Booting as u8, Ordering::Release);
        info.startup_attempts.fetch_add(1, Ordering::Relaxed);
        
        // Ensure all writes are visible before sending IPIs
        core::sync::atomic::fence(Ordering::SeqCst);
        
        // Verify trampoline data one more time before sending INIT
        extern "C" {
            static __ap_trampoline_start: u8;
            static ap_entry_ptr: u8;
        }
        let entry_offset = unsafe { 
            (&ap_entry_ptr as *const _ as usize) - (&__ap_trampoline_start as *const _ as usize) 
        };
        let check_addr = (TRAMPOLINE_BASE as usize + entry_offset) as *const u64;
        let check_val = unsafe { core::ptr::read_volatile(check_addr) };
        crate::kinfo!("SMP: [{}] Pre-IPI verify: entry at {:#x} = {:#x}", index, check_addr as usize, check_val);
        
        // Also dump the bytes around the entry pointer
        let dump_start = (TRAMPOLINE_BASE as usize + entry_offset - 8) as *const u8;
        let mut dump_str = [0u8; 48];
        for i in 0..24 {
            let b = unsafe { core::ptr::read_volatile(dump_start.add(i)) };
            let hi = (b >> 4) & 0xF;
            let lo = b & 0xF;
            dump_str[i*2] = if hi < 10 { b'0' + hi } else { b'A' + hi - 10 };
            dump_str[i*2+1] = if lo < 10 { b'0' + lo } else { b'A' + lo - 10 };
        }
        let dump_s = core::str::from_utf8(&dump_str).unwrap_or("???");
        crate::kinfo!("SMP: [{}] Bytes at {:#x}: {}", index, dump_start as usize, dump_s);
        
        // Send INIT IPI
        crate::kinfo!("SMP: [{}] Sending INIT IPI to APIC {:#x}", index, apic_id);
        lapic::send_init_ipi(apic_id);
        crate::kinfo!("SMP: [{}] INIT IPI sent, waiting 10ms...", index);
        busy_wait(100_000);  // 10ms delay after INIT
        
        // Send STARTUP IPI (twice per Intel spec)
        crate::kinfo!("SMP: [{}] Sending STARTUP IPI #1, vector {:#x}", index, TRAMPOLINE_VECTOR);
        lapic::send_startup_ipi(apic_id, TRAMPOLINE_VECTOR);
        crate::kinfo!("SMP: [{}] STARTUP IPI #1 sent, waiting 200us...", index);
        busy_wait(20_000);   // 200us delay between SIPIs
        
        crate::kinfo!("SMP: [{}] Sending STARTUP IPI #2", index);
        lapic::send_startup_ipi(apic_id, TRAMPOLINE_VECTOR);
        crate::kinfo!("SMP: [{}] STARTUP IPI #2 sent, waiting before check...", index);
        busy_wait(10_000);   // Extra delay before checking
        
        // Check if AP arrived at entry point
        let arrived = AP_ARRIVED[index].load(Ordering::SeqCst);
        let magic = unsafe { core::ptr::read_volatile(0x9000 as *const u32) };
        let flag_addr = (0x9000 + (index as u32 + 1) * 4) as *const u32;
        let flag_val = unsafe { core::ptr::read_volatile(flag_addr) };
        
        if arrived == 0xDEADBEEF {
            crate::kinfo!("SMP: [{}] AP successfully arrived at entry point!", index);
        } else {
            crate::kerror!("SMP: [{}] AP did NOT arrive (flag={:#x}, magic={:#x}, mem={:#x})", 
                index, arrived, magic, flag_val);
        }
        
        // Wait for AP to come online
        crate::kinfo!("SMP: [{}] Waiting for AP to signal online...", index);
        if wait_for_online(index, STARTUP_WAIT_LOOPS) {
            crate::kinfo!("SMP: [{}] AP online!", index);
            ONLINE_CPUS.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        }
        
        let status = CpuStatus::from_atomic(info.status.load(Ordering::Acquire));
        crate::kwarn!(
            "SMP: [{}] Failed to start (attempt {}, status: {:?})",
            index, attempt + 1, status
        );
        
        // Reset status for retry
        info.status.store(CpuStatus::Offline as u8, Ordering::Release);
        
        // Longer delay before retry
        busy_wait(100_000);
    }
    
    Err("AP failed to start after maximum retries")
}

unsafe fn prepare_ap_launch(index: usize) -> Result<(), &'static str> {
    extern "C" {
        static __ap_trampoline_start: u8;
        static ap_pml4_ptr: u8;
        static ap_stack_ptr: u8;
        static ap_entry_ptr: u8;
        static ap_arg_ptr: u8;
    }

    let pml4 = paging::current_pml4_phys();
    crate::kinfo!("SMP: [{}] PML4: {:#x}", index, pml4);
    write_trampoline_bytes(&__ap_trampoline_start, &ap_pml4_ptr, &pml4.to_le_bytes())?;

    let stack = stack_for(index)?;
    crate::kinfo!("SMP: [{}] Stack: {:#x}", index, stack);
    write_trampoline_bytes(&__ap_trampoline_start, &ap_stack_ptr, &stack.to_le_bytes())?;
    
    // Verify writes
    let pml4_offset = (&ap_pml4_ptr as *const _ as usize) - (&__ap_trampoline_start as *const _ as usize);
    let stack_offset = (&ap_stack_ptr as *const _ as usize) - (&__ap_trampoline_start as *const _ as usize);
    let written_pml4 = core::ptr::read_volatile((TRAMPOLINE_BASE as usize + pml4_offset) as *const u64);
    let written_stack = core::ptr::read_volatile((TRAMPOLINE_BASE as usize + stack_offset) as *const u64);
    crate::kinfo!("SMP: [{}] Trampoline has PML4={:#x}, stack={:#x}", index, written_pml4, written_stack);

    AP_BOOT_ARGS[index] = ApBootArgs {
        cpu_index: index as u32,
        apic_id: cpu_info(index).apic_id,
    };
    let arg_ptr = (&AP_BOOT_ARGS[index] as *const ApBootArgs) as u64;
    // NOTE: Static variable addresses don't need relocation because:
    // 1. The kernel code accesses them using link-time addresses
    // 2. These addresses are identity-mapped in the page tables
    // 3. BSP writes to link-time address, AP reads from same address
    crate::kinfo!("SMP: [{}] Boot args at: {:#x}", index, arg_ptr);
    write_trampoline_bytes(&__ap_trampoline_start, &ap_arg_ptr, &arg_ptr.to_le_bytes())?;

    let entry_raw = ap_entry as usize as u64;
    crate::kinfo!("SMP: [{}] Raw ap_entry pointer value: {:#x}", index, entry_raw);
    
    // Get relocation offset - try multiple sources for robustness
    let reloc_offset = get_kernel_relocation_offset();
    
    // Check if this looks like a link-time or run-time address
    // Link-time address would be around 0x12c100
    // Run-time address would be around 0x2382100
    let entry = if entry_raw > 0x1000000 {
        // Looks like already relocated (high address)
        crate::kinfo!("SMP: [{}] Entry appears already relocated, using as-is: {:#x}", index, entry_raw);
        entry_raw
    } else if let Some(offset) = reloc_offset {
        if offset != 0 {
            let relocated = (entry_raw as i64 + offset) as u64;
            crate::kinfo!("SMP: [{}] Entry point: {:#x} + offset {:#x} = {:#x}", 
                index, entry_raw, offset, relocated);
            relocated
        } else {
            crate::kinfo!("SMP: [{}] Entry point: {:#x} (no relocation)", index, entry_raw);
            entry_raw
        }
    } else {
        crate::kinfo!("SMP: [{}] Entry point: {:#x} (no offset info)", index, entry_raw);
        entry_raw
    };
    
    // Debug: log the addresses and offset used for writing
    let entry_offset_before = (&ap_entry_ptr as *const _ as usize) - (&__ap_trampoline_start as *const _ as usize);
    crate::kinfo!("SMP: [{}] ap_entry_ptr={:#x}, trampoline_start={:#x}, offset={:#x}", 
                  index, 
                  &ap_entry_ptr as *const _ as usize,
                  &__ap_trampoline_start as *const _ as usize,
                  entry_offset_before);
    crate::kinfo!("SMP: [{}] Writing entry {:#x} to trampoline offset {:#x} (dest addr {:#x})",
                  index, entry, entry_offset_before, TRAMPOLINE_BASE as usize + entry_offset_before);
    
    // Debug: Verify code exists at the entry address
    let entry_code = unsafe { core::ptr::read_volatile(entry as *const [u8; 16]) };
    crate::kinfo!("SMP: [{}] Code at entry {:#x}: {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
                  index, entry,
                  entry_code[0], entry_code[1], entry_code[2], entry_code[3],
                  entry_code[4], entry_code[5], entry_code[6], entry_code[7]);
    // Expected: 66 ba f8 03 b0 48 ee 57 (mov $0x3f8,%dx; mov $0x48,%al; out; push %rdi)
    
    write_trampoline_bytes(&__ap_trampoline_start, &ap_entry_ptr, &entry.to_le_bytes())?;
    
    // Verify the write
    let entry_offset = (&ap_entry_ptr as *const _ as usize) - (&__ap_trampoline_start as *const _ as usize);
    let written_entry_ptr = (TRAMPOLINE_BASE as usize + entry_offset) as *const u64;
    let written_entry = core::ptr::read_volatile(written_entry_ptr);
    crate::kinfo!("SMP: [{}] Verified entry in trampoline at {:#x}: {:#x}", index, written_entry_ptr as usize, written_entry);
    
    Ok(())
}

unsafe fn stack_for(index: usize) -> Result<u64, &'static str> {
    if index == 0 {
        return Err("Stack request for BSP");
    }
    let stack_index = index - 1;
    if stack_index >= MAX_CPUS - 1 {
        return Err("No AP stack slot available");
    }
    // Access aligned stack, stack grows downward so return top address
    // Ensure stack top is 16-byte aligned as required by x86_64 ABI
    let stack_base = ptr::addr_of!(AP_STACKS[stack_index].0) as usize;
    let stack_top = stack_base + AP_STACK_SIZE;
    let aligned_top = stack_top & !0xF;  // Align down to 16 bytes
    
    // NOTE: Static variable addresses don't need relocation because:
    // 1. The kernel code accesses them using link-time addresses
    // 2. Low memory (link-time addresses) is identity-mapped in page tables
    // 3. AP cores use the same page tables, so they can access these addresses
    crate::kinfo!("SMP: [{}] Stack top: {:#x}", index, aligned_top);
    
    Ok(aligned_top as u64)
}

fn busy_wait(mut iterations: u64) {
    while iterations > 0 {
        core::hint::spin_loop();
        iterations -= 1;
    }
}

unsafe fn wait_for_online(index: usize, mut loops: u64) -> bool {
    while loops > 0 {
        let status = CpuStatus::from_atomic(cpu_info(index).status.load(Ordering::SeqCst));
        if status == CpuStatus::Online {
            return true;
        }
        core::hint::spin_loop();
        loops -= 1;
    }
    false
}

unsafe fn write_trampoline_bytes(
    start: *const u8,
    field: *const u8,
    data: &[u8],
) -> Result<(), &'static str> {
    let offset = field as usize - start as usize;
    if offset + data.len() > TRAMPOLINE_MAX_SIZE {
        return Err("Trampoline patch exceeds bounds");
    }
    let dest = (TRAMPOLINE_BASE as usize + offset) as *mut u8;
    ptr::copy_nonoverlapping(data.as_ptr(), dest, data.len());
    Ok(())
}

#[no_mangle]
#[unsafe(naked)]
unsafe extern "C" fn ap_entry(arg: *const ApBootArgs) -> ! {
    // Naked function to have full control over the prologue
    // First output debug character before any Rust code executes
    core::arch::naked_asm!(
        // Output 'H' to serial port immediately upon entry
        "mov dx, 0x3F8",
        "mov al, 'H'",
        "out dx, al",
        
        // Output RSP alignment at entry
        "mov rax, rsp",
        "and rax, 0xF",
        "cmp rax, 10",
        "jb 2f",
        "add al, ('A' - 10)",
        "jmp 3f",
        "2: add al, '0'",
        "3: out dx, al",
        
        // Save rdi (argument) and call the actual entry function
        "push rdi",
        
        // Output 'I' to confirm push worked
        "mov al, 'I'",
        "out dx, al",
        
        // Output RSP alignment after push
        "mov rax, rsp",
        "and rax, 0xF",
        "cmp rax, 10",
        "jb 4f",
        "add al, ('A' - 10)",
        "jmp 5f",
        "4: add al, '0'",
        "5: out dx, al",
        
        // Pop argument and call inner function
        "pop rdi",
        
        // Output RSP alignment after pop (before jmp)
        "mov rax, rsp",
        "and rax, 0xF",
        "cmp rax, 10",
        "jb 6f",
        "add al, ('A' - 10)",
        "jmp 7f",
        "6: add al, '0'",
        "7: out dx, al",
        
        "jmp {inner}",
        inner = sym ap_entry_inner,
    );
}

#[no_mangle]
extern "C" fn ap_entry_inner(arg: *const ApBootArgs) -> ! {
    // NOTE: IDT is already loaded by trampoline, so exceptions won't cause triple fault
    // Interrupts are still disabled at this point
    
    unsafe {
        // Debug: Signal entry and check RSP alignment
        use x86_64::instructions::port::Port;
        let mut serial = Port::<u8>::new(0x3F8);
        serial.write(b'E');  // Entry
        
        // Check RSP alignment
        let rsp: u64;
        core::arch::asm!("mov {}, rsp", out(reg) rsp);
        // Output RSP low nibble to check alignment (should be 8 for correct ABI)
        let align_byte = (rsp & 0xF) as u8;
        serial.write(if align_byte < 10 { b'0' + align_byte } else { b'A' + align_byte - 10 });
        
        // Validate argument pointer
        if arg.is_null() {
            serial.write(b'N');  // Null arg
            loop { cpu_hlt(); }
        }
        serial.write(b'1');  // Arg not null
        
        let args = *arg;
        serial.write(b'2');  // Args copied
        
        let idx = args.cpu_index as usize;
        serial.write(b'3');  // Index extracted
        
        if idx >= MAX_CPUS {
            serial.write(b'X');  // Index too large
            loop { cpu_hlt(); }
        }
        serial.write(b'4');  // Index valid
        
        // Signal arrival for debugging
        AP_ARRIVED[idx].store(0xDEADBEEF, Ordering::SeqCst);
        serial.write(b'5');  // Arrival flag set
        
        // Step 1: Configure GS base with this CPU's dedicated GS data
        // NOTE: Static variable addresses don't need relocation - they use link-time
        // addresses which are identity-mapped in the page tables
        let gs_data_addr = &raw const AP_GS_DATA[idx] as *const _ as u64;
        serial.write(b'6');  // GS addr calculated
        
        use x86_64::registers::model_specific::Msr;
        Msr::new(0xc0000101).write(gs_data_addr);
        serial.write(b'7');  // GS written
        
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        serial.write(b'8');  // Fence done
        
        // Step 2: Initialize GDT (required for proper segmentation)
        serial.write(b'9');  // About to call gdt::init_ap
        
        // Initialize GDT and TSS for this AP core
        gdt::init_ap(idx);
        
        // Use unique markers that won't appear in other logs
        for byte in b"AP_GDT_OK\n" {
            serial.write(*byte);
        }
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        
        // Now we can log safely
        crate::kinfo!("SMP: AP core {} online (APIC {:#x})", idx, args.apic_id);
        
        // Step 3: Initialize per-CPU data
        CPU_DATA[idx].as_mut_ptr().write(CpuData::new(idx as u8, args.apic_id));
        core::sync::atomic::compiler_fence(Ordering::Release);
        
        // Step 4: Mark CPU as online
        if idx < CPU_TOTAL.load(Ordering::Acquire) {
            cpu_info(idx)
                .status
                .store(CpuStatus::Online as u8, Ordering::Release);
            for byte in b"AP_ONLINE\n" {
                serial.write(*byte);
            }
        } else {
            crate::kerror!("SMP: CPU index {} exceeds total count", idx);
            loop { cpu_hlt(); }
        }
        
        // Step 5: Enable interrupts
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        x86_64::instructions::interrupts::enable();
        
        for byte in b"AP_IDLE_LOOP\n" {
            serial.write(*byte);
        }
        crate::kinfo!("SMP: Core {} entering idle loop", idx);
        
        // Enter idle loop - scheduler will take over
        loop {
            cpu_hlt();
        }
    }
}
