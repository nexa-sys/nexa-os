/// Global Descriptor Table (GDT) setup for user/kernel mode separation
use core::ptr;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

const STACK_SIZE: usize = 4096 * 5;

#[repr(align(16))]
struct AlignedStack {
    bytes: [u8; STACK_SIZE],
}

/// Privilege stack table index for double fault handler
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// Task State Segment
static mut TSS: TaskStateSegment = TaskStateSegment::new();

/// Global Descriptor Table
static mut GDT: Option<GlobalDescriptorTable> = None;

/// Segment selectors
pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

static mut SELECTORS: Option<Selectors> = None;

/// Kernel stack for syscall
static mut KERNEL_STACK: AlignedStack = AlignedStack {
    bytes: [0; STACK_SIZE],
};
static mut DOUBLE_FAULT_STACK: AlignedStack = AlignedStack {
    bytes: [0; STACK_SIZE],
};

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
    // the compiler emits when saving SIMD registers in the double fault
    // handler's prologue.
    aligned_stack_top(stack)
}

#[inline(always)]
unsafe fn aligned_privilege_stack_top(stack: *const AlignedStack) -> VirtAddr {
    // On privilege transitions the CPU pushes SS, RSP, RFLAGS, CS and RIP
    // (5×8 bytes). Bias the initial stack pointer by 8 bytes so that, after
    // those pushes, the resulting RSP is still 16-byte aligned when our
    // interrupt handlers start executing.
    let top = aligned_stack_top(stack).as_u64();
    VirtAddr::new(top - 8)
}

/// Initialize GDT with kernel and user segments
pub fn init() {
    use x86_64::instructions::segmentation::{Segment, CS, DS};
    use x86_64::instructions::tables::load_tss;

    unsafe {
        let df_base = ptr::addr_of!(DOUBLE_FAULT_STACK.bytes).cast::<u8>() as u64;
        let df_top_aligned = aligned_ist_stack_top(ptr::addr_of!(DOUBLE_FAULT_STACK));
        crate::kinfo!(
            "Double fault stack base={:#x}, aligned_top={:#x}",
            df_base,
            df_top_aligned.as_u64()
        );

        // Setup TSS
        TSS.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = df_top_aligned;

        crate::kinfo!(
            "Double fault IST pointer set to {:#x}",
            TSS.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize].as_u64()
        );

        // Setup privilege stack for syscall (RSP0 for Ring 0)
        TSS.privilege_stack_table[0] = aligned_privilege_stack_top(ptr::addr_of!(KERNEL_STACK));
        crate::kinfo!(
            "Kernel privilege stack (RSP0) set to {:#x}",
            TSS.privilege_stack_table[0].as_u64()
        );

        // Create GDT
        let mut gdt = GlobalDescriptorTable::new();

        // Entry 0: Null descriptor (required)
        // Entry 1: Kernel code segment
        let kernel_code = gdt.append(Descriptor::kernel_code_segment());
        // Entry 2: Kernel data segment
        let kernel_data = gdt.append(Descriptor::kernel_data_segment());
        // Entry 3: User data segment (DPL=3) - MUST come before user code for SYSRET!
        let user_data_sel = gdt.append(Descriptor::user_data_segment());
        // Entry 4: User code segment (DPL=3) - MUST come after user data for SYSRET!
        let user_code_sel = gdt.append(Descriptor::user_code_segment());
        // Entry 5: TSS
        let tss_ptr = &raw const TSS as *const TaskStateSegment;
        let tss = gdt.append(Descriptor::tss_segment(&*tss_ptr));

        SELECTORS = Some(Selectors {
            code_selector: kernel_code,
            data_selector: kernel_data,
            user_code_selector: user_code_sel,
            user_data_selector: user_data_sel,
            tss_selector: tss,
        });

        // Set kernel stack for Ring 0
        TSS.privilege_stack_table[0] = x86_64::VirtAddr::new(get_kernel_stack_top());

        // crate::kinfo!("GDT selectors set: kernel_code={:#x}, kernel_data={:#x}, user_code={:#x}, user_data={:#x}, tss={:#x}",
        //     kernel_code.0, kernel_data.0, user_code_sel.0, user_data_sel.0, tss.0);

        GDT = Some(gdt);

        // Load GDT
        if let Some(ref gdt) = GDT {
            for (idx, entry) in gdt.entries().iter().enumerate() {
                crate::kinfo!("GDT[{}] = {:#018x}", idx, entry.raw());
            }
            gdt.load();
        }

        // Load segment selectors
        if let Some(ref selectors) = SELECTORS {
            CS::set_reg(selectors.code_selector);
            DS::set_reg(selectors.data_selector);
            load_tss(selectors.tss_selector);
        }
    }

    // Debug: Print selectors
    let _selectors = unsafe { get_selectors() };
}

/// Get the current selectors
#[allow(static_mut_refs)]
pub unsafe fn get_selectors() -> &'static Selectors {
    SELECTORS.as_ref().expect("GDT not initialized")
}

/// Get privilege stack for the given index
pub unsafe fn get_privilege_stack(index: usize) -> u64 {
    TSS.privilege_stack_table[index].as_u64()
}

pub fn get_kernel_stack_top() -> u64 {
    unsafe { aligned_privilege_stack_top(ptr::addr_of!(KERNEL_STACK)).as_u64() }
}
