use multiboot2::{BootInformation, MemoryAreaType};

pub fn log_memory_overview(boot_info: &BootInformation<'_>) {
    if let Some(memmap) = boot_info.memory_map_tag() {
        let areas = memmap.memory_areas();
        crate::kinfo!("[mem] Detected {} memory regions", areas.len());

        for area in areas.iter() {
            let start = area.start_address() as u64;
            let end = area.end_address() as u64;
            let size_kib = (area.size() / 1024).max(1);

            crate::kinfo!(
                "  - {:#012x} .. {:#012x} ({} KiB, {})",
                start,
                end,
                size_kib,
                classify_area(area.typ())
            );
        }
    } else {
        crate::kwarn!("[mem] No memory map provided by bootloader.");
    }

    let mut any_module = false;
    for module in boot_info.module_tags() {
        if !any_module {
            crate::kinfo!("[mem] Boot modules:");
            any_module = true;
        }

        let name = module.cmdline().unwrap_or("<invalid utf-8>");

        crate::kinfo!(
            "  - {:#010x} .. {:#010x} ({} bytes): {}",
            module.start_address(),
            module.end_address(),
            module.module_size(),
            name
        );
    }

    if !any_module {
        crate::kinfo!("[mem] No boot modules supplied.");
    }
}

pub fn find_modules_end(boot_info: &BootInformation<'_>) -> u64 {
    let mut max_end = 0;
    for module in boot_info.module_tags() {
        let end = module.end_address() as u64;
        if end > max_end {
            max_end = end;
        }
    }
    max_end
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

pub fn find_heap_region(boot_info: &BootInformation<'_>, min_size: u64) -> Option<(u64, u64)> {
    let memmap = boot_info.memory_map_tag()?;
    
    // Find the largest available region
    let mut best_region = None;
    let mut max_size = 0;

    for area in memmap.memory_areas() {
        if area.typ() == MemoryAreaType::Available {
            let start = area.start_address() as u64;
            let end = area.end_address() as u64;
            let size = end - start;

            // Skip low memory (< 1MB) to avoid BIOS/VGA conflicts
            if start < 0x100000 {
                continue;
            }

            // Check for overlap with modules
            if !is_overlap_with_modules(boot_info, start, size) {
                if size > max_size {
                    max_size = size;
                    best_region = Some((start, size));
                }
            }
        }
    }

    if max_size >= min_size {
        best_region
    } else {
        None
    }
}

fn is_overlap_with_modules(boot_info: &BootInformation<'_>, start: u64, size: u64) -> bool {
    let end = start + size;
    for module in boot_info.module_tags() {
        let mod_start = module.start_address() as u64;
        let mod_end = module.end_address() as u64;
        
        if start < mod_end && end > mod_start {
            return true;
        }
    }
    false
}
