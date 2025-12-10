/// Global Descriptor Table (GDT) setup for user/kernel mode separation
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

use crate::safety::{read_rsp, serial_debug_byte, serial_debug_hex, stack_alignment_offset};

const STACK_SIZE: usize = 4096 * 5;

#[repr(align(16))]
#[derive(Copy, Clone)]
struct AlignedStack {
    bytes: [u8; STACK_SIZE],
}

/// Privilege stack table index for double fault handler
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
/// IST slot dedicated to exceptions that push an error code (e.g. #PF, #GP)
pub const ERROR_CODE_IST_INDEX: u16 = 1;

/// Maximum number of CPUs supported (must match acpi::MAX_CPUS)
const MAX_CPUS: usize = crate::acpi::MAX_CPUS;

/// Static allocation only for BSP (CPU 0)
/// All AP cores (1..MAX_CPUS) use fully dynamic allocation
const STATIC_CPU_COUNT: usize = 1;

/// Per-CPU Task State Segment (must be 16-byte aligned for performance)
/// Using MaybeUninit to support large CPU counts without manual array initialization
#[repr(align(16))]
struct AlignedTss {
    tss: MaybeUninit<TaskStateSegment>,
}

impl AlignedTss {
    const fn uninit() -> Self {
        Self {
            tss: MaybeUninit::uninit(),
        }
    }

    /// Initialize the TSS with a new TaskStateSegment
    unsafe fn init(&mut self) {
        self.tss.write(TaskStateSegment::new());
    }

    /// Get a mutable reference to the TSS (must be initialized first)
    unsafe fn get_mut(&mut self) -> &mut TaskStateSegment {
        self.tss.assume_init_mut()
    }

    /// Get a reference to the TSS (must be initialized first)
    unsafe fn get_ref(&self) -> &TaskStateSegment {
        self.tss.assume_init_ref()
    }
}

/// Per-CPU TSS array - static allocation for first STATIC_CPU_COUNT CPUs
/// Additional CPUs use dynamic allocation
static mut PER_CPU_TSS: [AlignedTss; STATIC_CPU_COUNT] = {
    const UNINIT: AlignedTss = AlignedTss::uninit();
    [UNINIT; STATIC_CPU_COUNT]
};

/// Track which TSS entries have been initialized
static PER_CPU_TSS_READY: [AtomicBool; MAX_CPUS] = {
    const INIT: AtomicBool = AtomicBool::new(false);
    [INIT; MAX_CPUS]
};

/// Per-CPU Global Descriptor Table - static allocation for first STATIC_CPU_COUNT CPUs
static mut PER_CPU_GDT: [MaybeUninit<GlobalDescriptorTable>; STATIC_CPU_COUNT] = unsafe {
    // SAFETY: MaybeUninit::uninit() is safe for any type
    MaybeUninit::uninit().assume_init()
};

/// Per-CPU GDT initialized flags
static PER_CPU_GDT_READY: [AtomicBool; MAX_CPUS] = {
    const INIT: AtomicBool = AtomicBool::new(false);
    [INIT; MAX_CPUS]
};

/// Segment selectors (shared across all CPUs - same values, different TSS)
pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

static mut SELECTORS: MaybeUninit<Selectors> = MaybeUninit::uninit();
static SELECTORS_READY: AtomicBool = AtomicBool::new(false);

/// Per-CPU stacks
#[repr(align(16))]
#[derive(Copy, Clone)]
struct PerCpuStacks {
    kernel_stack: AlignedStack,
    double_fault_stack: AlignedStack,
    error_code_stack: AlignedStack,
}

const EMPTY_PERCPU_STACKS: PerCpuStacks = PerCpuStacks {
    kernel_stack: AlignedStack {
        bytes: [0; STACK_SIZE],
    },
    double_fault_stack: AlignedStack {
        bytes: [0; STACK_SIZE],
    },
    error_code_stack: AlignedStack {
        bytes: [0; STACK_SIZE],
    },
};

static mut PER_CPU_STACKS: [PerCpuStacks; STATIC_CPU_COUNT] =
    [EMPTY_PERCPU_STACKS; STATIC_CPU_COUNT];

#[inline(always)]
unsafe fn stack_top(stack: *const AlignedStack) -> VirtAddr {
    let base = ptr::addr_of!((*stack).bytes).cast::<u8>() as u64;
    VirtAddr::new(base + STACK_SIZE as u64)
}

#[inline(always)]
unsafe fn aligned_stack_top(stack: *const AlignedStack) -> VirtAddr {
    let raw_top = stack_top(stack).as_u64();
    // Align down to the nearest 16-byte boundary to satisfy the
    // x86-interrupt calling convention's requirement that the stack is
    // 16-byte aligned on entry.
    VirtAddr::new(raw_top & !0xFu64)
}

