//! Kernel Module (KMod) support for NexaOS
//!
//! This module provides infrastructure for loading and managing kernel modules
//! (.nkm files), similar to Linux's .ko modules.
//!
//! # NKM File Format
//!
//! NexaOS supports two module formats:
//!
//! ## Simple NKM Format (metadata-only)
//! - Header with magic number "NKM\x01", version, and metadata
//! - Used for built-in modules that are already linked into the kernel
//!
//! ## ELF-based NKM Format (loadable code)
//! - Standard ELF relocatable object (.o) or shared object
//! - Position-independent code with relocations
//! - Symbol table for kernel API bindings
//! - Loaded and dynamically linked at runtime
//!
//! # Module Lifecycle
//!
//! 1. Load: Read .nkm from initramfs or filesystem
//! 2. Verify: Check format (ELF or simple NKM)
//! 3. For ELF modules:
//!    a. Allocate memory for code/data sections
//!    b. Load sections and apply relocations
//!    c. Resolve external symbols from kernel symbol table
//! 4. Register: Add to kernel module list
//! 5. Init: Call module's init function
//! 6. Unload (optional): Call cleanup and remove

pub mod elf;
pub mod symbols;

use spin::Mutex;

/// NKM file magic number: "NKM\x01"
pub const NKM_MAGIC: [u8; 4] = [b'N', b'K', b'M', 0x01];

/// NKM format version
pub const NKM_VERSION: u8 = 1;

/// Maximum number of loaded modules
pub const MAX_MODULES: usize = 32;

/// Maximum module name length
pub const MAX_MODULE_NAME: usize = 32;

/// Maximum module dependencies
pub const MAX_DEPENDENCIES: usize = 8;

/// Module state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    /// Module is loaded but not initialized
    Loaded,
    /// Module is initialized and running
    Running,
    /// Module is being unloaded
    Unloading,
    /// Module encountered an error
    Error,
}

/// Module type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ModuleType {
    /// Filesystem driver
    Filesystem = 1,
    /// Block device driver
    BlockDevice = 2,
    /// Character device driver
    CharDevice = 3,
    /// Network driver
    Network = 4,
    /// Other module type
    Other = 255,
}

impl ModuleType {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => ModuleType::Filesystem,
            2 => ModuleType::BlockDevice,
            3 => ModuleType::CharDevice,
            4 => ModuleType::Network,
            _ => ModuleType::Other,
        }
    }
}

/// NKM file header (on-disk format)
/// Total size: 80 bytes
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct NkmHeader {
    /// Magic number: "NKM\x01"
    pub magic: [u8; 4],
    /// Format version
    pub version: u8,
    /// Module type
    pub module_type: u8,
    /// Number of dependencies
    pub dep_count: u8,
    /// Reserved for future use
    pub flags: u8,
    /// Offset to code section from start of file
    pub code_offset: u32,
    /// Size of code section in bytes
    pub code_size: u32,
    /// Offset to init function within code section
    pub init_offset: u32,
    /// Size of init function
    pub init_size: u32,
    /// Reserved for alignment
    pub reserved: [u8; 8],
    /// Offset to string table
    pub string_table_offset: u32,
    /// Size of string table
    pub string_table_size: u32,
    /// Module name (null-terminated)
    pub name: [u8; MAX_MODULE_NAME],
}

