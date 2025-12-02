//! SMP Dynamic Allocation
//!
//! This module handles dynamic allocation of per-CPU resources for all AP cores.
//! Only BSP (CPU 0) uses statically allocated resources (STATIC_CPU_COUNT = 1).
//! All AP cores (CPU 1..MAX_CPUS) get their resources dynamically allocated
//! during SMP initialization.
//!
//! This approach supports up to MAX_CPUS (1024) cores while keeping the kernel
//! image size minimal - resources are only allocated for CPUs that actually exist.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;
use x86_64::structures::gdt::GlobalDescriptorTable;
use x86_64::structures::tss::TaskStateSegment;

use super::types::{
    AlignedApStack, ApBootArgs, CpuData, CpuInfo, PerCpuGsData, AP_STACK_SIZE, MAX_CPUS,
    STATIC_CPU_COUNT,
};
use crate::safety::serial_debug_byte;

/// GDT stack size - matches gdt.rs STACK_SIZE (4096 * 5 = 20KB)
pub const GDT_STACK_SIZE: usize = 4096 * 5;

/// Simple static array to store GS data pointers for APs
/// This avoids Mutex/Vec complexity during early AP startup
pub static mut GS_DATA_PTRS: [u64; MAX_CPUS] = [0; MAX_CPUS];

/// Simple static array to store TSS pointers for APs
pub static mut TSS_PTRS: [u64; MAX_CPUS] = [0; MAX_CPUS];

/// Simple static array to store GDT pointers for APs
pub static mut GDT_PTRS: [u64; MAX_CPUS] = [0; MAX_CPUS];

/// Simple static array to store GDT stack pointers for APs
pub static mut GDT_STACKS_PTRS: [u64; MAX_CPUS] = [0; MAX_CPUS];

/// Aligned stack matching gdt.rs AlignedStack type
#[repr(C, align(16))]
#[derive(Clone)]
pub struct AlignedStack {
    pub bytes: [u8; GDT_STACK_SIZE],
}

impl AlignedStack {
    pub const fn new() -> Self {
        Self {
            bytes: [0; GDT_STACK_SIZE],
        }
    }
}

/// Aligned stack for GDT per-CPU stacks (matches gdt.rs PerCpuStacks)
#[repr(C, align(16))]
pub struct AlignedGdtStacks {
    pub kernel_stack: AlignedStack,
    pub double_fault_stack: AlignedStack,
    pub error_code_stack: AlignedStack,
}

impl AlignedGdtStacks {
    pub const fn new() -> Self {
        Self {
            kernel_stack: AlignedStack::new(),
            double_fault_stack: AlignedStack::new(),
            error_code_stack: AlignedStack::new(),
        }
    }
}

/// Dynamic per-CPU data storage
struct DynamicCpuResources {
    /// Dynamically allocated AP stacks (indexed by cpu_index - 1, since BSP doesn't need one)
    ap_stacks: Vec<Option<Box<AlignedApStack>>>,
    /// Dynamically allocated CPU data (for all CPUs, though BSP may use static)
    cpu_data: Vec<Option<Box<CpuData>>>,
    /// Dynamically allocated CPU info (for all CPUs, though BSP may use static)
    cpu_infos: Vec<Option<Box<CpuInfo>>>,
    /// Dynamically allocated GS data for APs
    gs_data: Vec<Option<Box<PerCpuGsData>>>,
    /// Dynamically allocated boot args
    boot_args: Vec<Option<Box<ApBootArgs>>>,
    /// Dynamically allocated TSS for all APs (CPU >= 1)
    tss: Vec<Option<Box<TaskStateSegment>>>,
    /// Dynamically allocated GDT for all APs (CPU >= 1)
    gdt: Vec<Option<Box<GlobalDescriptorTable>>>,
    /// Dynamically allocated GDT stacks for all APs (CPU >= 1)
    gdt_stacks: Vec<Option<Box<AlignedGdtStacks>>>,
    /// Number of CPUs resources have been allocated for
    allocated_count: usize,
}

