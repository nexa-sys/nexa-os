//! AP (Application Processor) Startup
//!
//! This module handles the startup sequence for secondary CPU cores,
//! including sending INIT/STARTUP IPIs and waiting for cores to come online.
//! 
//! Supports parallel AP initialization by providing each AP core with
//! its own independent startup data region to avoid race conditions.

use core::ptr;
use core::sync::atomic::Ordering;

use x86_64::instructions::hlt as cpu_hlt;
use x86_64::registers::model_specific::Msr;

use crate::safety::{serial_debug_byte, serial_debug_hex};
use crate::{gdt, lapic, paging};

use super::state::{CPU_TOTAL, ENABLE_AP_STARTUP, ONLINE_CPUS};
use super::trampoline::{get_kernel_relocation_offset, write_trampoline_bytes, write_per_cpu_data, set_apic_to_index_mapping};
use super::types::{
    cpu_info, AlignedApStack, ApBootArgs, CpuData, CpuStatus, PerCpuGsData, PerCpuTrampolineData,
    AP_ARRIVED, AP_BOOT_ARGS, AP_GS_DATA, AP_STACK_SIZE, AP_STACKS, CPU_DATA,
    MAX_CPUS, STARTUP_RETRY_MAX, STARTUP_WAIT_LOOPS, TRAMPOLINE_BASE, TRAMPOLINE_VECTOR,
};

/// Start a single AP core
pub unsafe fn start_ap(index: usize) -> Result<(), &'static str> {
    let info = cpu_info(index);
    let apic_id = info.apic_id;

    for attempt in 0..STARTUP_RETRY_MAX {
        crate::kinfo!(
            "SMP: Starting AP core {} (APIC {:#x}), attempt {}/{}",
            index,
            apic_id,
            attempt + 1,
            STARTUP_RETRY_MAX
        );

        crate::kinfo!("SMP: [{}] Preparing launch parameters...", index);
        prepare_ap_launch(index)?;

        crate::kinfo!("SMP: [{}] Setting Booting state", index);
        info.status
            .store(CpuStatus::Booting as u8, Ordering::Release);
        info.startup_attempts.fetch_add(1, Ordering::Relaxed);

        // Ensure all writes are visible before sending IPIs
        core::sync::atomic::fence(Ordering::SeqCst);

        // Verify trampoline data one more time before sending INIT
        extern "C" {
            static __ap_trampoline_start: u8;
            static ap_entry_ptr: u8;
        }
        let entry_offset = {
            (&ap_entry_ptr as *const _ as usize) - (&__ap_trampoline_start as *const _ as usize)
        };
        let check_addr = (TRAMPOLINE_BASE as usize + entry_offset) as *const u64;
        let check_val = core::ptr::read_volatile(check_addr);
        crate::kinfo!(
            "SMP: [{}] Pre-IPI verify: entry at {:#x} = {:#x}",
            index,
            check_addr as usize,
            check_val
        );

        // Also dump the bytes around the entry pointer
        let dump_start = (TRAMPOLINE_BASE as usize + entry_offset - 8) as *const u8;
        let mut dump_str = [0u8; 48];
        for i in 0..24 {
            let b = core::ptr::read_volatile(dump_start.add(i));
            let hi = (b >> 4) & 0xF;
            let lo = b & 0xF;
            dump_str[i * 2] = if hi < 10 { b'0' + hi } else { b'A' + hi - 10 };
            dump_str[i * 2 + 1] = if lo < 10 { b'0' + lo } else { b'A' + lo - 10 };
        }
        let dump_s = core::str::from_utf8(&dump_str).unwrap_or("???");
        crate::kinfo!(
            "SMP: [{}] Bytes at {:#x}: {}",
            index,
            dump_start as usize,
            dump_s
        );

        // Send INIT IPI
        crate::kinfo!("SMP: [{}] Sending INIT IPI to APIC {:#x}", index, apic_id);
        lapic::send_init_ipi(apic_id);
        crate::kinfo!("SMP: [{}] INIT IPI sent, waiting 10ms...", index);
        busy_wait(100_000); // 10ms delay after INIT

        // Send STARTUP IPI (twice per Intel spec)
        crate::kinfo!(
            "SMP: [{}] Sending STARTUP IPI #1, vector {:#x}",
            index,
            TRAMPOLINE_VECTOR
        );
        lapic::send_startup_ipi(apic_id, TRAMPOLINE_VECTOR);
        crate::kinfo!("SMP: [{}] STARTUP IPI #1 sent, waiting 200us...", index);
        busy_wait(20_000); // 200us delay between SIPIs

        crate::kinfo!("SMP: [{}] Sending STARTUP IPI #2", index);
        lapic::send_startup_ipi(apic_id, TRAMPOLINE_VECTOR);
        crate::kinfo!(
            "SMP: [{}] STARTUP IPI #2 sent, waiting before check...",
            index
        );
        busy_wait(10_000); // Extra delay before checking

        // Check if AP arrived at entry point
        let arrived = AP_ARRIVED[index].load(Ordering::SeqCst);
        let magic = core::ptr::read_volatile(0x9000 as *const u32);
        let flag_addr = (0x9000 + (index as u32 + 1) * 4) as *const u32;
        let flag_val = core::ptr::read_volatile(flag_addr);

        if arrived == 0xDEADBEEF {
            crate::kinfo!("SMP: [{}] AP successfully arrived at entry point!", index);
        } else {
            crate::kerror!(
                "SMP: [{}] AP did NOT arrive (flag={:#x}, magic={:#x}, mem={:#x})",
                index,
                arrived,
                magic,
                flag_val
            );
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
            index,
            attempt + 1,
            status
        );

        // Reset status for retry
        info.status
            .store(CpuStatus::Offline as u8, Ordering::Release);

        // Longer delay before retry
        busy_wait(100_000);
    }

    Err("AP failed to start after maximum retries")
}

