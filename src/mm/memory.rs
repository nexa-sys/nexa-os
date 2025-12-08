use multiboot2::{BootInformation, MemoryAreaType};
use core::sync::atomic::{AtomicU64, Ordering};

/// Total physical memory in bytes (detected from bootloader)
static TOTAL_PHYSICAL_MEMORY: AtomicU64 = AtomicU64::new(0);

/// Get total physical memory in bytes
pub fn get_total_physical_memory() -> u64 {
    TOTAL_PHYSICAL_MEMORY.load(Ordering::Relaxed)
}

/// Set total physical memory in bytes (for UEFI boot path)
pub fn set_total_physical_memory(size: u64) {
    TOTAL_PHYSICAL_MEMORY.store(size, Ordering::Relaxed);
}

pub fn log_memory_overview(boot_info: &BootInformation<'_>) {
    if let Some(memmap) = boot_info.memory_map_tag() {
        let areas = memmap.memory_areas();
        crate::kinfo!("[mem] Detected {} memory regions", areas.len());

        let mut total_available: u64 = 0;
        
        for area in areas.iter() {
            let start = area.start_address() as u64;
            let end = area.end_address() as u64;
            let size = area.size() as u64;
            let size_kib = (size / 1024).max(1);

            crate::kinfo!(
                "  - {:#012x} .. {:#012x} ({} KiB, {})",
                start,
                end,
                size_kib,
                classify_area(area.typ())
            );
            
            // Count available memory
            if area.typ() == MemoryAreaType::Available {
                total_available += size;
            }
        }
        
        // Store total physical memory
        TOTAL_PHYSICAL_MEMORY.store(total_available, Ordering::Relaxed);
        crate::kinfo!("[mem] Total available physical memory: {} MB", total_available / (1024 * 1024));
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
