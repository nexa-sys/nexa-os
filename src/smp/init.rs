//! SMP Initialization
//!
//! This module handles the initialization of the SMP subsystem,
//! including ACPI detection, LAPIC configuration, and AP startup.

use core::sync::atomic::Ordering;

use x86_64::instructions::tables::sgdt;

use crate::{acpi, lapic, paging};

use super::alloc as smp_alloc;
use super::ap_startup::{start_all_aps_parallel, start_ap};
use super::cpu::current_online;
use super::state::{CPU_TOTAL, ENABLE_AP_STARTUP, PARALLEL_AP_STARTUP, SMP_READY};
use super::trampoline::{install_trampoline, patch_gdt_descriptors};
use super::types::{
    CpuData, CpuInfo, BSP_APIC_ID, CPU_DATA, CPU_INFOS, MAX_CPUS, STATIC_CPU_COUNT,
};

/// Initialize the SMP subsystem
pub fn init() {
    if SMP_READY.load(Ordering::SeqCst) {
        return;
    }

    match unsafe { init_inner() } {
        Ok(()) => SMP_READY.store(true, Ordering::SeqCst),
        Err(err) => crate::kwarn!("SMP initialization skipped: {}", err),
    }
}

/// Inner initialization function
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

    // Count CPUs first (limited by MAX_CPUS)
    let mut count = 0usize;
    for _ in cpus.iter() {
        if count >= MAX_CPUS {
            break;
        }
        count += 1;
    }

    if count > MAX_CPUS {
        crate::kwarn!(
            "SMP: Limiting CPU count to {} (hardware reports {})",
            MAX_CPUS,
            cpus.len()
        );
        count = MAX_CPUS;
    }

    // Initialize dynamic allocation system and pre-allocate resources
    // This MUST be done before accessing any per-CPU data structures
    smp_alloc::init();
    if let Err(e) = smp_alloc::allocate_for_cpus(count) {
        crate::kwarn!("SMP: Failed to allocate dynamic resources: {}", e);
        // With STATIC_CPU_COUNT = 1, we can only run BSP if dynamic alloc fails
        count = count.min(STATIC_CPU_COUNT);
        crate::kwarn!("SMP: Falling back to {} CPUs (BSP only)", count);
    }

    // Now populate CPU info structures
    let mut idx = 0usize;
    for desc in cpus.iter() {
        if idx >= count {
            break;
        }

        let is_bsp = desc.apic_id as u32 == bsp_apic;

        // BSP (index 0) uses static array, all APs use dynamic allocation
        if idx < STATIC_CPU_COUNT {
            // Only BSP uses static allocation
            CPU_INFOS[idx].as_mut_ptr().write(CpuInfo::new(
                desc.apic_id as u32,
                desc.acpi_processor_id,
                is_bsp,
            ));
        } else {
            // All APs use dynamic allocation
            if let Err(e) =
                smp_alloc::init_cpu_info(idx, desc.apic_id as u32, desc.acpi_processor_id, is_bsp)
            {
                crate::kwarn!("SMP: Failed to init CPU {} info: {}", idx, e);
            }
        }
        idx += 1;
    }

    CPU_TOTAL.store(count, Ordering::SeqCst);
    core::sync::atomic::fence(Ordering::SeqCst); // Full memory barrier

    // Write CPU count to trampoline for AP cores to read
    // This is critical because AP cores may not see updates to static variables
    // due to kernel relocation - they need to read from fixed low memory addresses
    if let Err(e) = super::trampoline::set_cpu_total_in_trampoline(count) {
        crate::kwarn!("SMP: Failed to write CPU total to trampoline: {}", e);
    }

    // Verify the store worked by reading back
    let read_back = CPU_TOTAL.load(Ordering::SeqCst);

    // Debug: Log CPU_TOTAL address (BSP view)
    let cpu_total_addr = &CPU_TOTAL as *const _ as u64;
    crate::kinfo!(
        "SMP: CPU_TOTAL={} stored at address {:#x}, read_back={}",
        count,
        cpu_total_addr,
        read_back
    );

    // Initialize BSP CPU data (BSP is always index 0 and uses static allocation)
    for i in 0..count {
        let info = get_cpu_info_safe(i)?;
        if info.is_bsp {
            // BSP should always be at index 0 and use static allocation
            if i < STATIC_CPU_COUNT {
                CPU_DATA[i]
                    .as_mut_ptr()
                    .write(CpuData::new(i as u16, info.apic_id));
            } else {
                // Unexpected: BSP at index >= 1 would need dynamic alloc
                if let Err(e) = smp_alloc::init_cpu_data(i, i as u16, info.apic_id) {
                    crate::kwarn!("SMP: Failed to init BSP CPU data: {}", e);
                }
            }
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
    crate::kinfo!(
        "  Trampoline installed at: {:#x}",
        super::types::TRAMPOLINE_BASE
    );
    crate::kinfo!(
        "  Trampoline vector: {:#x}",
        super::types::TRAMPOLINE_VECTOR
    );
    crate::kinfo!(
        "  PML4 physical address: {:#x}",
        paging::current_pml4_phys()
    );

    // Verify GDT descriptor patching
    let descriptor = sgdt();
    crate::kinfo!(
        "  GDT base for APs: {:#x}, limit: {:#x}",
        descriptor.base.as_u64(),
        descriptor.limit
    );

    // Verify AP stacks are available
    for i in 1..count.min(3) {
        // Log first 2 APs
        match stack_for_debug(i) {
            Ok(stack) => crate::kinfo!("  AP {} stack top: {:#x}", i, stack),
            Err(e) => crate::kwarn!("  AP {} stack error: {}", i, e),
        }
    }

    // Stage 3: Start all AP cores
    crate::kinfo!("SMP: Stage 3 - Attempting to start all remaining AP cores");

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

    // Choose startup mode based on configuration
    if PARALLEL_AP_STARTUP {
        // === Parallel AP Startup Mode ===
        // All APs are started simultaneously, each with independent data regions
        crate::kinfo!("SMP: Using PARALLEL AP startup mode");

        match start_all_aps_parallel(count) {
            Ok(started) => {
                crate::kinfo!("SMP: ✓ Parallel startup completed: {} APs online", started);
            }
            Err(err) => {
                crate::kwarn!("SMP: Parallel startup failed: {}", err);
                crate::kwarn!("SMP: Falling back to sequential startup...");

                // Fallback to sequential startup
                let mut started = 0usize;
                for idx in 1..count {
                    match start_ap(idx) {
                        Ok(()) => started += 1,
                        Err(e) => crate::kwarn!("SMP: Failed to start AP {}: {}", idx, e),
                    }
                }
                crate::kinfo!("SMP: Sequential fallback: {} APs started", started);
            }
        }
    } else {
        // === Sequential AP Startup Mode (Original) ===
        crate::kinfo!("SMP: Using SEQUENTIAL AP startup mode");

        let mut started = 0usize;

        for idx in 1..count {
            let info = match get_cpu_info_safe(idx) {
                Ok(info) => info,
                Err(e) => {
                    crate::kwarn!("SMP: Cannot get info for CPU {}: {}", idx, e);
                    continue;
                }
            };

            crate::kinfo!(
                "SMP: Starting AP core {} (APIC ID {:#x})...",
                idx,
                info.apic_id
            );

            match start_ap(idx) {
                Ok(()) => {
                    started += 1;
                    crate::kinfo!("SMP: ✓ AP core {} started successfully!", idx);
                }
                Err(err) => {
                    crate::kwarn!(
                        "SMP: ✗ Failed to start AP core {} (APIC {:#x}): {}",
                        idx,
                        info.apic_id,
                        err
                    );
                    // Continue trying other cores even if one fails
                }
            }
        }

        crate::kinfo!(
            "SMP: {} / {} cores online (BSP + {} APs)",
            current_online(),
            CPU_TOTAL.load(Ordering::SeqCst),
            started
        );
    }

    crate::kinfo!(
        "SMP: Final status: {} / {} cores online",
        current_online(),
        CPU_TOTAL.load(Ordering::SeqCst)
    );

    // Initialize per-CPU scheduler data for all online CPUs
    crate::kinfo!("SMP: Initializing per-CPU scheduler data...");
    for cpu in 0..count {
        crate::scheduler::init_percpu_sched(cpu);
    }
    crate::kinfo!("SMP: Per-CPU scheduler initialization complete");

    Ok(())
}

/// Get stack address for debug logging (simplified version)
/// All AP stacks are dynamically allocated
unsafe fn stack_for_debug(index: usize) -> Result<u64, &'static str> {
    if index == 0 {
        return Err("Stack request for BSP");
    }
    // All AP stacks are dynamically allocated
    smp_alloc::get_ap_stack_top(index).map_err(|_| "No AP stack slot available")
}

/// Safe wrapper to get CPU info that handles both static and dynamic allocation
/// BSP (index 0) uses static, all APs use dynamic
unsafe fn get_cpu_info_safe(idx: usize) -> Result<&'static CpuInfo, &'static str> {
    if idx < STATIC_CPU_COUNT {
        // BSP uses static array
        Ok(CPU_INFOS[idx].assume_init_ref())
    } else {
        // All APs use dynamic allocation
        smp_alloc::get_cpu_info(idx)
    }
}
