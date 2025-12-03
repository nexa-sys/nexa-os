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
//! 3. Signature check: Verify module signature (if signed)
//! 4. For ELF modules:
//!    a. Allocate memory for code/data sections
//!    b. Load sections and apply relocations
//!    c. Resolve external symbols from kernel symbol table
//! 5. Taint check: Apply kernel taint flags if needed
//! 6. Register: Add to kernel module list
//! 7. Init: Call module's init function
//! 8. Unload (optional): Call cleanup and remove
//!
//! # Kernel Taint Flags (Linux-compatible)
//!
//! NexaOS implements Linux-compatible kernel taint flags:
//!
//! | Flag | Char | Description |
//! |------|------|-------------|
//! | P    | 1    | Proprietary module was loaded |
//! | F    | 2    | Module was force loaded |
//! | R    | 8    | User forced a module unload |
//! | U    | 64   | User requested taint |
//! | E    | 16384| Unsigned module was loaded |
//! | O    | 32768| Out-of-tree module was loaded |
//! | C    | 65536| Staging driver was loaded |
//!
//! # Module Licenses
//!
//! GPL-compatible licenses (don't taint kernel):
//! - GPL, GPL v2, GPL v2+
//! - Dual MIT/GPL, Dual BSD/GPL, Dual MPL/GPL
//!
//! Non-GPL licenses (taint kernel with P flag):
//! - BSD, MIT (standalone), Apache-2.0, Proprietary
//!
//! # Module Signatures
//!
//! Modules can be cryptographically signed. Unsigned modules taint the
//! kernel with the E flag. Signature format: append `~Module sig~` magic
//! at end of module file followed by signature data.
//!
//! # Examples
//!
//! ```ignore
//! // Load module from initramfs
//! kmod::load_from_initramfs("ext2")?;
//!
//! // Check kernel taint status
//! let taint = kmod::get_taint_string();
//! println!("Kernel: {}", taint);
//!
//! // Force unload a module
//! kmod::force_unload_module("ext2")?; // Taints kernel with R
//! ```

pub mod crypto;
pub mod elf;
pub mod embedded_keys;
pub mod pkcs7;
pub mod symbols;

// Re-export commonly used items from submodules
pub use crypto::{
    add_trusted_key, find_trusted_key, is_key_trusted, sha256, trusted_key_count, RsaPublicKey,
};
pub use pkcs7::{
    extract_module_signature, parse_pkcs7_signed_data,
    verify_module_signature as verify_pkcs7_signature, ModuleSigInfo, Pkcs7SignedData,
    SignatureVerifyResult, SignerInfo,
};

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

/// Maximum license string length
pub const MAX_LICENSE_LEN: usize = 32;

/// Maximum author string length
pub const MAX_AUTHOR_LEN: usize = 64;

// ============================================================================
// Kernel Taint Flags (Linux-compatible)
// ============================================================================

use core::sync::atomic::{AtomicU32, Ordering};

/// Global kernel taint flags
static KERNEL_TAINT: AtomicU32 = AtomicU32::new(0);

/// Taint flag bits (compatible with Linux kernel)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TaintFlag {
    /// Proprietary module was loaded (P)
    ProprietaryModule = 1 << 0,
    /// Module was force loaded (F)
    ForcedLoad = 1 << 1,
    /// SMP with CPUs not designed for SMP (S)
    Smp = 1 << 2,
    /// User forced a module unload (R)
    ForcedUnload = 1 << 3,
    /// System experienced a machine check exception (M)
    MachineCheck = 1 << 4,
    /// Page-release function has been replaced (B)
    BadPage = 1 << 5,
    /// User requested for taint (U)
    UserRequest = 1 << 6,
    /// Kernel died recently, i.e., OOPS or BUG (D)
    Die = 1 << 7,
    /// ACPI table overridden by user (A)
    OverriddenAcpiTable = 1 << 8,
    /// Taint on warning (W)
    Warn = 1 << 9,
    /// Kernel is live patched (K)
    LivePatch = 1 << 10,
    /// Hardware is unsupported (H)
    UnsupportedHardware = 1 << 11,
    /// Soft lockup previously occurred (L)
    Softlockup = 1 << 12,
    /// Firmware issues (I)
    FirmwareBug = 1 << 13,
    /// Unsigned module was loaded (E)
    UnsignedModule = 1 << 14,
    /// Out-of-tree module was loaded (O)
    OutOfTreeModule = 1 << 15,
    /// Staging driver was loaded (C)
    StagingDriver = 1 << 16,
    /// Hardware random number generator tampered with (T)
    RandomizeTampered = 1 << 17,
    /// Auxiliary taint, for distro use (X)
    Aux = 1 << 18,
}

