//! NexaOS Kernel Module API
//!
//! This crate provides the API interface for developing kernel modules for NexaOS.
//! It includes macros for module metadata, kernel symbol declarations, and common
//! patterns used in kernel module development.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! #![no_std]
//! use kmod_api::*;
//!
//! // Declare module metadata
//! module_metadata! {
//!     name: "my_module",
//!     version: "1.0.0",
//!     author: "Your Name",
//!     description: "My awesome kernel module",
//!     license: "GPL",
//! }
//!
//! // Module entry point
//! #[no_mangle]
//! pub extern "C" fn module_init() -> i32 {
//!     mod_info!("Module loaded!");
//!     0
//! }
//!
//! // Module exit point
//! #[no_mangle]
//! pub extern "C" fn module_exit() -> i32 {
//!     mod_info!("Module unloaded!");
//!     0
//! }
//! ```
//!
//! # Module Types
//!
//! NexaOS supports various types of kernel modules:
//! - **Filesystem** (Type 1): Provides filesystem implementations
//! - **Block Device** (Type 2): Block device drivers
//! - **Character Device** (Type 3): Character device drivers
//! - **Network** (Type 4): Network interface drivers
//! - **Input** (Type 5): Input device drivers (keyboard, mouse, etc.)
//! - **Graphics** (Type 6): Graphics/display drivers
//! - **Sound** (Type 7): Audio drivers
//! - **Security** (Type 8): Security modules
//!
//! # License Compatibility
//!
//! NexaOS is MIT licensed. Kernel modules can use any compatible license.
//! The kernel tracks module licenses for informational purposes.

#![no_std]
#![allow(dead_code)]

// ============================================================================
// Module Types
// ============================================================================

/// Module type identifiers
pub mod module_type {
    /// Unknown/generic module type
    pub const UNKNOWN: u8 = 0;
    /// Filesystem driver
    pub const FILESYSTEM: u8 = 1;
    /// Block device driver
    pub const BLOCK_DEVICE: u8 = 2;
    /// Character device driver
    pub const CHAR_DEVICE: u8 = 3;
    /// Network interface driver
    pub const NETWORK: u8 = 4;
    /// Input device driver
    pub const INPUT: u8 = 5;
    /// Graphics/display driver
    pub const GRAPHICS: u8 = 6;
    /// Audio driver
    pub const SOUND: u8 = 7;
    /// Security module
    pub const SECURITY: u8 = 8;
}

// ============================================================================
// Module States
// ============================================================================

/// Module state identifiers
pub mod module_state {
    /// Module is loaded but not initialized
    pub const LOADED: u8 = 0;
    /// Module is currently initializing
    pub const INITIALIZING: u8 = 1;
    /// Module is running normally
    pub const RUNNING: u8 = 2;
    /// Module is being unloaded
    pub const UNLOADING: u8 = 3;
    /// Module encountered an error
    pub const ERROR: u8 = 4;
    /// Module is waiting for dependencies
    pub const WAITING_DEPS: u8 = 5;
}

// ============================================================================
// Error Codes
// ============================================================================

/// Module error codes (to be returned from module_init/module_exit)
pub mod error {
    /// Success
    pub const SUCCESS: i32 = 0;
    /// Generic error
    pub const ERROR: i32 = -1;
    /// Out of memory
    pub const ENOMEM: i32 = -12;
    /// Invalid argument
    pub const EINVAL: i32 = -22;
    /// Resource busy
    pub const EBUSY: i32 = -16;
    /// No such device
    pub const ENODEV: i32 = -19;
    /// Operation not supported
    pub const ENOTSUP: i32 = -95;
    /// Permission denied
    pub const EPERM: i32 = -1;
    /// File exists (already registered)
    pub const EEXIST: i32 = -17;
    /// No such file or directory
    pub const ENOENT: i32 = -2;
}

// ============================================================================
// Kernel API Declarations
// ============================================================================

