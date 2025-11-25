//! AP (Application Processor) Startup
//!
//! This module handles the startup sequence for secondary CPU cores,
//! including sending INIT/STARTUP IPIs and waiting for cores to come online.

use core::ptr;
use core::sync::atomic::Ordering;

use x86_64::instructions::hlt as cpu_hlt;
use x86_64::registers::model_specific::Msr;

use crate::safety::{serial_debug_byte, serial_debug_hex};
use crate::{gdt, lapic, paging};

use super::state::{CPU_TOTAL, ENABLE_AP_STARTUP, ONLINE_CPUS};
use super::trampoline::{get_kernel_relocation_offset, write_trampoline_bytes};
use super::types::{
    cpu_info, AlignedApStack, ApBootArgs, CpuData, CpuStatus, PerCpuGsData,
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

/// Prepare AP launch parameters in the trampoline
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
    let pml4_offset =
        (&ap_pml4_ptr as *const _ as usize) - (&__ap_trampoline_start as *const _ as usize);
    let stack_offset =
        (&ap_stack_ptr as *const _ as usize) - (&__ap_trampoline_start as *const _ as usize);
    let written_pml4 =
        core::ptr::read_volatile((TRAMPOLINE_BASE as usize + pml4_offset) as *const u64);
    let written_stack =
        core::ptr::read_volatile((TRAMPOLINE_BASE as usize + stack_offset) as *const u64);
    crate::kinfo!(
        "SMP: [{}] Trampoline has PML4={:#x}, stack={:#x}",
        index,
        written_pml4,
        written_stack
    );

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
    crate::kinfo!(
        "SMP: [{}] Raw ap_entry pointer value: {:#x}",
        index,
        entry_raw
    );

    // Get relocation offset - try multiple sources for robustness
    let reloc_offset = get_kernel_relocation_offset();

    // Check if this looks like a link-time or run-time address
    // Link-time address would be around 0x12c100
    // Run-time address would be around 0x2382100
    let entry = if entry_raw > 0x1000000 {
        // Looks like already relocated (high address)
        crate::kinfo!(
            "SMP: [{}] Entry appears already relocated, using as-is: {:#x}",
            index,
            entry_raw
        );
        entry_raw
    } else if let Some(offset) = reloc_offset {
        if offset != 0 {
            let relocated = (entry_raw as i64 + offset) as u64;
            crate::kinfo!(
                "SMP: [{}] Entry point: {:#x} + offset {:#x} = {:#x}",
                index,
                entry_raw,
                offset,
                relocated
            );
            relocated
        } else {
            crate::kinfo!(
                "SMP: [{}] Entry point: {:#x} (no relocation)",
                index,
                entry_raw
            );
            entry_raw
        }
    } else {
        crate::kinfo!(
            "SMP: [{}] Entry point: {:#x} (no offset info)",
            index,
            entry_raw
        );
        entry_raw
    };

    // Debug: log the addresses and offset used for writing
    let entry_offset_before =
        (&ap_entry_ptr as *const _ as usize) - (&__ap_trampoline_start as *const _ as usize);
    crate::kinfo!(
        "SMP: [{}] ap_entry_ptr={:#x}, trampoline_start={:#x}, offset={:#x}",
        index,
        &ap_entry_ptr as *const _ as usize,
        &__ap_trampoline_start as *const _ as usize,
        entry_offset_before
    );
    crate::kinfo!(
        "SMP: [{}] Writing entry {:#x} to trampoline offset {:#x} (dest addr {:#x})",
        index,
        entry,
        entry_offset_before,
        TRAMPOLINE_BASE as usize + entry_offset_before
    );

    // Debug: Verify code exists at the entry address
    let entry_code = core::ptr::read_volatile(entry as *const [u8; 16]);
    crate::kinfo!(
        "SMP: [{}] Code at entry {:#x}: {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
        index,
        entry,
        entry_code[0],
        entry_code[1],
        entry_code[2],
        entry_code[3],
        entry_code[4],
        entry_code[5],
        entry_code[6],
        entry_code[7]
    );
    // Expected: 66 ba f8 03 b0 48 ee 57 (mov $0x3f8,%dx; mov $0x48,%al; out; push %rdi)

    write_trampoline_bytes(&__ap_trampoline_start, &ap_entry_ptr, &entry.to_le_bytes())?;

    // Verify the write
    let entry_offset =
        (&ap_entry_ptr as *const _ as usize) - (&__ap_trampoline_start as *const _ as usize);
    let written_entry_ptr = (TRAMPOLINE_BASE as usize + entry_offset) as *const u64;
    let written_entry = core::ptr::read_volatile(written_entry_ptr);
    crate::kinfo!(
        "SMP: [{}] Verified entry in trampoline at {:#x}: {:#x}",
        index,
        written_entry_ptr as usize,
        written_entry
    );

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
