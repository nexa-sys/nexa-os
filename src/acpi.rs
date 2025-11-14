use core::mem;
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicBool, Ordering};

const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";
const MADT_SIGNATURE: &[u8; 4] = b"APIC";
const EBDA_PTR: usize = 0x40E;
const EBDA_SEARCH_SIZE: usize = 1024;
const BIOS_SEARCH_START: usize = 0xE0000;
const BIOS_SEARCH_END: usize = 0x100000;
pub const MAX_CPUS: usize = 16;

#[repr(C, packed)]
struct RsdpV2 {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt_address: u64,
    extended_checksum: u8,
    reserved: [u8; 3],
}

#[repr(C, packed)]
struct SdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

#[repr(C, packed)]
struct Madt {
    header: SdtHeader,
    lapic_address: u32,
    flags: u32,
}

#[repr(C, packed)]
struct MadtLocalApic {
    entry_type: u8,
    length: u8,
    acpi_processor_id: u8,
    apic_id: u8,
    flags: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CpuDescriptor {
    pub acpi_processor_id: u8,
    pub apic_id: u8,
    pub enabled: bool,
}

impl CpuDescriptor {
    const fn empty() -> Self {
        Self {
            acpi_processor_id: 0,
            apic_id: 0,
            enabled: false,
        }
    }
}

static INIT_DONE: AtomicBool = AtomicBool::new(false);
static mut LAPIC_BASE: u64 = 0;
static mut CPU_COUNT: usize = 0;
static mut CPU_LIST: [CpuDescriptor; MAX_CPUS] = [CpuDescriptor::empty(); MAX_CPUS];

pub fn init() -> Result<(), &'static str> {
    if INIT_DONE.load(Ordering::SeqCst) {
        return Ok(());
    }

    unsafe {
        let rsdp = find_rsdp().ok_or("RSDP not found")?;
        crate::kinfo!("ACPI: RSDP located (revision {})", rsdp.revision);
        let madt = locate_madt(rsdp).ok_or("MADT not found")?;
        parse_madt(madt)?;
        crate::kinfo!(
            "ACPI: MADT reports LAPIC at {:#x} ({} CPUs)",
            LAPIC_BASE,
            CPU_COUNT
        );
    }

    INIT_DONE.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn lapic_base() -> Option<u64> {
    if !INIT_DONE.load(Ordering::SeqCst) {
        return None;
    }
    Some(unsafe { LAPIC_BASE })
}

pub fn cpus() -> &'static [CpuDescriptor] {
    if !INIT_DONE.load(Ordering::SeqCst) {
        return &[];
    }

    unsafe { slice::from_raw_parts(CPU_LIST.as_ptr(), CPU_COUNT) }
}

unsafe fn find_rsdp() -> Option<&'static RsdpV2> {
    let ebda_addr = ((ptr::read::<u16>(EBDA_PTR as *const u16) as usize) << 4) as usize;
    if ebda_addr >= 0x80000 {
        if let Some(rsdp) = search_rsdp(ebda_addr, ebda_addr + EBDA_SEARCH_SIZE) {
            return Some(rsdp);
        }
    }
    search_rsdp(BIOS_SEARCH_START, BIOS_SEARCH_END)
}

unsafe fn search_rsdp(start: usize, end: usize) -> Option<&'static RsdpV2> {
    let mut addr = start;
    while addr < end {
        let sig_ptr = addr as *const u8;
        let mut matches = true;
        for i in 0..RSDP_SIGNATURE.len() {
            if sig_ptr.add(i).read() != RSDP_SIGNATURE[i] {
                matches = false;
                break;
            }
        }

        if matches {
            let rsdp = &*(addr as *const RsdpV2);
            let length = if rsdp.revision >= 2 && rsdp.length as usize >= mem::size_of::<RsdpV2>() {
                rsdp.length as usize
            } else {
                20
            };

            if checksum(sig_ptr, length) == 0 {
                return Some(rsdp);
            }
        }

        addr += 16;
    }

    None
}

unsafe fn locate_madt(rsdp: &RsdpV2) -> Option<&'static Madt> {
    if rsdp.revision >= 2 && rsdp.xsdt_address != 0 {
        if let Some(header) = scan_sdt(rsdp.xsdt_address, true) {
            return Some(&*(header as *const SdtHeader as *const Madt));
        }
    }

    if rsdp.rsdt_address != 0 {
        if let Some(header) = scan_sdt(rsdp.rsdt_address as u64, false) {
            return Some(&*(header as *const SdtHeader as *const Madt));
        }
    }

    None
}

unsafe fn scan_sdt(addr: u64, is_xsdt: bool) -> Option<&'static SdtHeader> {
    let header = &*(addr as *const SdtHeader);
    let entries_len = (header.length as usize).saturating_sub(mem::size_of::<SdtHeader>());
    let entry_size = if is_xsdt { 8 } else { 4 };
    let entry_count = entries_len / entry_size;
    let entries_ptr = (header as *const SdtHeader as *const u8).add(mem::size_of::<SdtHeader>());

    if checksum(header as *const _ as *const u8, header.length as usize) != 0 {
        return None;
    }

    for idx in 0..entry_count {
        let entry_addr = if is_xsdt {
            *(entries_ptr.add(idx * entry_size) as *const u64)
        } else {
            *(entries_ptr.add(idx * entry_size) as *const u32) as u64
        };

        if entry_addr == 0 {
            continue;
        }

        let candidate = &*(entry_addr as *const SdtHeader);
        if &candidate.signature == MADT_SIGNATURE
            && checksum(candidate as *const _ as *const u8, candidate.length as usize) == 0
        {
            return Some(candidate);
        }
    }

    None
}

unsafe fn parse_madt(madt: &Madt) -> Result<(), &'static str> {
    let mut count = 0usize;
    LAPIC_BASE = madt.lapic_address as u64;

    let entries_start = (madt as *const Madt as *const u8).add(mem::size_of::<Madt>());
    let entries_len = madt
        .header
        .length
        .saturating_sub(mem::size_of::<Madt>() as u32) as usize;

    let mut offset = 0usize;
    while offset + 2 <= entries_len {
        let entry_ptr = entries_start.add(offset);
        let entry_type = entry_ptr.read();
        let entry_len = entry_ptr.add(1).read() as usize;

        if entry_len < 2 || offset + entry_len > entries_len {
            break;
        }

        if entry_type == 0 {
            if entry_len < mem::size_of::<MadtLocalApic>() {
                break;
            }
            let lapic = &*(entry_ptr as *const MadtLocalApic);
            let enabled = (lapic.flags & 1) != 0;
            if enabled && count < CPU_LIST.len() {
                CPU_LIST[count] = CpuDescriptor {
                    acpi_processor_id: lapic.acpi_processor_id,
                    apic_id: lapic.apic_id,
                    enabled,
                };
                count += 1;
            }
        }

        offset += entry_len;
    }

    if count == 0 {
        return Err("MADT contains no enabled processors");
    }

    CPU_COUNT = count;
    Ok(())
}

fn checksum(ptr: *const u8, len: usize) -> u8 {
    let mut sum = 0u8;
    for i in 0..len {
        unsafe {
            sum = sum.wrapping_add(ptr.add(i).read());
        }
    }
    sum
}
