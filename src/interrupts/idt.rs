//! IDT Initialization and Configuration
//!
//! This module handles the Interrupt Descriptor Table (IDT) setup,
//! including exception handlers, hardware interrupt handlers, and
//! syscall gates.
//!
//! # Per-CPU IDT Architecture
//!
//! Each CPU (BSP and APs) has its own dedicated IDT to enable:
//! - Per-CPU IST stack configuration for isolation
//! - Independent interrupt handling without cross-core contention
//! - True SMP safety with no shared mutable state
//!
//! The BSP's IDT is initialized via `init_interrupts()`, while AP cores
//! initialize their own IDTs via `init_interrupts_ap(cpu_id)`.

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use lazy_static::lazy_static;
use x86_64::instructions::port::Port;
use x86_64::registers::model_specific::Msr;
use x86_64::structures::idt::InterruptDescriptorTable;
use x86_64::PrivilegeLevel;

/// Maximum number of CPUs supported (must match acpi::MAX_CPUS)
const MAX_CPUS: usize = crate::acpi::MAX_CPUS;

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
/// CURRENT IMPLEMENTATION (Per-CPU IDT):
/// - Each CPU (BSP and APs) has its own dedicated IDT
/// - BSP uses the lazy_static IDT (index 0 in per-CPU array)
/// - APs initialize their own IDT via init_interrupts_ap(cpu_id)
/// - Each IDT can have independent IST stack configurations
///
/// BENEFITS:
/// 1. True per-CPU isolation - no shared mutable IDT state
/// 2. Per-CPU IST stacks prevent stack corruption in multi-core scenarios
/// 3. Independent interrupt handling without cross-core cache contention
/// 4. Future: Per-CPU interrupt affinity and customization
///
/// SAFETY GUARANTEES:
/// - Per-CPU IDT initialized flags prevent re-initialization
/// - Each CPU's IDT is only modified by that CPU during init
/// - interrupts::disable() during init prevents race conditions
/// - Memory barriers ensure visibility across cores
///
/// See: https://wiki.osdev.org/SMP for SMP initialization sequence
/// See: https://wiki.osdev.org/APIC for advanced interrupt routing

/// Flag to track if BSP IDT has been initialized (prevents re-initialization)
static IDT_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Per-CPU IDT initialized flags
static PER_CPU_IDT_INITIALIZED: [AtomicBool; MAX_CPUS] = {
    const INIT: AtomicBool = AtomicBool::new(false);
    [INIT; MAX_CPUS]
};