impl DynamicCpuResources {
    const fn new() -> Self {
        Self {
            ap_stacks: Vec::new(),
            cpu_data: Vec::new(),
            cpu_infos: Vec::new(),
            gs_data: Vec::new(),
            boot_args: Vec::new(),
            tss: Vec::new(),
            gdt: Vec::new(),
            gdt_stacks: Vec::new(),
            allocated_count: 0,
        }
    }
}

/// Global dynamic resources protected by mutex
static DYNAMIC_RESOURCES: Mutex<DynamicCpuResources> = Mutex::new(DynamicCpuResources::new());

/// Flag indicating dynamic allocation is ready
static DYNAMIC_ALLOC_READY: AtomicBool = AtomicBool::new(false);

/// Number of CPUs we've allocated resources for
static ALLOCATED_CPU_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Initialize dynamic allocation system
/// Called after heap is ready but before SMP init
pub fn init() {
    DYNAMIC_ALLOC_READY.store(true, Ordering::SeqCst);
    crate::kinfo!("SMP: Dynamic allocation system initialized");
}

/// Check if dynamic allocation is ready
pub fn is_ready() -> bool {
    DYNAMIC_ALLOC_READY.load(Ordering::Acquire)
}

/// Allocate resources for a specific number of CPUs
/// This pre-allocates all needed resources to avoid allocation during AP startup
pub fn allocate_for_cpus(cpu_count: usize) -> Result<(), &'static str> {
    if !is_ready() {
        return Err("Dynamic allocation not ready");
    }

    if cpu_count == 0 {
        return Err("CPU count must be > 0");
    }

    let mut resources = DYNAMIC_RESOURCES.lock();

    // Don't re-allocate if already done
    if resources.allocated_count >= cpu_count {
        return Ok(());
    }

    crate::kinfo!(
        "SMP: Allocating dynamic resources for {} CPUs (was {})",
        cpu_count,
        resources.allocated_count
    );

    // Resize vectors to accommodate all CPUs
    // AP stacks: cpu_count - 1 (BSP doesn't need one)
    let ap_stack_count = cpu_count.saturating_sub(1);

    // Number of dynamically allocated GDT resources (for all APs, i.e., CPU >= 1)
    // With STATIC_CPU_COUNT = 1, this equals ap_stack_count
    let dynamic_gdt_count = cpu_count.saturating_sub(STATIC_CPU_COUNT);

    // Extend vectors with None values
    while resources.ap_stacks.len() < ap_stack_count {
        resources.ap_stacks.push(None);
    }
    while resources.cpu_data.len() < cpu_count {
        resources.cpu_data.push(None);
    }
    while resources.cpu_infos.len() < cpu_count {
        resources.cpu_infos.push(None);
    }
    while resources.gs_data.len() < cpu_count {
        resources.gs_data.push(None);
    }
    while resources.boot_args.len() < cpu_count {
        resources.boot_args.push(None);
    }
    // GDT resources for all APs (CPU >= 1)
    while resources.tss.len() < dynamic_gdt_count {
        resources.tss.push(None);
    }
    while resources.gdt.len() < dynamic_gdt_count {
        resources.gdt.push(None);
    }
    while resources.gdt_stacks.len() < dynamic_gdt_count {
        resources.gdt_stacks.push(None);
    }

    // Pre-allocate AP stacks (these are large, so we allocate them upfront)
    for i in 0..ap_stack_count {
        if resources.ap_stacks[i].is_none() {
            // Allocate zeroed stack
            let stack = Box::new(AlignedApStack([0u8; AP_STACK_SIZE]));
            resources.ap_stacks[i] = Some(stack);
            crate::kdebug!("SMP: Allocated stack for AP {}", i + 1);
        }
    }

    // Pre-allocate boot args
    for i in 0..cpu_count {
        if resources.boot_args[i].is_none() {
            resources.boot_args[i] = Some(Box::new(ApBootArgs::new()));
        }
    }

    // Pre-allocate GS data
    for i in 0..cpu_count {
        if resources.gs_data[i].is_none() {
            let mut gs_data = Box::new(PerCpuGsData::new());
            // Store pointer in static array for lock-free access by APs
            unsafe {
                GS_DATA_PTRS[i] = &mut *gs_data as *mut _ as u64;
            }
            resources.gs_data[i] = Some(gs_data);
        }
    }

    // Pre-allocate GDT resources for dynamic CPUs
    for i in 0..dynamic_gdt_count {
        let cpu_idx = i + STATIC_CPU_COUNT;

        if resources.tss[i].is_none() {
            let mut tss = Box::new(TaskStateSegment::new());
            unsafe {
                TSS_PTRS[cpu_idx] = &mut *tss as *mut _ as u64;
            }
            resources.tss[i] = Some(tss);
            crate::kdebug!("SMP: Allocated TSS for CPU {}", cpu_idx);
        }
        if resources.gdt[i].is_none() {
            let mut gdt = Box::new(GlobalDescriptorTable::new());
            unsafe {
                GDT_PTRS[cpu_idx] = &mut *gdt as *mut _ as u64;
            }
            resources.gdt[i] = Some(gdt);
            crate::kdebug!("SMP: Allocated GDT for CPU {}", cpu_idx);
        }
        if resources.gdt_stacks[i].is_none() {
            let mut stacks = Box::new(AlignedGdtStacks::new());
            unsafe {
                GDT_STACKS_PTRS[cpu_idx] = &mut *stacks as *mut _ as u64;
            }
            resources.gdt_stacks[i] = Some(stacks);
            crate::kdebug!("SMP: Allocated GDT stacks for CPU {}", cpu_idx);
        }
    }

    resources.allocated_count = cpu_count;

    // Full memory fence to ensure all allocations are visible before updating the atomic
    // This is critical for AP cores to see the correct data when they read ALLOCATED_CPU_COUNT
    core::sync::atomic::fence(Ordering::SeqCst);

    ALLOCATED_CPU_COUNT.store(cpu_count, Ordering::SeqCst);

    // Additional fence after store to ensure AP cores see the updated count
    core::sync::atomic::fence(Ordering::SeqCst);

    let total_stack_mem = ap_stack_count * AP_STACK_SIZE;
    let total_gdt_stack_mem = dynamic_gdt_count * 2 * GDT_STACK_SIZE;
    crate::kinfo!(
        "SMP: Dynamic allocation complete: {} AP stacks ({} KB), {} dynamic GDT sets ({} KB)",
        ap_stack_count,
        total_stack_mem / 1024,
        dynamic_gdt_count,
        total_gdt_stack_mem / 1024
    );

    Ok(())
}

