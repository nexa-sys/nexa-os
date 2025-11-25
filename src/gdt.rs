/// Global Descriptor Table (GDT) setup for user/kernel mode separation
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

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

/// Maximum number of CPUs (must match smp::MAX_CPUS)
const MAX_CPUS: usize = 16;

/// Per-CPU Task State Segment (must be 16-byte aligned for performance)
#[repr(align(16))]
struct AlignedTss {
    tss: TaskStateSegment,
}

static mut PER_CPU_TSS: [AlignedTss; MAX_CPUS] = [
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
    AlignedTss {
        tss: TaskStateSegment::new(),
    },
];

/// Per-CPU Global Descriptor Table (stored as raw bytes since GDT is not Copy)
static mut PER_CPU_GDT: [MaybeUninit<GlobalDescriptorTable>; MAX_CPUS] = unsafe {
    // SAFETY: MaybeUninit::uninit() is safe for any type
    MaybeUninit::uninit().assume_init()
};

/// Per-CPU GDT initialized flags
static PER_CPU_GDT_READY: [AtomicBool; MAX_CPUS] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];

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

static mut PER_CPU_STACKS: [PerCpuStacks; MAX_CPUS] = [EMPTY_PERCPU_STACKS; MAX_CPUS];

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

/// Initialize GDT with kernel and user segments (BSP = CPU 0)
pub fn init() {
    init_cpu(0);
}

