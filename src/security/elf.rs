/// ELF loader implementation following POSIX and Unix-like standards
use crate::safety::{RawAccessError, RawReader};

/// ELF magic number
pub const ELF_MAGIC: u32 = 0x464C457F; // 0x7F 'E' 'L' 'F'

/// ELF class
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ElfClass {
    None = 0,
    Elf32 = 1,
    Elf64 = 2,
}

/// ELF data encoding
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ElfData {
    None = 0,
    LittleEndian = 1,
    BigEndian = 2,
}

/// ELF type
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ElfType {
    None = 0,
    Relocatable = 1,
    Executable = 2,
    Shared = 3,
    Core = 4,
}

/// Program header type
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PhType {
    Null = 0,
    Load = 1,
    Dynamic = 2,
    Interp = 3,
    Note = 4,
    ShLib = 5,
    Phdr = 6,
    Tls = 7,
}

/// Program header flags
pub mod ph_flags {
    pub const PF_X: u32 = 0x1; // Execute
    pub const PF_W: u32 = 0x2; // Write
    pub const PF_R: u32 = 0x4; // Read
}

/// ELF64 header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Header {
    pub e_ident: [u8; 16], // ELF identification
    pub e_type: u16,       // Object file type
    pub e_machine: u16,    // Machine type
    pub e_version: u32,    // Object file version
    pub e_entry: u64,      // Entry point address
    pub e_phoff: u64,      // Program header offset
    pub e_shoff: u64,      // Section header offset
    pub e_flags: u32,      // Processor-specific flags
    pub e_ehsize: u16,     // ELF header size
    pub e_phentsize: u16,  // Size of program header entry
    pub e_phnum: u16,      // Number of program header entries
    pub e_shentsize: u16,  // Size of section header entry
    pub e_shnum: u16,      // Number of section header entries
    pub e_shstrndx: u16,   // Section name string table index
}

/// ELF64 program header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64ProgramHeader {
    pub p_type: u32,   // Segment type
    pub p_flags: u32,  // Segment flags
    pub p_offset: u64, // Segment file offset
    pub p_vaddr: u64,  // Segment virtual address
    pub p_paddr: u64,  // Segment physical address
    pub p_filesz: u64, // Segment size in file
    pub p_memsz: u64,  // Segment size in memory
    pub p_align: u64,  // Segment alignment
}

impl Elf64Header {
    /// Check if the header is valid
    pub fn is_valid(&self) -> bool {
        // Check magic number
        let magic = u32::from_le_bytes([
            self.e_ident[0],
            self.e_ident[1],
            self.e_ident[2],
            self.e_ident[3],
        ]);

        if magic != ELF_MAGIC {
            return false;
        }

        // Check class (64-bit)
        if self.e_ident[4] != ElfClass::Elf64 as u8 {
            return false;
        }

        // Check data encoding (little endian)
        if self.e_ident[5] != ElfData::LittleEndian as u8 {
            return false;
        }

        // Check version
        if self.e_ident[6] != 1 {
            return false;
        }

        // Check machine type (x86-64)
        if self.e_machine != 0x3E {
            return false;
        }

        true
    }

    /// Get ELF type
    pub fn get_type(&self) -> ElfType {
        match self.e_type {
            2 => ElfType::Executable,
            3 => ElfType::Shared,
            _ => ElfType::None,
        }
    }

    /// Get entry point
    pub fn entry_point(&self) -> u64 {
        self.e_entry
    }
}

/// ELF loader
pub struct ElfLoader {
    reader: RawReader<'static>,
}

/// Result information describing how an ELF image was loaded into memory.
#[derive(Debug, Clone, Copy)]
pub struct LoadResult {
    /// Final entry point address to transfer control to.
    pub entry_point: u64,
    /// Runtime base address where the first PT_LOAD segment was placed.
    pub base_addr: u64,
    /// Original virtual address of the first PT_LOAD segment.
    pub first_load_vaddr: u64,
    /// Difference between runtime base address and original virtual address.
    pub load_bias: i64,
    /// Runtime virtual address of the program headers table.
    pub phdr_vaddr: u64,
    /// Size in bytes of each program header entry.
    pub phentsize: u16,
    /// Number of program header entries.
    pub phnum: u16,
}