/// Get the stack top address for an AP core (cpu_index >= 1)
pub fn get_ap_stack_top(cpu_index: usize) -> Result<u64, &'static str> {
    if cpu_index == 0 {
        return Err("BSP doesn't use AP stack");
    }

    let stack_index = cpu_index - 1;
    let resources = DYNAMIC_RESOURCES.lock();

    if stack_index >= resources.ap_stacks.len() {
        return Err("CPU index out of range");
    }

    match &resources.ap_stacks[stack_index] {
        Some(stack) => {
            let stack_base = stack.0.as_ptr() as u64;
            let stack_top = stack_base.wrapping_add(AP_STACK_SIZE as u64);
            // IMPORTANT: Align DOWN to 16 bytes (stack must be 16-byte aligned before call)
            // Note: GlobalAllocator ignores alignment, so Box may not respect align(16)
            // Use core::hint::black_box to prevent compiler from optimizing away the alignment
            let aligned_top = core::hint::black_box(stack_top) & !0xFu64;
            crate::kinfo!(
                "SMP: Dynamic stack for CPU {}: base={:#x}, top={:#x}, aligned={:#x}",
                cpu_index,
                stack_base,
                stack_top,
                aligned_top
            );
            Ok(aligned_top)
        }
        None => Err("Stack not allocated for this CPU"),
    }
}

