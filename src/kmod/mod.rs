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
//!
//! # Module Features
//!
//! - **Dependency Management**: Modules can declare dependencies on other modules
//! - **Parameters**: Modules can accept parameters at load time
//! - **Reference Counting**: Prevents unloading modules still in use
//! - **License Tracking**: Modules declare their license for compatibility
//! - **Tainting**: Non-GPL modules taint the kernel

pub mod elf;
pub mod symbols;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};

/// NKM file magic number: "NKM\x01"
pub const NKM_MAGIC: [u8; 4] = [b'N', b'K', b'M', 0x01];

/// NKM format version
pub const NKM_VERSION: u8 = 1;

/// Extended NKM format version (with parameters and dependencies)
pub const NKM_VERSION_EXT: u8 = 2;

/// Maximum number of loaded modules
pub const MAX_MODULES: usize = 64;

/// Maximum module name length
pub const MAX_MODULE_NAME: usize = 32;

/// Maximum module dependencies
pub const MAX_DEPENDENCIES: usize = 8;

/// Maximum module parameters
pub const MAX_PARAMETERS: usize = 16;

/// Maximum parameter name length
pub const MAX_PARAM_NAME: usize = 32;

/// Maximum parameter value length
pub const MAX_PARAM_VALUE: usize = 128;

/// Maximum author/license string length
pub const MAX_INFO_STRING: usize = 64;

/// Module state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    /// Module is loaded but not initialized
    Loaded,
    /// Module is being initialized
    Initializing,
    /// Module is initialized and running
    Running,
    /// Module is being unloaded
    Unloading,
    /// Module encountered an error
    Error,
    /// Module is waiting for dependencies
    WaitingDeps,
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
    /// Input driver
    Input = 5,
    /// Graphics driver
    Graphics = 6,
    /// Sound driver
    Sound = 7,
    /// Security module
    Security = 8,
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
            5 => ModuleType::Input,
            6 => ModuleType::Graphics,
            7 => ModuleType::Sound,
            8 => ModuleType::Security,
            _ => ModuleType::Other,
        }
    }
}

/// Module license type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ModuleLicense {
    /// GPL v2
    GPL = 1,
    /// GPL v2 or later
    GPLPlus = 2,
    /// Dual BSD/GPL
    DualBsdGpl = 3,
    /// Dual MIT/GPL
    DualMitGpl = 4,
    /// Dual MPL/GPL
    DualMplGpl = 5,
    /// Proprietary
    Proprietary = 6,
    /// Unknown license
    Unknown = 255,
}

impl ModuleLicense {
    pub fn from_str(s: &str) -> Self {
        match s {
            "GPL" | "GPL v2" => ModuleLicense::GPL,
            "GPL v2+" | "GPL+" => ModuleLicense::GPLPlus,
            "Dual BSD/GPL" => ModuleLicense::DualBsdGpl,
            "Dual MIT/GPL" => ModuleLicense::DualMitGpl,
            "Dual MPL/GPL" => ModuleLicense::DualMplGpl,
            "Proprietary" => ModuleLicense::Proprietary,
            _ => ModuleLicense::Unknown,
        }
    }

    pub fn is_gpl_compatible(&self) -> bool {
        matches!(
            self,
            ModuleLicense::GPL
                | ModuleLicense::GPLPlus
                | ModuleLicense::DualBsdGpl
                | ModuleLicense::DualMitGpl
                | ModuleLicense::DualMplGpl
        )
    }
}

/// Module parameter type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ParamType {
    /// Boolean parameter
    Bool = 1,
    /// Integer parameter
    Int = 2,
    /// Unsigned integer parameter
    UInt = 3,
    /// String parameter
    String = 4,
}

/// Module parameter definition
#[derive(Clone, Copy)]
pub struct ModuleParam {
    /// Parameter name
    pub name: [u8; MAX_PARAM_NAME],
    /// Parameter type
    pub param_type: ParamType,
    /// Current value (stored as bytes)
    pub value: [u8; MAX_PARAM_VALUE],
    /// Value length
    pub value_len: usize,
    /// Description
    pub description: [u8; 64],
}