/// Initialize GDT for a specific CPU
fn init_cpu(cpu_id: usize) {
    use x86_64::instructions::port::Port;
    use x86_64::instructions::segmentation::{Segment, CS, DS};
    use x86_64::instructions::tables::load_tss;

    if cpu_id >= MAX_CPUS {
        crate::kpanic!("CPU ID {} exceeds MAX_CPUS {}", cpu_id, MAX_CPUS);
    }

    unsafe {
        // Debug for AP - check current RSP
        if cpu_id > 0 {
            let current_rsp: u64;
            core::arch::asm!("mov {}, rsp", out(reg) current_rsp);
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'a'); // Entry
                           // Output RSP as hex (low 3 bytes should be enough to see offset)
            for i in (0..6).rev() {
                let nibble = ((current_rsp >> (i * 4)) & 0xF) as u8;
                s.write(if nibble < 10 {
                    b'0' + nibble
                } else {
                    b'A' + nibble - 10
                });
            }
            s.write(b'/');
        }

        let df_base =
            ptr::addr_of!(PER_CPU_STACKS[cpu_id].double_fault_stack.bytes).cast::<u8>() as u64;
        let df_top_aligned =
            aligned_ist_stack_top(ptr::addr_of!(PER_CPU_STACKS[cpu_id].double_fault_stack));

        if cpu_id == 0 {
            crate::kinfo!(
                "Double fault stack base={:#x}, aligned_top={:#x}",
                df_base,
                df_top_aligned.as_u64()
            );
        }

        // Setup TSS for this CPU
        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'b'); // Before TSS setup
        }

        let tss = &mut PER_CPU_TSS[cpu_id].tss;

        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'c'); // Got TSS reference
        }

        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = df_top_aligned;

        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'd'); // After IST[DOUBLE_FAULT]
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
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'D'); // Before error_code_stack
        }

        // Provide a dedicated IST for any exception that pushes an error code
        let ec_top_aligned =
            aligned_error_code_stack_top(ptr::addr_of!(PER_CPU_STACKS[cpu_id].error_code_stack));

        // Debug: after error_code stack calculation
        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'E'); // After error_code stack calculation
        }

        tss.interrupt_stack_table[ERROR_CODE_IST_INDEX as usize] = ec_top_aligned;

        // Debug: after writing IST entry
        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'F'); // After writing IST[ERROR_CODE]
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
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'P'); // Before privilege_stack_table
        }

        // Setup privilege stack for syscall (RSP0 for Ring 0)
        tss.privilege_stack_table[0] =
            aligned_privilege_stack_top(ptr::addr_of!(PER_CPU_STACKS[cpu_id].kernel_stack));

        // Debug: after privilege stack setup
        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'Q'); // After privilege_stack_table
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
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'R'); // Before GDT creation

            // Force 16-byte RSP alignment before calling GlobalDescriptorTable::new()
            // SSE instructions like movaps require 16-byte alignment
            let rsp: u64;
            core::arch::asm!("mov {}, rsp", out(reg) rsp);
            let align_nibble = (rsp & 0xF) as u8;
            s.write(if align_nibble < 10 {
                b'0' + align_nibble
            } else {
                b'A' + align_nibble - 10
            });
        }

        // Ensure 16-byte stack alignment before GDT creation
        // This is critical for AP cores where SSE instructions may be generated
        unsafe {
            core::arch::asm!(
                "and rsp, ~0xF", // Align RSP to 16 bytes
                options(nomem, nostack)
            );
        }

        // Create GDT for this CPU directly without temp variables to avoid move issues
        // (Move of GlobalDescriptorTable containing AtomicU64 was causing #GP on AP)
        let mut gdt = GlobalDescriptorTable::new();

        // Debug: after GDT::new()
        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'S'); // After GDT::new
        }

        // Entry 0: Null descriptor (required)
        // Entry 1: Kernel code segment
        let kernel_code = gdt.append(Descriptor::kernel_code_segment());

        // Debug: after kernel_code
        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'T'); // After kernel_code
        }

        // Entry 2: Kernel data segment
        let kernel_data = gdt.append(Descriptor::kernel_data_segment());

        // Debug: after kernel_data
        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'U'); // After kernel_data
        }

        // Entry 3: User data segment (DPL=3) - MUST come before user code for SYSRET!
        let user_data_sel = gdt.append(Descriptor::user_data_segment());
        // Entry 4: User code segment (DPL=3) - MUST come after user data for SYSRET!
        let user_code_sel = gdt.append(Descriptor::user_code_segment());

        // Debug: before TSS segment
        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'V'); // Before TSS segment
        }

        // Entry 5: TSS (per-CPU)
        let tss_ptr = &raw const PER_CPU_TSS[cpu_id].tss;

        // Debug: got tss_ptr
        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'W'); // Got tss_ptr
        }

        let tss_sel = gdt.append(Descriptor::tss_segment(&*tss_ptr));

        // Debug: after TSS segment
        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'X'); // After TSS segment
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
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'e'); // After GDT write
        }

        // Load GDT
        let gdt_ref = PER_CPU_GDT[cpu_id].assume_init_ref();

        if cpu_id == 0 {
            for (idx, entry) in gdt_ref.entries().iter().enumerate() {
                crate::kinfo!("CPU {}: GDT[{}] = {:#018x}", cpu_id, idx, entry.raw());
            }
        }

        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'f'); // Before GDT load
        }

        gdt_ref.load();

        if cpu_id > 0 {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'g'); // After GDT load
        }

        // Load segment selectors
        if let Some(selectors) = selectors_ref() {
            if cpu_id > 0 {
                let mut s = Port::<u8>::new(0x3F8);
                s.write(b'h'); // Before CS::set_reg
            }
            CS::set_reg(selectors.code_selector);
            if cpu_id > 0 {
                let mut s = Port::<u8>::new(0x3F8);
                s.write(b'i'); // After CS::set_reg
            }
            DS::set_reg(selectors.data_selector);
            if cpu_id > 0 {
                let mut s = Port::<u8>::new(0x3F8);
                s.write(b'j'); // After DS::set_reg
            }
            // Each CPU needs its own TSS loaded
            load_tss(selectors.tss_selector);
            if cpu_id > 0 {
                let mut s = Port::<u8>::new(0x3F8);
                s.write(b'k'); // After load_tss
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
    use x86_64::instructions::port::Port;

    unsafe {
        // Debug: Entry to init_ap - check RSP alignment
        let mut s = Port::<u8>::new(0x3F8);
        s.write(b'I'); // init_ap entry

        // Check RSP alignment
        let rsp: u64;
        core::arch::asm!("mov {}, rsp", out(reg) rsp);
        // Output RSP low byte to check alignment (should end in 8 for correct ABI)
        let align_byte = (rsp & 0xF) as u8;
        s.write(if align_byte < 10 {
            b'0' + align_byte
        } else {
            b'A' + align_byte - 10
        });

        // Print cpu_id as hex
        let nibble = ((cpu_id >> 4) & 0xF) as u8;
        s.write(if nibble < 10 {
            b'0' + nibble
        } else {
            b'A' + nibble - 10
        });
        let nibble = (cpu_id & 0xF) as u8;
        s.write(if nibble < 10 {
            b'0' + nibble
        } else {
            b'A' + nibble - 10
        });
    }

    if cpu_id == 0 {
        unsafe {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'Z'); // BSP error
        }
        loop {
            unsafe {
                core::arch::asm!("hlt");
            }
        }
    }
    if cpu_id >= MAX_CPUS {
        unsafe {
            let mut s = Port::<u8>::new(0x3F8);
            s.write(b'M'); // Max exceeded
        }
        loop {
            unsafe {
                core::arch::asm!("hlt");
            }
        }
    }

    unsafe {
        let mut s = Port::<u8>::new(0x3F8);
        s.write(b'G'); // About to call init_cpu
    }

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
    selectors_ref().expect("GDT not initialized")
}

/// Debug helper to dump selector values when tracking corruption
pub fn debug_dump_selectors(tag: &str) {
    let selectors = unsafe { get_selectors() };
    crate::kinfo!(
        "[selectors:{}] kernel_code={:#x}, kernel_data={:#x}, user_code={:#x}, user_data={:#x}, tss={:#x}",
        tag,
        selectors.code_selector.0,
        selectors.data_selector.0,
        selectors.user_code_selector.0,
        selectors.user_data_selector.0,
        selectors.tss_selector.0
    );
}

/// Get privilege stack for the given CPU and index
pub unsafe fn get_privilege_stack(cpu_id: usize, index: usize) -> u64 {
    if cpu_id >= MAX_CPUS {
        crate::kpanic!("CPU ID {} exceeds MAX_CPUS {}", cpu_id, MAX_CPUS);
    }
    PER_CPU_TSS[cpu_id].tss.privilege_stack_table[index].as_u64()
}

pub fn get_kernel_stack_top(cpu_id: usize) -> u64 {
    unsafe {
        aligned_privilege_stack_top(ptr::addr_of!(PER_CPU_STACKS[cpu_id].kernel_stack)).as_u64()
    }
}
