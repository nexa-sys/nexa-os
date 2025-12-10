//! Kernel module management syscalls
//!
//! Implements: init_module, delete_module, query_module
//!
//! These syscalls provide userspace access to the kernel module subsystem,
//! allowing tools like lsmod, insmod, rmmod, and modinfo to interact with
//! loadable kernel modules.

use super::types::*;
use crate::posix;
use crate::process::{USER_REGION_SIZE, USER_VIRT_BASE};
use crate::{kerror, kinfo, kwarn};
use core::slice;
use core::str;

// Query module operation types
/// Get list of all loaded modules
pub const QUERY_MODULE_LIST: u32 = 0;
/// Get detailed info about a specific module
pub const QUERY_MODULE_INFO: u32 = 1;
/// Get module parameters
pub const QUERY_MODULE_PARAMS: u32 = 2;
/// Get module dependencies
pub const QUERY_MODULE_DEPS: u32 = 3;
/// Get module statistics
pub const QUERY_MODULE_STATS: u32 = 4;
/// Get symbols exported by a module
pub const QUERY_MODULE_SYMBOLS: u32 = 5;

/// Module list entry for userspace (fixed size for easy transfer)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ModuleListEntry {
    /// Module name (null-terminated)
    pub name: [u8; 32],
    /// Module size in bytes
    pub size: u64,
    /// Reference count
    pub ref_count: u32,
    /// Module state (0=loading, 1=running, 2=unloading)
    pub state: u8,
    /// Module type (1=filesystem, 2=block, 3=char, 4=network, 5=other)
    pub module_type: u8,
    /// Is signed (0=no, 1=yes, 2=invalid sig)
    pub signed: u8,
    /// Taints kernel (0=no, 1=yes)
    pub taints: u8,
}

impl ModuleListEntry {
    fn from_module_info(info: &crate::kmod::ModuleInfo) -> Self {
        let mut entry = Self {
            name: [0u8; 32],
            size: info.size as u64,
            ref_count: info.ref_count as u32,
            state: match info.state {
                crate::kmod::ModuleState::Loaded => 0,
                crate::kmod::ModuleState::Running => 1,
                crate::kmod::ModuleState::Unloading => 2,
                crate::kmod::ModuleState::Error => 3,
            },
            module_type: match info.module_type {
                crate::kmod::ModuleType::Filesystem => 1,
                crate::kmod::ModuleType::BlockDevice => 2,
                crate::kmod::ModuleType::CharDevice => 3,
                crate::kmod::ModuleType::Network => 4,
                crate::kmod::ModuleType::Other => 5,
            },
            signed: match info.sig_status {
                crate::kmod::SignatureStatus::Unsigned => 0,
                crate::kmod::SignatureStatus::Valid => 1,
                crate::kmod::SignatureStatus::Invalid => 2,
                crate::kmod::SignatureStatus::KeyNotFound => 2,
                crate::kmod::SignatureStatus::UnknownFormat => 2,
            },
            taints: if info.taints_kernel { 1 } else { 0 },
        };
        let name_bytes = info.name.as_bytes();
        let copy_len = name_bytes.len().min(31);
        entry.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        entry
    }
}

/// Module detailed information for userspace
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ModuleDetailedInfo {
    /// Module name (null-terminated)
    pub name: [u8; 32],
    /// Module version (null-terminated)
    pub version: [u8; 32],
    /// Module description (null-terminated)
    pub description: [u8; 128],
    /// Module author (null-terminated)
    pub author: [u8; 64],
    /// Module license (null-terminated)
    pub license: [u8; 32],
    /// Module size in bytes
    pub size: u64,
    /// Module base address (kernel address, for debugging)
    pub base_addr: u64,
    /// Reference count
    pub ref_count: u32,
    /// Number of dependencies
    pub dep_count: u32,
    /// Number of exported symbols
    pub symbol_count: u32,
    /// Number of parameters
    pub param_count: u32,
    /// Module state
    pub state: u8,
    /// Module type
    pub module_type: u8,
    /// Is signed
    pub signed: u8,
    /// Taints kernel
    pub taints: u8,
}