impl TaintFlag {
    /// Get the character representation of this taint flag (Linux-compatible)
    pub fn as_char(self) -> char {
        match self {
            TaintFlag::ProprietaryModule => 'P',
            TaintFlag::ForcedLoad => 'F',
            TaintFlag::Smp => 'S',
            TaintFlag::ForcedUnload => 'R',
            TaintFlag::MachineCheck => 'M',
            TaintFlag::BadPage => 'B',
            TaintFlag::UserRequest => 'U',
            TaintFlag::Die => 'D',
            TaintFlag::OverriddenAcpiTable => 'A',
            TaintFlag::Warn => 'W',
            TaintFlag::LivePatch => 'K',
            TaintFlag::UnsupportedHardware => 'H',
            TaintFlag::Softlockup => 'L',
            TaintFlag::FirmwareBug => 'I',
            TaintFlag::UnsignedModule => 'E',
            TaintFlag::OutOfTreeModule => 'O',
            TaintFlag::StagingDriver => 'C',
            TaintFlag::RandomizeTampered => 'T',
            TaintFlag::Aux => 'X',
        }
    }
}

/// Add a taint flag to the kernel
pub fn add_taint(flag: TaintFlag) {
    KERNEL_TAINT.fetch_or(flag as u32, Ordering::SeqCst);
    crate::kwarn!("Kernel tainted: {} ({})", flag.as_char(), flag as u32);
}

/// Check if a specific taint flag is set
pub fn is_tainted(flag: TaintFlag) -> bool {
    (KERNEL_TAINT.load(Ordering::SeqCst) & (flag as u32)) != 0
}

/// Get the current taint value
pub fn get_taint() -> u32 {
    KERNEL_TAINT.load(Ordering::SeqCst)
}

/// Get taint flags as a string (Linux-compatible format)
pub fn get_taint_string() -> alloc::string::String {
    let taint = KERNEL_TAINT.load(Ordering::SeqCst);
    if taint == 0 {
        return alloc::string::String::from("Not tainted");
    }

    let mut s = alloc::string::String::from("Tainted: ");
    let flags = [
        (TaintFlag::ProprietaryModule, 'P', 'G'), // G = GPL'd
        (TaintFlag::ForcedLoad, 'F', ' '),
        (TaintFlag::Smp, 'S', ' '),
        (TaintFlag::ForcedUnload, 'R', ' '),
        (TaintFlag::MachineCheck, 'M', ' '),
        (TaintFlag::BadPage, 'B', ' '),
        (TaintFlag::UserRequest, 'U', ' '),
        (TaintFlag::Die, 'D', ' '),
        (TaintFlag::OverriddenAcpiTable, 'A', ' '),
        (TaintFlag::Warn, 'W', ' '),
        (TaintFlag::LivePatch, 'K', ' '),
        (TaintFlag::UnsupportedHardware, 'H', ' '),
        (TaintFlag::Softlockup, 'L', ' '),
        (TaintFlag::FirmwareBug, 'I', ' '),
        (TaintFlag::UnsignedModule, 'E', ' '),
        (TaintFlag::OutOfTreeModule, 'O', ' '),
        (TaintFlag::StagingDriver, 'C', ' '),
        (TaintFlag::RandomizeTampered, 'T', ' '),
        (TaintFlag::Aux, 'X', ' '),
    ];

    for (flag, tainted_char, clean_char) in flags {
        if (taint & (flag as u32)) != 0 {
            s.push(tainted_char);
        } else if clean_char != ' ' {
            s.push(clean_char);
        }
    }

    s
}

// ============================================================================
// Module License Types
// ============================================================================

/// Known module license types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LicenseType {
    /// Unknown or unspecified license
    Unknown = 0,
    /// GPL v2
    Gpl2 = 1,
    /// GPL v2 or later
    Gpl2Plus = 2,
    /// Dual MIT/GPL
    DualMitGpl = 3,
    /// Dual BSD/GPL
    DualBsdGpl = 4,
    /// Dual MPL/GPL
    DualMplGpl = 5,
    /// GPL and additional rights
    GplExtra = 6,
    /// BSD license
    Bsd = 7,
    /// MIT license
    Mit = 8,
    /// Apache 2.0
    Apache2 = 9,
    /// Proprietary
    Proprietary = 255,
}

