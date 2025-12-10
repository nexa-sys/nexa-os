//! OpenSSL Version Compatibility Functions
//!
//! Version query functions compatible with OpenSSL API.

/// Our version (pretending to be OpenSSL 3.0.x for compatibility)
pub const OPENSSL_VERSION_NUMBER: u64 = 0x30000000;

/// OpenSSL version constants
pub mod version {
    pub const OPENSSL_VERSION: i32 = 0;
    pub const OPENSSL_CFLAGS: i32 = 1;
    pub const OPENSSL_BUILT_ON: i32 = 2;
    pub const OPENSSL_PLATFORM: i32 = 3;
    pub const OPENSSL_DIR: i32 = 4;
    pub const OPENSSL_ENGINES_DIR: i32 = 5;
    pub const OPENSSL_VERSION_STRING: i32 = 6;
    pub const OPENSSL_FULL_VERSION_STRING: i32 = 7;
    pub const OPENSSL_MODULES_DIR: i32 = 8;
    pub const OPENSSL_CPU_INFO: i32 = 9;
}

// Version strings
static VERSION_STRING: &[u8] = b"NexaOS ncryptolib 1.0.0 (OpenSSL 3.0 compatible)\0";
static CFLAGS_STRING: &[u8] = b"compiler: rustc\0";
static BUILT_ON_STRING: &[u8] = b"built on: NexaOS\0";
static PLATFORM_STRING: &[u8] = b"platform: x86_64-nexaos\0";
static DIR_STRING: &[u8] = b"OPENSSLDIR: /etc/ssl\0";
static ENGINES_DIR_STRING: &[u8] = b"ENGINESDIR: /lib64/engines\0";
static MODULES_DIR_STRING: &[u8] = b"MODULESDIR: /lib64/ossl-modules\0";
static CPU_INFO_STRING: &[u8] = b"CPUINFO: x86_64\0";

// ============================================================================
// C ABI Exports
// NOTE: OpenSSL_version, OpenSSL_version_num, SSLeay, SSLeay_version
// are defined in lib.rs. This module contains additional version APIs.
// ============================================================================

// NOTE: SSLeay_version, OPENSSL_init_crypto, and related functions are defined in lib.rs
// NOTE: OPENSSL_init_ssl is defined in nssl (libssl), not here in libcrypto

/// OPENSSL_cleanup - Cleanup OpenSSL
#[no_mangle]
pub extern "C" fn OPENSSL_cleanup() {
    // No-op: Rust handles cleanup automatically
}

/// CRYPTO_num_locks - Get number of locks (deprecated)
#[no_mangle]
pub extern "C" fn CRYPTO_num_locks() -> i32 {
    0
}

/// CRYPTO_set_locking_callback - Set locking callback (deprecated)
#[no_mangle]
pub extern "C" fn CRYPTO_set_locking_callback(_func: *const core::ffi::c_void) {
    // No-op: Rust's mutex handles this
}

/// CRYPTO_set_id_callback - Set thread ID callback (deprecated)
#[no_mangle]
pub extern "C" fn CRYPTO_set_id_callback(_func: *const core::ffi::c_void) {
    // No-op
}

/// CRYPTO_THREADID_set_callback - Set thread ID callback
#[no_mangle]
pub extern "C" fn CRYPTO_THREADID_set_callback(_func: *const core::ffi::c_void) -> i32 {
    1 // Success
}

/// CRYPTO_THREADID_set_numeric - Set numeric thread ID
#[no_mangle]
pub extern "C" fn CRYPTO_THREADID_set_numeric(_id: *mut core::ffi::c_void, _val: u64) {
    // No-op
}

/// CRYPTO_THREADID_set_pointer - Set pointer thread ID
#[no_mangle]
pub extern "C" fn CRYPTO_THREADID_set_pointer(
    _id: *mut core::ffi::c_void,
    _ptr: *mut core::ffi::c_void,
) {
    // No-op
}

/// OPENSSL_INIT options
pub mod init_opts {
    pub const OPENSSL_INIT_NO_LOAD_CRYPTO_STRINGS: u64 = 0x00000001;
    pub const OPENSSL_INIT_LOAD_CRYPTO_STRINGS: u64 = 0x00000002;
    pub const OPENSSL_INIT_ADD_ALL_CIPHERS: u64 = 0x00000004;
    pub const OPENSSL_INIT_ADD_ALL_DIGESTS: u64 = 0x00000008;
    pub const OPENSSL_INIT_NO_ADD_ALL_CIPHERS: u64 = 0x00000010;
    pub const OPENSSL_INIT_NO_ADD_ALL_DIGESTS: u64 = 0x00000020;
    pub const OPENSSL_INIT_LOAD_CONFIG: u64 = 0x00000040;
    pub const OPENSSL_INIT_NO_LOAD_CONFIG: u64 = 0x00000080;
    pub const OPENSSL_INIT_ASYNC: u64 = 0x00000100;
    pub const OPENSSL_INIT_ENGINE_RDRAND: u64 = 0x00000200;
    pub const OPENSSL_INIT_ENGINE_DYNAMIC: u64 = 0x00000400;
    pub const OPENSSL_INIT_ENGINE_OPENSSL: u64 = 0x00000800;
    pub const OPENSSL_INIT_ENGINE_CRYPTODEV: u64 = 0x00001000;
    pub const OPENSSL_INIT_ENGINE_CAPI: u64 = 0x00002000;
    pub const OPENSSL_INIT_ENGINE_PADLOCK: u64 = 0x00004000;
    pub const OPENSSL_INIT_ENGINE_AFALG: u64 = 0x00008000;
    pub const OPENSSL_INIT_NO_LOAD_SSL_STRINGS: u64 = 0x00100000;
    pub const OPENSSL_INIT_LOAD_SSL_STRINGS: u64 = 0x00200000;
    pub const OPENSSL_INIT_NO_ATEXIT: u64 = 0x00080000;
}

/// Feature flags for OPENSSL_issetugid
#[no_mangle]
pub extern "C" fn OPENSSL_issetugid() -> i32 {
    0 // Not setuid/setgid
}

/// CRYPTO_get_ex_new_index - Get new ex_data index
#[no_mangle]
pub extern "C" fn CRYPTO_get_ex_new_index(
    _class_index: i32,
    _argl: i64,
    _argp: *mut core::ffi::c_void,
    _new_func: *const core::ffi::c_void,
    _dup_func: *const core::ffi::c_void,
    _free_func: *const core::ffi::c_void,
) -> i32 {
    static COUNTER: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
    COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

/// Library feature checking
#[no_mangle]
pub extern "C" fn OPENSSL_isservice() -> i32 {
    0
}