#[inline(always)]
unsafe fn aligned_ist_stack_top(stack: *const AlignedStack) -> VirtAddr {
    // Provide a 16-byte aligned IST pointer so that, after the CPU pushes the
    // interrupt frame (which toggles the stack alignment) and the handler
    // subtracts its stack frame size, the final spill slots targeted by the
    // compiler remain 16-byte aligned. Returning the aligned top directly keeps
    // the resulting stack layout compatible with the `movaps` instructions that
    // the compiler emits when saving SIMD registers in the exception handler
    // prologues.
    aligned_stack_top(stack)
}

#[inline(always)]
unsafe fn aligned_privilege_stack_top(stack: *const AlignedStack) -> VirtAddr {
    // On privilege transitions the CPU pushes SS, RSP, RFLAGS, CS and RIP
    // (5×8 bytes) when no error code is involved. Bias the initial stack
    // pointer by 8 bytes so that, after those pushes, the resulting RSP is
    // still 16-byte aligned when our interrupt handlers start executing.
    //
    // Exceptions that *do* push an error code (6×8 bytes) re-use the same bias
    // through a dedicated IST stack configured below so the handler sees a
    // naturally aligned stack frame.
    let top = aligned_stack_top(stack).as_u64();
    VirtAddr::new(top - 8)
}

#[inline(always)]
unsafe fn aligned_error_code_stack_top(stack: *const AlignedStack) -> VirtAddr {
    // Error-code exceptions need the same 8-byte bias so that, after their
    // six-quadword frame is pushed (48 bytes), the stack pointer observed by
    // the handler still satisfies the ABI's expectation of `rsp % 16 == 8`.
    let top = aligned_stack_top(stack).as_u64();
    VirtAddr::new(top - 8)
}

/// Get aligned stack top for dynamic AlignedStack from smp::alloc
#[inline(always)]
unsafe fn aligned_stack_top_dyn(stack: &crate::smp::alloc::AlignedStack) -> VirtAddr {
    let base = stack.bytes.as_ptr() as u64;
    let top = base + crate::smp::alloc::GDT_STACK_SIZE as u64;
    VirtAddr::new(top & !0xFu64)
}

/// Initialize GDT with kernel and user segments (BSP = CPU 0)
pub fn init() {
    init_cpu(0);
}