impl NkmHeader {
    /// Parse NKM header from raw bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < core::mem::size_of::<Self>() {
            return None;
        }

        let magic = [data[0], data[1], data[2], data[3]];
        if magic != NKM_MAGIC {
            return None;
        }

        let version = data[4];
        if version != NKM_VERSION {
            return None;
        }

        let module_type = data[5];
        let dep_count = data[6];
        let flags = data[7];
        let code_offset = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let code_size = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        let init_offset = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        let init_size = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
        let mut reserved = [0u8; 8];
        reserved.copy_from_slice(&data[24..32]);
        let string_table_offset = u32::from_le_bytes([data[32], data[33], data[34], data[35]]);
        let string_table_size = u32::from_le_bytes([data[36], data[37], data[38], data[39]]);
        let mut name = [0u8; MAX_MODULE_NAME];
        name.copy_from_slice(&data[40..40 + MAX_MODULE_NAME]);

        Some(Self {
            magic,
            version,
            module_type,
            dep_count,
            flags,
            code_offset,
            code_size,
            init_offset,
            init_size,
            reserved,
            string_table_offset,
            string_table_size,
            name,
        })
    }

    /// Get module name as string
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&c| c == 0).unwrap_or(MAX_MODULE_NAME);
        core::str::from_utf8(&self.name[..end]).unwrap_or("unknown")
    }

    /// Get module type
    pub fn module_type(&self) -> ModuleType {
        ModuleType::from_u8(self.module_type)
    }
}

/// Module metadata extracted from NKM file
#[derive(Clone, Copy)]
pub struct ModuleInfo {
    /// Module name
    pub name: [u8; MAX_MODULE_NAME],
    /// Module version string
    pub version: [u8; 16],
    /// Module description
    pub description: [u8; 64],
    /// Module type
    pub module_type: ModuleType,
    /// Current state
    pub state: ModuleState,
    /// Data pointer (for module-specific data)
    pub data_ptr: usize,
}

impl ModuleInfo {
    const fn empty() -> Self {
        Self {
            name: [0; MAX_MODULE_NAME],
            version: [0; 16],
            description: [0; 64],
            module_type: ModuleType::Other,
            state: ModuleState::Loaded,
            data_ptr: 0,
        }
    }

    /// Get module name as string
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&c| c == 0).unwrap_or(MAX_MODULE_NAME);
        core::str::from_utf8(&self.name[..end]).unwrap_or("unknown")
    }

    /// Get version as string
    pub fn version_str(&self) -> &str {
        let end = self.version.iter().position(|&c| c == 0).unwrap_or(16);
        core::str::from_utf8(&self.version[..end]).unwrap_or("0.0.0")
    }

    /// Get description as string
    pub fn description_str(&self) -> &str {
        let end = self.description.iter().position(|&c| c == 0).unwrap_or(64);
        core::str::from_utf8(&self.description[..end]).unwrap_or("")
    }
}

/// Module registry
struct ModuleRegistry {
    modules: [Option<ModuleInfo>; MAX_MODULES],
    count: usize,
}

impl ModuleRegistry {
    const fn new() -> Self {
        const NONE: Option<ModuleInfo> = None;
        Self {
            modules: [NONE; MAX_MODULES],
            count: 0,
        }
    }

    fn register(&mut self, info: ModuleInfo) -> Result<usize, ModuleError> {
        if self.count >= MAX_MODULES {
            return Err(ModuleError::TooManyModules);
        }

        // Check for duplicate
        let name = info.name_str();
        for slot in self.modules.iter() {
            if let Some(existing) = slot {
                if existing.name_str() == name {
                    return Err(ModuleError::AlreadyLoaded);
                }
            }
        }

        // Find empty slot
        for (idx, slot) in self.modules.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(info);
                self.count += 1;
                return Ok(idx);
            }
        }

        Err(ModuleError::TooManyModules)
    }

    fn find(&self, name: &str) -> Option<&ModuleInfo> {
        for slot in self.modules.iter() {
            if let Some(info) = slot {
                if info.name_str() == name {
                    return Some(info);
                }
            }
        }
        None
    }

    fn find_mut(&mut self, name: &str) -> Option<&mut ModuleInfo> {
        for slot in self.modules.iter_mut() {
            if let Some(info) = slot {
                if info.name_str() == name {
                    return Some(info);
                }
            }
        }
        None
    }

    fn unregister(&mut self, name: &str) -> Result<(), ModuleError> {
        for slot in self.modules.iter_mut() {
            if let Some(info) = slot {
                if info.name_str() == name {
                    *slot = None;
                    self.count -= 1;
                    return Ok(());
                }
            }
        }
        Err(ModuleError::NotFound)
    }
}