impl ElfLoader {
    /// Create a new ELF loader from raw bytes
    pub fn new(data: &'static [u8]) -> Result<Self, &'static str> {
        let reader = RawReader::new(data);

        crate::kinfo!("ElfLoader::new called with {} bytes", reader.len());
        if reader.len() < 64 {
            crate::kerror!("Data too small for ELF header");
            return Err("Data too small for ELF header");
        }

        macro_rules! read_field {
            ($method:ident, $offset:expr, $label:expr) => {{
                match reader.$method($offset) {
                    Ok(value) => value,
                    Err(err) => {
                        crate::kerror!(
                            "Failed to read {} at offset {:#x}: {:?}",
                            $label,
                            $offset,
                            err
                        );
                        return Err("Malformed ELF header");
                    }
                }
            }};
        }

        let magic = read_field!(u32, 0, "ELF magic");
        if magic != ELF_MAGIC {
            crate::kerror!("Invalid ELF magic: {:#x}", magic);
            return Err("Invalid ELF magic");
        }

        let class = read_field!(u8, 4, "ELF class");
        if class != ElfClass::Elf64 as u8 {
            crate::kerror!("Not ELF64");
            return Err("Not ELF64");
        }

        let data_enc = read_field!(u8, 5, "ELF data encoding");
        if data_enc != ElfData::LittleEndian as u8 {
            crate::kerror!("Not little endian");
            return Err("Not little endian");
        }

        let version = read_field!(u8, 6, "ELF version");
        if version != 1 {
            crate::kerror!("Invalid version");
            return Err("Invalid version");
        }

        let machine = read_field!(u16, 18, "ELF machine");
        if machine != 0x3E {
            crate::kerror!("Not x86-64");
            return Err("Not x86-64");
        }

        crate::kinfo!("ELF header is valid");

        let e_phoff = read_field!(u64, 32, "program header offset");
        let e_phentsize = read_field!(u16, 54, "program header size");
        let e_phnum = read_field!(u16, 56, "program header count");
        let e_entry = read_field!(u64, 24, "entry point");

        crate::kinfo!(
            "ELF header: e_phoff={:#x}, e_phnum={}, e_phentsize={}, e_entry={:#x}",
            e_phoff,
            e_phnum,
            e_phentsize,
            e_entry
        );

        Ok(Self { reader })
    }

    /// Get the ELF header
    pub fn header(&self) -> Result<Elf64Header, RawAccessError> {
        self.reader.read::<Elf64Header>(0)
    }

    /// Get program headers
    pub fn program_headers(&self) -> &[Elf64ProgramHeader] {
        let reader = self.reader;

        let e_phoff = match reader.u64(32) {
            Ok(v) => v as usize,
            Err(err) => {
                crate::kerror!("program_headers: failed to read e_phoff: {:?}", err);
                return &[];
            }
        };
        let e_phnum = match reader.u16(56) {
            Ok(v) => v as usize,
            Err(err) => {
                crate::kerror!("program_headers: failed to read e_phnum: {:?}", err);
                return &[];
            }
        };
        let e_phentsize = match reader.u16(54) {
            Ok(v) => v as usize,
            Err(err) => {
                crate::kerror!("program_headers: failed to read e_phentsize: {:?}", err);
                return &[];
            }
        };

        crate::kinfo!(
            "program_headers: offset={:#x}, count={}, size={}, expected={}",
            e_phoff,
            e_phnum,
            e_phentsize,
            core::mem::size_of::<Elf64ProgramHeader>()
        );

        if e_phentsize != core::mem::size_of::<Elf64ProgramHeader>() {
            crate::kinfo!("program_headers: size mismatch");
            return &[];
        }

        match reader.slice::<Elf64ProgramHeader>(e_phoff, e_phnum) {
            Ok(slice) => {
                crate::kinfo!("program_headers: returning {} headers", slice.len());
                slice
            }
            Err(err) => {
                crate::kerror!("program_headers: failed to read slice: {:?}", err);
                &[]
            }
        }
    }

    /// Check if the ELF file has a PT_INTERP segment (dynamic linking)
    pub fn has_interpreter(&self) -> bool {
        let reader = self.reader;

        let e_phoff = match reader.u64(32) {
            Ok(v) => v as usize,
            Err(err) => {
                crate::kerror!("has_interpreter: failed to read e_phoff: {:?}", err);
                return false;
            }
        };
        let e_phnum = match reader.u16(56) {
            Ok(v) => v as usize,
            Err(err) => {
                crate::kerror!("has_interpreter: failed to read e_phnum: {:?}", err);
                return false;
            }
        };
        let e_phentsize = match reader.u16(54) {
            Ok(v) => v as usize,
            Err(err) => {
                crate::kerror!("has_interpreter: failed to read e_phentsize: {:?}", err);
                return false;
            }
        };

        for i in 0..e_phnum {
            let delta = match i.checked_mul(e_phentsize) {
                Some(v) => v,
                None => return false,
            };
            let ph_offset = match e_phoff.checked_add(delta) {
                Some(v) => v,
                None => return false,
            };

            match reader.u32(ph_offset) {
                Ok(p_type) if p_type == PhType::Interp as u32 => return true,
                Ok(_) => {}
                Err(err) => {
                    crate::kerror!(
                        "has_interpreter: failed to read program header {}: {:?}",
                        i,
                        err
                    );
                    return false;
                }
            }
        }

        false
    }

    /// Get the interpreter path from PT_INTERP segment
    /// Returns None if no interpreter is specified
    pub fn get_interpreter(&self) -> Option<&str> {
        let reader = self.reader;

        let e_phoff = reader.u64(32).ok()? as usize;
        let e_phnum = reader.u16(56).ok()? as usize;
        let e_phentsize = reader.u16(54).ok()? as usize;

        for i in 0..e_phnum {
            let delta = i.checked_mul(e_phentsize)?;
            let ph_offset = e_phoff.checked_add(delta)?;

            let p_type = reader.u32(ph_offset).ok()?;

            if p_type == PhType::Interp as u32 {
                let p_offset = reader.u64(ph_offset + 8).ok()? as usize;
                let p_filesz = reader.u64(ph_offset + 32).ok()? as usize;

                let interp_bytes = reader.bytes(p_offset, p_filesz).ok()?;
                let null_pos = interp_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(p_filesz);

                if let Ok(s) = core::str::from_utf8(&interp_bytes[..null_pos]) {
                    return Some(s);
                }
                return None;
            }
        }

        None
    }

    /// Load the ELF into memory at the specified base address
    /// For position-independent executables, base_addr is used as offset
    /// For static executables (with absolute addresses), segments are loaded at their p_vaddr
    pub fn load(&self, base_addr: u64) -> Result<LoadResult, &'static str> {
        use core::ptr;

        let reader = self.reader;

        let e_phoff = match reader.u64(32) {
            Ok(v) => v as usize,
            Err(err) => {
                crate::kerror!("load: failed to read e_phoff: {:?}", err);
                return Err("Malformed program header table");
            }
        };
        let e_phnum = match reader.u16(56) {
            Ok(v) => v as usize,
            Err(err) => {
                crate::kerror!("load: failed to read e_phnum: {:?}", err);
                return Err("Malformed program header table");
            }
        };
        let e_phentsize = match reader.u16(54) {
            Ok(v) => v as usize,
            Err(err) => {
                crate::kerror!("load: failed to read e_phentsize: {:?}", err);
                return Err("Malformed program header table");
            }
        };

        let header = self.header().map_err(|err| {
            crate::kerror!("load: failed to read ELF header: {:?}", err);
            "Malformed ELF header"
        })?;

        let ph_offset = |index: usize| -> Result<usize, &'static str> {
            let delta = index
                .checked_mul(e_phentsize)
                .ok_or("Program header overflow")?;
            e_phoff.checked_add(delta).ok_or("Program header overflow")
        };

