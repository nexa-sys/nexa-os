/// Global Descriptor Table (GDT) setup for user/kernel mode separation
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
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
/// IST slot dedicated to exceptions that push an error code (e.g. #PF, #GP)
pub const ERROR_CODE_IST_INDEX: u16 = 1;

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

static mut SELECTORS: MaybeUninit<Selectors> = MaybeUninit::uninit();
static SELECTORS_READY: AtomicBool = AtomicBool::new(false);

/// Kernel stack for syscall
static mut KERNEL_STACK: AlignedStack = AlignedStack {
    bytes: [0; STACK_SIZE],
};
static mut DOUBLE_FAULT_STACK: AlignedStack = AlignedStack {
    bytes: [0; STACK_SIZE],
};
static mut ERROR_CODE_STACK: AlignedStack = AlignedStack {
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

        // Provide a dedicated IST for any exception that pushes an error code
        // (e.g. #PF, #GP) so their stacks remain 16-byte aligned even though
        // the hardware pushes an extra quadword compared to normal interrupts.
        let ec_top_aligned = aligned_error_code_stack_top(ptr::addr_of!(ERROR_CODE_STACK));
        TSS.interrupt_stack_table[ERROR_CODE_IST_INDEX as usize] = ec_top_aligned;
        crate::kinfo!(
            "Error-code IST pointer set to {:#x}",
            TSS.interrupt_stack_table[ERROR_CODE_IST_INDEX as usize].as_u64()
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

        let selectors = Selectors {
            code_selector: kernel_code,
            data_selector: kernel_data,
            user_code_selector: user_code_sel,
            user_data_selector: user_data_sel,
            tss_selector: tss,
        };

        crate::kinfo!("GDT selectors calculated: kernel_code={:#x}, kernel_data={:#x}, user_code={:#x}, user_data={:#x}, tss={:#x}",
            kernel_code.0, kernel_data.0, user_code_sel.0, user_data_sel.0, tss.0);

        // Write selectors to static storage using MaybeUninit::write
        #[allow(static_mut_refs)]
        {
            SELECTORS.write(selectors);
        }
        SELECTORS_READY.store(true, Ordering::SeqCst);

        crate::kinfo!("GDT selectors stored and ready");

        GDT = Some(gdt);

        // Load GDT
        if let Some(ref gdt) = GDT {
            for (idx, entry) in gdt.entries().iter().enumerate() {
                crate::kinfo!("GDT[{}] = {:#018x}", idx, entry.raw());
            }
            gdt.load();
        }

        // Load segment selectors
        if let Some(selectors) = selectors_ref() {
            CS::set_reg(selectors.code_selector);
            DS::set_reg(selectors.data_selector);
            load_tss(selectors.tss_selector);
        }
    }

    // Debug: Print selectors
    let _selectors = unsafe { get_selectors() };
}

/// Load the shared GDT/TSS on an application processor after BSP setup.
pub fn init_ap() {
    use x86_64::instructions::segmentation::{Segment, CS, DS};
    use x86_64::instructions::tables::load_tss;

    unsafe {
        if let Some(ref gdt) = GDT {
            gdt.load();
        } else {
            crate::kpanic!("AP attempted to load GDT before BSP initialization");
        }

        let selectors = get_selectors();
        CS::set_reg(selectors.code_selector);
        DS::set_reg(selectors.data_selector);
        load_tss(selectors.tss_selector);
    }
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

/// Get privilege stack for the given index
pub unsafe fn get_privilege_stack(index: usize) -> u64 {
    TSS.privilege_stack_table[index].as_u64()
}

pub fn get_kernel_stack_top() -> u64 {
    unsafe { aligned_privilege_stack_top(ptr::addr_of!(KERNEL_STACK)).as_u64() }
}