/// Get a raw pointer to the AP stack for a CPU (used by trampoline setup)
pub fn get_ap_stack_ptr(cpu_index: usize) -> Result<*const u8, &'static str> {
    if cpu_index == 0 {
        return Err("BSP doesn't use AP stack");
    }

    let stack_index = cpu_index - 1;
    let resources = DYNAMIC_RESOURCES.lock();

    if stack_index >= resources.ap_stacks.len() {
        return Err("CPU index out of range");
    }

    match &resources.ap_stacks[stack_index] {
        Some(stack) => Ok(stack.0.as_ptr()),
        None => Err("Stack not allocated for this CPU"),
    }
}

/// Initialize CpuInfo for a CPU
pub fn init_cpu_info(
    cpu_index: usize,
    apic_id: u32,
    acpi_id: u8,
    is_bsp: bool,
) -> Result<(), &'static str> {
    let mut resources = DYNAMIC_RESOURCES.lock();

    if cpu_index >= resources.cpu_infos.len() {
        return Err("CPU index out of range");
    }

    resources.cpu_infos[cpu_index] = Some(Box::new(CpuInfo::new(apic_id, acpi_id, is_bsp)));
    Ok(())
}

/// Get CpuInfo for a CPU
pub fn get_cpu_info(cpu_index: usize) -> Result<&'static CpuInfo, &'static str> {
    let resources = DYNAMIC_RESOURCES.lock();

    if cpu_index >= resources.cpu_infos.len() {
        return Err("CPU index out of range");
    }

    match &resources.cpu_infos[cpu_index] {
        Some(info) => {
            // SAFETY: The Box lives for 'static since we never remove it
            let ptr = &**info as *const CpuInfo;
            Ok(unsafe { &*ptr })
        }
        None => Err("CpuInfo not initialized"),
    }
}

/// Initialize CpuData for a CPU
pub fn init_cpu_data(cpu_index: usize, cpu_id: u16, apic_id: u32) -> Result<(), &'static str> {
    // Read CPU total from trampoline - this is reliable for AP cores
    // because it's in a fixed low memory location that doesn't get relocated
    let cpu_total = unsafe { super::trampoline::get_cpu_total_from_trampoline() };

    if cpu_index >= cpu_total {
        // CPU index exceeds the total count written by BSP
        return Err("CPU index exceeds CPU total");
    }

    // Memory fence to ensure we see the latest Vec data after allocation
    core::sync::atomic::fence(Ordering::SeqCst);

    let mut resources = DYNAMIC_RESOURCES.lock();

    // Double-check the Vec length (should match allocated count)
    if cpu_index >= resources.cpu_data.len() {
        return Err("CPU index out of range (Vec not ready)");
    }

    resources.cpu_data[cpu_index] = Some(Box::new(CpuData::new(cpu_id, apic_id)));
    Ok(())
}

/// Get CpuData for a CPU
pub fn get_cpu_data(cpu_index: usize) -> Result<&'static CpuData, &'static str> {
    let resources = DYNAMIC_RESOURCES.lock();

    if cpu_index >= resources.cpu_data.len() {
        return Err("CPU index out of range");
    }

    match &resources.cpu_data[cpu_index] {
        Some(data) => {
            // SAFETY: The Box lives for 'static since we never remove it
            let ptr = &**data as *const CpuData;
            Ok(unsafe { &*ptr })
        }
        None => Err("CpuData not initialized"),
    }
}