/// Initialize GDT for a specific CPU
fn init_cpu(cpu_id: usize) {
    use x86_64::instructions::segmentation::{Segment, CS, DS};
    use x86_64::instructions::tables::load_tss;

    if cpu_id >= MAX_CPUS {
        crate::kpanic!("CPU ID {} exceeds MAX_CPUS {}", cpu_id, MAX_CPUS);
    }

    // Determine if this CPU uses dynamic allocation
    let uses_dynamic = cpu_id >= STATIC_CPU_COUNT;

    unsafe {
        // Debug for AP - check current RSP
        if cpu_id > 0 {
            let current_rsp = read_rsp();
            serial_debug_byte(b'a'); // Entry
                                     // Output RSP as hex (low 3 bytes should be enough to see offset)
            serial_debug_hex(current_rsp, 6);
            serial_debug_byte(b'/');
            if uses_dynamic {
                serial_debug_byte(b'D'); // Dynamic mode indicator
            }
        }

        // Get stack pointers - either from static or dynamic allocation
        let (df_top_aligned, ec_top_aligned, kernel_stack_top): (VirtAddr, VirtAddr, VirtAddr) =
            if uses_dynamic {
                // Dynamic allocation path
                let gdt_stacks = crate::smp::alloc::get_dynamic_gdt_stacks(cpu_id)
                    .expect("Dynamic GDT stacks not allocated");

                let df_top = aligned_stack_top_dyn(&gdt_stacks.double_fault_stack);
                // Error code stack needs -8 bias like static version
                let ec_top = {
                    let top = aligned_stack_top_dyn(&gdt_stacks.error_code_stack).as_u64();
                    VirtAddr::new(top - 8)
                };
                // Kernel stack also needs -8 bias for privilege stack
                let ks_top = {
                    let top = aligned_stack_top_dyn(&gdt_stacks.kernel_stack).as_u64();
                    VirtAddr::new(top - 8)
                };
                (df_top, ec_top, ks_top)
            } else {
                // Static allocation path
                let df_top =
                    aligned_ist_stack_top(ptr::addr_of!(PER_CPU_STACKS[cpu_id].double_fault_stack));
                let ec_top = aligned_error_code_stack_top(ptr::addr_of!(
                    PER_CPU_STACKS[cpu_id].error_code_stack
                ));
                let ks_top =
                    aligned_privilege_stack_top(ptr::addr_of!(PER_CPU_STACKS[cpu_id].kernel_stack));
                (df_top, ec_top, ks_top)
            };

        if cpu_id == 0 {
            crate::kinfo!(
                "Double fault stack aligned_top={:#x}",
                df_top_aligned.as_u64()
            );
        }

        // Setup TSS for this CPU
        if cpu_id > 0 {
            serial_debug_byte(b'b'); // Before TSS setup
        }

        // Get TSS - either from static or dynamic allocation
        let tss: &mut TaskStateSegment = if uses_dynamic {
            crate::smp::alloc::get_dynamic_tss_mut(cpu_id).expect("Dynamic TSS not allocated")
        } else {
            // Initialize TSS if not already done
            if !PER_CPU_TSS_READY[cpu_id].load(Ordering::Acquire) {
                PER_CPU_TSS[cpu_id].init();
                PER_CPU_TSS_READY[cpu_id].store(true, Ordering::Release);
            }
            PER_CPU_TSS[cpu_id].get_mut()
        };

        if cpu_id > 0 {
            serial_debug_byte(b'c'); // Got TSS reference
        }

        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = df_top_aligned;

        if cpu_id > 0 {
            serial_debug_byte(b'd'); // After IST[DOUBLE_FAULT]
        }

        if cpu_id == 0 {
            crate::kinfo!(
                "CPU {}: Double fault IST pointer set to {:#x}",
                cpu_id,
                tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize].as_u64()
            );
        }

        // Debug: before error_code stack access
        if cpu_id > 0 {
            serial_debug_byte(b'D'); // Before error_code_stack
        }

        // Debug: after error_code stack calculation
        if cpu_id > 0 {
            serial_debug_byte(b'E'); // After error_code stack calculation
        }

        tss.interrupt_stack_table[ERROR_CODE_IST_INDEX as usize] = ec_top_aligned;

        // Debug: after writing IST entry
        if cpu_id > 0 {
            serial_debug_byte(b'F'); // After writing IST[ERROR_CODE]
        }

        if cpu_id == 0 {
            crate::kinfo!(
                "CPU {}: Error-code IST pointer set to {:#x}",
                cpu_id,
                tss.interrupt_stack_table[ERROR_CODE_IST_INDEX as usize].as_u64()
            );
        }

        // Debug: before privilege stack setup
        if cpu_id > 0 {
            serial_debug_byte(b'P'); // Before privilege_stack_table
        }

        // Setup privilege stack for syscall (RSP0 for Ring 0)
        tss.privilege_stack_table[0] = kernel_stack_top;

        // Debug: after privilege stack setup
        if cpu_id > 0 {
            serial_debug_byte(b'Q'); // After privilege_stack_table
        }

        if cpu_id == 0 {
            crate::kinfo!(
                "CPU {}: Kernel privilege stack (RSP0) set to {:#x}",
                cpu_id,
                tss.privilege_stack_table[0].as_u64()
            );
        }

        // Debug: before GDT creation
        if cpu_id > 0 {
            serial_debug_byte(b'R'); // Before GDT creation

            // Force 16-byte RSP alignment before calling GlobalDescriptorTable::new()
            // SSE instructions like movaps require 16-byte alignment
            serial_debug_hex(stack_alignment_offset() as u64, 1);
        }

        // Ensure 16-byte stack alignment before GDT creation
        // This is critical for AP cores where SSE instructions may be generated
        core::arch::asm!(
            "and rsp, ~0xF", // Align RSP to 16 bytes
            options(nomem, nostack)
        );

        // Get GDT reference - either static or dynamic
        let gdt_ref: &'static GlobalDescriptorTable = if uses_dynamic {
            // For dynamic allocation, we need to populate the GDT
            let gdt =
                crate::smp::alloc::get_dynamic_gdt_mut(cpu_id).expect("Dynamic GDT not allocated");

            // The GDT is freshly allocated, we need to set it up
            // Entry 0: Null descriptor (required) - already present in new()
            // Entry 1: Kernel code segment
            let kernel_code = gdt.append(Descriptor::kernel_code_segment());
            // Entry 2: Kernel data segment
            let kernel_data = gdt.append(Descriptor::kernel_data_segment());
            // Entry 3: User data segment (DPL=3) - MUST come before user code for SYSRET!
            let _user_data_sel = gdt.append(Descriptor::user_data_segment());
            // Entry 4: User code segment (DPL=3) - MUST come after user data for SYSRET!
            let _user_code_sel = gdt.append(Descriptor::user_code_segment());

            // Entry 5: TSS (per-CPU) - use the dynamically allocated TSS
            let tss_ptr = tss as *const TaskStateSegment;
            let _tss_sel = gdt.append(Descriptor::tss_segment(&*tss_ptr));

            if cpu_id > 0 {
                serial_debug_byte(b'X'); // After TSS segment
            }

            // Return static reference (safe because dynamic allocation lives forever)
            &*(gdt as *const GlobalDescriptorTable)
        } else {
            // Static allocation path
            // Create GDT for this CPU directly without temp variables to avoid move issues
            let mut gdt = GlobalDescriptorTable::new();

            // Debug: after GDT::new()
            if cpu_id > 0 {
                serial_debug_byte(b'S'); // After GDT::new
            }

            // Entry 0: Null descriptor (required)
            // Entry 1: Kernel code segment
            let kernel_code = gdt.append(Descriptor::kernel_code_segment());

            // Debug: after kernel_code
            if cpu_id > 0 {
                serial_debug_byte(b'T'); // After kernel_code
            }

            // Entry 2: Kernel data segment
            let kernel_data = gdt.append(Descriptor::kernel_data_segment());

            // Debug: after kernel_data
            if cpu_id > 0 {
                serial_debug_byte(b'U'); // After kernel_data
            }

            // Entry 3: User data segment (DPL=3) - MUST come before user code for SYSRET!
            let user_data_sel = gdt.append(Descriptor::user_data_segment());
            // Entry 4: User code segment (DPL=3) - MUST come after user data for SYSRET!
            let user_code_sel = gdt.append(Descriptor::user_code_segment());

            // Debug: before TSS segment
            if cpu_id > 0 {
                serial_debug_byte(b'V'); // Before TSS segment
            }

            // Entry 5: TSS (per-CPU)
            let tss_ptr: *const TaskStateSegment = PER_CPU_TSS[cpu_id].get_ref();

            // Debug: got tss_ptr
            if cpu_id > 0 {
                serial_debug_byte(b'W'); // Got tss_ptr
            }

            let tss_sel = gdt.append(Descriptor::tss_segment(&*tss_ptr));

            // Debug: after TSS segment
            if cpu_id > 0 {
                serial_debug_byte(b'X'); // After TSS segment
            }

            // On BSP (CPU 0), store selectors for all CPUs to use
            if cpu_id == 0 {
                let selectors = Selectors {
                    code_selector: kernel_code,
                    data_selector: kernel_data,
                    user_code_selector: user_code_sel,
                    user_data_selector: user_data_sel,
                    tss_selector: tss_sel,
                };

                crate::kinfo!("GDT selectors calculated: kernel_code={:#x}, kernel_data={:#x}, user_code={:#x}, user_data={:#x}, tss={:#x}",
                    kernel_code.0, kernel_data.0, user_code_sel.0, user_data_sel.0, tss_sel.0);

                #[allow(static_mut_refs)]
                {
                    SELECTORS.write(selectors);
                }
                SELECTORS_READY.store(true, Ordering::SeqCst);
                crate::kinfo!("GDT selectors stored and ready");
            }

            // Store GDT for this CPU
            PER_CPU_GDT[cpu_id].write(gdt);
            PER_CPU_GDT_READY[cpu_id].store(true, Ordering::SeqCst);

            if cpu_id > 0 {
                serial_debug_byte(b'e'); // After GDT write
            }

            // Return reference to stored GDT
            PER_CPU_GDT[cpu_id].assume_init_ref()
        };

        if cpu_id == 0 {
            for (idx, entry) in gdt_ref.entries().iter().enumerate() {
                crate::kinfo!("CPU {}: GDT[{}] = {:#018x}", cpu_id, idx, entry.raw());
            }
        }

        if cpu_id > 0 {
            serial_debug_byte(b'f'); // Before GDT load
        }

        gdt_ref.load();

        if cpu_id > 0 {
            serial_debug_byte(b'g'); // After GDT load
        }

        // Load segment selectors
        if let Some(selectors) = selectors_ref() {
            if cpu_id > 0 {
                serial_debug_byte(b'h'); // Before CS::set_reg
            }
            CS::set_reg(selectors.code_selector);
            if cpu_id > 0 {
                serial_debug_byte(b'i'); // After CS::set_reg
            }
            DS::set_reg(selectors.data_selector);
            if cpu_id > 0 {
                serial_debug_byte(b'j'); // After DS::set_reg
            }
            // Each CPU needs its own TSS loaded
            load_tss(selectors.tss_selector);
            if cpu_id > 0 {
                serial_debug_byte(b'k'); // After load_tss
            }
        }
    }

    if cpu_id == 0 {
        // Debug: Print selectors
        let _selectors = unsafe { get_selectors() };
    }
}