        let mut first_load_vaddr: Option<u64> = None;
        let mut first_load_offset: Option<u64> = None;
        let mut first_load_filesz: Option<u64> = None;
        for i in 0..e_phnum {
            let offset = ph_offset(i)?;
            let p_type = reader.u32(offset).map_err(|err| {
                crate::kerror!("load: failed to read p_type for {}: {:?}", i, err);
                "Invalid program header"
            })?;

            if p_type == PhType::Load as u32 {
                let p_vaddr = reader.u64(offset + 16).map_err(|err| {
                    crate::kerror!("load: failed to read p_vaddr for {}: {:?}", i, err);
                    "Invalid program header"
                })?;
                let p_offset = reader.u64(offset + 8).map_err(|err| {
                    crate::kerror!("load: failed to read p_offset for {}: {:?}", i, err);
                    "Invalid program header"
                })?;
                let p_filesz = reader.u64(offset + 32).map_err(|err| {
                    crate::kerror!("load: failed to read p_filesz for {}: {:?}", i, err);
                    "Invalid program header"
                })?;

                first_load_vaddr = Some(p_vaddr);
                first_load_offset = Some(p_offset);
                first_load_filesz = Some(p_filesz);
                break;
            }
        }

        let first_load_vaddr = first_load_vaddr.ok_or("ELF has no loadable segments")?;
        let first_load_offset = first_load_offset.ok_or("ELF has no loadable segments")?;
        let first_load_filesz = first_load_filesz.ok_or("ELF has no loadable segments")?;