extern "C" {
    // =========================
    // Logging Functions
    // =========================
    /// Log an informational message
    pub fn kmod_log_info(msg: *const u8, len: usize);
    /// Log an error message
    pub fn kmod_log_error(msg: *const u8, len: usize);
    /// Log a warning message
    pub fn kmod_log_warn(msg: *const u8, len: usize);
    /// Log a debug message
    pub fn kmod_log_debug(msg: *const u8, len: usize);

    // =========================
    // Memory Management
    // =========================
    /// Allocate memory with specified alignment
    pub fn kmod_alloc(size: usize, align: usize) -> *mut u8;
    /// Allocate zeroed memory with specified alignment
    pub fn kmod_alloc_zeroed(size: usize, align: usize) -> *mut u8;
    /// Deallocate previously allocated memory
    pub fn kmod_dealloc(ptr: *mut u8, size: usize, align: usize);
    /// Copy memory regions
    pub fn kmod_memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
    /// Set memory to a value
    pub fn kmod_memset(dest: *mut u8, c: i32, n: usize) -> *mut u8;
    /// Move memory (handles overlapping regions)
    pub fn kmod_memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;

    // =========================
    // String Operations
    // =========================
    /// Get length of null-terminated string
    pub fn kmod_strlen(s: *const u8) -> usize;
    /// Compare two strings
    pub fn kmod_strcmp(s1: *const u8, s2: *const u8) -> i32;
    /// Compare two strings up to n bytes
    pub fn kmod_strncmp(s1: *const u8, s2: *const u8, n: usize) -> i32;
    /// Copy a string
    pub fn kmod_strcpy(dest: *mut u8, src: *const u8) -> *mut u8;
    /// Copy a string up to n bytes
    pub fn kmod_strncpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;

    // =========================
    // Synchronization
    // =========================
    /// Acquire a spinlock
    pub fn kmod_spinlock_lock(lock: *mut u8);
    /// Release a spinlock
    pub fn kmod_spinlock_unlock(lock: *mut u8);
    /// Try to acquire a spinlock (non-blocking)
    pub fn kmod_spinlock_trylock(lock: *mut u8) -> bool;

    // =========================
    // Filesystem Registration
    // =========================
    /// Register a filesystem with the kernel
    pub fn kmod_register_fs(
        name: *const u8,
        name_len: usize,
        init_fn: usize,
        lookup_fn: usize,
    ) -> i32;
    /// Unregister a filesystem
    pub fn kmod_unregister_fs(name: *const u8, name_len: usize) -> i32;

    // =========================
    // Block Device Registration
    // =========================
    /// Register a block device driver
    pub fn kmod_register_blkdev(major: u32, name: *const u8, name_len: usize) -> i32;
    /// Unregister a block device driver
    pub fn kmod_unregister_blkdev(major: u32, name: *const u8, name_len: usize) -> i32;

    // =========================
    // Character Device Registration
    // =========================
    /// Register a character device driver
    pub fn kmod_register_chrdev(major: u32, name: *const u8, name_len: usize) -> i32;
    /// Unregister a character device driver
    pub fn kmod_unregister_chrdev(major: u32, name: *const u8, name_len: usize) -> i32;

    // =========================
    // Time and Scheduling
    // =========================
    /// Get current jiffies (kernel ticks)
    pub fn kmod_get_jiffies() -> u64;
    /// Sleep for specified milliseconds
    pub fn kmod_msleep(ms: u32);
    /// Busy-wait for specified microseconds
    pub fn kmod_udelay(us: u32);
    /// Yield to scheduler
    pub fn kmod_schedule();

    // =========================
    // Interrupt Handling
    // =========================
    /// Request an IRQ handler
    pub fn kmod_request_irq(irq: u32, handler: usize, flags: u32, name: *const u8) -> i32;
    /// Free a previously requested IRQ
    pub fn kmod_free_irq(irq: u32, handler: usize);
    /// Disable an IRQ line
    pub fn kmod_disable_irq(irq: u32);
    /// Enable an IRQ line
    pub fn kmod_enable_irq(irq: u32);

    // =========================
    // Module Dependency Management
    // =========================
    /// Increment module reference count
    pub fn kmod_module_get(name: *const u8, name_len: usize) -> i32;
    /// Decrement module reference count
    pub fn kmod_module_put(name: *const u8, name_len: usize) -> i32;
    /// Try to increment reference count (non-blocking)
    pub fn kmod_try_module_get(name: *const u8, name_len: usize) -> bool;

    // =========================
    // Kernel Printing
    // =========================
    /// Kernel printk (formatted print)
    pub fn kmod_printk(level: i32, msg: *const u8, len: usize);

    // =========================
    // Error Handling
    // =========================
    /// Trigger kernel panic
    pub fn kmod_panic(msg: *const u8, len: usize) -> !;
    /// Report a kernel bug (non-fatal)
    pub fn kmod_bug(msg: *const u8, len: usize);
}