impl ModuleDetailedInfo {
    fn from_module_info(info: &crate::kmod::ModuleInfo) -> Self {
        let mut entry = Self {
            name: [0u8; 32],
            version: [0u8; 32],
            description: [0u8; 128],
            author: [0u8; 64],
            license: [0u8; 32],
            size: info.size as u64,
            base_addr: info.base_addr as u64,
            ref_count: info.ref_count as u32,
            dep_count: info.dependencies.len() as u32,
            symbol_count: info.exported_symbols.len() as u32,
            param_count: info.params.len() as u32,
            state: match info.state {
                crate::kmod::ModuleState::Loaded => 0,
                crate::kmod::ModuleState::Running => 1,
                crate::kmod::ModuleState::Unloading => 2,
                crate::kmod::ModuleState::Error => 3,
            },
            module_type: match info.module_type {
                crate::kmod::ModuleType::Filesystem => 1,
                crate::kmod::ModuleType::BlockDevice => 2,
                crate::kmod::ModuleType::CharDevice => 3,
                crate::kmod::ModuleType::Network => 4,
                crate::kmod::ModuleType::Other => 5,
            },
            signed: match info.sig_status {
                crate::kmod::SignatureStatus::Unsigned => 0,
                crate::kmod::SignatureStatus::Valid => 1,
                crate::kmod::SignatureStatus::Invalid => 2,
                crate::kmod::SignatureStatus::KeyNotFound => 2,
                crate::kmod::SignatureStatus::UnknownFormat => 2,
            },
            taints: if info.taints_kernel { 1 } else { 0 },
        };

        // Copy strings
        let copy_str = |dest: &mut [u8], src: &str| {
            let bytes = src.as_bytes();
            let len = bytes.len().min(dest.len() - 1);
            dest[..len].copy_from_slice(&bytes[..len]);
        };

        copy_str(&mut entry.name, &info.name);
        copy_str(&mut entry.version, &info.version);
        copy_str(&mut entry.description, &info.description);
        copy_str(&mut entry.author, &info.author);
        copy_str(&mut entry.license, info.license.as_str());

        entry
    }
}

/// Module statistics structure
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ModuleStatistics {
    /// Total number of loaded modules
    pub loaded_count: u32,
    /// Total memory used by modules
    pub total_memory: u64,
    /// Filesystem modules count
    pub fs_count: u32,
    /// Block device modules count
    pub blk_count: u32,
    /// Character device modules count
    pub chr_count: u32,
    /// Network modules count
    pub net_count: u32,
    /// Other modules count
    pub other_count: u32,
    /// Kernel symbol count
    pub symbol_count: u32,
    /// Is kernel tainted
    pub is_tainted: u8,
    /// Reserved for alignment
    pub _reserved: [u8; 3],
    /// Taint string (null-terminated)
    pub taint_string: [u8; 32],
}

/// Module dependency entry
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ModuleDependency {
    /// Dependency module name
    pub name: [u8; 32],
}

/// Module symbol entry
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ModuleSymbol {
    /// Symbol name (null-terminated)
    pub name: [u8; 64],
    /// Symbol address
    pub address: u64,
    /// Symbol type (0=function, 1=data)
    pub sym_type: u8,
    /// GPL-only flag
    pub gpl_only: u8,
    /// Reserved
    pub _reserved: [u8; 6],
}

/// Validate userspace pointer
fn validate_user_ptr(ptr: u64, size: usize) -> bool {
    if ptr == 0 || size == 0 {
        return false;
    }
    let end = ptr.saturating_add(size as u64);
    ptr >= USER_VIRT_BASE && end <= USER_VIRT_BASE + USER_REGION_SIZE
}