        let mut phdr_runtime: Option<u64> = None;

        for i in 0..e_phnum {
            let offset = ph_offset(i)?;
            let p_type = reader.u32(offset).map_err(|err| {
                crate::kerror!("load: failed to read p_type for {}: {:?}", i, err);
                "Invalid program header"
            })?;
            let p_offset_val = reader.u64(offset + 8).map_err(|err| {
                crate::kerror!("load: failed to read p_offset for {}: {:?}", i, err);
                "Invalid program header"
            })? as usize;
            let p_vaddr = reader.u64(offset + 16).map_err(|err| {
                crate::kerror!("load: failed to read p_vaddr for {}: {:?}", i, err);
                "Invalid program header"
            })?;
            let p_filesz = reader.u64(offset + 32).map_err(|err| {
                crate::kerror!("load: failed to read p_filesz for {}: {:?}", i, err);
                "Invalid program header"
            })? as usize;
            let p_memsz = reader.u64(offset + 40).map_err(|err| {
                crate::kerror!("load: failed to read p_memsz for {}: {:?}", i, err);
                "Invalid program header"
            })? as usize;

            crate::kinfo!(
                "Segment {}: p_type={}, p_vaddr={:#x}, p_filesz={:#x}, p_memsz={:#x}",
                i,
                p_type,
                p_vaddr,
                p_filesz,
                p_memsz
            );

            if p_type != PhType::Load as u32 {
                if phdr_runtime.is_none() && p_type == PhType::Phdr as u32 {
                    // Use signed arithmetic to avoid overflow when p_vaddr < first_load_vaddr
                    let offset = p_vaddr as i64 - first_load_vaddr as i64;
                    let target_addr = (base_addr as i64 + offset) as u64;
                    phdr_runtime = Some(target_addr);
                }
                continue;
            }

            if p_vaddr < first_load_vaddr {
                return Err("Segment virtual address before base segment");
            }

            let relative_offset = p_vaddr - first_load_vaddr;
            let target_addr = base_addr + relative_offset;

            crate::kinfo!(
                "Loading segment p_vaddr={:#x}, p_filesz={:#x}, p_memsz={:#x}, target_addr={:#x}",
                p_vaddr,
                p_filesz,
                p_memsz,
                target_addr
            );

            if p_filesz > 0 {
                let segment = reader.bytes(p_offset_val, p_filesz).map_err(|err| {
                    crate::kerror!("load: segment {} source out of range: {:?}", i, err);
                    "Invalid program header"
                })?;

                let dst = target_addr as *mut u8;

                if p_memsz > 0 {
                    crate::kinfo!(
                        "Zeroing segment bytes: addr={:#x}, memsz={:#x}",
                        target_addr,
                        p_memsz
                    );
                    unsafe {
                        ptr::write_bytes(dst, 0, p_memsz);
                    }
                    crate::kinfo!("Zero complete at {:#x}", target_addr);
                }

                crate::kinfo!(
                    "Copying segment data: src_off={:#x}, dst={:#x}, size={:#x}",
                    p_offset_val,
                    target_addr,
                    p_filesz
                );
                unsafe {
                    ptr::copy_nonoverlapping(segment.as_ptr(), dst, p_filesz);
                }
                crate::kinfo!("Copy complete to {:#x}", target_addr);
            }

            if phdr_runtime.is_none() {
                let ph_table_offset = header.e_phoff as u64;
                let ph_table_size = (header.e_phentsize as u64) * (header.e_phnum as u64);
                if ph_table_size != 0 {
                    let seg_offset = p_offset_val as u64;
                    if ph_table_offset >= seg_offset
                        && ph_table_offset + ph_table_size <= seg_offset + p_filesz as u64
                    {
                        let within = ph_table_offset - seg_offset;
                        phdr_runtime = Some(target_addr + within);
                    }
                }
            }
        }