// ============================================================================
// Logging Macros
// ============================================================================

/// Log an informational message
/// 
/// # Example
/// ```rust,ignore
/// mod_info!("Module initialized successfully");
/// mod_info!(b"Raw byte message");
/// ```
#[macro_export]
macro_rules! mod_info {
    ($msg:expr) => {
        unsafe { $crate::kmod_log_info($msg.as_ptr(), $msg.len()) }
    };
}

/// Log an error message
#[macro_export]
macro_rules! mod_error {
    ($msg:expr) => {
        unsafe { $crate::kmod_log_error($msg.as_ptr(), $msg.len()) }
    };
}

/// Log a warning message
#[macro_export]
macro_rules! mod_warn {
    ($msg:expr) => {
        unsafe { $crate::kmod_log_warn($msg.as_ptr(), $msg.len()) }
    };
}

/// Log a debug message
#[macro_export]
macro_rules! mod_debug {
    ($msg:expr) => {
        unsafe { $crate::kmod_log_debug($msg.as_ptr(), $msg.len()) }
    };
}

// ============================================================================
// Module Metadata Macro
// ============================================================================

/// Declare module metadata that will be embedded in the .modinfo section
///
/// This macro creates the necessary metadata strings that the kernel module
/// loader will parse to understand the module's properties.
///
/// # Example
///
/// ```rust,ignore
/// module_metadata! {
///     name: "my_driver",
///     version: "1.0.0",
///     author: "John Doe <john@example.com>",
///     description: "My awesome driver module",
///     license: "GPL",
///     depends: "usb_core,scsi",  // Optional: comma-separated dependencies
/// }
/// ```
#[macro_export]
macro_rules! module_metadata {
    (
        name: $name:expr,
        version: $version:expr,
        author: $author:expr,
        description: $desc:expr,
        license: $license:expr
        $(, depends: $depends:expr)?
        $(,)?
    ) => {
        #[link_section = ".modinfo"]
        #[used]
        static __MODINFO_NAME: [u8; { concat!("name=", $name, "\0").len() }] =
            *concat!("name=", $name, "\0").as_bytes().try_into().unwrap();

        #[link_section = ".modinfo"]
        #[used]
        static __MODINFO_VERSION: [u8; { concat!("version=", $version, "\0").len() }] =
            *concat!("version=", $version, "\0").as_bytes().try_into().unwrap();

        #[link_section = ".modinfo"]
        #[used]
        static __MODINFO_AUTHOR: [u8; { concat!("author=", $author, "\0").len() }] =
            *concat!("author=", $author, "\0").as_bytes().try_into().unwrap();

        #[link_section = ".modinfo"]
        #[used]
        static __MODINFO_DESCRIPTION: [u8; { concat!("description=", $desc, "\0").len() }] =
            *concat!("description=", $desc, "\0").as_bytes().try_into().unwrap();

        #[link_section = ".modinfo"]
        #[used]
        static __MODINFO_LICENSE: [u8; { concat!("license=", $license, "\0").len() }] =
            *concat!("license=", $license, "\0").as_bytes().try_into().unwrap();

        $(
            #[link_section = ".modinfo"]
            #[used]
            static __MODINFO_DEPENDS: [u8; { concat!("depends=", $depends, "\0").len() }] =
                *concat!("depends=", $depends, "\0").as_bytes().try_into().unwrap();
        )?

        /// Module name constant for runtime use
        pub const MODULE_NAME: &str = $name;
        /// Module version constant for runtime use
        pub const MODULE_VERSION: &str = $version;
    };
}

