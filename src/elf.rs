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
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Header {
    pub e_ident: [u8; 16],      // ELF identification
    pub e_type: u16,            // Object file type
    pub e_machine: u16,         // Machine type
    pub e_version: u32,         // Object file version
    pub e_entry: u64,           // Entry point address
    pub e_phoff: u64,           // Program header offset
    pub e_shoff: u64,           // Section header offset
    pub e_flags: u32,           // Processor-specific flags
    pub e_ehsize: u16,          // ELF header size
    pub e_phentsize: u16,       // Size of program header entry
    pub e_phnum: u16,           // Number of program header entries
    pub e_shentsize: u16,       // Size of section header entry
    pub e_shnum: u16,           // Number of section header entries
    pub e_shstrndx: u16,        // Section name string table index
}

/// ELF64 program header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64ProgramHeader {
    pub p_type: u32,            // Segment type
    pub p_flags: u32,           // Segment flags
    pub p_offset: u64,          // Segment file offset
    pub p_vaddr: u64,           // Segment virtual address
    pub p_paddr: u64,           // Segment physical address
    pub p_filesz: u64,          // Segment size in file
    pub p_memsz: u64,           // Segment size in memory
    pub p_align: u64,           // Segment alignment
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
    header: &'static Elf64Header,
}

impl ElfLoader {
    /// Create a new ELF loader from raw bytes
    pub fn new(data: &'static [u8]) -> Result<Self, &'static str> {
        crate::kinfo!("ElfLoader::new called with {} bytes", data.len());
        if data.len() < core::mem::size_of::<Elf64Header>() {
            crate::kerror!("Data too small for ELF header");
            return Err("Data too small for ELF header");
        }

        let header = unsafe { &*(data.as_ptr() as *const Elf64Header) };
        crate::kinfo!("ELF magic: {:#x}", header.e_ident[0] as u32 | 
            (header.e_ident[1] as u32) << 8 | 
            (header.e_ident[2] as u32) << 16 | 
            (header.e_ident[3] as u32) << 24);

        if !header.is_valid() {
            crate::kerror!("Invalid ELF header");
            return Err("Invalid ELF header");
        }

        crate::kinfo!("ELF header is valid");
        Ok(Self { data, header })
    }

    /// Get the ELF header
    pub fn header(&self) -> &Elf64Header {
        self.header
    }

    /// Get program headers
    pub fn program_headers(&self) -> &[Elf64ProgramHeader] {
        let offset = self.header.e_phoff as usize;
        let count = self.header.e_phnum as usize;
        let size = self.header.e_phentsize as usize;

        if size != core::mem::size_of::<Elf64ProgramHeader>() {
            return &[];
        }

        let ptr = unsafe { self.data.as_ptr().add(offset) as *const Elf64ProgramHeader };
        unsafe { slice::from_raw_parts(ptr, count) }
    }

    /// Load the ELF into memory at the specified base address
    /// For position-independent executables, base_addr is used as offset
    /// For static executables (with absolute addresses), segments are loaded at their p_vaddr
    pub fn load(&self, base_addr: u64) -> Result<u64, &'static str> {
        crate::kinfo!("Loading ELF with {} program headers", self.header.e_phnum);
        
        for (i, ph) in self.program_headers().iter().enumerate() {
            crate::kinfo!("  Program header {}: type={:#x}, vaddr={:#x}, filesz={:#x}", 
                i, ph.p_type, ph.p_vaddr, ph.p_filesz);
            if ph.p_type != PhType::Load as u32 {
                crate::kinfo!("    Skipping non-LOAD segment (type {:#x})", ph.p_type);
                continue;
            }

            // Always relocate to user space base address
            let vaddr = base_addr + ph.p_vaddr;
            let offset = ph.p_offset as usize;
            let filesz = ph.p_filesz as usize;
            let memsz = ph.p_memsz as usize;
            
            crate::kinfo!("  Loading segment: vaddr={:#x}, filesz={:#x}, memsz={:#x}", 
                vaddr, filesz, memsz);

            if offset + filesz > self.data.len() {
                return Err("Program header extends beyond file");
            }

            // Copy file data to memory
            let dest = vaddr as *mut u8;
            let src = &self.data[offset..offset + filesz];

            unsafe {
                core::ptr::copy_nonoverlapping(src.as_ptr(), dest, filesz);

                // Zero out BSS section
                if memsz > filesz {
                    core::ptr::write_bytes(dest.add(filesz), 0, memsz - filesz);
                }
            }
            
            crate::kinfo!("  Loaded {} bytes to {:#x}", filesz, vaddr);
        }

        let entry = self.header.entry_point();
        crate::kinfo!("ELF entry point: {:#x}", entry);
        Ok(base_addr + entry)
    }
}