/// Per-CPU IDT storage (CPU 0/BSP uses the lazy_static IDT, APs use this array)
/// Using MaybeUninit to avoid requiring Default/Copy for InterruptDescriptorTable
static mut PER_CPU_IDT: [MaybeUninit<InterruptDescriptorTable>; MAX_CPUS] =
    unsafe { MaybeUninit::uninit().assume_init() };

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
            // Use inline asm with RIP-relative addressing to get the correct runtime address
            // regardless of where the kernel is loaded
            let handler_addr: u64;
            unsafe {
                core::arch::asm!(
                    "lea {}, [rip + syscall_interrupt_handler]",
                    out(reg) handler_addr,
                    options(nostack, nomem)
                );
            }
            crate::kinfo!("syscall_interrupt_handler: using RIP-relative addr {:#x}", handler_addr);
            idt[0x81]
                .set_handler_addr(x86_64::VirtAddr::new_truncate(handler_addr))
                .set_privilege_level(PrivilegeLevel::Ring3);

            // Similarly fix ring3_switch_handler
            let ring3_addr: u64;
            unsafe {
                core::arch::asm!(
                    "lea {}, [rip + ring3_switch_handler]",
                    out(reg) ring3_addr,
                    options(nostack, nomem)
                );
            }
            idt[0x80]
                .set_handler_addr(x86_64::VirtAddr::new_truncate(ring3_addr))
                .set_privilege_level(PrivilegeLevel::Ring3);

            // Set up IPI handlers for SMP (vectors 0xF0-0xF3)
            idt[0xF0].set_handler_fn(ipi_reschedule_handler);
            idt[0xF1].set_handler_fn(ipi_tlb_flush_handler);
            idt[0xF2].set_handler_fn(ipi_call_function_handler);
            idt[0xF3].set_handler_fn(ipi_halt_handler);

            // Set up LAPIC timer handler (vector 0xEC = 236)
            // This is primarily for AP cores but also registered on BSP for consistency
            idt[LAPIC_TIMER_VECTOR].set_handler_fn(lapic_timer_handler);
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
    let idt_addr = &*IDT as *const InterruptDescriptorTable as u64;
    crate::kinfo!("init_interrupts: BSP IDT address = {:#x}", idt_addr);
    IDT.load();
    // Ensure load completes before continuing
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    crate::kinfo!("init_interrupts: IDT loaded successfully");

    // Debug: Print IDT[0x81] entry details
    {
        let idt_ptr = &*IDT as *const InterruptDescriptorTable as *const u8;
        // Each IDT entry is 16 bytes
        let entry_0x81 = unsafe { idt_ptr.add(0x81 * 16) as *const u64 };
        let low = unsafe { *entry_0x81 };
        let high = unsafe { *entry_0x81.add(1) };
        crate::kinfo!("IDT[0x81] raw: low={:#018x} high={:#018x}", low, high);
        // Parse the entry
        let handler_addr = (low & 0xFFFF) | ((low >> 48) << 16) | (high << 32);
        let cs_sel = ((low >> 16) & 0xFFFF) as u16;
        let options = ((low >> 32) & 0xFFFF) as u16;
        let dpl = (options >> 13) & 0x3;
        let present = (options >> 15) & 0x1;
        let gate_type = (options >> 8) & 0xF;
        crate::kinfo!(
            "IDT[0x81] parsed: handler={:#x} cs={:#x} dpl={} present={} gate_type={:#x}",
            handler_addr,
            cs_sel,
            dpl,
            present,
            gate_type
        );
    }

    // Now set final PIC masks - unmask timer (IRQ0) and keyboard (IRQ1)
    unsafe {
        let mut master_port = Port::<u8>::new(0x21);
        master_port.write(0xFC); // Unmask timer IRQ (IRQ0) and keyboard IRQ (IRQ1)
        let mut slave_port = Port::<u8>::new(0xA1);
        slave_port.write(0xFF); // Keep all slave IRQs masked
    }
    crate::kinfo!("init_interrupts: PIC masks applied (timer and keyboard unmasked)");

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

