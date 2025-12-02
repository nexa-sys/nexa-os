//! AP Trampoline Installation and Configuration
//!
//! This module handles the installation and patching of the AP (Application Processor)
//! trampoline code used for booting secondary CPU cores.

use core::mem;
use core::ptr;
use core::sync::atomic::Ordering;

use x86_64::instructions::tables::sgdt;

use crate::bootinfo;

use super::state::TRAMPOLINE_READY;
use super::types::{
    PerCpuTrampolineData, MAX_CPUS, PER_CPU_DATA_SIZE, TRAMPOLINE_BASE, TRAMPOLINE_MAX_SIZE,
};

/// Get kernel relocation offset using multiple fallback methods.
/// Returns None if kernel was not relocated or offset cannot be determined.
pub fn get_kernel_relocation_offset() -> Option<i64> {
    // First try the direct kernel_load_offset from boot info
    if let Some(offset) = bootinfo::kernel_load_offset() {
        return Some(offset);
    }

    // Fallback: calculate from entry points if available
    if let Some((expected, actual)) = bootinfo::kernel_entry_points() {
        if expected != 0 && actual != 0 && expected != actual {
            let offset = actual as i64 - expected as i64;
            crate::kinfo!(
                "SMP: Calculated relocation offset from entry points: {:#x}",
                offset
            );
            return Some(offset);
        }
    }

    None
}

/// Install the AP trampoline code at low memory
pub unsafe fn install_trampoline() -> Result<(), &'static str> {
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
        crate::kinfo!(
            "SMP: Trampoline source: link={:#x}, offset={:#x}, relocated={:#x}",
            link_start,
            offset,
            relocated
        );
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

    crate::kinfo!(
        "SMP: Installing trampoline at {:#x} (size {} bytes)",
        TRAMPOLINE_BASE,
        size
    );

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
    let code_offset = 0x140; // Around where entry reading code should be
    let code_at = (TRAMPOLINE_BASE as usize + code_offset) as *const u8;
    let mut dump_code = [0u8; 64];
    for i in 0..64 {
        dump_code[i] = ptr::read_volatile(code_at.add(i));
    }
    crate::kinfo!(
        "SMP: Code at 0x{:x}:",
        TRAMPOLINE_BASE as usize + code_offset
    );
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

/// Patch GDT and IDT descriptors in the trampoline
pub unsafe fn patch_gdt_descriptors() -> Result<(), &'static str> {
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
    crate::kinfo!(
        "SMP: IDT configured for AP cores (base={:#x}, limit={:#x})",
        idt_base,
        idt_limit
    );

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

/// Write bytes to the trampoline at a specific field offset
pub unsafe fn write_trampoline_bytes(
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

// ============================================================================
// Per-CPU Data for Parallel AP Initialization
// ============================================================================

/// Write per-CPU trampoline data for a specific CPU index
/// This allows each AP to have its own independent startup parameters
/// enabling parallel initialization without data races.
pub unsafe fn write_per_cpu_data(
    cpu_index: usize,
    data: &PerCpuTrampolineData,
) -> Result<(), &'static str> {
    extern "C" {
        static __ap_trampoline_start: u8;
        static ap_per_cpu_data: u8;
    }

    if cpu_index >= MAX_CPUS {
        return Err("CPU index out of range");
    }

    // Calculate offset of ap_per_cpu_data from trampoline start
    let per_cpu_base_offset =
        (&ap_per_cpu_data as *const u8 as usize) - (&__ap_trampoline_start as *const u8 as usize);

    // Calculate address: TRAMPOLINE_BASE + per_cpu_base_offset + cpu_index * PER_CPU_DATA_SIZE
    let per_cpu_addr =
        TRAMPOLINE_BASE as usize + per_cpu_base_offset + cpu_index * PER_CPU_DATA_SIZE;

    // Write the per-CPU data structure
    let dest = per_cpu_addr as *mut PerCpuTrampolineData;
    ptr::write_volatile(dest, *data);

    // Memory fence to ensure write is visible to other cores
    core::sync::atomic::fence(Ordering::SeqCst);

    crate::kinfo!(
        "SMP: Per-CPU data for CPU {} at {:#x}: stack={:#x}, entry={:#x}, arg={:#x}",
        cpu_index,
        per_cpu_addr,
        data.stack_ptr,
        data.entry_ptr,
        data.arg_ptr
    );

    Ok(())
}

/// Set the APIC ID to CPU index mapping
/// This mapping is used by AP cores to find their per-CPU data
pub unsafe fn set_apic_to_index_mapping(apic_id: u32, cpu_index: u8) -> Result<(), &'static str> {
    extern "C" {
        static __ap_trampoline_start: u8;
        static ap_apic_to_index: u8;
    }

    if apic_id > 255 {
        return Err("APIC ID out of range (max 255)");
    }

    // Calculate offset of ap_apic_to_index table in trampoline
    let offset =
        (&ap_apic_to_index as *const u8 as usize) - (&__ap_trampoline_start as *const u8 as usize);

    // Write to the mapping table at TRAMPOLINE_BASE + offset + apic_id
    let mapping_addr = (TRAMPOLINE_BASE as usize + offset + apic_id as usize) as *mut u8;
    ptr::write_volatile(mapping_addr, cpu_index);

    crate::kinfo!(
        "SMP: APIC ID {} -> CPU index {} (mapping at {:#x})",
        apic_id,
        cpu_index,
        mapping_addr as usize
    );

    Ok(())
}