        let e_entry = reader.u64(24).map_err(|err| {
            crate::kerror!("load: failed to read entry point: {:?}", err);
            "Malformed ELF header"
        })?;

        // Calculate load_bias first (using signed arithmetic to avoid overflow)
        let load_bias = base_addr as i64 - first_load_vaddr as i64;

        // Use load_bias to relocate entry point safely
        let relocated_entry = (e_entry as i64 + load_bias) as u64;

        if phdr_runtime.is_none() {
            let ph_table_offset = header.e_phoff as u64;
            let ph_table_size = (header.e_phentsize as u64) * (header.e_phnum as u64);
            if ph_table_size != 0
                && ph_table_offset >= first_load_offset
                && ph_table_offset + ph_table_size <= first_load_offset + first_load_filesz
            {
                let within = ph_table_offset - first_load_offset;
                phdr_runtime = Some(base_addr + within);
            }
        }

        // Fallback: if we still haven't found PHDRs, but we have a base address,
        // assume they are at base_addr + e_phoff (common for PIE/executables)
        if phdr_runtime.is_none() {
            crate::kwarn!(
                "phdr_runtime not found via PT_PHDR or PT_LOAD scan, assuming base + e_phoff"
            );
            // Use signed arithmetic for safety
            let offset = header.e_phoff as i64;
            // If base_addr is 0 (e.g. first load of PIE), this is just offset
            // If base_addr is loaded address, this is loaded_addr + offset
            // Note: this assumes the ELF header and PHDRs are mapped at the beginning of the file
            // which is true for almost all standard ELF binaries.
            phdr_runtime = Some((base_addr as i64 + offset) as u64);
        }

        let phdr_vaddr = phdr_runtime.ok_or("Failed to locate program headers in memory")?;

        Ok(LoadResult {
            entry_point: relocated_entry,
            base_addr,
            first_load_vaddr,
            load_bias,
            phdr_vaddr,
            phentsize: header.e_phentsize,
            phnum: header.e_phnum,
        })
    }
}