impl LicenseType {
    /// Check if this license is GPL-compatible (doesn't taint kernel)
    pub fn is_gpl_compatible(self) -> bool {
        matches!(
            self,
            LicenseType::Gpl2
                | LicenseType::Gpl2Plus
                | LicenseType::DualMitGpl
                | LicenseType::DualBsdGpl
                | LicenseType::DualMplGpl
                | LicenseType::GplExtra
        )
    }

    /// Parse license string to LicenseType
    pub fn from_string(s: &str) -> Self {
        match s.trim() {
            "GPL" | "GPL v2" | "GPLv2" => LicenseType::Gpl2,
            "GPL v2+" | "GPL-2.0+" | "GPL-2.0-or-later" => LicenseType::Gpl2Plus,
            "Dual MIT/GPL" | "MIT/GPL" => LicenseType::DualMitGpl,
            "Dual BSD/GPL" | "BSD/GPL" => LicenseType::DualBsdGpl,
            "Dual MPL/GPL" | "MPL/GPL" => LicenseType::DualMplGpl,
            "GPL with additional rights" => LicenseType::GplExtra,
            "BSD" | "BSD-3-Clause" | "BSD-2-Clause" => LicenseType::Bsd,
            "MIT" => LicenseType::Mit,
            "Apache-2.0" | "Apache 2.0" => LicenseType::Apache2,
            "Proprietary" | "PROPRIETARY" => LicenseType::Proprietary,
            _ => LicenseType::Unknown,
        }
    }

    /// Get license string representation
    pub fn as_str(self) -> &'static str {
        match self {
            LicenseType::Unknown => "Unknown",
            LicenseType::Gpl2 => "GPL v2",
            LicenseType::Gpl2Plus => "GPL v2+",
            LicenseType::DualMitGpl => "Dual MIT/GPL",
            LicenseType::DualBsdGpl => "Dual BSD/GPL",
            LicenseType::DualMplGpl => "Dual MPL/GPL",
            LicenseType::GplExtra => "GPL+extra",
            LicenseType::Bsd => "BSD",
            LicenseType::Mit => "MIT",
            LicenseType::Apache2 => "Apache-2.0",
            LicenseType::Proprietary => "Proprietary",
        }
    }
}

// ============================================================================
// Module Signature Verification
// ============================================================================

/// Module signature status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Module is not signed
    Unsigned,
    /// Signature verification passed
    Valid,
    /// Signature verification failed
    Invalid,
    /// Signature format is unknown
    UnknownFormat,
    /// Key not found in keyring
    KeyNotFound,
}

impl SignatureStatus {
    /// Check if the signature is considered good (module can be loaded)
    pub fn is_ok(self) -> bool {
        matches!(self, SignatureStatus::Valid)
    }

    /// Get human-readable status
    pub fn as_str(self) -> &'static str {
        match self {
            SignatureStatus::Unsigned => "unsigned",
            SignatureStatus::Valid => "valid",
            SignatureStatus::Invalid => "INVALID",
            SignatureStatus::UnknownFormat => "unknown format",
            SignatureStatus::KeyNotFound => "key not found",
        }
    }
}

/// Module signature magic at end of module (re-exported from pkcs7)
pub use pkcs7::MODULE_SIG_MAGIC;

