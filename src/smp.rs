use core::mem::{self, MaybeUninit};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use x86_64::instructions::tables::sgdt;
use x86_64::instructions::{hlt as cpu_hlt, interrupts};

use crate::{acpi, gdt, interrupts as idt, lapic, paging};

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

static SMP_READY: AtomicBool = AtomicBool::new(false);
static TRAMPOLINE_READY: AtomicBool = AtomicBool::new(false);
static CPU_TOTAL: AtomicUsize = AtomicUsize::new(1);
static ONLINE_CPUS: AtomicUsize = AtomicUsize::new(1);

static mut AP_STACKS: [[u8; AP_STACK_SIZE]; MAX_CPUS - 1] = [[0; AP_STACK_SIZE]; MAX_CPUS - 1];

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
    
    // TEMPORARY: Skip AP startup for debugging
    crate::kwarn!("SMP: AP core startup temporarily disabled for debugging");
    crate::kinfo!(
        "SMP: {} / {} cores online (BSP only, {} APs skipped)",
        current_online(),
        CPU_TOTAL.load(Ordering::SeqCst),
        count - 1
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

unsafe fn install_trampoline() -> Result<(), &'static str> {
    if TRAMPOLINE_READY.load(Ordering::SeqCst) {
        return Ok(());
    }

    extern "C" {
        static __ap_trampoline_start: u8;
        static __ap_trampoline_end: u8;
    }

    let start = &__ap_trampoline_start as *const u8 as usize;
    let end = &__ap_trampoline_end as *const u8 as usize;
    let size = end - start;
    
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

    TRAMPOLINE_READY.store(true, Ordering::SeqCst);
    Ok(())
}

unsafe fn patch_gdt_descriptors() -> Result<(), &'static str> {
    extern "C" {
        static __ap_trampoline_start: u8;
        static ap_gdt16_ptr: u8;
        static ap_gdt64_ptr: u8;
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
    Ok(())
}

fn gdt16_as_bytes(ptr: &impl Sized) -> &[u8] {
    unsafe { core::slice::from_raw_parts(ptr as *const _ as *const u8, mem::size_of_val(ptr)) }
}

fn gdt64_as_bytes(ptr: &impl Sized) -> &[u8] {
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
        
        crate::kdebug!("SMP: Preparing launch parameters for core {}...", index);
        prepare_ap_launch(index)?;
        
        crate::kdebug!("SMP: Setting core {} to Booting state", index);
        info.status.store(CpuStatus::Booting as u8, Ordering::Release);
        info.startup_attempts.fetch_add(1, Ordering::Relaxed);
        
        // Ensure all writes are visible before sending IPIs
        core::sync::atomic::fence(Ordering::SeqCst);
        
        // Send INIT IPI
        crate::kdebug!("SMP: Sending INIT IPI to APIC {:#x}", apic_id);
        lapic::send_init_ipi(apic_id);
        busy_wait(100_000);  // 10ms delay after INIT
        
        // Send STARTUP IPI (twice per Intel spec)
        crate::kdebug!("SMP: Sending STARTUP IPI #1 to APIC {:#x}, vector {:#x}", apic_id, TRAMPOLINE_VECTOR);
        lapic::send_startup_ipi(apic_id, TRAMPOLINE_VECTOR);
        busy_wait(20_000);   // 200us delay between SIPIs
        
        crate::kdebug!("SMP: Sending STARTUP IPI #2 to APIC {:#x}", apic_id);
        lapic::send_startup_ipi(apic_id, TRAMPOLINE_VECTOR);
        busy_wait(10_000);   // Extra delay before checking
        
        // Wait for AP to come online
        crate::kdebug!("SMP: Waiting for AP core {} to signal online...", index);
        if wait_for_online(index, STARTUP_WAIT_LOOPS) {
            crate::kinfo!("SMP: AP core {} (APIC {:#x}) online", index, apic_id);
            ONLINE_CPUS.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        }
        
        let status = CpuStatus::from_atomic(info.status.load(Ordering::Acquire));
        crate::kwarn!(
            "SMP: AP core {} (APIC {:#x}) failed to start (attempt {}, status: {:?})",
            index, apic_id, attempt + 1, status
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
    crate::kdebug!("SMP: PML4 for AP core {}: {:#x}", index, pml4);
    write_trampoline_bytes(&__ap_trampoline_start, &ap_pml4_ptr, &pml4.to_le_bytes())?;

    let stack = stack_for(index)?;
    crate::kdebug!("SMP: Stack for AP core {}: {:#x}", index, stack);
    write_trampoline_bytes(&__ap_trampoline_start, &ap_stack_ptr, &stack.to_le_bytes())?;

    AP_BOOT_ARGS[index] = ApBootArgs {
        cpu_index: index as u32,
        apic_id: cpu_info(index).apic_id,
    };
    let arg_ptr = (&AP_BOOT_ARGS[index] as *const ApBootArgs) as u64;
    crate::kdebug!("SMP: Boot args for AP core {}: {:#x}", index, arg_ptr);
    write_trampoline_bytes(&__ap_trampoline_start, &ap_arg_ptr, &arg_ptr.to_le_bytes())?;

    let entry = ap_entry as usize as u64;
    crate::kdebug!("SMP: Entry point for AP core {}: {:#x}", index, entry);
    write_trampoline_bytes(&__ap_trampoline_start, &ap_entry_ptr, &entry.to_le_bytes())?;

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
    let stack_ptr = ptr::addr_of!(AP_STACKS) as usize + stack_index * AP_STACK_SIZE;
    Ok(stack_ptr as u64 + AP_STACK_SIZE as u64)
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
extern "C" fn ap_entry(arg: *const ApBootArgs) -> ! {
    // CRITICAL: Keep interrupts disabled during entire initialization
    // AP cores start with interrupts disabled from trampoline
    
    unsafe {
        // Basic validation without logging (GS not set up yet)
        if arg.is_null() {
            // Can't log yet, just halt
            loop { cpu_hlt(); }
        }
        
        let args = *arg;
        let idx = args.cpu_index as usize;
        
        // Step 1: Configure GS base (required for logging and syscalls)
        crate::configure_gs_base();
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        
        // Step 2: Initialize GDT (required for proper segmentation)
        gdt::init_ap();
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        
        // Step 3: Load IDT (but keep interrupts disabled)
        idt::init_interrupts_ap();
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        
        // Now it's safe to log
        crate::kinfo!("SMP: AP core {} starting (APIC {:#x})", idx, args.apic_id);
        
        // Step 4: Initialize per-CPU data
        if idx < MAX_CPUS {
            CPU_DATA[idx].as_mut_ptr().write(CpuData::new(idx as u8, args.apic_id));
            core::sync::atomic::compiler_fence(Ordering::Release);
        } else {
            crate::kerror!("SMP: Invalid CPU index {}", idx);
            loop { cpu_hlt(); }
        }
        
        // Step 5: Mark CPU as online
        if idx < CPU_TOTAL.load(Ordering::Acquire) {
            cpu_info(idx)
                .status
                .store(CpuStatus::Online as u8, Ordering::Release);
            crate::kinfo!("SMP: Core {} (APIC {:#x}) online", idx, args.apic_id);
        } else {
            crate::kerror!("SMP: CPU index {} exceeds total count", idx);
            loop { cpu_hlt(); }
        }
        
        // Step 6: Enable interrupts only after everything is ready
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        interrupts::enable();
        
        crate::kinfo!("SMP: Core {} entering idle loop", idx);
        
        // Enter idle loop - scheduler will take over
        loop {
            cpu_hlt();
        }
    }
}
