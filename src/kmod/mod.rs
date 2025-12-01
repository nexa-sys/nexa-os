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

/// Module metadata (heap-allocated for dynamic strings)
#[derive(Clone)]
pub struct ModuleInfo {
    /// Module name (heap-allocated)
    pub name: alloc::string::String,
    /// Module version string (heap-allocated)
    pub version: alloc::string::String,
    /// Module description (heap-allocated)
    pub description: alloc::string::String,
    /// Module type
    pub module_type: ModuleType,
    /// Current state
    pub state: ModuleState,
    /// Base address of loaded code
    pub base_addr: usize,
    /// Size of loaded module in memory
    pub size: usize,
    /// Init function address (if available)
    pub init_fn: Option<u64>,
    /// Exit function address (if available)
    pub exit_fn: Option<u64>,
    /// Module dependencies (names of required modules)
    pub dependencies: alloc::vec::Vec<alloc::string::String>,
    /// Reference count (how many modules depend on this)
    pub ref_count: usize,
}

impl ModuleInfo {
    /// Create an empty module info
    fn new() -> Self {
        Self {
            name: alloc::string::String::new(),
            version: alloc::string::String::new(),
            description: alloc::string::String::new(),
            module_type: ModuleType::Other,
            state: ModuleState::Loaded,
            base_addr: 0,
            size: 0,
            init_fn: None,
            exit_fn: None,
            dependencies: alloc::vec::Vec::new(),
            ref_count: 0,
        }
    }

    /// Create module info with name
    fn with_name(name: &str) -> Self {
        let mut info = Self::new();
        info.name = alloc::string::String::from(name);
        info
    }

    /// Get module name as string slice
    pub fn name_str(&self) -> &str {
        &self.name
    }

    /// Get version as string slice
    pub fn version_str(&self) -> &str {
        &self.version
    }

    /// Get description as string slice
    pub fn description_str(&self) -> &str {
        &self.description
    }

    /// Check if module can be safely unloaded
    pub fn can_unload(&self) -> bool {
        self.ref_count == 0 && self.state == ModuleState::Running
    }
}

/// Module registry (heap-allocated)
struct ModuleRegistry {
    /// Loaded modules stored in heap-allocated Vec
    modules: alloc::vec::Vec<ModuleInfo>,
    /// Whether the registry has been initialized
    initialized: bool,
}

impl ModuleRegistry {
    /// Create uninitialized registry (call init() before use)
    const fn new_uninit() -> Self {
        Self {
            modules: alloc::vec::Vec::new(),
            initialized: false,
        }
    }

    /// Initialize the registry with heap allocation
    fn init(&mut self) {
        if self.initialized {
            return;
        }
        self.modules = alloc::vec::Vec::with_capacity(16);
        self.initialized = true;
    }

    fn register(&mut self, info: ModuleInfo) -> Result<usize, ModuleError> {
        if !self.initialized {
            self.init();
        }

        // Check soft limit
        if self.modules.len() >= MAX_MODULES {
            return Err(ModuleError::TooManyModules);
        }

        // Check for duplicate
        if self.modules.iter().any(|m| m.name == info.name) {
            return Err(ModuleError::AlreadyLoaded);
        }

        let idx = self.modules.len();
        self.modules.push(info);
        Ok(idx)
    }

    fn find(&self, name: &str) -> Option<&ModuleInfo> {
        self.modules.iter().find(|m| m.name == name)
    }

    fn find_mut(&mut self, name: &str) -> Option<&mut ModuleInfo> {
        self.modules.iter_mut().find(|m| m.name == name)
    }

    fn unregister(&mut self, name: &str) -> Result<ModuleInfo, ModuleError> {
        let pos = self.modules.iter().position(|m| m.name == name)
            .ok_or(ModuleError::NotFound)?;
        
        let info = &self.modules[pos];
        if info.ref_count > 0 {
            return Err(ModuleError::InUse);
        }
        
        Ok(self.modules.swap_remove(pos))
    }

    fn count(&self) -> usize {
        self.modules.len()
    }

    /// Get all loaded modules
    fn list(&self) -> &[ModuleInfo] {
        &self.modules
    }