/// Declare module parameters
///
/// Module parameters can be set during module load and modified at runtime.
///
/// # Example
///
/// ```rust,ignore
/// module_param! {
///     /// Enable debug mode
///     debug: bool = false,
///     /// Buffer size in bytes
///     bufsize: u32 = 4096,
/// }
/// ```
#[macro_export]
macro_rules! module_param {
    (
        $(
            $(#[$meta:meta])*
            $name:ident : $type:ty = $default:expr
        ),* $(,)?
    ) => {
        $(
            $(#[$meta])*
            #[no_mangle]
            pub static mut $name: $type = $default;

            #[link_section = ".modinfo"]
            #[used]
            static $crate::paste::paste!([<__MODPARAM_ $name:upper>]): [u8; {
                concat!("parm=", stringify!($name), ":", stringify!($type), "\0").len()
            }] = *concat!("parm=", stringify!($name), ":", stringify!($type), "\0")
                .as_bytes()
                .try_into()
                .unwrap();
        )*
    };
}

// ============================================================================
// Module Init/Exit Macros
// ============================================================================

/// Declare the module initialization function
///
/// # Example
///
/// ```rust,ignore
/// module_init!(my_init_function);
///
/// fn my_init_function() -> i32 {
///     mod_info!("Module loaded!");
///     0 // Success
/// }
/// ```
#[macro_export]
macro_rules! module_init {
    ($func:ident) => {
        #[no_mangle]
        pub extern "C" fn module_init() -> i32 {
            $func()
        }
    };
}

/// Declare the module exit/cleanup function
///
/// # Example
///
/// ```rust,ignore
/// module_exit!(my_exit_function);
///
/// fn my_exit_function() -> i32 {
///     mod_info!("Module unloaded!");
///     0
/// }
/// ```
#[macro_export]
macro_rules! module_exit {
    ($func:ident) => {
        #[no_mangle]
        pub extern "C" fn module_exit() -> i32 {
            $func()
        }
    };
}

// ============================================================================
// Memory Allocation Helpers
// ============================================================================

/// Allocate memory of a specific type
///
/// Returns a raw pointer that must be freed with `kfree`.
///
/// # Safety
/// The returned pointer must be properly freed to avoid memory leaks.
#[inline]
pub unsafe fn kmalloc<T>() -> *mut T {
    kmod_alloc(core::mem::size_of::<T>(), core::mem::align_of::<T>()) as *mut T
}

/// Allocate zeroed memory of a specific type
///
/// # Safety
/// The returned pointer must be properly freed to avoid memory leaks.
#[inline]
pub unsafe fn kzalloc<T>() -> *mut T {
    kmod_alloc_zeroed(core::mem::size_of::<T>(), core::mem::align_of::<T>()) as *mut T
}

/// Free memory allocated with kmalloc/kzalloc
///
/// # Safety
/// - `ptr` must have been allocated by kmalloc or kzalloc
/// - `ptr` must not be freed more than once
#[inline]
pub unsafe fn kfree<T>(ptr: *mut T) {
    kmod_dealloc(ptr as *mut u8, core::mem::size_of::<T>(), core::mem::align_of::<T>())
}

/// Allocate an array of elements
///
/// # Safety
/// The returned pointer must be properly freed.
#[inline]
pub unsafe fn kmalloc_array<T>(count: usize) -> *mut T {
    let size = core::mem::size_of::<T>().saturating_mul(count);
    kmod_alloc(size, core::mem::align_of::<T>()) as *mut T
}

/// Allocate a zeroed array of elements
///
/// # Safety
/// The returned pointer must be properly freed.
#[inline]
pub unsafe fn kzalloc_array<T>(count: usize) -> *mut T {
    let size = core::mem::size_of::<T>().saturating_mul(count);
    kmod_alloc_zeroed(size, core::mem::align_of::<T>()) as *mut T
}

/// Free an array allocated with kmalloc_array/kzalloc_array
///
/// # Safety
/// Same requirements as kfree.
#[inline]
pub unsafe fn kfree_array<T>(ptr: *mut T, count: usize) {
    let size = core::mem::size_of::<T>().saturating_mul(count);
    kmod_dealloc(ptr as *mut u8, size, core::mem::align_of::<T>())
}

// ============================================================================
// Spinlock Helper
// ============================================================================

/// A simple spinlock wrapper for kernel modules
#[repr(C)]
pub struct Spinlock {
    lock: u8,
}

impl Spinlock {
    /// Create a new unlocked spinlock
    pub const fn new() -> Self {
        Self { lock: 0 }
    }

    /// Acquire the spinlock (blocking)
    pub fn lock(&mut self) {
        unsafe { kmod_spinlock_lock(&mut self.lock) }
    }

    /// Release the spinlock
    pub fn unlock(&mut self) {
        unsafe { kmod_spinlock_unlock(&mut self.lock) }
    }

    /// Try to acquire the spinlock (non-blocking)
    pub fn try_lock(&mut self) -> bool {
        unsafe { kmod_spinlock_trylock(&mut self.lock) }
    }
}

impl Default for Spinlock {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Module Reference Counting Helper
// ============================================================================

/// RAII guard for module reference counting
pub struct ModuleRef {
    name: &'static [u8],
    held: bool,
}

impl ModuleRef {
    /// Try to get a reference to a module
    pub fn try_get(name: &'static str) -> Option<Self> {
        let name_bytes = name.as_bytes();
        let success = unsafe { kmod_try_module_get(name_bytes.as_ptr(), name_bytes.len()) };
        if success {
            Some(Self {
                name: name_bytes,
                held: true,
            })
        } else {
            None
        }
    }

    /// Get a reference to a module (blocking)
    pub fn get(name: &'static str) -> Result<Self, i32> {
        let name_bytes = name.as_bytes();
        let result = unsafe { kmod_module_get(name_bytes.as_ptr(), name_bytes.len()) };
        if result == 0 {
            Ok(Self {
                name: name_bytes,
                held: true,
            })
        } else {
            Err(result)
        }
    }
}

impl Drop for ModuleRef {
    fn drop(&mut self) {
        if self.held {
            unsafe { kmod_module_put(self.name.as_ptr(), self.name.len()) };
        }
    }
}

// ============================================================================
// IRQ Handler Registration
// ============================================================================

/// IRQ handler flags
pub mod irq_flags {
    /// No special flags
    pub const NONE: u32 = 0;
    /// Interrupt is shared between devices
    pub const SHARED: u32 = 1 << 0;
    /// Interrupt is edge-triggered
    pub const TRIGGER_RISING: u32 = 1 << 1;
    /// Interrupt is falling-edge triggered
    pub const TRIGGER_FALLING: u32 = 1 << 2;
    /// Interrupt is level-triggered high
    pub const TRIGGER_HIGH: u32 = 1 << 3;
    /// Interrupt is level-triggered low
    pub const TRIGGER_LOW: u32 = 1 << 4;
}

// ============================================================================
// Printk Log Levels
// ============================================================================

/// Printk log levels (compatible with Linux kernel)
pub mod printk_level {
    /// Emergency - system is unusable
    pub const KERN_EMERG: i32 = 0;
    /// Alert - action must be taken immediately
    pub const KERN_ALERT: i32 = 1;
    /// Critical - critical conditions
    pub const KERN_CRIT: i32 = 2;
    /// Error - error conditions
    pub const KERN_ERR: i32 = 3;
    /// Warning - warning conditions
    pub const KERN_WARNING: i32 = 4;
    /// Notice - normal but significant condition
    pub const KERN_NOTICE: i32 = 5;
    /// Info - informational
    pub const KERN_INFO: i32 = 6;
    /// Debug - debug-level messages
    pub const KERN_DEBUG: i32 = 7;
}

// ============================================================================
// Panic macro for kernel modules
// ============================================================================

/// Trigger a kernel panic with a message
#[macro_export]
macro_rules! kpanic {
    ($msg:expr) => {{
        unsafe { $crate::kmod_panic($msg.as_ptr(), $msg.len()) }
    }};
}

/// Report a kernel bug (non-fatal warning)
#[macro_export]
macro_rules! kbug {
    ($msg:expr) => {{
        unsafe { $crate::kmod_bug($msg.as_ptr(), $msg.len()) }
    }};
}
