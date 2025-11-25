//! IDT Initialization and Configuration
//!
//! This module handles the Interrupt Descriptor Table (IDT) setup,
//! including exception handlers, hardware interrupt handlers, and
//! syscall gates.

use core::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use lazy_static::lazy_static;
use x86_64::instructions::port::Port;
use x86_64::registers::model_specific::Msr;
use x86_64::structures::idt::InterruptDescriptorTable;
use x86_64::PrivilegeLevel;

use crate::interrupts::exceptions::*;
use crate::interrupts::gs_context::GS_SLOT_KERNEL_RSP;
use crate::interrupts::gs_context::GS_SLOT_KERNEL_STACK_GUARD;
use crate::interrupts::gs_context::GS_SLOT_KERNEL_STACK_SNAPSHOT;
use crate::interrupts::gs_context::GS_SLOT_USER_CS;
use crate::interrupts::gs_context::GS_SLOT_USER_DS;
use crate::interrupts::gs_context::GS_SLOT_USER_SS;
use crate::interrupts::handlers::*;
use crate::interrupts::ipi::*;
use crate::interrupts::syscall_asm::{
    ring3_switch_handler, syscall_instruction_handler, syscall_interrupt_handler,
};

/// Toggle IA32_* MSR configuration for `syscall` fast path.
/// With dynamic linking we now encounter Glibc-generated `syscall` instructions
/// immediately during interpreter startup, so keep this enabled by default and
/// fall back to the legacy `int 0x81` gateway only if MSR programming fails.
const ENABLE_SYSCALL_MSRS: bool = true;

/// SMP/Multi-core IDT Strategy Documentation
///
/// CURRENT IMPLEMENTATION (Single-core):
/// - This IDT is shared across all cores (if SMP is enabled in future)
/// - Initialization happens on the BSP (Bootstrap Processor) only
/// - APs (Application Processors) will load the same IDT via IDT.load()
/// - This is safe for read-only operations but limits per-core customization
///
/// ASSUMPTIONS:
/// 1. IDT initialization completes on BSP before any AP starts
/// 2. All cores share the same interrupt handlers (no per-core handlers yet)
/// 3. IST (Interrupt Stack Table) entries point to BSP stacks (NOT per-core)
/// 4. No concurrent modifications to IDT after initialization
///
/// FUTURE SMP IMPROVEMENTS (TODO):
/// - Implement per-core IDT tables for true isolation
/// - Per-core IST stacks to avoid stack corruption in multi-core scenarios
/// - Per-core interrupt affinity and load balancing
/// - Spinlock protection for any runtime IDT modifications
/// - Proper APIC initialization and IPI handling
///
/// TEMPORARY PROTECTION:
/// - IDT_INITIALIZED flag prevents re-initialization
/// - lazy_static ensures single initialization even with concurrent access
/// - interrupts::disable() during init prevents race conditions
///
/// See: https://wiki.osdev.org/SMP for SMP initialization sequence
/// See: https://wiki.osdev.org/APIC for advanced interrupt routing

/// Flag to track if IDT has been initialized (prevents re-initialization)
static IDT_INITIALIZED: AtomicBool = AtomicBool::new(false);