/// Prepare AP launch parameters in the trampoline (per-CPU data version)
/// This version writes to per-CPU data region for parallel initialization
unsafe fn prepare_ap_launch(index: usize) -> Result<(), &'static str> {
    extern "C" {
        static __ap_trampoline_start: u8;
        static ap_pml4_ptr: u8;
    }

    // PML4 is shared by all APs (same page table)
    let pml4 = paging::current_pml4_phys();
    crate::kinfo!("SMP: [{}] PML4: {:#x}", index, pml4);
    write_trampoline_bytes(&__ap_trampoline_start, &ap_pml4_ptr, &pml4.to_le_bytes())?;

    // Get stack for this AP
    let stack = stack_for(index)?;
    crate::kinfo!("SMP: [{}] Stack: {:#x}", index, stack);
    
    // Prepare boot arguments
    let info = cpu_info(index);
    AP_BOOT_ARGS[index] = ApBootArgs {
        cpu_index: index as u32,
        apic_id: info.apic_id,
    };
    let arg_ptr = (&AP_BOOT_ARGS[index] as *const ApBootArgs) as u64;
    crate::kinfo!("SMP: [{}] Boot args at: {:#x}", index, arg_ptr);
    
    // Calculate entry point
    let entry_raw = ap_entry as usize as u64;
    let reloc_offset = get_kernel_relocation_offset();
    
    let entry = if entry_raw > 0x1000000 {
        crate::kinfo!(
            "SMP: [{}] Entry appears already relocated, using as-is: {:#x}",
            index, entry_raw
        );
        entry_raw
    } else if let Some(offset) = reloc_offset {
        if offset != 0 {
            let relocated = (entry_raw as i64 + offset) as u64;
            crate::kinfo!(
                "SMP: [{}] Entry point: {:#x} + offset {:#x} = {:#x}",
                index, entry_raw, offset, relocated
            );
            relocated
        } else {
            crate::kinfo!("SMP: [{}] Entry point: {:#x} (no relocation)", index, entry_raw);
            entry_raw
        }
    } else {
        crate::kinfo!("SMP: [{}] Entry point: {:#x} (no offset info)", index, entry_raw);
        entry_raw
    };
    
    // Write per-CPU trampoline data
    let per_cpu_data = PerCpuTrampolineData {
        stack_ptr: stack,
        entry_ptr: entry,
        arg_ptr,
    };
    write_per_cpu_data(index, &per_cpu_data)?;
    
    // Set APIC ID to CPU index mapping
    set_apic_to_index_mapping(info.apic_id, index as u8)?;

    Ok(())
}

/// Get the stack top address for an AP
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
    let aligned_top = stack_top & !0xF; // Align down to 16 bytes

    // NOTE: Static variable addresses don't need relocation because:
    // 1. The kernel code accesses them using link-time addresses
    // 2. Low memory (link-time addresses) is identity-mapped in page tables
    // 3. AP cores use the same page tables, so they can access these addresses
    crate::kinfo!("SMP: [{}] Stack top: {:#x}", index, aligned_top);

    Ok(aligned_top as u64)
}

