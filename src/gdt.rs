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
    // Ensure the resulting stack is 16-byte aligned after the CPU pushes
    // the interrupt frame (which is 24 bytes without an error code and
    // 32 bytes with an error code when there is no privilege change).
    let raw_top = stack_top(stack).as_u64();
    VirtAddr::new(raw_top.saturating_sub(8))
}

/// Initialize GDT with kernel and user segments
pub fn init() {
    use x86_64::instructions::segmentation::{Segment, CS, DS};
    use x86_64::instructions::tables::load_tss;

    unsafe {
        // Setup TSS
        TSS.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] =
            aligned_stack_top(ptr::addr_of!(DOUBLE_FAULT_STACK));

        // Setup privilege stack for syscall (RSP0 for Ring 0)
        TSS.privilege_stack_table[0] = aligned_stack_top(ptr::addr_of!(KERNEL_STACK));

        // Create GDT
        let mut gdt = GlobalDescriptorTable::new();

        // Entry 0: Null descriptor (required)
        // Entry 1: Kernel code segment
        let kernel_code = gdt.append(Descriptor::kernel_code_segment());
        // Entry 2: Kernel data segment
        let kernel_data = gdt.append(Descriptor::kernel_data_segment());
        // Entry 3: User code segment (DPL=3)
        let user_code_sel = gdt.append(Descriptor::user_code_segment());
        // Entry 4: User data segment (DPL=3)
        let user_data_sel = gdt.append(Descriptor::user_data_segment());
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
    unsafe { aligned_stack_top(ptr::addr_of!(KERNEL_STACK)).as_u64() }
}
