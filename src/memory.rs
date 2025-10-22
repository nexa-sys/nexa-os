use multiboot2::{BootInformation, MemoryAreaType};

pub fn log_memory_overview(boot_info: &BootInformation<'_>) {
    if let Some(memmap) = boot_info.memory_map_tag() {
        let areas = memmap.memory_areas();
        crate::serial_println!("[mem] Detected {} memory regions:", areas.len());

        for area in areas.iter() {
            let start = area.start_address() as u64;
            let end = area.end_address() as u64;
            let size_kib = (area.size() / 1024).max(1);

            crate::serial_println!(
                "  - {:#012x} .. {:#012x} ({} KiB, {})",
                start,
                end,
                size_kib,
                classify_area(area.typ())
            );
        }
    } else {
        crate::serial_println!("[mem] No memory map provided by bootloader.");
    }

    let mut any_module = false;
    for module in boot_info.module_tags() {
        if !any_module {
            crate::serial_println!("[mem] Boot modules:");
            any_module = true;
        }

        let name = module.cmdline().unwrap_or("<invalid utf-8>");

        crate::serial_println!(
            "  - {:#010x} .. {:#010x} ({} bytes): {}",
            module.start_address(),
            module.end_address(),
            module.module_size(),
            name
        );
    }

    if !any_module {
        crate::serial_println!("[mem] No boot modules supplied.");
    }
}

fn classify_area(area_type: multiboot2::MemoryAreaTypeId) -> &'static str {
    match MemoryAreaType::from(area_type) {
        MemoryAreaType::Available => "Usable",
        MemoryAreaType::Reserved => "Reserved",
        MemoryAreaType::AcpiAvailable => "ACPI",
        MemoryAreaType::ReservedHibernate => "ACPI NVS",
        MemoryAreaType::Defective => "Defective",
        MemoryAreaType::Custom(_) => "Custom",
    }
}
