//! Module management syscalls
//!
//! This module provides syscall implementations for kernel module management:
//! - init_module: Load a kernel module from memory
//! - delete_module: Unload a kernel module
//! - finit_module: Load a kernel module from file descriptor
//! - module_info: Get information about a loaded module
//! - module_list: List all loaded modules
//! - module_param: Get/set module parameters

use crate::kmod::{
    self, ModuleError, ModuleInfo, ModuleState, ModuleType, MAX_MODULE_NAME, MAX_MODULES,
};
use crate::posix;

/// Request structure for init_module syscall
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InitModuleRequest {
    /// Pointer to module image data
    pub module_image: *const u8,
    /// Length of module image
    pub len: usize,
    /// Module parameters (null-terminated string)
    pub param_values: *const u8,
}

/// Request structure for delete_module syscall
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DeleteModuleRequest {
    /// Module name (null-terminated)
    pub name: *const u8,
    /// Flags (O_NONBLOCK, O_TRUNC for force)
    pub flags: u32,
}

/// Response structure for module_info syscall
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ModuleInfoResponse {
    /// Module name
    pub name: [u8; MAX_MODULE_NAME],
    /// Module version
    pub version: [u8; 16],
    /// Module description
    pub description: [u8; 64],
    /// Module author
    pub author: [u8; 64],
    /// Module type
    pub module_type: u8,
    /// Module state
    pub state: u8,
    /// Reference count
    pub refcount: u32,
    /// Module size in memory
    pub size: usize,
    /// Number of dependencies
    pub dep_count: u8,
    /// Number of parameters
    pub param_count: u8,
    /// Reserved padding
    pub _reserved: [u8; 6],
}

impl From<&ModuleInfo> for ModuleInfoResponse {
    fn from(info: &ModuleInfo) -> Self {
        Self {
            name: info.name,
            version: info.version,
            description: info.description,
            author: info.author,
            module_type: info.module_type as u8,
            state: match info.state {
                ModuleState::Loaded => 0,
                ModuleState::Initializing => 1,
                ModuleState::Running => 2,
                ModuleState::Unloading => 3,
                ModuleState::Error => 4,
                ModuleState::WaitingDeps => 5,
            },
            refcount: info.refcount,
            size: info.size,
            dep_count: info.dep_count,
            param_count: info.param_count,
            _reserved: [0; 6],
        }
    }
}

/// Module list entry for module_list syscall
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ModuleListEntry {
    /// Module name
    pub name: [u8; MAX_MODULE_NAME],
    /// Module state
    pub state: u8,
    /// Module type
    pub module_type: u8,
    /// Reference count
    pub refcount: u32,
    /// Reserved padding
    pub _reserved: [u8; 2],
}

/// Request for module parameter operations
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ModuleParamRequest {
    /// Module name
    pub module_name: *const u8,
    /// Module name length
    pub module_name_len: usize,
    /// Parameter name
    pub param_name: *const u8,
    /// Parameter name length
    pub param_name_len: usize,
    /// Parameter value (for set operations)
    pub value: *const u8,
    /// Value length
    pub value_len: usize,
    /// Operation: 0 = get, 1 = set
    pub operation: u32,
    /// Output buffer for get operations
    pub out_buffer: *mut u8,
    /// Output buffer length
    pub out_buffer_len: usize,
}

// Flag constants for delete_module
pub const O_NONBLOCK: u32 = 0x800;
pub const O_TRUNC: u32 = 0x200;

/// Convert ModuleError to errno
fn module_error_to_errno(e: ModuleError) -> u64 {
    let errno = match e {
        ModuleError::InvalidMagic => posix::errno::ENOEXEC,
        ModuleError::UnsupportedVersion => posix::errno::ENOEXEC,
        ModuleError::FileTooSmall => posix::errno::ENOEXEC,
        ModuleError::AlreadyLoaded => posix::errno::EEXIST,
        ModuleError::TooManyModules => posix::errno::ENOMEM,
        ModuleError::NotFound => posix::errno::ENOENT,
        ModuleError::MissingDependency => posix::errno::ENOENT,
        ModuleError::InitFailed => posix::errno::ENOEXEC,
        ModuleError::InvalidFormat => posix::errno::ENOEXEC,
        ModuleError::SymbolNotFound => posix::errno::ENOENT,
        ModuleError::AllocationFailed => posix::errno::ENOMEM,
        ModuleError::RelocationFailed => posix::errno::ENOEXEC,
        ModuleError::ParamNotFound => posix::errno::EINVAL,
        ModuleError::ModuleInUse => posix::errno::EBUSY,
        ModuleError::DependencyCycle => posix::errno::ELOOP,
        ModuleError::PermissionDenied => posix::errno::EPERM,
        ModuleError::InvalidParam => posix::errno::EINVAL,
    };
    (-errno as i64) as u64
}