lazy_static! {
    /// Global IDT instance - using lazy_static to avoid stack overflow
    /// InterruptDescriptorTable is ~4KB and would overflow the stack if created inline
    ///
    /// IMPORTANT: Currently shared across all cores in SMP configurations.
    /// This is safe for our current single-core focus but will need per-core
    /// IDTs for true SMP support with per-core interrupt handling.
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        let error_code_ist = crate::gdt::ERROR_CODE_IST_INDEX as u16;

        unsafe {
            // Set up interrupt handlers
            idt.breakpoint.set_handler_fn(breakpoint_handler);
            idt.page_fault
                .set_handler_fn(page_fault_handler)
                .set_stack_index(error_code_ist);
            idt.general_protection_fault
                .set_handler_fn(general_protection_fault_handler)
                .set_stack_index(error_code_ist);
            idt.divide_error.set_handler_fn(divide_error_handler);
            // Use a dedicated IST entry for double fault to ensure the CPU
            // switches to a known-good stack when a double fault occurs. This
            // reduces the chance of a triple fault caused by stack corruption.
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX as u16);
            idt.segment_not_present
                .set_handler_fn(segment_not_present_handler)
                .set_stack_index(error_code_ist);
            idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
            idt.invalid_tss
                .set_handler_fn(segment_not_present_handler)
                .set_stack_index(error_code_ist); // Reuse handler
            idt.stack_segment_fault
                .set_handler_fn(segment_not_present_handler)
                .set_stack_index(error_code_ist); // Reuse handler

            // Set up hardware interrupts
            idt[PIC_1_OFFSET].set_handler_fn(timer_interrupt_handler);
            idt[PIC_1_OFFSET + 1].set_handler_fn(keyboard_interrupt_handler);
            idt[PIC_1_OFFSET + 2].set_handler_fn(spurious_irq2_handler);
            idt[PIC_1_OFFSET + 3].set_handler_fn(spurious_irq3_handler);
            idt[PIC_1_OFFSET + 4].set_handler_fn(spurious_irq4_handler);
            idt[PIC_1_OFFSET + 5].set_handler_fn(spurious_irq5_handler);
            idt[PIC_1_OFFSET + 6].set_handler_fn(spurious_irq6_handler);
            idt[PIC_1_OFFSET + 7].set_handler_fn(spurious_irq7_handler);

            idt[PIC_2_OFFSET].set_handler_fn(spurious_irq8_handler);
            idt[PIC_2_OFFSET + 1].set_handler_fn(spurious_irq9_handler);
            idt[PIC_2_OFFSET + 2].set_handler_fn(spurious_irq10_handler);
            idt[PIC_2_OFFSET + 3].set_handler_fn(spurious_irq11_handler);
            idt[PIC_2_OFFSET + 4].set_handler_fn(spurious_irq12_handler);
            idt[PIC_2_OFFSET + 5].set_handler_fn(spurious_irq13_handler);
            idt[PIC_2_OFFSET + 6].set_handler_fn(spurious_irq14_handler);
            idt[PIC_2_OFFSET + 7].set_handler_fn(spurious_irq15_handler);

            // Set up syscall interrupt handler at 0x81 (callable from Ring 3)
            idt[0x81]
                .set_handler_addr(x86_64::VirtAddr::new_truncate(
                    syscall_interrupt_handler as u64,
                ))
                .set_privilege_level(PrivilegeLevel::Ring3);

            // Set up ring3 switch handler at 0x80 (also callable from Ring 3)
            idt[0x80]
                .set_handler_addr(x86_64::VirtAddr::new_truncate(ring3_switch_handler as u64))
                .set_privilege_level(PrivilegeLevel::Ring3);

            // Set up IPI handlers for SMP (vectors 0xF0-0xF3)
            idt[0xF0].set_handler_fn(ipi_reschedule_handler);
            idt[0xF1].set_handler_fn(ipi_tlb_flush_handler);
            idt[0xF2].set_handler_fn(ipi_call_function_handler);
            idt[0xF3].set_handler_fn(ipi_halt_handler);
        }

        idt
    };
}

/// Initialize IDT with interrupt handlers
///
/// CRITICAL SAFETY NOTES:
/// 1. This function MUST be called only once per system boot (on BSP)
/// 2. All interrupts are disabled during initialization to prevent races
/// 3. IDT is loaded before unmasking any hardware interrupts
/// 4. Uses atomic flag to prevent accidental re-initialization
///
/// SMP CONSIDERATIONS:
/// - Currently designed for single-core/BSP initialization only
/// - APs should NOT call this function (they should only call IDT.load())
/// - Future: Implement init_interrupts_ap() for per-core setup
///
/// CALL SEQUENCE:
/// 1. Disable interrupts globally
/// 2. Check if already initialized (prevent double-init)
/// 3. Mask all PIC interrupts
/// 4. Initialize PIC hardware
/// 5. Load IDT (triggers lazy_static initialization)
/// 6. Apply final interrupt masks
/// 7. Mark as initialized
pub fn init_interrupts() {
    // Ensure interrupts are disabled during initialization
    // This is critical to prevent race conditions and ensure atomic setup
    x86_64::instructions::interrupts::disable();

    // Check if already initialized (protection against double-init)
    if IDT_INITIALIZED.load(AtomicOrdering::SeqCst) {
        crate::kwarn!("init_interrupts: Already initialized, skipping");
        return;
    }

    // Safe to log now that interrupts are disabled
    crate::kinfo!("init_interrupts: Starting IDT initialization (BSP)");

    // Mask all interrupts BEFORE initializing PICs to prevent spurious interrupts during setup
    unsafe {
        let mut port = Port::<u8>::new(0x21); // Master PIC IMR
        port.write(0xFF);
        let mut port = Port::<u8>::new(0xA1); // Slave PIC IMR
        port.write(0xFF);
    }
    crate::kinfo!("init_interrupts: interrupts masked");

    // Initialize PICs AFTER masking interrupts
    unsafe {
        PICS.lock().initialize();
    }
    crate::kinfo!("init_interrupts: PICs initialized");

    // Load IDT before applying PIC masks to ensure handlers are in place
    // Access to IDT via lazy_static will initialize it on first access
    crate::kinfo!("init_interrupts: loading IDT");

    // Ensure IDT structure is fully written before loading
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    IDT.load();
    // Ensure load completes before continuing
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    crate::kinfo!("init_interrupts: IDT loaded successfully");

    // Now set final PIC masks - unmask only keyboard IRQ
    unsafe {
        let mut master_port = Port::<u8>::new(0x21);
        master_port.write(0xFD); // Unmask only keyboard IRQ (IRQ1)
        let mut slave_port = Port::<u8>::new(0xA1);
        slave_port.write(0xFF); // Keep all slave IRQs masked
    }
    crate::kinfo!("init_interrupts: PIC masks applied (keyboard unmasked)");

    if ENABLE_SYSCALL_MSRS {
        crate::kinfo!("init_interrupts: enabling SYSCALL MSR fast path");
        setup_syscall();
        crate::kinfo!("init_interrupts: setup_syscall completed");
    } else {
        crate::kinfo!("init_interrupts: skipping SYSCALL MSR setup (using int 0x81 gateway)");
    }

    // Mark IDT as initialized to prevent re-initialization
    IDT_INITIALIZED.store(true, AtomicOrdering::SeqCst);

    // Final memory barrier to ensure all initialization is visible to other cores
    core::sync::atomic::fence(AtomicOrdering::SeqCst);

    crate::kinfo!("init_interrupts: Initialization complete and marked ready");
}