/// Busy wait for a number of iterations
fn busy_wait(mut iterations: u64) {
    while iterations > 0 {
        core::hint::spin_loop();
        iterations -= 1;
    }
}

/// Wait for an AP to come online
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

// ============================================================================
// AP Entry Points
// ============================================================================

/// Naked entry point for AP cores (called from trampoline)
#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn ap_entry(arg: *const ApBootArgs) -> ! {
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

/// Inner entry function for AP cores (called from ap_entry)
#[no_mangle]
extern "C" fn ap_entry_inner(arg: *const ApBootArgs) -> ! {
    // NOTE: IDT is already loaded by trampoline, so exceptions won't cause triple fault
    // Interrupts are still disabled at this point

    unsafe {
        // Debug: Signal entry and check RSP alignment
        serial_debug_byte(b'E'); // Entry

        // Check RSP alignment using safety abstraction
        let rsp = crate::safety::read_rsp();
        // Output RSP low nibble to check alignment (should be 8 for correct ABI)
        serial_debug_hex(rsp & 0xF, 1);

        // Validate argument pointer
        if arg.is_null() {
            serial_debug_byte(b'N'); // Null arg
            loop {
                cpu_hlt();
            }
        }
        serial_debug_byte(b'1'); // Arg not null

        let args = *arg;
        serial_debug_byte(b'2'); // Args copied

        let idx = args.cpu_index as usize;
        serial_debug_byte(b'3'); // Index extracted

        if idx >= MAX_CPUS {
            serial_debug_byte(b'X'); // Index too large
            loop {
                cpu_hlt();
            }
        }
        serial_debug_byte(b'4'); // Index valid

        // Signal arrival for debugging
        AP_ARRIVED[idx].store(0xDEADBEEF, Ordering::SeqCst);
        serial_debug_byte(b'5'); // Arrival flag set

        // Step 1: Configure GS base with this CPU's dedicated GS data
        // NOTE: Static variable addresses don't need relocation - they use link-time
        // addresses which are identity-mapped in the page tables
        let gs_data_addr = &raw const AP_GS_DATA[idx] as *const _ as u64;
        serial_debug_byte(b'6'); // GS addr calculated

        Msr::new(0xc0000101).write(gs_data_addr);
        serial_debug_byte(b'7'); // GS written

        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        serial_debug_byte(b'8'); // Fence done

        // Step 2: Initialize GDT (required for proper segmentation)
        serial_debug_byte(b'9'); // About to call gdt::init_ap

        // Initialize GDT and TSS for this AP core
        gdt::init_ap(idx);

        // Use unique markers that won't appear in other logs
        crate::safety::serial_debug_str("AP_GDT_OK\n");
        core::sync::atomic::compiler_fence(Ordering::SeqCst);

        // Now we can log safely
        crate::kinfo!("SMP: AP core {} online (APIC {:#x})", idx, args.apic_id);

        // Step 3: Initialize per-CPU data
        CPU_DATA[idx]
            .as_mut_ptr()
            .write(CpuData::new(idx as u8, args.apic_id));
        core::sync::atomic::compiler_fence(Ordering::Release);

        // Step 4: Mark CPU as online
        // CRITICAL: Full memory fence before reading any shared state
        // AP cores may have stale cache lines from before BSP wrote to memory
        core::sync::atomic::fence(Ordering::SeqCst);

        // Read CPU_TOTAL and debug output it
        let total = CPU_TOTAL.load(Ordering::SeqCst);

        // Debug: Output CPU_TOTAL value and address
        serial_debug_byte(b'T');
        let t_digit = if total < 10 { b'0' + total as u8 } else { b'?' };
        serial_debug_byte(t_digit);
        serial_debug_byte(b'@');

        // Output CPU_TOTAL address (link-time address used by AP)
        let cpu_total_addr = &CPU_TOTAL as *const _ as u64;
        serial_debug_hex(cpu_total_addr, 8);

        // For comparison, use relocated address (where BSP actually wrote)
        // The relocation offset moves data from link address to load address
        serial_debug_byte(b'|');
        let reloc_offset = get_kernel_relocation_offset().unwrap_or(0);

        // Debug: output the relocation offset value
        serial_debug_byte(b'[');
        serial_debug_hex(reloc_offset as u64, 8);
        serial_debug_byte(b']');

        // BSP uses addresses that are reloc_offset higher than link addresses
        // So to read what BSP wrote, we need: link_addr + reloc_offset
        let bsp_addr = cpu_total_addr.wrapping_add(reloc_offset as u64);
        serial_debug_hex(bsp_addr, 8);
        serial_debug_byte(b'=');
        let bsp_value = core::ptr::read_volatile(bsp_addr as *const usize);
        let bsp_digit = if bsp_value < 10 {
            b'0' + bsp_value as u8
        } else {
            b'?'
        };
        serial_debug_byte(bsp_digit);
        serial_debug_byte(b'/');

        // Use the BSP's view of CPU_TOTAL for the comparison
        let actual_total = bsp_value;
        if idx < actual_total {
            cpu_info(idx)
                .status
                .store(CpuStatus::Online as u8, Ordering::Release);
            crate::safety::serial_debug_str("AP_ONLINE\n");
        } else {
            crate::kerror!("SMP: CPU index {} exceeds total count", idx);
            loop {
                cpu_hlt();
            }
        }

        // Step 5: Enable interrupts
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        x86_64::instructions::interrupts::enable();

        crate::safety::serial_debug_str("AP_IDLE_LOOP\n");
        crate::kinfo!("SMP: Core {} entering idle loop", idx);

        // Enter idle loop - scheduler will take over
        loop {
            cpu_hlt();
        }
    }
}