impl ModuleParam {
    pub const fn empty() -> Self {
        Self {
            name: [0; MAX_PARAM_NAME],
            param_type: ParamType::Int,
            value: [0; MAX_PARAM_VALUE],
            value_len: 0,
            description: [0; 64],
        }
    }

    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&c| c == 0).unwrap_or(MAX_PARAM_NAME);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }

    pub fn value_str(&self) -> &str {
        core::str::from_utf8(&self.value[..self.value_len]).unwrap_or("")
    }

    pub fn value_as_int(&self) -> Option<i64> {
        self.value_str().parse().ok()
    }

    pub fn value_as_bool(&self) -> Option<bool> {
        match self.value_str() {
            "1" | "true" | "yes" | "Y" | "y" => Some(true),
            "0" | "false" | "no" | "N" | "n" => Some(false),
            _ => None,
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
    /// Module author
    pub author: [u8; MAX_INFO_STRING],
    /// Module license
    pub license: ModuleLicense,
    /// Module type
    pub module_type: ModuleType,
    /// Current state
    pub state: ModuleState,
    /// Data pointer (for module-specific data)
    pub data_ptr: usize,
    /// Module size in memory
    pub size: usize,
    /// Reference count (number of users)
    pub refcount: u32,
    /// Dependencies (module names)
    pub dependencies: [[u8; MAX_MODULE_NAME]; MAX_DEPENDENCIES],
    /// Number of dependencies
    pub dep_count: u8,
    /// Module parameters
    pub params: [ModuleParam; MAX_PARAMETERS],
    /// Number of parameters
    pub param_count: u8,
    /// Init function address
    pub init_fn: Option<u64>,
    /// Exit function address
    pub exit_fn: Option<u64>,
    /// Whether this module taints the kernel
    pub taints_kernel: bool,
    /// Load timestamp (ticks since boot)
    pub load_time: u64,
}

impl ModuleInfo {
    const fn empty() -> Self {
        const EMPTY_DEP: [u8; MAX_MODULE_NAME] = [0; MAX_MODULE_NAME];
        Self {
            name: [0; MAX_MODULE_NAME],
            version: [0; 16],
            description: [0; 64],
            author: [0; MAX_INFO_STRING],
            license: ModuleLicense::Unknown,
            module_type: ModuleType::Other,
            state: ModuleState::Loaded,
            data_ptr: 0,
            size: 0,
            refcount: 0,
            dependencies: [EMPTY_DEP; MAX_DEPENDENCIES],
            dep_count: 0,
            params: [ModuleParam::empty(); MAX_PARAMETERS],
            param_count: 0,
            init_fn: None,
            exit_fn: None,
            taints_kernel: false,
            load_time: 0,
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

    /// Get author as string
    pub fn author_str(&self) -> &str {
        let end = self.author.iter().position(|&c| c == 0).unwrap_or(MAX_INFO_STRING);
        core::str::from_utf8(&self.author[..end]).unwrap_or("")
    }

    /// Get dependency name by index
    pub fn dependency_str(&self, idx: usize) -> Option<&str> {
        if idx >= self.dep_count as usize {
            return None;
        }
        let end = self.dependencies[idx].iter().position(|&c| c == 0).unwrap_or(MAX_MODULE_NAME);
        Some(core::str::from_utf8(&self.dependencies[idx][..end]).unwrap_or(""))
    }

    /// Increment reference count
    pub fn get(&mut self) -> u32 {
        self.refcount += 1;
        self.refcount
    }

    /// Decrement reference count
    pub fn put(&mut self) -> u32 {
        if self.refcount > 0 {
            self.refcount -= 1;
        }
        self.refcount
    }

    /// Check if module can be unloaded
    pub fn can_unload(&self) -> bool {
        self.refcount == 0 && self.state == ModuleState::Running
    }

    /// Set parameter value
    pub fn set_param(&mut self, name: &str, value: &str) -> Result<(), ModuleError> {
        for i in 0..self.param_count as usize {
            if self.params[i].name_str() == name {
                let value_bytes = value.as_bytes();
                let len = value_bytes.len().min(MAX_PARAM_VALUE);
                self.params[i].value[..len].copy_from_slice(&value_bytes[..len]);
                self.params[i].value_len = len;
                return Ok(());
            }
        }
        Err(ModuleError::ParamNotFound)
    }

    /// Get parameter value
    pub fn get_param(&self, name: &str) -> Option<&str> {
        for i in 0..self.param_count as usize {
            if self.params[i].name_str() == name {
                return Some(self.params[i].value_str());
            }
        }
        None
    }
}

/// Extended module registry with dependency tracking
struct ModuleRegistry {
    modules: [Option<ModuleInfo>; MAX_MODULES],
    count: usize,
    /// Kernel tainted flag (set when non-GPL module loads)
    tainted: bool,
}

impl ModuleRegistry {
    const fn new() -> Self {
        const NONE: Option<ModuleInfo> = None;
        Self {
            modules: [NONE; MAX_MODULES],
            count: 0,
            tainted: false,
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
    /// Parameter not found
    ParamNotFound,
    /// Module still in use (refcount > 0)
    ModuleInUse,
    /// Dependency cycle detected
    DependencyCycle,
    /// Permission denied
    PermissionDenied,
    /// Invalid parameter
    InvalidParam,
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

/// Increment module reference count
pub fn module_get(name: &str) -> Result<u32, ModuleError> {
    let mut registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find_mut(name) {
        Ok(info.get())
    } else {
        Err(ModuleError::NotFound)
    }
}

/// Decrement module reference count
pub fn module_put(name: &str) -> Result<u32, ModuleError> {
    let mut registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find_mut(name) {
        Ok(info.put())
    } else {
        Err(ModuleError::NotFound)
    }
}

/// Unload a module with safety checks
pub fn unload_module(name: &str) -> Result<(), ModuleError> {
    unload_module_with_flags(name, false)
}

/// Force unload a module (ignores refcount)
pub fn force_unload_module(name: &str) -> Result<(), ModuleError> {
    unload_module_with_flags(name, true)
}

/// Unload a module with optional force flag
fn unload_module_with_flags(name: &str, force: bool) -> Result<(), ModuleError> {
    crate::kinfo!("Unloading kernel module: {} (force={})", name, force);

    // Check if module can be unloaded
    {
        let registry = MODULE_REGISTRY.lock();
        if let Some(info) = registry.find(name) {
            if !force && info.refcount > 0 {
                crate::kwarn!("Module '{}' still in use (refcount={})", name, info.refcount);
                return Err(ModuleError::ModuleInUse);
            }
            if info.state == ModuleState::Unloading {
                crate::kwarn!("Module '{}' is already being unloaded", name);
                return Err(ModuleError::NotFound);
            }
        } else {
            return Err(ModuleError::NotFound);
        }
    }

    // Call module exit function if present
    {
        let registry = MODULE_REGISTRY.lock();
        if let Some(info) = registry.find(name) {
            if let Some(exit_addr) = info.exit_fn {
                let exit: extern "C" fn() -> i32 = unsafe {
                    core::mem::transmute(exit_addr)
                };
                let ret = exit();
                if ret != 0 {
                    crate::kwarn!("Module '{}' exit function returned {}", name, ret);
                }
            }
        }
    }

    // Set state to unloading
    {
        let mut registry = MODULE_REGISTRY.lock();
        if let Some(info) = registry.find_mut(name) {
            info.state = ModuleState::Unloading;
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

/// Load module with parameters
/// Parameters are passed as "param1=value1 param2=value2" string
pub fn load_module_with_params(data: &[u8], params: &str) -> Result<(), ModuleError> {
    // First load the module
    load_module(data)?;
    
    // Then set parameters
    // Parse params string
    if !params.is_empty() {
        // Get the module name from the loaded modules (it's the last one)
        let module_name = {
            let registry = MODULE_REGISTRY.lock();
            let mut name = [0u8; MAX_MODULE_NAME];
            for slot in registry.modules.iter() {
                if let Some(info) = slot {
                    if info.state == ModuleState::Running || info.state == ModuleState::Loaded {
                        name.copy_from_slice(&info.name);
                        break;
                    }
                }
            }
            name
        };
        
        let name_str = {
            let end = module_name.iter().position(|&c| c == 0).unwrap_or(MAX_MODULE_NAME);
            core::str::from_utf8(&module_name[..end]).unwrap_or("")
        };
        
        for param_pair in params.split_whitespace() {
            if let Some(eq_pos) = param_pair.find('=') {
                let key = &param_pair[..eq_pos];
                let value = &param_pair[eq_pos + 1..];
                if let Err(e) = set_module_param(name_str, key, value) {
                    crate::kwarn!("Failed to set parameter {}={}: {:?}", key, value, e);
                }
            }
        }
    }
    
    Ok(())
}

/// Set a module parameter
pub fn set_module_param(module_name: &str, param_name: &str, value: &str) -> Result<(), ModuleError> {
    let mut registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find_mut(module_name) {
        info.set_param(param_name, value)
    } else {
        Err(ModuleError::NotFound)
    }
}

/// Get a module parameter
pub fn get_module_param(module_name: &str, param_name: &str) -> Result<alloc::string::String, ModuleError> {
    let registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find(module_name) {
        if let Some(value) = info.get_param(param_name) {
            Ok(alloc::string::String::from(value))
        } else {
            Err(ModuleError::ParamNotFound)
        }
    } else {
        Err(ModuleError::NotFound)
    }
}

/// Check if all dependencies of a module are loaded
pub fn check_dependencies(module_name: &str) -> Result<(), ModuleError> {
    let registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find(module_name) {
        for i in 0..info.dep_count as usize {
            if let Some(dep_name) = info.dependency_str(i) {
                if !dep_name.is_empty() && registry.find(dep_name).is_none() {
                    crate::kwarn!("Module '{}' missing dependency: {}", module_name, dep_name);
                    return Err(ModuleError::MissingDependency);
                }
            }
        }
        Ok(())
    } else {
        Err(ModuleError::NotFound)
    }
}

/// Get number of loaded modules
pub fn module_count() -> usize {
    MODULE_REGISTRY.lock().count
}

/// Check if kernel is tainted by non-GPL modules
pub fn is_kernel_tainted() -> bool {
    MODULE_REGISTRY.lock().tainted
}

/// Get module state
pub fn get_module_state(name: &str) -> Option<ModuleState> {
    MODULE_REGISTRY.lock().find(name).map(|info| info.state)
}

/// Get module reference count
pub fn get_module_refcount(name: &str) -> Option<u32> {
    MODULE_REGISTRY.lock().find(name).map(|info| info.refcount)
}

/// Request to use a module (increments refcount if module exists and is running)
pub fn try_module_get(name: &str) -> Result<(), ModuleError> {
    let mut registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find_mut(name) {
        if info.state == ModuleState::Running {
            info.get();
            Ok(())
        } else {
            Err(ModuleError::NotFound)
        }
    } else {
        Err(ModuleError::NotFound)
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