static MODULE_REGISTRY: Mutex<ModuleRegistry> = Mutex::new(ModuleRegistry::new());

/// Module loading errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleError {
    /// Invalid magic number
    InvalidMagic,
    /// Unsupported version
    UnsupportedVersion,
    /// File too small
    FileTooSmall,
    /// Module already loaded
    AlreadyLoaded,
    /// Too many modules loaded
    TooManyModules,
    /// Module not found
    NotFound,
    /// Missing dependency
    MissingDependency,
    /// Initialization failed
    InitFailed,
    /// Invalid module format
    InvalidFormat,
    /// Symbol not found during relocation
    SymbolNotFound,
    /// Memory allocation failed
    AllocationFailed,
    /// Relocation failed
    RelocationFailed,
}

impl From<elf::LoaderError> for ModuleError {
    fn from(e: elf::LoaderError) -> Self {
        match e {
            elf::LoaderError::InvalidMagic => ModuleError::InvalidMagic,
            elf::LoaderError::InvalidClass => ModuleError::InvalidFormat,
            elf::LoaderError::InvalidMachine => ModuleError::InvalidFormat,
            elf::LoaderError::InvalidType => ModuleError::InvalidFormat,
            elf::LoaderError::SectionOutOfBounds => ModuleError::InvalidFormat,
            elf::LoaderError::SymbolNotFound => ModuleError::SymbolNotFound,
            elf::LoaderError::RelocationFailed => ModuleError::RelocationFailed,
            elf::LoaderError::AllocationFailed => ModuleError::AllocationFailed,
            elf::LoaderError::InvalidSection => ModuleError::InvalidFormat,
            elf::LoaderError::UnsupportedRelocation => ModuleError::RelocationFailed,
        }
    }
}

/// Initialize the kmod subsystem
pub fn init() {
    // Initialize kernel symbol table first
    symbols::init();
    crate::kinfo!("Kernel module system initialized (max {} modules)", MAX_MODULES);
}

/// Check if data is an ELF file
fn is_elf(data: &[u8]) -> bool {
    data.len() >= 4 && data[0..4] == [0x7F, b'E', b'L', b'F']
}

/// Load a kernel module from NKM data (supports both simple NKM and ELF formats)
pub fn load_module(data: &[u8]) -> Result<(), ModuleError> {
    if data.len() < 4 {
        return Err(ModuleError::FileTooSmall);
    }

    // Check if this is an ELF module
    if is_elf(data) {
        return load_elf_module(data);
    }

    // Otherwise, try simple NKM format
    load_simple_nkm(data)
}

/// Load an ELF-based kernel module
fn load_elf_module(data: &[u8]) -> Result<(), ModuleError> {
    crate::kinfo!("Loading ELF kernel module ({} bytes)", data.len());

    // Load the ELF module
    let loaded = elf::load_elf_module(data)?;
    
    crate::kinfo!(
        "ELF module loaded at {:#x}, size {} bytes",
        loaded.base,
        loaded.size
    );

    // Create module info
    let mut info = ModuleInfo::empty();
    
    // Try to extract module name from special section or use default
    let name = b"elf_module";
    let name_len = name.len().min(MAX_MODULE_NAME - 1);
    info.name[..name_len].copy_from_slice(&name[..name_len]);
    info.module_type = ModuleType::Other;
    info.state = ModuleState::Loaded;
    info.data_ptr = loaded.base;

    // Copy version
    let version = b"1.0.0";
    info.version[..version.len()].copy_from_slice(version);

    // Register module
    let _idx = MODULE_REGISTRY.lock().register(info)?;

    // Call module init function if present
    if loaded.init_fn.is_some() {
        crate::kinfo!("Calling module init function...");
        match loaded.init() {
            Ok(ret) => {
                if ret != 0 {
                    crate::kwarn!("Module init returned error code: {}", ret);
                    return Err(ModuleError::InitFailed);
                }
                crate::kinfo!("Module init completed successfully");
            }
            Err(_) => {
                crate::kwarn!("Module has no init function");
            }
        }
    }

    // Mark as running
    {
        let mut registry = MODULE_REGISTRY.lock();
        for slot in registry.modules.iter_mut() {
            if let Some(mod_info) = slot {
                if mod_info.data_ptr == loaded.base {
                    mod_info.state = ModuleState::Running;
                    break;
                }
            }
        }
    }

    crate::kinfo!("ELF module loaded successfully");
    Ok(())
}