/// Initialize and load a per-CPU IDT on an AP (Application Processor) core
///
/// This function creates a dedicated IDT for the specified AP core, configuring
/// per-CPU IST stacks for exception handlers. Each AP gets its own IDT to enable:
/// - True isolation from other cores
/// - Per-CPU IST stacks to prevent stack corruption
/// - Independent interrupt handling
///
/// # Arguments
/// * `cpu_id` - The CPU index (1..MAX_CPUS for APs, 0 is BSP)
///
/// # Safety
/// Must be called with interrupts disabled. The cpu_id must be valid and
/// correspond to the currently executing CPU. The SMP initialization sequence
/// must ensure BSP's IDT is initialized before starting APs.
///
/// REQUIREMENTS:
/// - Interrupts must be disabled before calling
/// - cpu_id must be > 0 (APs only) and < MAX_CPUS
/// - SMP startup sequence must ensure proper ordering (BSP IDT before AP startup)
///
/// NOTE: Due to kernel relocation, we cannot reliably check IDT_INITIALIZED from AP
/// cores (they may see stale values). The SMP startup sequence guarantees ordering.
#[allow(dead_code)]
pub fn init_interrupts_ap(cpu_id: usize) {
    // Ensure interrupts are disabled
    x86_64::instructions::interrupts::disable();

    // Validate cpu_id
    if cpu_id == 0 {
        crate::kpanic!("init_interrupts_ap called for BSP (cpu_id=0), use init_interrupts instead");
    }
    if cpu_id >= MAX_CPUS {
        crate::kpanic!(
            "init_interrupts_ap: cpu_id {} exceeds MAX_CPUS {}",
            cpu_id,
            MAX_CPUS
        );
    }

    // NOTE: We skip checking IDT_INITIALIZED because AP cores may see stale values
    // due to kernel relocation (AP runs at link-time addresses, BSP at relocated).
    // The SMP startup sequence guarantees BSP's init_interrupts() completes before
    // any AP reaches this point (AP startup IPIs are sent after init_interrupts()).

    // Check if this AP's IDT is already initialized (using per-CPU flag at link address)
    // This check is valid because we're checking the per-CPU flag which is set by this
    // same AP, not by BSP
    if PER_CPU_IDT_INITIALIZED[cpu_id].load(AtomicOrdering::SeqCst) {
        crate::kwarn!(
            "init_interrupts_ap: CPU {} IDT already initialized, skipping",
            cpu_id
        );
        return;
    }

    crate::kinfo!(
        "init_interrupts_ap: Initializing per-CPU IDT for AP core {}",
        cpu_id
    );

    // Get IST indices (same as BSP)
    let error_code_ist = crate::gdt::ERROR_CODE_IST_INDEX as u16;

    unsafe {
        // Create a new IDT for this AP
        let idt = PER_CPU_IDT[cpu_id].as_mut_ptr();

        // Initialize with a new InterruptDescriptorTable
        core::ptr::write(idt, InterruptDescriptorTable::new());
        let idt_ref = &mut *idt;

        // Set up exception handlers (same as BSP)
        idt_ref.breakpoint.set_handler_fn(breakpoint_handler);
        idt_ref
            .page_fault
            .set_handler_fn(page_fault_handler)
            .set_stack_index(error_code_ist);
        idt_ref
            .general_protection_fault
            .set_handler_fn(general_protection_fault_handler)
            .set_stack_index(error_code_ist);
        idt_ref.divide_error.set_handler_fn(divide_error_handler);
        idt_ref
            .double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX as u16);
        idt_ref
            .segment_not_present
            .set_handler_fn(segment_not_present_handler)
            .set_stack_index(error_code_ist);
        idt_ref
            .invalid_opcode
            .set_handler_fn(invalid_opcode_handler);
        idt_ref
            .invalid_tss
            .set_handler_fn(segment_not_present_handler)
            .set_stack_index(error_code_ist);
        idt_ref
            .stack_segment_fault
            .set_handler_fn(segment_not_present_handler)
            .set_stack_index(error_code_ist);

        // Set up hardware interrupts (APs also need these for timer, etc.)
        idt_ref[PIC_1_OFFSET].set_handler_fn(timer_interrupt_handler);
        idt_ref[PIC_1_OFFSET + 1].set_handler_fn(keyboard_interrupt_handler);
        idt_ref[PIC_1_OFFSET + 2].set_handler_fn(spurious_irq2_handler);
        idt_ref[PIC_1_OFFSET + 3].set_handler_fn(spurious_irq3_handler);
        idt_ref[PIC_1_OFFSET + 4].set_handler_fn(spurious_irq4_handler);
        idt_ref[PIC_1_OFFSET + 5].set_handler_fn(spurious_irq5_handler);
        idt_ref[PIC_1_OFFSET + 6].set_handler_fn(spurious_irq6_handler);
        idt_ref[PIC_1_OFFSET + 7].set_handler_fn(spurious_irq7_handler);

        idt_ref[PIC_2_OFFSET].set_handler_fn(spurious_irq8_handler);
        idt_ref[PIC_2_OFFSET + 1].set_handler_fn(spurious_irq9_handler);
        idt_ref[PIC_2_OFFSET + 2].set_handler_fn(spurious_irq10_handler);
        idt_ref[PIC_2_OFFSET + 3].set_handler_fn(spurious_irq11_handler);
        idt_ref[PIC_2_OFFSET + 4].set_handler_fn(spurious_irq12_handler);
        idt_ref[PIC_2_OFFSET + 5].set_handler_fn(spurious_irq13_handler);
        idt_ref[PIC_2_OFFSET + 6].set_handler_fn(spurious_irq14_handler);
        idt_ref[PIC_2_OFFSET + 7].set_handler_fn(spurious_irq15_handler);

        // Set up syscall interrupt handler at 0x81 (callable from Ring 3)
        // Use RIP-relative addressing to get correct runtime address
        let syscall_handler_addr: u64;
        core::arch::asm!(
            "lea {}, [rip + syscall_interrupt_handler]",
            out(reg) syscall_handler_addr,
            options(nostack, nomem)
        );
        crate::kinfo!(
            "AP {}: syscall_interrupt_handler RIP-relative addr 0x{:x}",
            cpu_id,
            syscall_handler_addr
        );
        idt_ref[0x81]
            .set_handler_addr(x86_64::VirtAddr::new_truncate(syscall_handler_addr))
            .set_privilege_level(PrivilegeLevel::Ring3);

        // Set up ring3 switch handler at 0x80 (also callable from Ring 3)
        let ring3_switch_addr: u64;
        core::arch::asm!(
            "lea {}, [rip + ring3_switch_handler]",
            out(reg) ring3_switch_addr,
            options(nostack, nomem)
        );
        idt_ref[0x80]
            .set_handler_addr(x86_64::VirtAddr::new_truncate(ring3_switch_addr))
            .set_privilege_level(PrivilegeLevel::Ring3);

        // Set up IPI handlers for SMP (vectors 0xF0-0xF3)
        idt_ref[0xF0].set_handler_fn(ipi_reschedule_handler);
        idt_ref[0xF1].set_handler_fn(ipi_tlb_flush_handler);
        idt_ref[0xF2].set_handler_fn(ipi_call_function_handler);
        idt_ref[0xF3].set_handler_fn(ipi_halt_handler);

        // Set up LAPIC timer handler for AP cores (vector 0xEC = 236)
        // This provides the timer tick for scheduling on non-BSP cores
        idt_ref[LAPIC_TIMER_VECTOR].set_handler_fn(lapic_timer_handler);

        // Mark as initialized before loading
        PER_CPU_IDT_INITIALIZED[cpu_id].store(true, AtomicOrdering::SeqCst);

        // Ensure all writes are visible
        core::sync::atomic::compiler_fence(AtomicOrdering::SeqCst);

        // Load the per-CPU IDT
        idt_ref.load();

        // Ensure load completes
        core::sync::atomic::compiler_fence(AtomicOrdering::SeqCst);
    }

    crate::kinfo!(
        "init_interrupts_ap: Per-CPU IDT loaded for AP core {}",
        cpu_id
    );
}