/// Check if IDT has been initialized
///
/// Useful for AP (Application Processor) cores in SMP configurations
/// to verify that BSP has completed IDT setup before loading it.
#[allow(dead_code)]
pub fn is_idt_initialized() -> bool {
    IDT_INITIALIZED.load(AtomicOrdering::SeqCst)
}

/// Load IDT on an AP (Application Processor) core
///
/// This function should be called by AP cores after BSP has initialized
/// the shared IDT. It only loads the IDT without re-initializing PICs.
///
/// REQUIREMENTS:
/// - BSP must have called init_interrupts() first
/// - Interrupts should be disabled before calling
/// - PICs are already initialized by BSP
///
/// TODO: In future SMP implementation, this should:
/// - Load per-core IDT instead of shared IDT
/// - Set up per-core APIC instead of PIC
/// - Configure per-core IST stacks
#[allow(dead_code)]
pub fn init_interrupts_ap() {
    // Ensure interrupts are disabled
    x86_64::instructions::interrupts::disable();

    // Verify that BSP has initialized the IDT
    if !is_idt_initialized() {
        crate::kpanic!("AP attempted to load IDT before BSP initialization");
    }

    crate::kinfo!("init_interrupts_ap: Loading IDT on AP core");

    // Load the shared IDT on this core
    core::sync::atomic::compiler_fence(AtomicOrdering::SeqCst);
    IDT.load();
    core::sync::atomic::compiler_fence(AtomicOrdering::SeqCst);

    crate::kinfo!("init_interrupts_ap: IDT loaded on AP core");
}