/// SYS_INIT_MODULE - Load a kernel module from userspace memory
///
/// Arguments:
/// - arg1: pointer to InitModuleRequest structure
///
/// Returns: 0 on success, negative errno on failure
pub fn init_module(request_ptr: *const InitModuleRequest) -> u64 {
    if request_ptr.is_null() {
        return (-posix::errno::EFAULT as i64) as u64;
    }

    // Copy request from userspace
    let request = unsafe { *request_ptr };

    if request.module_image.is_null() || request.len == 0 {
        return (-posix::errno::EINVAL as i64) as u64;
    }

    // TODO: Check capabilities (CAP_SYS_MODULE)
    // For now, we'll allow all module operations

    // Copy module data from userspace
    let module_data = unsafe { core::slice::from_raw_parts(request.module_image, request.len) };

    // Parse parameters if provided
    let params_str = if !request.param_values.is_null() {
        unsafe {
            let mut len = 0;
            while *request.param_values.add(len) != 0 && len < 1024 {
                len += 1;
            }
            core::str::from_utf8(core::slice::from_raw_parts(request.param_values, len))
                .unwrap_or("")
        }
    } else {
        ""
    };

    crate::kinfo!(
        "SYS_INIT_MODULE: loading module ({} bytes, params='{}')",
        request.len,
        params_str
    );

    // Load the module
    let result = if params_str.is_empty() {
        kmod::load_module(module_data)
    } else {
        kmod::load_module_with_params(module_data, params_str)
    };

    match result {
        Ok(()) => {
            crate::kinfo!("SYS_INIT_MODULE: module loaded successfully");
            0
        }
        Err(e) => {
            crate::kwarn!("SYS_INIT_MODULE: failed to load module: {:?}", e);
            module_error_to_errno(e)
        }
    }
}

/// SYS_DELETE_MODULE - Unload a kernel module
///
/// Arguments:
/// - arg1: pointer to DeleteModuleRequest structure
///
/// Returns: 0 on success, negative errno on failure
pub fn delete_module(request_ptr: *const DeleteModuleRequest) -> u64 {
    if request_ptr.is_null() {
        return (-posix::errno::EFAULT as i64) as u64;
    }

    let request = unsafe { *request_ptr };

    if request.name.is_null() {
        return (-posix::errno::EINVAL as i64) as u64;
    }

    // Get module name
    let name = unsafe {
        let mut len = 0;
        while *request.name.add(len) != 0 && len < MAX_MODULE_NAME {
            len += 1;
        }
        core::str::from_utf8(core::slice::from_raw_parts(request.name, len)).unwrap_or("")
    };

    if name.is_empty() {
        return (-posix::errno::EINVAL as i64) as u64;
    }

    let force = (request.flags & O_TRUNC) != 0;

    crate::kinfo!(
        "SYS_DELETE_MODULE: unloading module '{}' (force={})",
        name,
        force
    );

    let result = if force {
        kmod::force_unload_module(name)
    } else {
        kmod::unload_module(name)
    };

    match result {
        Ok(()) => {
            crate::kinfo!("SYS_DELETE_MODULE: module '{}' unloaded successfully", name);
            0
        }
        Err(e) => {
            crate::kwarn!("SYS_DELETE_MODULE: failed to unload module '{}': {:?}", name, e);
            module_error_to_errno(e)
        }
    }
}

/// SYS_FINIT_MODULE - Load a kernel module from file descriptor
///
/// Arguments:
/// - arg1: file descriptor
/// - arg2: pointer to parameters string
/// - arg3: flags
///
/// Returns: 0 on success, negative errno on failure
pub fn finit_module(fd: u64, params: *const u8, _flags: u64) -> u64 {
    // Read module data from file descriptor
    // For now, we'll use the VFS to read the file

    // TODO: Implement proper file descriptor reading
    // This is a simplified implementation

    crate::kinfo!("SYS_FINIT_MODULE: fd={}", fd);

    // Get params string
    let params_str = if !params.is_null() {
        unsafe {
            let mut len = 0;
            while *params.add(len) != 0 && len < 1024 {
                len += 1;
            }
            core::str::from_utf8(core::slice::from_raw_parts(params, len)).unwrap_or("")
        }
    } else {
        ""
    };

    // For now, return ENOSYS as this requires file descriptor support
    crate::kwarn!("SYS_FINIT_MODULE: not fully implemented (params='{}')", params_str);
    (-posix::errno::ENOSYS as i64) as u64
}