/// Write the CPU total count to the trampoline area
/// This value is written by BSP and read by AP cores for validation
/// Uses a fixed location in low memory that AP cores can reliably access
pub unsafe fn set_cpu_total_in_trampoline(cpu_count: usize) -> Result<(), &'static str> {
    extern "C" {
        static __ap_trampoline_start: u8;
        static ap_cpu_total: u8;
    }

    // Calculate offset of ap_cpu_total in trampoline
    let offset =
        (&ap_cpu_total as *const u8 as usize) - (&__ap_trampoline_start as *const u8 as usize);

    // Write to TRAMPOLINE_BASE + offset
    let addr = (TRAMPOLINE_BASE as usize + offset) as *mut u64;
    ptr::write_volatile(addr, cpu_count as u64);

    // Memory fence to ensure write is visible
    core::sync::atomic::fence(Ordering::SeqCst);

    crate::kinfo!(
        "SMP: CPU total {} written to trampoline at {:#x}",
        cpu_count,
        addr as usize
    );

    Ok(())
}

/// Read the CPU total count from the trampoline area
/// This function is called by AP cores to get the validated CPU count
/// Returns the CPU count from the fixed trampoline location
pub unsafe fn get_cpu_total_from_trampoline() -> usize {
    extern "C" {
        static __ap_trampoline_start: u8;
        static ap_cpu_total: u8;
    }

    // Calculate offset of ap_cpu_total in trampoline
    let offset =
        (&ap_cpu_total as *const u8 as usize) - (&__ap_trampoline_start as *const u8 as usize);

    // Read from TRAMPOLINE_BASE + offset
    let addr = (TRAMPOLINE_BASE as usize + offset) as *const u64;
    let count = ptr::read_volatile(addr) as usize;

    count
}

/// CPU status values for trampoline status array
pub const CPU_STATUS_OFFLINE: u8 = 0;
pub const CPU_STATUS_BOOTING: u8 = 1;
pub const CPU_STATUS_ONLINE: u8 = 2;

/// Set CPU status in trampoline area
/// This is used by both BSP (to set booting) and AP (to set online)
pub unsafe fn set_cpu_status_in_trampoline(cpu_index: usize, status: u8) {
    extern "C" {
        static __ap_trampoline_start: u8;
        static ap_cpu_status: u8;
    }

    // Calculate offset of ap_cpu_status array
    let offset =
        (&ap_cpu_status as *const u8 as usize) - (&__ap_trampoline_start as *const u8 as usize);

    // Write to TRAMPOLINE_BASE + offset + cpu_index
    let addr = (TRAMPOLINE_BASE as usize + offset + cpu_index) as *mut u8;
    ptr::write_volatile(addr, status);

    // Memory fence
    core::sync::atomic::fence(Ordering::SeqCst);
}

/// Get CPU status from trampoline area
/// Returns the status byte for the given CPU index
pub unsafe fn get_cpu_status_from_trampoline(cpu_index: usize) -> u8 {
    extern "C" {
        static __ap_trampoline_start: u8;
        static ap_cpu_status: u8;
    }

    // Calculate offset of ap_cpu_status array
    let offset =
        (&ap_cpu_status as *const u8 as usize) - (&__ap_trampoline_start as *const u8 as usize);

    // Read from TRAMPOLINE_BASE + offset + cpu_index
    let addr = (TRAMPOLINE_BASE as usize + offset + cpu_index) as *const u8;
    ptr::read_volatile(addr)
}

/// Initialize all APIC-to-index mappings for detected CPUs
pub unsafe fn init_apic_mappings(cpus: &[(u32, u8)]) -> Result<(), &'static str> {
    for &(apic_id, cpu_index) in cpus {
        set_apic_to_index_mapping(apic_id, cpu_index)?;
    }
    Ok(())
}

/// Prepare all per-CPU data for parallel startup
/// Returns the number of CPUs prepared
pub unsafe fn prepare_all_per_cpu_data<F>(
    cpu_count: usize,
    mut data_fn: F,
) -> Result<usize, &'static str>
where
    F: FnMut(usize) -> Result<PerCpuTrampolineData, &'static str>,
{
    let mut prepared = 0;

    for idx in 0..cpu_count {
        match data_fn(idx) {
            Ok(data) => {
                write_per_cpu_data(idx, &data)?;
                prepared += 1;
            }
            Err(e) => {
                crate::kwarn!("SMP: Failed to prepare data for CPU {}: {}", idx, e);
            }
        }
    }

    Ok(prepared)
}
