use core::mem::{self, MaybeUninit};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use x86_64::instructions::tables::sgdt;
use x86_64::instructions::{hlt as cpu_hlt, interrupts};

use crate::{acpi, gdt, interrupts as idt, lapic, paging};

pub const MAX_CPUS: usize = acpi::MAX_CPUS;
const TRAMPOLINE_BASE: u64 = 0x8000;
const TRAMPOLINE_MAX_SIZE: usize = 4096;
const AP_STACK_SIZE: usize = 16 * 4096;
const TRAMPOLINE_VECTOR: u8 = (TRAMPOLINE_BASE >> 12) as u8;
const STARTUP_WAIT_LOOPS: u64 = 5_000_000;

static SMP_READY: AtomicBool = AtomicBool::new(false);
static TRAMPOLINE_READY: AtomicBool = AtomicBool::new(false);
static CPU_TOTAL: AtomicUsize = AtomicUsize::new(1);

static mut AP_STACKS: [[u8; AP_STACK_SIZE]; MAX_CPUS - 1] = [[0; AP_STACK_SIZE]; MAX_CPUS - 1];

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq)]
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

struct CpuInfo {
    apic_id: u32,
    acpi_id: u8,
    is_bsp: bool,
    status: AtomicU8,
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

    crate::kinfo!(
        "SMP: Detected {} logical CPUs (BSP APIC {:#x})",
        count,
        bsp_apic
    );

    // Temporarily disable AP startup to debug the issue
    crate::kinfo!("SMP: AP core startup temporarily disabled for stability");
    crate::kinfo!(
        "SMP: {} / {} cores online (BSP only)",
        1,
        CPU_TOTAL.load(Ordering::SeqCst)
    );

    /* TODO: Re-enable after fixing trampoline/paging issues
    for idx in 0..count {
        let info = cpu_info(idx);
        if info.is_bsp {
            continue;
        }
        if let Err(err) = start_ap(idx) {
            crate::kwarn!(
                "SMP: Failed to start APIC {:#x} (index {}): {}",
                info.apic_id,
                idx,
                err
            );
        }
    }

    crate::kinfo!(
        "SMP: {} / {} cores online",
        current_online(),
        CPU_TOTAL.load(Ordering::SeqCst)
    );
    */

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
    prepare_ap_launch(index)?;
    cpu_info(index)
        .status
        .store(CpuStatus::Booting as u8, Ordering::SeqCst);

    let apic_id = cpu_info(index).apic_id;
    lapic::send_init_ipi(apic_id);
    busy_wait(10_000);
    lapic::send_startup_ipi(apic_id, TRAMPOLINE_VECTOR);
    busy_wait(2_000);
    lapic::send_startup_ipi(apic_id, TRAMPOLINE_VECTOR);

    if wait_for_online(index, STARTUP_WAIT_LOOPS) {
        crate::kinfo!("SMP: APIC {:#x} online", apic_id);
        Ok(())
    } else {
        Err("AP failed to signal ready")
    }
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
    write_trampoline_bytes(&__ap_trampoline_start, &ap_pml4_ptr, &pml4.to_le_bytes())?;

    let stack = stack_for(index)?;
    write_trampoline_bytes(&__ap_trampoline_start, &ap_stack_ptr, &stack.to_le_bytes())?;

    AP_BOOT_ARGS[index] = ApBootArgs {
        cpu_index: index as u32,
        apic_id: cpu_info(index).apic_id,
    };
    let arg_ptr = (&AP_BOOT_ARGS[index] as *const ApBootArgs) as u64;
    write_trampoline_bytes(&__ap_trampoline_start, &ap_arg_ptr, &arg_ptr.to_le_bytes())?;

    let entry = ap_entry as usize as u64;
    write_trampoline_bytes(&__ap_trampoline_start, &ap_entry_ptr, &entry.to_le_bytes())?;

    Ok(())
}

unsafe fn stack_for(index: usize) -> Result<u64, &'static str> {
    if index == 0 {
        return Err("Stack request for BSP");
    }
    if index - 1 >= AP_STACKS.len() {
        return Err("No AP stack slot available");
    }
    let stack = &AP_STACKS[index - 1];
    Ok(stack.as_ptr() as u64 + AP_STACK_SIZE as u64)
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
    unsafe {
        if arg.is_null() {
            crate::kfatal!("AP entry received null args");
        }
        let args = *arg;
        crate::configure_gs_base();
        gdt::init_ap();
        idt::init_interrupts_ap();

        let idx = args.cpu_index as usize;
        if idx < CPU_TOTAL.load(Ordering::SeqCst) {
            cpu_info(idx)
                .status
                .store(CpuStatus::Online as u8, Ordering::SeqCst);
            crate::kinfo!("SMP: Core {} (APIC {:#x}) initialized", idx, args.apic_id);
        } else {
            crate::kwarn!("SMP: AP reported invalid cpu_index {}", args.cpu_index);
        }

        interrupts::enable();
        loop {
            cpu_hlt();
        }
    }
}