/// Legacy wrapper for backward compatibility - loads shared BSP IDT on AP
/// Deprecated: Use init_interrupts_ap(cpu_id) for proper per-CPU IDT support
#[allow(dead_code)]
#[deprecated(note = "Use init_interrupts_ap(cpu_id) for per-CPU IDT support")]
pub fn init_interrupts_ap_legacy() {
    // Ensure interrupts are disabled
    x86_64::instructions::interrupts::disable();

    // Verify that BSP has initialized the IDT
    if !is_idt_initialized() {
        crate::kpanic!("AP attempted to load IDT before BSP initialization");
    }

    crate::kwarn!("init_interrupts_ap_legacy: Loading shared BSP IDT (not recommended)");

    // Load the shared IDT on this core
    core::sync::atomic::compiler_fence(AtomicOrdering::SeqCst);
    IDT.load();
    core::sync::atomic::compiler_fence(AtomicOrdering::SeqCst);

    crate::kinfo!("init_interrupts_ap_legacy: Shared IDT loaded on AP core");
}

/// Check if a specific CPU's IDT has been initialized
#[allow(dead_code)]
pub fn is_cpu_idt_initialized(cpu_id: usize) -> bool {
    if cpu_id >= MAX_CPUS {
        return false;
    }
    if cpu_id == 0 {
        IDT_INITIALIZED.load(AtomicOrdering::SeqCst)
    } else {
        PER_CPU_IDT_INITIALIZED[cpu_id].load(AtomicOrdering::SeqCst)
    }
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