/// Test IPI mechanism before attempting AP startup
#[allow(dead_code)]
pub unsafe fn test_ipi_mechanism() {
    crate::kinfo!("SMP: [IPI Test] Starting IPI self-test on BSP...");

    // Read LAPIC error status before test
    let error_before = lapic::read_error();
    crate::kinfo!(
        "SMP: [IPI Test] LAPIC error status before: {:#x}",
        error_before
    );

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
    crate::kinfo!(
        "SMP: [IPI Test] LAPIC error status after: {:#x}",
        error_after
    );

    crate::kinfo!("SMP: [IPI Test] Completed");
}

// ============================================================================
// Parallel AP Startup Support
// ============================================================================

/// Prepare all AP cores for parallel startup
/// This writes per-CPU data for all APs so they can be started simultaneously
pub unsafe fn prepare_all_aps(count: usize) -> Result<usize, &'static str> {
    extern "C" {
        static __ap_trampoline_start: u8;
        static ap_pml4_ptr: u8;
    }

    // First, write the shared PML4 pointer (same for all APs)
    let pml4 = paging::current_pml4_phys();
    crate::kinfo!("SMP: [Parallel] Setting shared PML4: {:#x}", pml4);
    write_trampoline_bytes(&__ap_trampoline_start, &ap_pml4_ptr, &pml4.to_le_bytes())?;

    // Calculate entry point (same for all APs)
    let entry_raw = ap_entry as usize as u64;
    let reloc_offset = get_kernel_relocation_offset();
    
    let entry = if entry_raw > 0x1000000 {
        entry_raw
    } else if let Some(offset) = reloc_offset {
        if offset != 0 {
            (entry_raw as i64 + offset) as u64
        } else {
            entry_raw
        }
    } else {
        entry_raw
    };
    crate::kinfo!("SMP: [Parallel] Common entry point: {:#x}", entry);

    let mut prepared = 0;

    // Prepare per-CPU data for each AP (skip BSP at index 0)
    for index in 1..count {
        let info = cpu_info(index);
        
        // Get stack for this AP
        let stack = match stack_for(index) {
            Ok(s) => s,
            Err(e) => {
                crate::kwarn!("SMP: [Parallel] Skip AP {} - no stack: {}", index, e);
                continue;
            }
        };

        // Prepare boot arguments
        AP_BOOT_ARGS[index] = ApBootArgs {
            cpu_index: index as u32,
            apic_id: info.apic_id,
        };
        let arg_ptr = (&AP_BOOT_ARGS[index] as *const ApBootArgs) as u64;

        // Write per-CPU trampoline data
        let per_cpu_data = PerCpuTrampolineData {
            stack_ptr: stack,
            entry_ptr: entry,
            arg_ptr,
        };
        
        if let Err(e) = write_per_cpu_data(index, &per_cpu_data) {
            crate::kwarn!("SMP: [Parallel] Failed to write per-CPU data for AP {}: {}", index, e);
            continue;
        }

        // Set APIC ID to CPU index mapping
        if let Err(e) = set_apic_to_index_mapping(info.apic_id, index as u8) {
            crate::kwarn!("SMP: [Parallel] Failed to set APIC mapping for AP {}: {}", index, e);
            continue;
        }

        // Mark as ready to boot
        info.status.store(CpuStatus::Booting as u8, Ordering::Release);
        prepared += 1;
    }

    // Memory barrier to ensure all writes are visible
    core::sync::atomic::fence(Ordering::SeqCst);

    crate::kinfo!("SMP: [Parallel] Prepared {} APs for startup", prepared);
    Ok(prepared)
}