/// Verify module signature using PKCS#7
///
/// This function performs full cryptographic verification of signed modules:
/// 1. Extracts PKCS#7 signature from module data
/// 2. Parses PKCS#7 SignedData structure
/// 3. Verifies digest matches module content
/// 4. Validates RSA signature against trusted keyring
///
/// Returns SignatureStatus indicating verification result.
fn verify_module_signature(data: &[u8]) -> SignatureStatus {
    let result = pkcs7::verify_module_signature(data);

    // Log detailed verification result
    match result {
        pkcs7::SignatureVerifyResult::Valid => {
            crate::kinfo!("Module signature verified successfully");
        }
        pkcs7::SignatureVerifyResult::Unsigned => {
            crate::kdebug!("Module is not signed");
        }
        _ => {
            crate::kwarn!("Module signature verification failed: {}", result.as_str());
        }
    }

    result.to_signature_status()
}

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
        let end = self
            .name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(MAX_MODULE_NAME);
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
    /// Module license type
    pub license: LicenseType,
    /// Module author(s)
    pub author: alloc::string::String,
    /// Source location (e.g., "in-tree", "out-of-tree", path)
    pub srcversion: alloc::string::String,
    /// Signature verification status
    pub sig_status: SignatureStatus,
    /// Whether this module taints the kernel
    pub taints_kernel: bool,
    /// Taint flags this module adds
    pub taint_flags: u32,
    /// Module load timestamp (ticks since boot)
    pub load_time: u64,
    /// CRC of module text section (for vermagic-like checks)
    pub text_crc: u32,
    /// Kernel version this module was built for
    pub vermagic: alloc::string::String,
    /// Module parameters
    pub params: alloc::vec::Vec<ModuleParam>,
}

/// Module parameter
#[derive(Clone, Debug)]
pub struct ModuleParam {
    /// Parameter name
    pub name: alloc::string::String,
    /// Parameter type
    pub param_type: ParamType,
    /// Parameter value (as string)
    pub value: alloc::string::String,
    /// Parameter description
    pub description: alloc::string::String,
    /// Whether this parameter can be changed at runtime
    pub writable: bool,
}