/// Get GS data pointer for a CPU
pub fn get_gs_data_ptr(cpu_index: usize) -> Result<*mut PerCpuGsData, &'static str> {
    // Use the static array instead of Mutex/Vec to avoid lock contention/initialization issues
    if cpu_index >= MAX_CPUS {
        return Err("CPU index out of range");
    }

    let ptr = unsafe { GS_DATA_PTRS[cpu_index] };
    if ptr == 0 {
        return Err("GS data not allocated");
    }

    Ok(ptr as *mut PerCpuGsData)
}

/// Get boot args for a CPU
pub fn get_boot_args_ptr(cpu_index: usize) -> Result<*mut ApBootArgs, &'static str> {
    let mut resources = DYNAMIC_RESOURCES.lock();

    if cpu_index >= resources.boot_args.len() {
        return Err("CPU index out of range");
    }

    match &mut resources.boot_args[cpu_index] {
        Some(args) => Ok(&mut **args as *mut ApBootArgs),
        None => Err("Boot args not allocated"),
    }
}

/// Get the number of CPUs resources have been allocated for
pub fn allocated_cpu_count() -> usize {
    ALLOCATED_CPU_COUNT.load(Ordering::Acquire)
}

// ============================================================================
// GDT/TSS Dynamic Allocation Accessors
// ============================================================================

/// Get mutable TSS reference for a dynamically allocated CPU
/// cpu_index must be >= STATIC_CPU_COUNT
pub fn get_dynamic_tss_mut(
    cpu_index: usize,
) -> Result<&'static mut TaskStateSegment, &'static str> {
    if cpu_index < STATIC_CPU_COUNT {
        return Err("CPU uses static TSS allocation");
    }

    if cpu_index >= MAX_CPUS {
        return Err("CPU index out of range");
    }

    let ptr = unsafe { TSS_PTRS[cpu_index] };
    if ptr == 0 {
        return Err("TSS not allocated");
    }

    Ok(unsafe { &mut *(ptr as *mut TaskStateSegment) })
}

/// Get TSS reference for a dynamically allocated CPU (const version)
pub fn get_dynamic_tss(cpu_index: usize) -> Result<&'static TaskStateSegment, &'static str> {
    if cpu_index < STATIC_CPU_COUNT {
        return Err("CPU uses static TSS allocation");
    }

    if cpu_index >= MAX_CPUS {
        return Err("CPU index out of range");
    }

    let ptr = unsafe { TSS_PTRS[cpu_index] };
    if ptr == 0 {
        return Err("TSS not allocated");
    }

    Ok(unsafe { &*(ptr as *const TaskStateSegment) })
}

/// Get mutable GDT reference for a dynamically allocated CPU
pub fn get_dynamic_gdt_mut(
    cpu_index: usize,
) -> Result<&'static mut GlobalDescriptorTable, &'static str> {
    if cpu_index < STATIC_CPU_COUNT {
        return Err("CPU uses static GDT allocation");
    }

    if cpu_index >= MAX_CPUS {
        return Err("CPU index out of range");
    }

    let ptr = unsafe { GDT_PTRS[cpu_index] };
    if ptr == 0 {
        return Err("GDT not allocated");
    }

    Ok(unsafe { &mut *(ptr as *mut GlobalDescriptorTable) })
}

/// Get GDT stacks for a dynamically allocated CPU
pub fn get_dynamic_gdt_stacks(cpu_index: usize) -> Result<&'static AlignedGdtStacks, &'static str> {
    if cpu_index < STATIC_CPU_COUNT {
        return Err("CPU uses static GDT stack allocation");
    }

    if cpu_index >= MAX_CPUS {
        return Err("CPU index out of range");
    }

    let ptr = unsafe { GDT_STACKS_PTRS[cpu_index] };
    if ptr == 0 {
        return Err("GDT stacks not allocated for this CPU");
    }

    Ok(unsafe { &*(ptr as *const AlignedGdtStacks) })
}

/// Check if a CPU uses dynamic allocation
pub fn uses_dynamic_allocation(cpu_index: usize) -> bool {
    cpu_index >= STATIC_CPU_COUNT
}