/// Send INIT IPI to all AP cores simultaneously
pub unsafe fn send_init_to_all_aps(count: usize) {
    crate::kinfo!("SMP: [Parallel] Sending INIT IPI to all APs...");
    
    for index in 1..count {
        let info = cpu_info(index);
        let status = CpuStatus::from_atomic(info.status.load(Ordering::Acquire));
        
        // Only send to cores that are in Booting state (prepared)
        if status == CpuStatus::Booting {
            crate::kinfo!("SMP: [Parallel] INIT IPI -> AP {} (APIC {:#x})", index, info.apic_id);
            lapic::send_init_ipi(info.apic_id);
        }
    }
    
    // Wait 10ms after INIT (per Intel spec)
    busy_wait(100_000);
}

/// Send STARTUP IPI to all AP cores simultaneously
pub unsafe fn send_startup_to_all_aps(count: usize) {
    crate::kinfo!("SMP: [Parallel] Sending STARTUP IPI #1 to all APs...");
    
    for index in 1..count {
        let info = cpu_info(index);
        let status = CpuStatus::from_atomic(info.status.load(Ordering::Acquire));
        
        if status == CpuStatus::Booting {
            lapic::send_startup_ipi(info.apic_id, TRAMPOLINE_VECTOR);
        }
    }
    
    // Wait 200us between SIPIs
    busy_wait(20_000);
    
    crate::kinfo!("SMP: [Parallel] Sending STARTUP IPI #2 to all APs...");
    
    for index in 1..count {
        let info = cpu_info(index);
        let status = CpuStatus::from_atomic(info.status.load(Ordering::Acquire));
        
        if status == CpuStatus::Booting {
            lapic::send_startup_ipi(info.apic_id, TRAMPOLINE_VECTOR);
        }
    }
}

/// Start all AP cores in parallel
/// This is the main entry point for parallel AP initialization
pub unsafe fn start_all_aps_parallel(count: usize) -> Result<usize, &'static str> {
    crate::kinfo!("SMP: Starting parallel AP initialization for {} cores", count);
    
    // Phase 1: Prepare all APs with their per-CPU data
    let prepared = prepare_all_aps(count)?;
    if prepared == 0 {
        return Err("No APs could be prepared for startup");
    }
    
    // Phase 2: Send INIT IPI to all APs
    send_init_to_all_aps(count);
    
    // Phase 3: Send STARTUP IPIs to all APs
    send_startup_to_all_aps(count);
    
    // Extra delay to let APs boot
    busy_wait(50_000);
    
    // Phase 4: Wait for all APs to come online
    crate::kinfo!("SMP: [Parallel] Waiting for APs to come online...");
    
    let mut online = 0;
    let max_wait_iterations = 10;
    
    for _ in 0..max_wait_iterations {
        online = 0;
        
        for index in 1..count {
            let info = cpu_info(index);
            let status = CpuStatus::from_atomic(info.status.load(Ordering::Acquire));
            if status == CpuStatus::Online {
                online += 1;
            }
        }
        
        if online == prepared {
            break;
        }
        
        // Wait and try again
        busy_wait(STARTUP_WAIT_LOOPS / 10);
    }
    
    // Update online count
    ONLINE_CPUS.fetch_add(online, Ordering::SeqCst);
    
    // Log results
    crate::kinfo!(
        "SMP: [Parallel] {} / {} APs came online (prepared: {})",
        online, count - 1, prepared
    );
    
    // Mark any non-online APs as failed
    for index in 1..count {
        let info = cpu_info(index);
        let status = CpuStatus::from_atomic(info.status.load(Ordering::Acquire));
        if status == CpuStatus::Booting {
            crate::kwarn!("SMP: [Parallel] AP {} failed to come online", index);
            info.status.store(CpuStatus::Offline as u8, Ordering::Release);
        }
    }
    
    Ok(online)
}