/// Module parameter types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamType {
    /// Boolean parameter
    Bool,
    /// Integer parameter
    Int,
    /// Unsigned integer parameter
    UInt,
    /// Long integer
    Long,
    /// Unsigned long
    ULong,
    /// String parameter
    String,
    /// Array of integers
    IntArray,
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
            license: LicenseType::Unknown,
            author: alloc::string::String::new(),
            srcversion: alloc::string::String::new(),
            sig_status: SignatureStatus::Unsigned,
            taints_kernel: false,
            taint_flags: 0,
            load_time: 0,
            text_crc: 0,
            vermagic: alloc::string::String::new(),
            params: alloc::vec::Vec::new(),
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

    /// Get license as string
    pub fn license_str(&self) -> &'static str {
        self.license.as_str()
    }

    /// Get author as string slice
    pub fn author_str(&self) -> &str {
        &self.author
    }

    /// Check if module taints the kernel
    pub fn will_taint(&self) -> bool {
        self.taints_kernel
            || !self.license.is_gpl_compatible()
            || self.sig_status == SignatureStatus::Unsigned
    }

    /// Get taint flags this module would add
    pub fn get_taint_flags(&self) -> u32 {
        let mut flags = self.taint_flags;
        if !self.license.is_gpl_compatible() {
            flags |= TaintFlag::ProprietaryModule as u32;
        }
        if self.sig_status == SignatureStatus::Unsigned {
            flags |= TaintFlag::UnsignedModule as u32;
        }
        if self.srcversion.contains("out-of-tree") {
            flags |= TaintFlag::OutOfTreeModule as u32;
        }
        flags
    }

    /// Get signature status as string
    pub fn sig_status_str(&self) -> &'static str {
        self.sig_status.as_str()
    }

    /// Add a parameter to this module
    pub fn add_param(&mut self, param: ModuleParam) {
        self.params.push(param);
    }

    /// Get a parameter value by name
    pub fn get_param(&self, name: &str) -> Option<&ModuleParam> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Set a parameter value by name
    pub fn set_param(&mut self, name: &str, value: &str) -> bool {
        if let Some(param) = self.params.iter_mut().find(|p| p.name == name) {
            if param.writable {
                param.value = alloc::string::String::from(value);
                return true;
            }
        }
        false
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
        let pos = self
            .modules
            .iter()
            .position(|m| m.name == name)
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
    /// Module signature is missing (required)
    SignatureRequired,
    /// Module signature verification failed
    SignatureInvalid,
    /// Signing key not found in trusted keyring
    SigningKeyNotFound,
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

    // Initialize PKCS#7 signature verification subsystem
    pkcs7::init();

    // Load embedded signing keys into trusted keyring
    let key_count = embedded_keys::init();
    crate::kinfo!(
        "Loaded {} embedded signing key(s) into trusted keyring",
        key_count
    );

    // Register ext2 modular filesystem symbols
    crate::fs::ext2_modular::init();

    // Register network modular driver symbols
    crate::net::modular::register_symbols();

    crate::kinfo!(
        "Kernel module system initialized (max {} modules)",
        MAX_MODULES
    );
    crate::kinfo!("Module signature verification: PKCS#7/CMS with SHA-256/RSA");
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

    // Verify module signature (REQUIRED)
    let sig_status = verify_module_signature(data);
    crate::kinfo!("Module signature status: {}", sig_status.as_str());

    // Enforce signature requirement
    match sig_status {
        SignatureStatus::Valid => {
            crate::kinfo!("Module signature verified successfully");
        }
        SignatureStatus::Unsigned => {
            crate::kerror!("SECURITY: Module is not signed - loading DENIED");
            crate::kerror!("All kernel modules must be signed with a trusted key.");
            crate::kerror!("Use 'scripts/sign-module.sh' to sign the module.");
            return Err(ModuleError::SignatureRequired);
        }
        SignatureStatus::Invalid => {
            crate::kerror!("SECURITY: Module signature is INVALID - loading DENIED");
            return Err(ModuleError::SignatureInvalid);
        }
        SignatureStatus::KeyNotFound => {
            crate::kerror!("SECURITY: Signing key not in trusted keyring - loading DENIED");
            crate::kerror!("Add the signing key to the kernel's trusted keyring.");
            return Err(ModuleError::SigningKeyNotFound);
        }
        SignatureStatus::UnknownFormat => {
            crate::kerror!("SECURITY: Unknown signature format - loading DENIED");
            return Err(ModuleError::SignatureInvalid);
        }
    }

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
    info.sig_status = sig_status;
    info.srcversion = alloc::string::String::from("in-tree");
    info.license = LicenseType::Mit; // Default for NexaOS modules
    info.load_time = crate::safety::rdtsc();

    // Check and apply taint flags
    let taint_flags = info.get_taint_flags();
    if taint_flags != 0 {
        info.taints_kernel = true;
        info.taint_flags = taint_flags;

        // Apply taint to kernel
        if taint_flags & (TaintFlag::ProprietaryModule as u32) != 0 {
            add_taint(TaintFlag::ProprietaryModule);
        }
        if taint_flags & (TaintFlag::UnsignedModule as u32) != 0 {
            add_taint(TaintFlag::UnsignedModule);
        }
        if taint_flags & (TaintFlag::OutOfTreeModule as u32) != 0 {
            add_taint(TaintFlag::OutOfTreeModule);
        }

        crate::kwarn!("Module taints kernel: {:#x}", taint_flags);
    }

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

    // Verify module signature (REQUIRED)
    let sig_status = verify_module_signature(data);
    crate::kinfo!("Module signature status: {}", sig_status.as_str());

    // Enforce signature requirement
    match sig_status {
        SignatureStatus::Valid => {
            crate::kinfo!("Module signature verified successfully");
        }
        SignatureStatus::Unsigned => {
            crate::kerror!(
                "SECURITY: Module '{}' is not signed - loading DENIED",
                header.name_str()
            );
            crate::kerror!("All kernel modules must be signed with a trusted key.");
            crate::kerror!("Use 'scripts/sign-module.sh' to sign the module.");
            return Err(ModuleError::SignatureRequired);
        }
        SignatureStatus::Invalid => {
            crate::kerror!(
                "SECURITY: Module '{}' signature is INVALID - loading DENIED",
                header.name_str()
            );
            return Err(ModuleError::SignatureInvalid);
        }
        SignatureStatus::KeyNotFound => {
            crate::kerror!(
                "SECURITY: Signing key for '{}' not in trusted keyring - loading DENIED",
                header.name_str()
            );
            return Err(ModuleError::SigningKeyNotFound);
        }
        SignatureStatus::UnknownFormat => {
            crate::kerror!(
                "SECURITY: Unknown signature format for '{}' - loading DENIED",
                header.name_str()
            );
            return Err(ModuleError::SignatureInvalid);
        }
    }

    // Create module info with heap-allocated strings
    let mut info = ModuleInfo::with_name(header.name_str());
    info.module_type = header.module_type();
    info.state = ModuleState::Loaded;
    info.sig_status = sig_status;
    info.srcversion = alloc::string::String::from("in-tree");
    info.license = LicenseType::Mit; // Default for NexaOS modules
    info.load_time = crate::safety::rdtsc();

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
                crate::kwarn!(
                    "Module '{}' cannot be unloaded (ref_count={})",
                    name,
                    info.ref_count
                );
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
        crate::kinfo!(
            "Module memory at {:#x} ({} bytes) released",
            removed.base_addr,
            removed.size
        );
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

    let path =
        core::str::from_utf8(&path_buf[..total_len]).map_err(|_| ModuleError::InvalidFormat)?;

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
    let taint_string = get_taint_string();

    crate::kinfo!("=== Kernel Module Subsystem Status ===");
    crate::kinfo!("Kernel: {}", taint_string);
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

    // List all loaded modules with extended info
    let registry = MODULE_REGISTRY.lock();
    for info in registry.list() {
        let taint_marker = if info.taints_kernel { "(+)" } else { "" };
        crate::kinfo!(
            "  - {} v{} ({:?}, {:?}) @ {:#x} ({} bytes, refs={}) [{}] {}{}",
            info.name,
            info.version,
            info.module_type,
            info.state,
            info.base_addr,
            info.size,
            info.ref_count,
            info.license_str(),
            info.sig_status_str(),
            taint_marker
        );
    }
}