/// Set up SYSCALL/SYSRET MSRs for fast system call handling
pub fn setup_syscall() {
    let handler_addr = syscall_instruction_handler as u64;

    crate::kinfo!("Setting syscall handler to {:#x}", handler_addr);

    if !cpu_supports_syscall() {
        crate::kwarn!("CPU lacks SYSCALL/SYSRET support; skipping setup");
        return;
    }

    if !is_canonical_address(handler_addr) {
        crate::kerror!(
            "SYSCALL handler address {:#x} is non-canonical; keeping int 0x81 path",
            handler_addr
        );
        return;
    }

    let selectors = unsafe { crate::gdt::get_selectors() };
    let kernel_cs = selectors.code_selector.0 as u16;
    let kernel_ss = selectors.data_selector.0 as u16;
    let user_cs = selectors.user_code_selector.0 as u16;
    let user_ss = selectors.user_data_selector.0 as u16;

    if kernel_ss != kernel_cs + 8 {
        crate::kwarn!(
            "Kernel SS ({:#x}) does not equal kernel CS+8 ({:#x}); STAR will assume the latter",
            kernel_ss,
            kernel_cs + 8
        );
    }

    // For SYSRET in 64-bit mode, the CPU calculates:
    //   CS = STAR[63:48] + 16
    //   SS = STAR[63:48] + 8
    // So SS should be CS - 8, not CS + 8!
    if user_ss != user_cs - 8 {
        crate::kwarn!(
            "User SS ({:#x}) does not equal user CS-8 ({:#x}); check GDT layout for SYSRET",
            user_ss,
            user_cs - 8
        );
    }

    let kernel_cs_star = (kernel_cs & !0x7) as u64; // ensure RPL=0
                                                    // For SYSRET in 64-bit mode:
                                                    // CS ← STAR[63:48] + 16
                                                    // SS ← STAR[63:48] + 8
                                                    // So STAR[63:48] should be set to kernel_data (0x10), which gives:
                                                    // CS = 0x10 + 16 = 0x20 (user code, entry 4)
                                                    // SS = 0x10 + 8 = 0x18 (user data, entry 3)
    let user_cs_star = (kernel_ss & !0x7) as u64; // use kernel_ss(0x10) as base for SYSRET

    let star_value = (kernel_cs_star << 32) | (user_cs_star << 48);
    crate::kdebug!(
        "MSR: STAR composed from selectors kernel_cs={:#x}, kernel_ss={:#x} -> {:#x}",
        kernel_cs,
        kernel_ss,
        star_value
    );

    unsafe {
        // Get GS_DATA address without creating a reference that might corrupt nearby statics
        let gs_data_addr = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;

        // Initialize GS data for syscall - write directly to the address
        let gs_data_ptr = gs_data_addr as *mut u64;
        gs_data_ptr
            .add(GS_SLOT_KERNEL_RSP)
            .write(crate::gdt::get_kernel_stack_top(0)); // BSP = CPU 0

        let selectors = crate::gdt::get_selectors();
        let user_cs = (selectors.user_code_selector.0 | 0x3) as u64;
        let user_ss = (selectors.user_data_selector.0 | 0x3) as u64;
        let user_ds = user_ss;

        gs_data_ptr.add(GS_SLOT_USER_CS).write(user_cs);
        gs_data_ptr.add(GS_SLOT_USER_SS).write(user_ss);
        gs_data_ptr.add(GS_SLOT_USER_DS).write(user_ds);
        gs_data_ptr.add(GS_SLOT_KERNEL_STACK_GUARD).write(0);
        gs_data_ptr.add(GS_SLOT_KERNEL_STACK_SNAPSHOT).write(0);

        crate::kdebug!(
            "setup_syscall: initramfs available? {}",
            crate::initramfs::get().is_some()
        );

        crate::kdebug!("MSR: about to write KERNEL_GS_BASE");
        Msr::new(0xc0000102).write(gs_data_addr); // Kernel GS base used by swapgs
        crate::kdebug!("MSR: KERNEL_GS_BASE written");

        // Set GS base to GS_DATA address
        // GS base is already set in kernel_main before interrupt initialization
        // let gs_base = gs_data_addr;
        // Msr::new(0xc0000101).write(gs_base); // GS base

        // Use kernel logging for MSR write tracing so it follows the
        // kernel logging convention (serial + optional VGA). logger
        // will skip VGA until it's ready, so this is safe during early boot.
        crate::kdebug!("MSR: about to enable EFER.SCE");
        let mut efer_msr = Msr::new(0xc0000080);
        let mut efer_val = efer_msr.read();
        let had_sce = (efer_val & (1 << 0)) != 0;
        efer_val |= 1 << 0; // IA32_EFER.SCE
        efer_msr.write(efer_val);
        crate::kdebug!(
            "MSR: EFER updated (prev_sce={}, new_val={:#x})",
            had_sce,
            efer_val
        );

        crate::kdebug!("MSR: about to write STAR");
        Msr::new(0xc0000081).write(star_value);
        crate::kdebug!("MSR: STAR written");

        // Point LSTAR to the Rust/assembly syscall handler which prepares
        // arguments (moves rax->rdi, etc.) and uses sysretq.
        crate::kdebug!("MSR: about to write LSTAR");
        Msr::new(0xc0000082).write(handler_addr); // LSTAR
        let lstar_val = Msr::new(0xc0000082).read();
        crate::kdebug!(
            "MSR: LSTAR written (handler={:#x}, readback={:#x})",
            handler_addr,
            lstar_val
        );

        crate::kdebug!("MSR: about to write FMASK");
        Msr::new(0xc0000084).write(0x200); // FMASK
        crate::kdebug!("MSR: FMASK written");
    }
}

fn cpu_supports_syscall() -> bool {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let res = core::arch::x86_64::__cpuid(0x8000_0001);
        (res.edx & (1 << 11)) != 0
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

fn is_canonical_address(addr: u64) -> bool {
    let sign = (addr >> 47) & 1;
    let upper = addr >> 48;
    if sign == 0 {
        upper == 0
    } else {
        upper == 0xFFFF
    }
}