    /// Increment reference count for a module (for dependency tracking)
    #[allow(dead_code)]
    fn inc_ref(&mut self, name: &str) -> Result<(), ModuleError> {
        if let Some(info) = self.find_mut(name) {
            info.ref_count += 1;
            Ok(())
        } else {
            Err(ModuleError::NotFound)
        }
    }

    /// Decrement reference count for a module (for dependency tracking)
    #[allow(dead_code)]
    fn dec_ref(&mut self, name: &str) -> Result<(), ModuleError> {
        if let Some(info) = self.find_mut(name) {
            if info.ref_count > 0 {
                info.ref_count -= 1;
            }
            Ok(())
        } else {
            Err(ModuleError::NotFound)
        }
    }
}

static MODULE_REGISTRY: Mutex<ModuleRegistry> = Mutex::new(ModuleRegistry::new_uninit());

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
    /// Module is in use (has dependents)
    InUse,
    /// Module exit failed
    ExitFailed,
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

    // Create module info using heap allocation
    let mut info = ModuleInfo::with_name("elf_module");
    info.version = alloc::string::String::from("1.0.0");
    info.module_type = ModuleType::Other;
    info.state = ModuleState::Loaded;
    info.base_addr = loaded.base;
    info.size = loaded.size;
    info.init_fn = loaded.init_fn;
    info.exit_fn = loaded.exit_fn;

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
        if let Some(mod_info) = registry.find_mut("elf_module") {
            mod_info.state = ModuleState::Running;
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

    // Create module info with heap-allocated strings
    let mut info = ModuleInfo::with_name(header.name_str());
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
                if let Ok(version) = core::str::from_utf8(&str_data[..null_pos]) {
                    info.version = alloc::string::String::from(version);
                }

                if null_pos + 1 < str_data.len() {
                    let desc_start = null_pos + 1;
                    let desc_end = str_data[desc_start..]
                        .iter()
                        .position(|&c| c == 0)
                        .map(|p| desc_start + p)
                        .unwrap_or(str_data.len());
                    if let Ok(desc) = core::str::from_utf8(&str_data[desc_start..desc_end]) {
                        info.description = alloc::string::String::from(desc);
                    }
                }
            }
        }
    }

    let module_name = info.name.clone();
    let version = info.version.clone();

    // Register module
    let _idx = MODULE_REGISTRY.lock().register(info)?;

    // Mark as running
    if let Some(mod_info) = MODULE_REGISTRY.lock().find_mut(&module_name) {
        mod_info.state = ModuleState::Running;
    }

    crate::kinfo!(
        "Module '{}' loaded successfully (version: {})",
        module_name,
        version
    );

    Ok(())
}

/// Check if a module is loaded
pub fn is_loaded(name: &str) -> bool {
    MODULE_REGISTRY.lock().find(name).is_some()
}

/// Get module info (returns cloned data)
pub fn get_module_info(name: &str) -> Option<ModuleInfo> {
    MODULE_REGISTRY.lock().find(name).cloned()
}

/// Unload a module
pub fn unload_module(name: &str) -> Result<(), ModuleError> {
    crate::kinfo!("Unloading kernel module: {}", name);

    // Check if module can be unloaded and set state
    {
        let mut registry = MODULE_REGISTRY.lock();
        if let Some(info) = registry.find_mut(name) {
            if !info.can_unload() {
                crate::kwarn!("Module '{}' cannot be unloaded (ref_count={})", name, info.ref_count);
                return Err(ModuleError::InUse);
            }
            info.state = ModuleState::Unloading;
        } else {
            return Err(ModuleError::NotFound);
        }
    }

    // TODO: Call module exit function if present

    // Remove from registry
    let removed = MODULE_REGISTRY.lock().unregister(name)?;
    
    // Free module memory if it was dynamically loaded
    if removed.base_addr != 0 && removed.size > 0 {
        // Memory will be freed when the containing allocation is dropped
        crate::kinfo!("Module memory at {:#x} ({} bytes) released", removed.base_addr, removed.size);
    }

    crate::kinfo!("Module '{}' unloaded", name);
    Ok(())
}

/// List all loaded modules
pub fn list_modules() -> alloc::vec::Vec<ModuleInfo> {
    MODULE_REGISTRY.lock().list().to_vec()
}

