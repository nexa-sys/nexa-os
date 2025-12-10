//! Environment functions
//!
//! Provides getenv, setenv, and related environment functions.
//! Note: getcwd and strerror are now in fs.rs and string.rs respectively.

use crate::{c_char, c_int, size_t};
use core::ptr;

// ============================================================================
// Environment Variable Storage
// ============================================================================

/// Maximum number of environment variables
const MAX_ENV_VARS: usize = 64;
/// Maximum length of an environment variable entry (name=value)
const MAX_ENV_ENTRY_LEN: usize = 256;

/// Environment variable storage
static mut ENV_STORAGE: [[u8; MAX_ENV_ENTRY_LEN]; MAX_ENV_VARS] =
    [[0; MAX_ENV_ENTRY_LEN]; MAX_ENV_VARS];
static mut ENV_COUNT: usize = 0;

// ============================================================================
// Environment Functions
// ============================================================================

/// Get environment variable value
#[no_mangle]
pub unsafe extern "C" fn getenv(name: *const c_char) -> *mut c_char {
    if name.is_null() {
        return ptr::null_mut();
    }

    let name_len = crate::strlen(name as *const u8);
    if name_len == 0 {
        return ptr::null_mut();
    }

    for i in 0..ENV_COUNT {
        let entry = &ENV_STORAGE[i];
        // Find '=' in entry
        let mut eq_pos = 0;
        while eq_pos < MAX_ENV_ENTRY_LEN && entry[eq_pos] != 0 && entry[eq_pos] != b'=' {
            eq_pos += 1;
        }

        if eq_pos == name_len && entry[eq_pos] == b'=' {
            // Check if names match
            let name_slice = core::slice::from_raw_parts(name as *const u8, name_len);
            if &entry[..name_len] == name_slice {
                // Return pointer to value (after '=')
                return entry.as_ptr().add(eq_pos + 1) as *mut c_char;
            }
        }
    }

    ptr::null_mut()
}

/// Set environment variable
#[no_mangle]
pub unsafe extern "C" fn setenv(
    name: *const c_char,
    value: *const c_char,
    overwrite: c_int,
) -> c_int {
    if name.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let name_len = crate::strlen(name as *const u8);
    if name_len == 0 {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    // Check if name contains '='
    let name_slice = core::slice::from_raw_parts(name as *const u8, name_len);
    if name_slice.contains(&b'=') {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let value_len = if value.is_null() {
        0
    } else {
        crate::strlen(value as *const u8)
    };
    let entry_len = name_len + 1 + value_len; // name=value

    if entry_len >= MAX_ENV_ENTRY_LEN {
        crate::set_errno(crate::ENOMEM);
        return -1;
    }

    // Check if variable already exists
    for i in 0..ENV_COUNT {
        let entry = &mut ENV_STORAGE[i];
        let mut eq_pos = 0;
        while eq_pos < MAX_ENV_ENTRY_LEN && entry[eq_pos] != 0 && entry[eq_pos] != b'=' {
            eq_pos += 1;
        }

        if eq_pos == name_len && entry[eq_pos] == b'=' && &entry[..name_len] == name_slice {
            // Found existing entry
            if overwrite == 0 {
                return 0; // Don't overwrite
            }

            // Update value
            entry[..name_len].copy_from_slice(name_slice);
            entry[name_len] = b'=';
            if !value.is_null() {
                let value_slice = core::slice::from_raw_parts(value as *const u8, value_len);
                entry[name_len + 1..name_len + 1 + value_len].copy_from_slice(value_slice);
            }
            entry[entry_len] = 0;
            return 0;
        }
    }

    // Add new entry
    if ENV_COUNT >= MAX_ENV_VARS {
        crate::set_errno(crate::ENOMEM);
        return -1;
    }

    let entry = &mut ENV_STORAGE[ENV_COUNT];
    entry[..name_len].copy_from_slice(name_slice);
    entry[name_len] = b'=';
    if !value.is_null() {
        let value_slice = core::slice::from_raw_parts(value as *const u8, value_len);
        entry[name_len + 1..name_len + 1 + value_len].copy_from_slice(value_slice);
    }
    entry[entry_len] = 0;
    ENV_COUNT += 1;

    0
}

/// Remove environment variable
#[no_mangle]
pub unsafe extern "C" fn unsetenv(name: *const c_char) -> c_int {
    if name.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let name_len = crate::strlen(name as *const u8);
    if name_len == 0 {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let name_slice = core::slice::from_raw_parts(name as *const u8, name_len);

    // Check if name contains '='
    if name_slice.contains(&b'=') {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    // Find and remove entry
    for i in 0..ENV_COUNT {
        let entry = &ENV_STORAGE[i];
        let mut eq_pos = 0;
        while eq_pos < MAX_ENV_ENTRY_LEN && entry[eq_pos] != 0 && entry[eq_pos] != b'=' {
            eq_pos += 1;
        }

        if eq_pos == name_len && entry[eq_pos] == b'=' && &entry[..name_len] == name_slice {
            // Found entry - remove by shifting remaining entries
            for j in i..ENV_COUNT - 1 {
                ENV_STORAGE[j] = ENV_STORAGE[j + 1];
            }
            ENV_STORAGE[ENV_COUNT - 1] = [0; MAX_ENV_ENTRY_LEN];
            ENV_COUNT -= 1;
            return 0;
        }
    }

    0 // Variable not found is not an error
}

/// Put environment variable (name=value format)
#[no_mangle]
pub unsafe extern "C" fn putenv(string: *mut c_char) -> c_int {
    if string.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    // Find '=' in string
    let mut eq_pos = 0;
    while *(string.add(eq_pos)) != 0 && *(string.add(eq_pos)) as u8 != b'=' {
        eq_pos += 1;
    }

    if *(string.add(eq_pos)) == 0 {
        // No '=' found - unset the variable
        return unsetenv(string);
    }

    // Split into name and value
    let name_len = eq_pos;
    if name_len == 0 {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    // Temporarily null-terminate the name
    let old_char = *(string.add(eq_pos));
    *(string.add(eq_pos)) = 0;

    let result = setenv(string, string.add(eq_pos + 1), 1);

    // Restore original character
    *(string.add(eq_pos)) = old_char;

    result
}

/// Clear all environment variables
#[no_mangle]
pub unsafe extern "C" fn clearenv() -> c_int {
    for i in 0..MAX_ENV_VARS {
        ENV_STORAGE[i] = [0; MAX_ENV_ENTRY_LEN];
    }
    ENV_COUNT = 0;
    0
}

// ============================================================================
// Auxiliary Vector
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn getauxval(_type: u64) -> u64 {
    0
}

// ============================================================================
// Helper Functions
// ============================================================================

pub fn simple_itoa(mut n: u64, buf: &mut [u8]) -> &[u8] {
    if n == 0 {
        buf[0] = b'0';
        return &buf[0..1];
    }
    let mut i = 0;
    while n > 0 {
        buf[i] = (n % 10) as u8 + b'0';
        n /= 10;
        i += 1;
    }
    buf[..i].reverse();
    &buf[..i]
}