/// Load the per-CPU GDT/TSS on an application processor after BSP setup.
/// cpu_id must be obtained from smp module
pub fn init_ap(cpu_id: usize) {
    // Debug: Entry to init_ap - check RSP alignment
    serial_debug_byte(b'I'); // init_ap entry

    // Output RSP alignment (should end in 8 for correct ABI)
    serial_debug_hex(stack_alignment_offset() as u64, 1);

    // Print cpu_id as hex
    serial_debug_hex(cpu_id as u64, 2);

    if cpu_id == 0 {
        serial_debug_byte(b'Z'); // BSP error
        loop {
            crate::safety::hlt();
        }
    }
    if cpu_id >= MAX_CPUS {
        serial_debug_byte(b'M'); // Max exceeded
        loop {
            crate::safety::hlt();
        }
    }

    serial_debug_byte(b'G'); // About to call init_cpu

    // Initialize this CPU's GDT and TSS
    init_cpu(cpu_id);
}

/// Get the current selectors
fn selectors_ref() -> Option<&'static Selectors> {
    if !SELECTORS_READY.load(Ordering::SeqCst) {
        return None;
    }
    #[allow(static_mut_refs)]
    let ptr = unsafe { SELECTORS.as_ptr() };
    Some(unsafe { &*ptr })
}

