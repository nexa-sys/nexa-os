/// Global Descriptor Table (GDT) setup for user/kernel mode separation
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
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
            VirtAddr::from_ptr(&STACK) + STACK_SIZE as u64
        };

        // Setup privilege stack for syscall
        TSS.privilege_stack_table[0] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
            VirtAddr::from_ptr(&STACK) + STACK_SIZE as u64
        };

        // Create GDT
        let mut gdt = GlobalDescriptorTable::new();
        
        // Entry 0: Null descriptor (required)
        // Entry 1: Kernel code segment
        let kernel_code = gdt.append(Descriptor::kernel_code_segment());
        // Entry 2: Kernel data segment
        let kernel_data = gdt.append(Descriptor::kernel_data_segment());
        // Entry 3: User code segment
        let user_code = gdt.append(Descriptor::user_code_segment());
        // Entry 4: User data segment
        let user_data = gdt.append(Descriptor::user_data_segment());
        // Entry 5: TSS
        let tss = gdt.append(Descriptor::tss_segment(&TSS));

        SELECTORS = Some(Selectors {
            code_selector: kernel_code,
            data_selector: kernel_data,
            user_code_selector: user_code,
            user_data_selector: user_data,
            tss_selector: tss,
        });

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

    crate::kinfo!("GDT initialized with user/kernel segments");
}

/// Get the current selectors
pub fn get_selectors() -> &'static Selectors {
    unsafe {
        SELECTORS.as_ref().expect("GDT not initialized")
    }
}