/// Print detailed module information
pub fn print_module_details(name: &str) {
    let registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find(name) {
        crate::kinfo!("=== Module: {} ===", info.name);
        crate::kinfo!("Version: {}", info.version);
        crate::kinfo!("Description: {}", info.description);
        crate::kinfo!("Type: {:?}", info.module_type);
        crate::kinfo!("State: {:?}", info.state);
        crate::kinfo!("License: {}", info.license_str());
        crate::kinfo!("Author: {}", info.author);
        crate::kinfo!("Source: {}", info.srcversion);
        crate::kinfo!("Signature: {}", info.sig_status_str());
        crate::kinfo!("Base address: {:#x}", info.base_addr);
        crate::kinfo!("Size: {} bytes", info.size);
        crate::kinfo!("References: {}", info.ref_count);
        crate::kinfo!("Taints kernel: {}", info.taints_kernel);
        if info.taints_kernel {
            crate::kinfo!("Taint flags: {:#x}", info.taint_flags);
        }
        crate::kinfo!("Load time (TSC): {}", info.load_time);
        crate::kinfo!("Vermagic: {}", info.vermagic);
        if !info.dependencies.is_empty() {
            crate::kinfo!("Dependencies: {:?}", info.dependencies);
        }
        if !info.params.is_empty() {
            crate::kinfo!("Parameters:");
            for param in &info.params {
                crate::kinfo!(
                    "  {}: {:?} = {} ({})",
                    param.name,
                    param.param_type,
                    param.value,
                    if param.writable { "rw" } else { "ro" }
                );
            }
        }
    } else {
        crate::kwarn!("Module '{}' not found", name);
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
                info.dependencies = deps
                    .iter()
                    .map(|s| alloc::string::String::from(*s))
                    .collect();
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

// ============================================================================
// Force Loading and Advanced Module Control
// ============================================================================

/// Module loading options
#[derive(Debug, Clone, Default)]
pub struct ModuleLoadOptions {
    /// Force load even if signature verification fails
    pub force: bool,
    /// Skip version/vermagic checks
    pub skip_version_check: bool,
    /// Override license (for testing only)
    pub override_license: Option<LicenseType>,
    /// Module parameters to set
    pub params: alloc::vec::Vec<(alloc::string::String, alloc::string::String)>,
    /// Allow loading even if module taints kernel
    pub allow_taint: bool,
}

/// Force load a module (taints kernel)
pub fn force_load_module(data: &[u8]) -> Result<(), ModuleError> {
    // Add forced load taint
    add_taint(TaintFlag::ForcedLoad);
    crate::kwarn!("Force loading module - kernel will be tainted");

    load_module(data)
}

/// Force unload a module (taints kernel)
pub fn force_unload_module(name: &str) -> Result<(), ModuleError> {
    // Add forced unload taint
    add_taint(TaintFlag::ForcedUnload);
    crate::kwarn!("Force unloading module '{}' - kernel will be tainted", name);

    // Force set ref_count to 0
    {
        let mut registry = MODULE_REGISTRY.lock();
        if let Some(info) = registry.find_mut(name) {
            if info.ref_count > 0 {
                crate::kwarn!(
                    "Module '{}' has {} references, forcing unload",
                    name,
                    info.ref_count
                );
                info.ref_count = 0;
            }
        }
    }

    unload_module(name)
}

/// Load module with custom options
pub fn load_module_with_options(
    data: &[u8],
    options: &ModuleLoadOptions,
) -> Result<(), ModuleError> {
    if options.force {
        add_taint(TaintFlag::ForcedLoad);
        crate::kwarn!("Force loading module with options");
    }

    // Load the module
    load_module(data)?;

    // Apply parameters if specified
    // Note: This is a simplified implementation; actual param handling would need module name
    if !options.params.is_empty() {
        crate::kinfo!("Module parameters specified: {:?}", options.params);
        // TODO: Apply parameters after loading
    }

    Ok(())
}

/// Set a module parameter at runtime
pub fn set_module_param(
    module_name: &str,
    param_name: &str,
    value: &str,
) -> Result<(), ModuleError> {
    let mut registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find_mut(module_name) {
        if info.set_param(param_name, value) {
            crate::kinfo!(
                "Set module '{}' param '{}' = '{}'",
                module_name,
                param_name,
                value
            );
            Ok(())
        } else {
            crate::kwarn!(
                "Failed to set param '{}' on module '{}' (not found or read-only)",
                param_name,
                module_name
            );
            Err(ModuleError::InvalidFormat)
        }
    } else {
        Err(ModuleError::NotFound)
    }
}

/// Get module parameter value
pub fn get_module_param(module_name: &str, param_name: &str) -> Option<alloc::string::String> {
    let registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find(module_name) {
        info.get_param(param_name).map(|p| p.value.clone())
    } else {
        None
    }
}

/// Update module license information
pub fn set_module_license(module_name: &str, license: &str) -> Result<(), ModuleError> {
    let mut registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find_mut(module_name) {
        let license_type = LicenseType::from_string(license);
        let old_taints = !info.license.is_gpl_compatible();
        info.license = license_type;

        // Check if taint status changed
        let new_taints = !license_type.is_gpl_compatible();
        if new_taints && !old_taints {
            drop(registry); // Release lock before adding taint
            add_taint(TaintFlag::ProprietaryModule);
        }

        crate::kinfo!(
            "Module '{}' license set to {}",
            module_name,
            license_type.as_str()
        );
        Ok(())
    } else {
        Err(ModuleError::NotFound)
    }
}

/// Update module author information
pub fn set_module_author(module_name: &str, author: &str) -> Result<(), ModuleError> {
    let mut registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find_mut(module_name) {
        info.author = alloc::string::String::from(author);
        Ok(())
    } else {
        Err(ModuleError::NotFound)
    }
}

/// Mark module as out-of-tree (taints kernel)
pub fn set_module_out_of_tree(module_name: &str) -> Result<(), ModuleError> {
    let mut registry = MODULE_REGISTRY.lock();
    if let Some(info) = registry.find_mut(module_name) {
        info.srcversion = alloc::string::String::from("out-of-tree");
        info.taints_kernel = true;
        info.taint_flags |= TaintFlag::OutOfTreeModule as u32;
        drop(registry);
        add_taint(TaintFlag::OutOfTreeModule);
        Ok(())
    } else {
        Err(ModuleError::NotFound)
    }
}

/// Get modules that taint the kernel
pub fn get_tainting_modules() -> alloc::vec::Vec<ModuleInfo> {
    MODULE_REGISTRY
        .lock()
        .list()
        .iter()
        .filter(|m| m.taints_kernel)
        .cloned()
        .collect()
}

/// Get count of tainting modules
pub fn get_tainting_module_count() -> usize {
    MODULE_REGISTRY
        .lock()
        .list()
        .iter()
        .filter(|m| m.taints_kernel)
        .count()
}

/// Check if kernel is tainted by any module
pub fn is_kernel_tainted_by_modules() -> bool {
    let taint = get_taint();
    (taint
        & (TaintFlag::ProprietaryModule as u32
            | TaintFlag::UnsignedModule as u32
            | TaintFlag::OutOfTreeModule as u32
            | TaintFlag::ForcedLoad as u32))
        != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_parse_nkm() {
        let data = generate_nkm(
            "ext2",
            ModuleType::Filesystem,
            "1.0.0",
            "ext2 filesystem driver",
        );
        let header = NkmHeader::parse(&data).expect("parse failed");
        assert_eq!(header.name_str(), "ext2");
        assert_eq!(header.module_type(), ModuleType::Filesystem);
    }
}