/// Get module count
pub fn module_count() -> usize {
    MODULE_REGISTRY.lock().count()
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

// ============================================================================
// Module Statistics and Diagnostics
// ============================================================================

/// Module subsystem statistics
#[derive(Debug, Clone)]
pub struct ModuleStats {
    /// Total number of loaded modules
    pub loaded_count: usize,
    /// Total memory used by loaded modules
    pub total_memory: usize,
    /// Number of modules by type
    pub by_type: ModuleTypeStats,
}

/// Module counts by type
#[derive(Debug, Clone, Default)]
pub struct ModuleTypeStats {
    pub filesystem: usize,
    pub block_device: usize,
    pub char_device: usize,
    pub network: usize,
    pub other: usize,
}

/// Get module subsystem statistics
pub fn get_module_stats() -> ModuleStats {
    let registry = MODULE_REGISTRY.lock();
    let mut stats = ModuleStats {
        loaded_count: registry.count(),
        total_memory: 0,
        by_type: ModuleTypeStats::default(),
    };

    for info in registry.list() {
        stats.total_memory += info.size;
        match info.module_type {
            ModuleType::Filesystem => stats.by_type.filesystem += 1,
            ModuleType::BlockDevice => stats.by_type.block_device += 1,
            ModuleType::CharDevice => stats.by_type.char_device += 1,
            ModuleType::Network => stats.by_type.network += 1,
            ModuleType::Other => stats.by_type.other += 1,
        }
    }

    stats
}

/// Print module subsystem status to kernel log
pub fn print_module_status() {
    let stats = get_module_stats();
    let symbol_stats = symbols::get_symbol_stats();

    crate::kinfo!("=== Kernel Module Subsystem Status ===");
    crate::kinfo!("Loaded modules: {}", stats.loaded_count);
    crate::kinfo!("Total module memory: {} bytes", stats.total_memory);
    crate::kinfo!(
        "By type: fs={} blk={} chr={} net={} other={}",
        stats.by_type.filesystem,
        stats.by_type.block_device,
        stats.by_type.char_device,
        stats.by_type.network,
        stats.by_type.other
    );
    crate::kinfo!(
        "Symbol table: {} symbols, {} bytes",
        symbol_stats.symbol_count,
        symbol_stats.total_bytes
    );

    // List all loaded modules
    let registry = MODULE_REGISTRY.lock();
    for info in registry.list() {
        crate::kinfo!(
            "  - {} v{} ({:?}, {:?}) @ {:#x} ({} bytes, refs={})",
            info.name,
            info.version,
            info.module_type,
            info.state,
            info.base_addr,
            info.size,
            info.ref_count
        );
    }
}

/// Find modules by type
pub fn find_modules_by_type(module_type: ModuleType) -> alloc::vec::Vec<ModuleInfo> {
    MODULE_REGISTRY
        .lock()
        .list()
        .iter()
        .filter(|m| m.module_type == module_type)
        .cloned()
        .collect()
}

/// Check if all dependencies for a module are loaded
pub fn check_dependencies(deps: &[&str]) -> Result<(), ModuleError> {
    let registry = MODULE_REGISTRY.lock();
    for dep in deps {
        if registry.find(dep).is_none() {
            crate::kwarn!("Missing module dependency: {}", dep);
            return Err(ModuleError::MissingDependency);
        }
    }
    Ok(())
}

/// Load a module with its dependencies (simple dependency resolution)
pub fn load_module_with_deps(name: &str, data: &[u8], deps: &[&str]) -> Result<(), ModuleError> {
    // First check if all dependencies are loaded
    check_dependencies(deps)?;

    // Increment reference counts for dependencies
    {
        let mut registry = MODULE_REGISTRY.lock();
        for dep in deps {
            if let Some(info) = registry.find_mut(dep) {
                info.ref_count += 1;
            }
        }
    }

    // Load the module
    match load_module(data) {
        Ok(()) => {
            // Store dependencies in the module info
            if let Some(info) = MODULE_REGISTRY.lock().find_mut(name) {
                info.dependencies = deps.iter().map(|s| alloc::string::String::from(*s)).collect();
            }
            Ok(())
        }
        Err(e) => {
            // Rollback reference counts on failure
            let mut registry = MODULE_REGISTRY.lock();
            for dep in deps {
                if let Some(info) = registry.find_mut(dep) {
                    if info.ref_count > 0 {
                        info.ref_count -= 1;
                    }
                }
            }
            Err(e)
        }
    }
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
