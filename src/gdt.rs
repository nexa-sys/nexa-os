/// Global Descriptor Table (GDT) setup for user/kernel mode separation
use x86_64::structures::gdt::{Descriptor, DescriptorFlags, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

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

/// Initialize GDT with kernel and user segments
pub fn init() {
    use x86_64::instructions::segmentation::{CS, DS, Segment};
    use x86_64::instructions::tables::load_tss;

    unsafe {
        // Setup TSS
        TSS.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
            // Ensure 16-byte alignment after CPU pushes interrupt frame (adds 8 bytes).
            let stack_ptr = unsafe { &STACK as *const _ as u64 };
            let top = VirtAddr::new(stack_ptr + STACK_SIZE as u64);
            top
        };

        // Setup privilege stack for syscall (RSP0 for Ring 0)
        TSS.privilege_stack_table[0] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
            // Align so that after the processor pushes the interrupt frame (40 bytes)
            // the resulting RSP is still 16-byte aligned, which avoids #GP faults
            // when the compiler emits aligned SSE stores (handlers.
            let top = VirtAddr::from_ptr(&raw const STACK) + STACK_SIZE as u64;
            // Subtract 8 bytes since 40 mod 16 = 8. This keeps (top - 8 - 40) % 16 == 0.
            top - 8u64
        };

        // Create GDT
        let mut gdt = GlobalDescriptorTable::new();
        
        // Entry 0: Null descriptor (required)
        // Entry 1: Kernel code segment
        let kernel_code = gdt.append(Descriptor::kernel_code_segment());
        // Entry 2: Kernel data segment
        let kernel_data = gdt.append(Descriptor::kernel_data_segment());
        // Entry 3: User code segment - manually set DPL=3
        let user_code = Descriptor::user_code_segment();
        let user_code_sel = gdt.append(user_code);
        // Entry 4: User data segment - manually set DPL=3  
        let user_data = Descriptor::user_data_segment();
        let user_data_sel = gdt.append(user_data);
        // Entry 5: TSS
        let tss = gdt.append(Descriptor::tss_segment(&TSS));

        SELECTORS = Some(Selectors {
            code_selector: kernel_code,
            data_selector: kernel_data,
            user_code_selector: user_code_sel,
            user_data_selector: user_data_sel,
            tss_selector: tss,
        });

        crate::kinfo!("GDT selectors set: kernel_code={:#x}, kernel_data={:#x}, user_code={:#x}, user_data={:#x}, tss={:#x}",
            kernel_code.0, kernel_data.0, user_code_sel.0, user_data_sel.0, tss.0);

        GDT = Some(gdt);

        // Load GDT
        if let Some(ref gdt) = GDT {
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
    crate::kinfo!("TSS privilege stack[0]: {:#x}", unsafe { TSS.privilege_stack_table[0].as_u64() });

    crate::kinfo!("GDT initialized with user/kernel segments");
}

/// Get the current selectors
pub unsafe fn get_selectors() -> &'static Selectors {
    SELECTORS.as_ref().expect("GDT not initialized")
}