#[allow(static_mut_refs)]
pub unsafe fn get_selectors() -> &'static Selectors {
    match selectors_ref() {
        Some(s) => s,
        None => {
            let cpu_id = crate::smp::current_cpu_id();
            crate::kpanic!("GDT not initialized on CPU {}", cpu_id);
        }
    }
}

/// Debug helper to dump selector values when tracking corruption
pub fn debug_dump_selectors(tag: &str) {
    if let Some(selectors) = selectors_ref() {
        crate::kinfo!(
            "[selectors:{}] kernel_code={:#x}, kernel_data={:#x}, user_code={:#x}, user_data={:#x}, tss={:#x}",
            tag,
            selectors.code_selector.0,
            selectors.data_selector.0,
            selectors.user_code_selector.0,
            selectors.user_data_selector.0,
            selectors.tss_selector.0
        );
    } else {
        crate::kwarn!("[selectors:{}] GDT not yet initialized!", tag);
    }
}

/// Get privilege stack for the given CPU and index
pub unsafe fn get_privilege_stack(cpu_id: usize, index: usize) -> u64 {
    if cpu_id >= MAX_CPUS {
        crate::kpanic!("CPU ID {} exceeds MAX_CPUS {}", cpu_id, MAX_CPUS);
    }
    PER_CPU_TSS[cpu_id].get_ref().privilege_stack_table[index].as_u64()
}

pub fn get_kernel_stack_top(cpu_id: usize) -> u64 {
    unsafe {
        aligned_privilege_stack_top(ptr::addr_of!(PER_CPU_STACKS[cpu_id].kernel_stack)).as_u64()
    }
}

/// Update TSS RSP0 (privilege level 0 stack) for the current CPU.
/// This should be called during context switch to set the kernel stack
/// for the next process. When an interrupt/syscall occurs from Ring 3,
/// the CPU will use this stack.
///
/// # Safety
/// - `new_rsp0` must be a valid kernel stack pointer (top of stack)
/// - Must be called with interrupts disabled
pub unsafe fn update_tss_rsp0(cpu_id: usize, new_rsp0: u64) {
    use x86_64::VirtAddr;

    if cpu_id >= MAX_CPUS {
        return; // Silently ignore invalid CPU IDs in release mode
    }

    // Determine if this CPU uses dynamic or static TSS
    let uses_dynamic = cpu_id >= STATIC_CPU_COUNT;

    let tss: &mut x86_64::structures::tss::TaskStateSegment = if uses_dynamic {
        match crate::smp::alloc::get_dynamic_tss_mut(cpu_id) {
            Ok(t) => t,
            Err(_) => return, // Dynamic TSS not allocated yet
        }
    } else {
        if !PER_CPU_TSS_READY[cpu_id].load(core::sync::atomic::Ordering::Acquire) {
            return; // TSS not initialized yet
        }
        PER_CPU_TSS[cpu_id].get_mut()
    };

    tss.privilege_stack_table[0] = VirtAddr::new(new_rsp0);
}