/// Load a simple NKM format module (metadata only)
fn load_simple_nkm(data: &[u8]) -> Result<(), ModuleError> {
    // Parse header
    let header = NkmHeader::parse(data).ok_or(ModuleError::InvalidFormat)?;

    crate::kinfo!(
        "Loading kernel module: {} (type: {:?})",
        header.name_str(),
        header.module_type()
    );

    // Parse string table for version and description
    let mut info = ModuleInfo::empty();
    info.name.copy_from_slice(&header.name);
    info.module_type = header.module_type();
    info.state = ModuleState::Loaded;

    // Extract version and description from string table if present
    if header.string_table_offset > 0 && header.string_table_size > 0 {
        let str_start = header.string_table_offset as usize;
        let str_end = str_start + header.string_table_size as usize;
        if str_end <= data.len() {
            let str_data = &data[str_start..str_end];
            // String table format: version\0description\0
            if let Some(null_pos) = str_data.iter().position(|&c| c == 0) {
                let version_len = null_pos.min(15);
                info.version[..version_len].copy_from_slice(&str_data[..version_len]);

                if null_pos + 1 < str_data.len() {
                    let desc_start = null_pos + 1;
                    let desc_end = str_data[desc_start..]
                        .iter()
                        .position(|&c| c == 0)
                        .map(|p| desc_start + p)
                        .unwrap_or(str_data.len());
                    let desc_len = (desc_end - desc_start).min(63);
                    info.description[..desc_len]
                        .copy_from_slice(&str_data[desc_start..desc_start + desc_len]);
                }
            }
        }
    }

    // Register module
    let _idx = MODULE_REGISTRY.lock().register(info)?;

    // Mark as running
    if let Some(mod_info) = MODULE_REGISTRY.lock().find_mut(header.name_str()) {
        mod_info.state = ModuleState::Running;
    }

    crate::kinfo!(
        "Module '{}' loaded successfully (version: {})",
        header.name_str(),
        info.version_str()
    );

    Ok(())
}

/// Check if a module is loaded
pub fn is_loaded(name: &str) -> bool {
    MODULE_REGISTRY.lock().find(name).is_some()
}

/// Get module info
pub fn get_module_info(name: &str) -> Option<ModuleInfo> {
    MODULE_REGISTRY.lock().find(name).copied()
}

/// Unload a module
pub fn unload_module(name: &str) -> Result<(), ModuleError> {
    crate::kinfo!("Unloading kernel module: {}", name);

    // Set state to unloading
    {
        let mut registry = MODULE_REGISTRY.lock();
        if let Some(info) = registry.find_mut(name) {
            info.state = ModuleState::Unloading;
        } else {
            return Err(ModuleError::NotFound);
        }
    }

    // Remove from registry
    MODULE_REGISTRY.lock().unregister(name)?;

    crate::kinfo!("Module '{}' unloaded", name);
    Ok(())
}

/// List all loaded modules
pub fn list_modules() -> alloc::vec::Vec<ModuleInfo> {
    let registry = MODULE_REGISTRY.lock();
    let mut result = alloc::vec::Vec::new();
    for slot in registry.modules.iter() {
        if let Some(info) = slot {
            result.push(*info);
        }
    }
    result
}