/// SYS_INIT_MODULE - Load a kernel module from userspace
///
/// Arguments:
///   arg1: pointer to module image (raw module binary data)
///   arg2: size of module image in bytes
///   arg3: pointer to null-terminated options string (may be NULL)
///
/// Returns:
///   0 on success, -1 on failure with errno set
pub fn init_module(module_image: *const u8, len: usize, param_values: *const u8) -> u64 {
    kinfo!(
        "syscall: init_module(image={:#x}, len={})",
        module_image as u64,
        len
    );

    // Check privilege - only root can load modules
    if !crate::auth::is_superuser() {
        kwarn!("init_module: permission denied (not root)");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    // Validate module image pointer
    if !validate_user_ptr(module_image as u64, len) {
        kerror!("init_module: invalid module image pointer");
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Validate size
    if len < 64 || len > 16 * 1024 * 1024 {
        // 64 bytes min, 16MB max
        kerror!("init_module: invalid module size: {}", len);
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Read module data from userspace
    let module_data = unsafe { slice::from_raw_parts(module_image, len) };

    // Parse options if provided
    let _options = if !param_values.is_null() {
        let opt_ptr = param_values as u64;
        if opt_ptr >= USER_VIRT_BASE && opt_ptr < USER_VIRT_BASE + USER_REGION_SIZE {
            // Read option string (limit to 1KB)
            let opt_slice = unsafe {
                let mut len = 0;
                let mut p = param_values;
                while len < 1024 {
                    if *p == 0 {
                        break;
                    }
                    p = p.add(1);
                    len += 1;
                }
                slice::from_raw_parts(param_values, len)
            };
            str::from_utf8(opt_slice).ok()
        } else {
            None
        }
    } else {
        None
    };

    // Load the module
    match crate::kmod::load_module(module_data) {
        Ok(()) => {
            kinfo!("init_module: module loaded successfully");
            posix::set_errno(0);
            0
        }
        Err(e) => {
            kerror!("init_module: failed to load module: {:?}", e);
            let errno = match e {
                crate::kmod::ModuleError::AlreadyLoaded => posix::errno::EEXIST,
                crate::kmod::ModuleError::TooManyModules => posix::errno::ENOMEM,
                crate::kmod::ModuleError::InvalidFormat
                | crate::kmod::ModuleError::InvalidMagic
                | crate::kmod::ModuleError::UnsupportedVersion => posix::errno::ENOEXEC,
                crate::kmod::ModuleError::SignatureRequired
                | crate::kmod::ModuleError::SignatureInvalid
                | crate::kmod::ModuleError::SigningKeyNotFound => posix::errno::EKEYREJECTED,
                crate::kmod::ModuleError::MissingDependency
                | crate::kmod::ModuleError::CircularDependency => posix::errno::ENOENT,
                crate::kmod::ModuleError::AllocationFailed => posix::errno::ENOMEM,
                crate::kmod::ModuleError::SymbolNotFound
                | crate::kmod::ModuleError::RelocationFailed => posix::errno::ENOEXEC,
                crate::kmod::ModuleError::InitFailed => posix::errno::ENOEXEC,
                _ => posix::errno::EINVAL,
            };
            posix::set_errno(errno);
            u64::MAX
        }
    }
}

/// SYS_DELETE_MODULE - Unload a kernel module
///
/// Arguments:
///   arg1: pointer to null-terminated module name
///   arg2: flags (O_NONBLOCK=1, O_TRUNC=2 for force)
///
/// Returns:
///   0 on success, -1 on failure with errno set
pub fn delete_module(name_ptr: *const u8, flags: u32) -> u64 {
    // Check privilege
    if !crate::auth::is_superuser() {
        kwarn!("delete_module: permission denied (not root)");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    // Validate name pointer
    let name_addr = name_ptr as u64;
    if name_ptr.is_null()
        || name_addr < USER_VIRT_BASE
        || name_addr >= USER_VIRT_BASE + USER_REGION_SIZE
    {
        kerror!("delete_module: invalid name pointer");
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Read module name from userspace
    let name = unsafe {
        let mut len = 0;
        let mut p = name_ptr;
        while len < 64 {
            if *p == 0 {
                break;
            }
            p = p.add(1);
            len += 1;
        }
        let slice = slice::from_raw_parts(name_ptr, len);
        match str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => {
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }
        }
    };

    kinfo!(
        "syscall: delete_module(name='{}', flags={:#x})",
        name,
        flags
    );

    // Check for force flag (O_TRUNC = 0x200 in some systems, we use 2)
    let force = (flags & 2) != 0;

    // Unload the module
    let result = if force {
        crate::kmod::force_unload_module(name)
    } else {
        crate::kmod::unload_module(name)
    };

    match result {
        Ok(()) => {
            kinfo!("delete_module: module '{}' unloaded successfully", name);
            posix::set_errno(0);
            0
        }
        Err(e) => {
            kerror!("delete_module: failed to unload module '{}': {:?}", name, e);
            let errno = match e {
                crate::kmod::ModuleError::NotFound => posix::errno::ENOENT,
                crate::kmod::ModuleError::InUse => posix::errno::EBUSY,
                crate::kmod::ModuleError::ExitFailed => posix::errno::EBUSY,
                _ => posix::errno::EINVAL,
            };
            posix::set_errno(errno);
            u64::MAX
        }
    }
}

/// SYS_QUERY_MODULE - Query kernel module information
///
/// Arguments:
///   arg1: operation type (QUERY_MODULE_*)
///   arg2: pointer to module name (NULL for some operations)
///   arg3: pointer to output buffer
///   arg4: size of output buffer
///
/// Returns:
///   Number of entries/bytes written on success, -1 on failure
pub fn query_module(operation: u32, name_ptr: *const u8, buf_ptr: *mut u8, buf_size: usize) -> u64 {
    // Validate output buffer
    if !validate_user_ptr(buf_ptr as u64, buf_size) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Read module name if provided
    let name = if !name_ptr.is_null() {
        let name_addr = name_ptr as u64;
        if name_addr < USER_VIRT_BASE || name_addr >= USER_VIRT_BASE + USER_REGION_SIZE {
            posix::set_errno(posix::errno::EFAULT);
            return u64::MAX;
        }
        unsafe {
            let mut len = 0;
            let mut p = name_ptr;
            while len < 64 {
                if *p == 0 {
                    break;
                }
                p = p.add(1);
                len += 1;
            }
            let slice = slice::from_raw_parts(name_ptr, len);
            str::from_utf8(slice).ok()
        }
    } else {
        None
    };

    match operation {
        QUERY_MODULE_LIST => query_module_list(buf_ptr, buf_size),
        QUERY_MODULE_INFO => {
            let name = match name {
                Some(n) => n,
                None => {
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
            };
            query_module_info(name, buf_ptr, buf_size)
        }
        QUERY_MODULE_STATS => query_module_stats(buf_ptr, buf_size),
        QUERY_MODULE_DEPS => {
            let name = match name {
                Some(n) => n,
                None => {
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
            };
            query_module_deps(name, buf_ptr, buf_size)
        }
        QUERY_MODULE_SYMBOLS => {
            let name = match name {
                Some(n) => n,
                None => {
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
            };
            query_module_symbols(name, buf_ptr, buf_size)
        }
        _ => {
            posix::set_errno(posix::errno::EINVAL);
            u64::MAX
        }
    }
}

/// Query list of all loaded modules
fn query_module_list(buf_ptr: *mut u8, buf_size: usize) -> u64 {
    let modules = crate::kmod::list_modules();
    let entry_size = core::mem::size_of::<ModuleListEntry>();
    let max_entries = buf_size / entry_size;
    let entries_to_copy = modules.len().min(max_entries);

    let out_buf =
        unsafe { slice::from_raw_parts_mut(buf_ptr as *mut ModuleListEntry, entries_to_copy) };

    for (i, info) in modules.iter().take(entries_to_copy).enumerate() {
        out_buf[i] = ModuleListEntry::from_module_info(info);
    }

    posix::set_errno(0);
    entries_to_copy as u64
}

/// Query detailed info about a specific module
fn query_module_info(name: &str, buf_ptr: *mut u8, buf_size: usize) -> u64 {
    let info_size = core::mem::size_of::<ModuleDetailedInfo>();
    if buf_size < info_size {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    match crate::kmod::get_module_info(name) {
        Some(info) => {
            let out = unsafe { &mut *(buf_ptr as *mut ModuleDetailedInfo) };
            *out = ModuleDetailedInfo::from_module_info(&info);
            posix::set_errno(0);
            info_size as u64
        }
        None => {
            posix::set_errno(posix::errno::ENOENT);
            u64::MAX
        }
    }
}

/// Query module subsystem statistics
fn query_module_stats(buf_ptr: *mut u8, buf_size: usize) -> u64 {
    let stats_size = core::mem::size_of::<ModuleStatistics>();
    if buf_size < stats_size {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let kstats = crate::kmod::get_module_stats();
    let taint_str = crate::kmod::get_taint_string();
    let symbol_stats = crate::kmod::symbols::get_symbol_stats();

    let out = unsafe { &mut *(buf_ptr as *mut ModuleStatistics) };
    *out = ModuleStatistics {
        loaded_count: kstats.loaded_count as u32,
        total_memory: kstats.total_memory as u64,
        fs_count: kstats.by_type.filesystem as u32,
        blk_count: kstats.by_type.block_device as u32,
        chr_count: kstats.by_type.char_device as u32,
        net_count: kstats.by_type.network as u32,
        other_count: kstats.by_type.other as u32,
        symbol_count: symbol_stats.symbol_count as u32,
        is_tainted: if crate::kmod::get_taint() != 0 { 1 } else { 0 },
        _reserved: [0; 3],
        taint_string: [0; 32],
    };

    // Copy taint string
    let taint_bytes = taint_str.as_bytes();
    let copy_len = taint_bytes.len().min(31);
    out.taint_string[..copy_len].copy_from_slice(&taint_bytes[..copy_len]);

    posix::set_errno(0);
    stats_size as u64
}

/// Query module dependencies
fn query_module_deps(name: &str, buf_ptr: *mut u8, buf_size: usize) -> u64 {
    match crate::kmod::get_module_info(name) {
        Some(info) => {
            let entry_size = core::mem::size_of::<ModuleDependency>();
            let max_entries = buf_size / entry_size;
            let entries_to_copy = info.dependencies.len().min(max_entries);

            let out_buf = unsafe {
                slice::from_raw_parts_mut(buf_ptr as *mut ModuleDependency, entries_to_copy)
            };

            for (i, dep) in info.dependencies.iter().take(entries_to_copy).enumerate() {
                out_buf[i] = ModuleDependency { name: [0; 32] };
                let dep_bytes = dep.as_bytes();
                let copy_len = dep_bytes.len().min(31);
                out_buf[i].name[..copy_len].copy_from_slice(&dep_bytes[..copy_len]);
            }

            posix::set_errno(0);
            entries_to_copy as u64
        }
        None => {
            posix::set_errno(posix::errno::ENOENT);
            u64::MAX
        }
    }
}

/// Query symbols exported by a module
fn query_module_symbols(name: &str, buf_ptr: *mut u8, buf_size: usize) -> u64 {
    let symbols = crate::kmod::list_module_symbols(name);
    if symbols.is_empty() {
        // Check if module exists
        if crate::kmod::get_module_info(name).is_none() {
            posix::set_errno(posix::errno::ENOENT);
            return u64::MAX;
        }
    }

    let entry_size = core::mem::size_of::<ModuleSymbol>();
    let max_entries = buf_size / entry_size;
    let entries_to_copy = symbols.len().min(max_entries);

    let out_buf =
        unsafe { slice::from_raw_parts_mut(buf_ptr as *mut ModuleSymbol, entries_to_copy) };

    for (i, sym) in symbols.iter().take(entries_to_copy).enumerate() {
        out_buf[i] = ModuleSymbol {
            name: [0; 64],
            address: sym.address,
            sym_type: match sym.sym_type {
                crate::kmod::ExportedSymbolType::Function => 0,
                crate::kmod::ExportedSymbolType::Data => 1,
            },
            gpl_only: if sym.gpl_only { 1 } else { 0 },
            _reserved: [0; 6],
        };
        let sym_bytes = sym.name.as_bytes();
        let copy_len = sym_bytes.len().min(63);
        out_buf[i].name[..copy_len].copy_from_slice(&sym_bytes[..copy_len]);
    }

    posix::set_errno(0);
    entries_to_copy as u64
}
