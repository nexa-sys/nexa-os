/// ELF loader implementation following POSIX and Unix-like standards
use core::slice;

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
    data: &'static [u8],
}

impl ElfLoader {
    /// Create a new ELF loader from raw bytes
    pub fn new(data: &'static [u8]) -> Result<Self, &'static str> {
        use core::ptr;

        crate::kinfo!("ElfLoader::new called with {} bytes", data.len());
        if data.len() < 64 {
            crate::kerror!("Data too small for ELF header");
            return Err("Data too small for ELF header");
        }

        // Check magic
        let magic = unsafe { ptr::read_unaligned(data.as_ptr() as *const u32) };
        if magic != ELF_MAGIC {
            crate::kerror!("Invalid ELF magic: {:#x}", magic);
            return Err("Invalid ELF magic");
        }

        // Check class (64-bit)
        let class = unsafe { ptr::read_unaligned(data.as_ptr().add(4) as *const u8) };
        if class != ElfClass::Elf64 as u8 {
            crate::kerror!("Not ELF64");
            return Err("Not ELF64");
        }

        // Check data encoding (little endian)
        let data_enc = unsafe { ptr::read_unaligned(data.as_ptr().add(5) as *const u8) };
        if data_enc != ElfData::LittleEndian as u8 {
            crate::kerror!("Not little endian");
            return Err("Not little endian");
        }

        // Check version
        let version = unsafe { ptr::read_unaligned(data.as_ptr().add(6) as *const u8) };
        if version != 1 {
            crate::kerror!("Invalid version");
            return Err("Invalid version");
        }

        // Check machine type (x86-64)
        let machine = unsafe { ptr::read_unaligned(data.as_ptr().add(18) as *const u16) };
        if machine != 0x3E {
            crate::kerror!("Not x86-64");
            return Err("Not x86-64");
        }

        crate::kinfo!("ELF header is valid");

        // Read header fields
        let e_phoff = unsafe { ptr::read_unaligned(data.as_ptr().add(32) as *const u64) };
        let e_phentsize = unsafe { ptr::read_unaligned(data.as_ptr().add(54) as *const u16) };
        let e_phnum = unsafe { ptr::read_unaligned(data.as_ptr().add(56) as *const u16) };
        let e_entry = unsafe { ptr::read_unaligned(data.as_ptr().add(24) as *const u64) };

        crate::kinfo!(
            "ELF header: e_phoff={:#x}, e_phnum={}, e_phentsize={}, e_entry={:#x}",
            e_phoff,
            e_phnum,
            e_phentsize,
            e_entry
        );

        Ok(Self { data })
    }

    /// Get the ELF header
    pub fn header(&self) -> &Elf64Header {
        unsafe { &*(self.data.as_ptr() as *const Elf64Header) }
    }

    /// Get program headers
    pub fn program_headers(&self) -> &[Elf64ProgramHeader] {
        use core::ptr;

        let e_phoff =
            unsafe { ptr::read_unaligned(self.data.as_ptr().add(32) as *const u64) } as usize;
        let e_phnum =
            unsafe { ptr::read_unaligned(self.data.as_ptr().add(56) as *const u16) } as usize;
        let e_phentsize =
            unsafe { ptr::read_unaligned(self.data.as_ptr().add(54) as *const u16) } as usize;

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

        if e_phoff + (e_phnum * e_phentsize) > self.data.len() {
            crate::kinfo!(
                "program_headers: offset + size * count ({}) > data.len() ({})",
                e_phoff + (e_phnum * e_phentsize),
                self.data.len()
            );
            return &[];
        }

        let ptr = unsafe { self.data.as_ptr().add(e_phoff) as *const Elf64ProgramHeader };
        let slice = unsafe { slice::from_raw_parts(ptr, e_phnum) };
        crate::kinfo!("program_headers: returning {} headers", slice.len());
        slice
    }

    /// Load the ELF into memory at the specified base address
    /// For position-independent executables, base_addr is used as offset
    /// For static executables (with absolute addresses), segments are loaded at their p_vaddr
    pub fn load(&self, base_addr: u64) -> Result<u64, &'static str> {
        use core::ptr;

        let e_phoff =
            unsafe { ptr::read_unaligned(self.data.as_ptr().add(32) as *const u64) } as usize;
        let e_phnum =
            unsafe { ptr::read_unaligned(self.data.as_ptr().add(56) as *const u16) } as usize;
        let e_phentsize =
            unsafe { ptr::read_unaligned(self.data.as_ptr().add(54) as *const u16) } as usize;

        // Determine the base virtual address of the first loadable segment so we
        // can relocate all program segments relative to it. This lets us place
        // the executable at an arbitrary physical base while preserving the
        // virtual addresses expected by the binary.
        let mut first_load_vaddr: Option<u64> = None;
        for i in 0..e_phnum {
            let ph_offset = e_phoff + i * e_phentsize;
            if ph_offset + 56 > self.data.len() {
                continue;
            }

            let p_type =
                unsafe { ptr::read_unaligned(self.data.as_ptr().add(ph_offset) as *const u32) };
            if p_type == PhType::Load as u32 {
                let p_vaddr = unsafe {
                    ptr::read_unaligned(self.data.as_ptr().add(ph_offset + 16) as *const u64)
                };
                first_load_vaddr = Some(p_vaddr);
                break;
            }
        }

        let first_load_vaddr = first_load_vaddr.ok_or("ELF has no loadable segments")?;

        for i in 0..e_phnum {
            let ph_offset = e_phoff + i * e_phentsize;
            if ph_offset + 56 > self.data.len() {
                continue;
            }

            // Read program header fields directly from bytes
            let p_type =
                unsafe { ptr::read_unaligned(self.data.as_ptr().add(ph_offset) as *const u32) };
            let p_offset_val =
                unsafe { ptr::read_unaligned(self.data.as_ptr().add(ph_offset + 8) as *const u64) }
                    as usize;
            let p_vaddr = unsafe {
                ptr::read_unaligned(self.data.as_ptr().add(ph_offset + 16) as *const u64)
            };
            let p_filesz = unsafe {
                ptr::read_unaligned(self.data.as_ptr().add(ph_offset + 32) as *const u64)
            } as usize;
            let p_memsz = unsafe {
                ptr::read_unaligned(self.data.as_ptr().add(ph_offset + 40) as *const u64)
            } as usize;

            crate::kinfo!(
                "Segment {}: p_type={}, p_vaddr={:#x}, p_filesz={:#x}, p_memsz={:#x}",
                i,
                p_type,
                p_vaddr,
                p_filesz,
                p_memsz
            );

            if p_type != PhType::Load as u32 {
                continue;
            }

            let base_vaddr = first_load_vaddr;
            if p_vaddr < base_vaddr {
                return Err("Segment virtual address before base segment");
            }

            // Relocate relative to the first loadable segment so the binary can
            // run at its intended virtual addresses while we choose the
            // physical placement.
            let relative_offset = p_vaddr - base_vaddr;
            let target_addr = base_addr + relative_offset;

            crate::kinfo!(
                "Loading segment p_vaddr={:#x}, p_filesz={:#x}, p_memsz={:#x}, target_addr={:#x}",
                p_vaddr,
                p_filesz,
                p_memsz,
                target_addr
            );

            // Copy data from ELF to memory
            if p_filesz > 0 {
                if p_offset_val + p_filesz > self.data.len() {
                    return Err("Invalid program header");
                }

                let src = unsafe { self.data.as_ptr().add(p_offset_val) };
                let dst = target_addr as *mut u8;

                // Zero out the memory first
                if p_memsz > 0 {
                    crate::kinfo!(
                        "Zeroing segment bytes: addr={:#x}, memsz={:#x}",
                        target_addr,
                        p_memsz
                    );
                    unsafe {
                        core::ptr::write_bytes(dst, 0, p_memsz);
                    }
                    crate::kinfo!("Zero complete at {:#x}", target_addr);
                }

                // Copy the file data
                crate::kinfo!(
                    "Copying segment data: src_off={:#x}, dst={:#x}, size={:#x}",
                    p_offset_val,
                    target_addr,
                    p_filesz
                );
                unsafe {
                    core::ptr::copy_nonoverlapping(src, dst, p_filesz);
                }
                crate::kinfo!("Copy complete to {:#x}", target_addr);
            }
        }

        // Get entry point and relocate it
        let e_entry = unsafe { ptr::read_unaligned(self.data.as_ptr().add(24) as *const u64) };

        // For static executables, entry point is absolute virtual address
        // We need to relocate it to our user space base address
        // Use the first load segment as the base for relocation
        let relocated_entry = base_addr + (e_entry - first_load_vaddr);

        Ok(relocated_entry)
    }
}