/// Load a module from initramfs by name
pub fn load_from_initramfs(name: &str) -> Result<(), ModuleError> {
    // Construct path: /lib/modules/<name>.nkm
    let mut path_buf = [0u8; 64];
    let prefix = b"/lib/modules/";
    let suffix = b".nkm";

    let prefix_len = prefix.len();
    let name_len = name.len().min(32);
    let suffix_len = suffix.len();
    let total_len = prefix_len + name_len + suffix_len;

    if total_len >= 64 {
        return Err(ModuleError::InvalidFormat);
    }

    path_buf[..prefix_len].copy_from_slice(prefix);
    path_buf[prefix_len..prefix_len + name_len].copy_from_slice(&name.as_bytes()[..name_len]);
    path_buf[prefix_len + name_len..total_len].copy_from_slice(suffix);

    let path = core::str::from_utf8(&path_buf[..total_len]).map_err(|_| ModuleError::InvalidFormat)?;

    crate::kinfo!("Loading module from initramfs: {}", path);

    // Try to read from initramfs
    if let Some(initramfs) = crate::fs::get_initramfs() {
        if let Some(entry) = initramfs.lookup(path) {
            return load_module(entry.data());
        }
    }

    crate::kwarn!("Module file not found: {}", path);
    Err(ModuleError::NotFound)
}

/// Load all modules from /lib/modules in initramfs
pub fn load_initramfs_modules() {
    crate::kinfo!("Scanning initramfs for kernel modules...");

    if let Some(initramfs) = crate::fs::get_initramfs() {
        // List /lib/modules directory
        let mut found_count = 0;
        let mut loaded_count = 0;

        initramfs.for_each("/lib/modules", |name, _metadata| {
            if name.ends_with(".nkm") {
                found_count += 1;
                // Extract module name (without .nkm extension)
                let mod_name = &name[..name.len() - 4];
                match load_from_initramfs(mod_name) {
                    Ok(()) => {
                        loaded_count += 1;
                    }
                    Err(e) => {
                        crate::kwarn!("Failed to load module '{}': {:?}", mod_name, e);
                    }
                }
            }
        });

        crate::kinfo!(
            "Module scan complete: {} found, {} loaded",
            found_count,
            loaded_count
        );
    } else {
        crate::kwarn!("Initramfs not available, skipping module load");
    }
}

/// Generate an NKM file for a built-in module (for packaging)
pub fn generate_nkm(
    name: &str,
    module_type: ModuleType,
    version: &str,
    description: &str,
) -> alloc::vec::Vec<u8> {
    let mut data = alloc::vec::Vec::new();

    // Header
    data.extend_from_slice(&NKM_MAGIC);
    data.push(NKM_VERSION);
    data.push(module_type as u8);
    data.push(0); // dep_count
    data.push(0); // flags

    // String table offset (after header, at byte 80)
    let header_size = 80u32;
    data.extend_from_slice(&header_size.to_le_bytes()); // code_offset (placeholder)
    data.extend_from_slice(&0u32.to_le_bytes()); // code_size

    // init_offset and init_size
    data.extend_from_slice(&header_size.to_le_bytes()); // init_offset
    let string_len = (version.len() + 1 + description.len() + 1) as u32;
    data.extend_from_slice(&string_len.to_le_bytes()); // init_size (reused for string len)

    // Reserved
    data.extend_from_slice(&[0u8; 8]);

    // String table offset and size
    data.extend_from_slice(&header_size.to_le_bytes());
    data.extend_from_slice(&string_len.to_le_bytes());

    // Module name (32 bytes)
    let mut name_buf = [0u8; MAX_MODULE_NAME];
    let name_len = name.len().min(MAX_MODULE_NAME - 1);
    name_buf[..name_len].copy_from_slice(&name.as_bytes()[..name_len]);
    data.extend_from_slice(&name_buf);

    // Pad header to 80 bytes if needed
    while data.len() < header_size as usize {
        data.push(0);
    }

    // String table: version\0description\0
    data.extend_from_slice(version.as_bytes());
    data.push(0);
    data.extend_from_slice(description.as_bytes());
    data.push(0);

    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_parse_nkm() {
        let data = generate_nkm("ext2", ModuleType::Filesystem, "1.0.0", "ext2 filesystem driver");
        let header = NkmHeader::parse(&data).expect("parse failed");
        assert_eq!(header.name_str(), "ext2");
        assert_eq!(header.module_type(), ModuleType::Filesystem);
    }
}
