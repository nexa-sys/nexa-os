//! SMP Initialization
//!
//! This module handles the initialization of the SMP subsystem,
//! including ACPI detection, LAPIC configuration, and AP startup.

use core::mem::MaybeUninit;
use core::sync::atomic::Ordering;

use x86_64::instructions::tables::sgdt;

use crate::{acpi, bootinfo, lapic, paging};

use super::ap_startup::start_ap;
use super::cpu::current_online;
use super::state::{CPU_TOTAL, ENABLE_AP_STARTUP, SMP_READY};
use super::trampoline::{install_trampoline, patch_gdt_descriptors};
use super::types::{
    cpu_info, CpuData, CpuInfo, BSP_APIC_ID, CPU_DATA, CPU_INFOS, MAX_CPUS,
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
    core::sync::atomic::fence(Ordering::SeqCst); // Full memory barrier

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

    // Initialize BSP CPU data
    for i in 0..count {
        let info = cpu_info(i);
        if info.is_bsp {
            CPU_DATA[i]
                .as_mut_ptr()
                .write(CpuData::new(i as u8, info.apic_id));
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
    crate::kinfo!("  Trampoline installed at: {:#x}", super::types::TRAMPOLINE_BASE);
    crate::kinfo!("  Trampoline vector: {:#x}", super::types::TRAMPOLINE_VECTOR);
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

    // Start all AP cores
    let mut started = 0usize;

    for idx in 1..count {
        let info = cpu_info(idx);

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

    Ok(())
}

/// Get stack address for debug logging (simplified version)
unsafe fn stack_for_debug(index: usize) -> Result<u64, &'static str> {
    use core::ptr;
    use super::types::{AP_STACKS, AP_STACK_SIZE};
    
    if index == 0 {
        return Err("Stack request for BSP");
    }
    let stack_index = index - 1;
    if stack_index >= MAX_CPUS - 1 {
        return Err("No AP stack slot available");
    }
    let stack_base = ptr::addr_of!(AP_STACKS[stack_index].0) as usize;
    let stack_top = stack_base + AP_STACK_SIZE;
    let aligned_top = stack_top & !0xF;
    Ok(aligned_top as u64)
}