/// SYS_MODULE_INFO - Get information about a loaded module
///
/// Arguments:
/// - arg1: pointer to module name (null-terminated)
/// - arg2: name length
/// - arg3: pointer to ModuleInfoResponse buffer
///
/// Returns: 0 on success, negative errno on failure
pub fn module_info(name_ptr: *const u8, name_len: usize, response_ptr: *mut ModuleInfoResponse) -> u64 {
    if name_ptr.is_null() || response_ptr.is_null() {
        return (-posix::errno::EFAULT as i64) as u64;
    }

    let name = unsafe {
        let len = name_len.min(MAX_MODULE_NAME);
        core::str::from_utf8(core::slice::from_raw_parts(name_ptr, len)).unwrap_or("")
    };

    if name.is_empty() {
        return (-posix::errno::EINVAL as i64) as u64;
    }

    match kmod::get_module_info(name) {
        Some(info) => {
            let response = ModuleInfoResponse::from(&info);
            unsafe {
                *response_ptr = response;
            }
            0
        }
        None => {
            crate::kwarn!("SYS_MODULE_INFO: module '{}' not found", name);
            (-posix::errno::ENOENT as i64) as u64
        }
    }
}

/// SYS_MODULE_LIST - List all loaded modules
///
/// Arguments:
/// - arg1: pointer to ModuleListEntry array
/// - arg2: maximum number of entries
///
/// Returns: number of modules on success, negative errno on failure
pub fn module_list(entries_ptr: *mut ModuleListEntry, max_entries: usize) -> u64 {
    if entries_ptr.is_null() {
        return (-posix::errno::EFAULT as i64) as u64;
    }

    let modules = kmod::list_modules();
    let count = modules.len().min(max_entries);

    for (i, info) in modules.iter().take(count).enumerate() {
        let entry = ModuleListEntry {
            name: info.name,
            state: match info.state {
                ModuleState::Loaded => 0,
                ModuleState::Initializing => 1,
                ModuleState::Running => 2,
                ModuleState::Unloading => 3,
                ModuleState::Error => 4,
                ModuleState::WaitingDeps => 5,
            },
            module_type: info.module_type as u8,
            refcount: info.refcount,
            _reserved: [0; 2],
        };
        unsafe {
            *entries_ptr.add(i) = entry;
        }
    }

    count as u64
}

/// SYS_MODULE_PARAM - Get or set module parameters
///
/// Arguments:
/// - arg1: pointer to ModuleParamRequest structure
///
/// Returns: 0 on success, negative errno on failure
pub fn module_param(request_ptr: *const ModuleParamRequest) -> u64 {
    if request_ptr.is_null() {
        return (-posix::errno::EFAULT as i64) as u64;
    }

    let request = unsafe { *request_ptr };

    if request.module_name.is_null() || request.param_name.is_null() {
        return (-posix::errno::EINVAL as i64) as u64;
    }

    let module_name = unsafe {
        core::str::from_utf8(core::slice::from_raw_parts(
            request.module_name,
            request.module_name_len.min(MAX_MODULE_NAME),
        ))
        .unwrap_or("")
    };

    let param_name = unsafe {
        core::str::from_utf8(core::slice::from_raw_parts(
            request.param_name,
            request.param_name_len.min(64),
        ))
        .unwrap_or("")
    };

    if module_name.is_empty() || param_name.is_empty() {
        return (-posix::errno::EINVAL as i64) as u64;
    }

    match request.operation {
        0 => {
            // Get parameter
            match kmod::get_module_param(module_name, param_name) {
                Ok(value) => {
                    if !request.out_buffer.is_null() && request.out_buffer_len > 0 {
                        let bytes = value.as_bytes();
                        let copy_len = bytes.len().min(request.out_buffer_len - 1);
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                bytes.as_ptr(),
                                request.out_buffer,
                                copy_len,
                            );
                            *request.out_buffer.add(copy_len) = 0; // null terminate
                        }
                    }
                    0
                }
                Err(e) => module_error_to_errno(e),
            }
        }
        1 => {
            // Set parameter
            if request.value.is_null() {
                return (-posix::errno::EINVAL as i64) as u64;
            }

            let value = unsafe {
                core::str::from_utf8(core::slice::from_raw_parts(
                    request.value,
                    request.value_len.min(128),
                ))
                .unwrap_or("")
            };

            match kmod::set_module_param(module_name, param_name, value) {
                Ok(()) => 0,
                Err(e) => module_error_to_errno(e),
            }
        }
        _ => (-posix::errno::EINVAL as i64) as u64,
    }
}
