//! String and error handling functions
//!
//! Provides POSIX string functions and error message handling.

use crate::{c_char, c_int, c_void, size_t};
use core::ptr;

// ============================================================================
// Error Messages
// ============================================================================

/// Error number to message mapping
static ERROR_MESSAGES: &[(&str, i32)] = &[
    ("Success", 0),
    ("Operation not permitted", 1),                 // EPERM
    ("No such file or directory", 2),               // ENOENT
    ("No such process", 3),                         // ESRCH
    ("Interrupted system call", 4),                 // EINTR
    ("Input/output error", 5),                      // EIO
    ("No such device or address", 6),               // ENXIO
    ("Argument list too long", 7),                  // E2BIG
    ("Exec format error", 8),                       // ENOEXEC
    ("Bad file descriptor", 9),                     // EBADF
    ("No child processes", 10),                     // ECHILD
    ("Resource temporarily unavailable", 11),       // EAGAIN / EWOULDBLOCK
    ("Cannot allocate memory", 12),                 // ENOMEM
    ("Permission denied", 13),                      // EACCES
    ("Bad address", 14),                            // EFAULT
    ("Block device required", 15),                  // ENOTBLK
    ("Device or resource busy", 16),                // EBUSY
    ("File exists", 17),                            // EEXIST
    ("Invalid cross-device link", 18),              // EXDEV
    ("No such device", 19),                         // ENODEV
    ("Not a directory", 20),                        // ENOTDIR
    ("Is a directory", 21),                         // EISDIR
    ("Invalid argument", 22),                       // EINVAL
    ("Too many open files in system", 23),          // ENFILE
    ("Too many open files", 24),                    // EMFILE
    ("Inappropriate ioctl for device", 25),         // ENOTTY
    ("Text file busy", 26),                         // ETXTBSY
    ("File too large", 27),                         // EFBIG
    ("No space left on device", 28),                // ENOSPC
    ("Illegal seek", 29),                           // ESPIPE
    ("Read-only file system", 30),                  // EROFS
    ("Too many links", 31),                         // EMLINK
    ("Broken pipe", 32),                            // EPIPE
    ("Numerical argument out of domain", 33),       // EDOM
    ("Numerical result out of range", 34),          // ERANGE
    ("Resource deadlock avoided", 35),              // EDEADLK
    ("File name too long", 36),                     // ENAMETOOLONG
    ("No locks available", 37),                     // ENOLCK
    ("Function not implemented", 38),               // ENOSYS
    ("Directory not empty", 39),                    // ENOTEMPTY
    ("Too many levels of symbolic links", 40),      // ELOOP
    ("No message of desired type", 42),             // ENOMSG
    ("Identifier removed", 43),                     // EIDRM
    ("Channel number out of range", 44),            // ECHRNG
    ("Level 2 not synchronized", 45),               // EL2NSYNC
    ("Level 3 halted", 46),                         // EL3HLT
    ("Level 3 reset", 47),                          // EL3RST
    ("Link number out of range", 48),               // ELNRNG
    ("Protocol driver not attached", 49),           // EUNATCH
    ("No CSI structure available", 50),             // ENOCSI
    ("Level 2 halted", 51),                         // EL2HLT
    ("Invalid exchange", 52),                       // EBADE
    ("Invalid request descriptor", 53),             // EBADR
    ("Exchange full", 54),                          // EXFULL
    ("No anode", 55),                               // ENOANO
    ("Invalid request code", 56),                   // EBADRQC
    ("Invalid slot", 57),                           // EBADSLT
    ("Bad font file format", 59),                   // EBFONT
    ("Device not a stream", 60),                    // ENOSTR
    ("No data available", 61),                      // ENODATA
    ("Timer expired", 62),                          // ETIME
    ("Out of streams resources", 63),               // ENOSR
    ("Machine is not on the network", 64),          // ENONET
    ("Package not installed", 65),                  // ENOPKG
    ("Object is remote", 66),                       // EREMOTE
    ("Link has been severed", 67),                  // ENOLINK
    ("Advertise error", 68),                        // EADV
    ("Srmount error", 69),                          // ESRMNT
    ("Communication error on send", 70),            // ECOMM
    ("Protocol error", 71),                         // EPROTO
    ("Multihop attempted", 72),                     // EMULTIHOP
    ("RFS specific error", 73),                     // EDOTDOT
    ("Bad message", 74),                            // EBADMSG
    ("Value too large for defined data type", 75),  // EOVERFLOW
    ("Name not unique on network", 76),             // ENOTUNIQ
    ("File descriptor in bad state", 77),           // EBADFD
    ("Remote address changed", 78),                 // EREMCHG
    ("Can not access a needed shared library", 79), // ELIBACC
    ("Accessing a corrupted shared library", 80),   // ELIBBAD
    ("Connection reset by peer", 104),              // ECONNRESET
    ("No buffer space available", 105),             // ENOBUFS
    ("Connection refused", 111),                    // ECONNREFUSED
    ("Connection timed out", 110),                  // ETIMEDOUT
    ("Network is unreachable", 101),                // ENETUNREACH
];

/// Get error message string
#[no_mangle]
pub unsafe extern "C" fn strerror(errnum: c_int) -> *mut c_char {
    static mut BUFFER: [u8; 64] = [0; 64];

    for &(msg, num) in ERROR_MESSAGES {
        if num == errnum {
            let msg_bytes = msg.as_bytes();
            let len = core::cmp::min(msg_bytes.len(), 63);
            ptr::copy_nonoverlapping(msg_bytes.as_ptr(), BUFFER.as_mut_ptr(), len);
            BUFFER[len] = 0;
            return BUFFER.as_mut_ptr() as *mut c_char;
        }
    }

    // Unknown error - format as "Unknown error N"
    let prefix = b"Unknown error ";
    let prefix_len = prefix.len();
    ptr::copy_nonoverlapping(prefix.as_ptr(), BUFFER.as_mut_ptr(), prefix_len);

    // Convert error number to string
    let mut num = if errnum < 0 {
        BUFFER[prefix_len] = b'-';
        (-errnum) as u32
    } else {
        errnum as u32
    };

    let offset = if errnum < 0 {
        prefix_len + 1
    } else {
        prefix_len
    };

    if num == 0 {
        BUFFER[offset] = b'0';
        BUFFER[offset + 1] = 0;
    } else {
        let mut digits = [0u8; 10];
        let mut digit_count = 0;
        while num > 0 && digit_count < 10 {
            digits[digit_count] = (num % 10) as u8 + b'0';
            num /= 10;
            digit_count += 1;
        }
        for i in 0..digit_count {
            BUFFER[offset + i] = digits[digit_count - 1 - i];
        }
        BUFFER[offset + digit_count] = 0;
    }

    BUFFER.as_mut_ptr() as *mut c_char
}

/// Thread-safe strerror
#[no_mangle]
pub unsafe extern "C" fn strerror_r(errnum: c_int, buf: *mut c_char, buflen: size_t) -> c_int {
    if buf.is_null() || buflen == 0 {
        return crate::EINVAL;
    }

    for &(msg, num) in ERROR_MESSAGES {
        if num == errnum {
            let msg_bytes = msg.as_bytes();
            let len = core::cmp::min(msg_bytes.len(), buflen - 1);
            ptr::copy_nonoverlapping(msg_bytes.as_ptr(), buf as *mut u8, len);
            *(buf.add(len)) = 0;
            return 0;
        }
    }

    // Unknown error
    let prefix = b"Unknown error ";
    if buflen < prefix.len() + 5 {
        *(buf as *mut u8) = 0;
        return 34; // ERANGE
    }

    ptr::copy_nonoverlapping(prefix.as_ptr(), buf as *mut u8, prefix.len());
    let mut pos = prefix.len();

    let mut num = if errnum < 0 {
        *(buf as *mut u8).add(pos) = b'-';
        pos += 1;
        (-errnum) as u32
    } else {
        errnum as u32
    };

    if num == 0 {
        *(buf as *mut u8).add(pos) = b'0';
        pos += 1;
    } else {
        let mut digits = [0u8; 10];
        let mut digit_count = 0;
        while num > 0 && digit_count < 10 {
            digits[digit_count] = (num % 10) as u8 + b'0';
            num /= 10;
            digit_count += 1;
        }
        for i in 0..digit_count {
            if pos < buflen - 1 {
                *(buf as *mut u8).add(pos) = digits[digit_count - 1 - i];
                pos += 1;
            }
        }
    }
    *(buf as *mut u8).add(pos) = 0;

    0
}

/// XPG strerror_r variant
#[no_mangle]
pub unsafe extern "C" fn __xpg_strerror_r(
    errnum: c_int,
    buf: *mut c_char,
    buflen: size_t,
) -> c_int {
    strerror_r(errnum, buf, buflen)
}

/// Print error message
#[no_mangle]
pub unsafe extern "C" fn perror(s: *const c_char) {
    let errno = crate::get_errno();
    let msg = strerror(errno);

    if !s.is_null() && *(s as *const u8) != 0 {
        let s_len = crate::strlen(s as *const u8);
        let _ = crate::write(2, s as *const c_void, s_len);
        let _ = crate::write(2, b": ".as_ptr() as *const c_void, 2);
    }

    let msg_len = crate::strlen(msg as *const u8);
    let _ = crate::write(2, msg as *const c_void, msg_len);
    let _ = crate::write(2, b"\n".as_ptr() as *const c_void, 1);
}

// ============================================================================
// String Functions
// ============================================================================

/// Copy string
#[no_mangle]
pub unsafe extern "C" fn strcpy(dest: *mut c_char, src: *const c_char) -> *mut c_char {
    if dest.is_null() || src.is_null() {
        return dest;
    }

    let mut i = 0isize;
    loop {
        let c = *src.offset(i);
        *dest.offset(i) = c;
        if c == 0 {
            break;
        }
        i += 1;
    }
    dest
}

/// Copy string with length limit
/// NOTE: We use volatile operations to prevent the compiler from "optimizing"
/// this into a call to strncpy (which would cause infinite recursion).
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn strncpy(dest: *mut c_char, src: *const c_char, n: size_t) -> *mut c_char {
    if dest.is_null() || src.is_null() {
        return dest;
    }

    let mut i = 0;
    while i < n {
        let c = core::ptr::read_volatile(src.add(i));
        core::ptr::write_volatile(dest.add(i), c);
        if c == 0 {
            // Pad with zeros
            i += 1;
            while i < n {
                core::ptr::write_volatile(dest.add(i), 0);
                i += 1;
            }
            break;
        }
        i += 1;
    }
    dest
}

/// Concatenate strings
#[no_mangle]
pub unsafe extern "C" fn strcat(dest: *mut c_char, src: *const c_char) -> *mut c_char {
    if dest.is_null() || src.is_null() {
        return dest;
    }

    let dest_len = crate::strlen(dest as *const u8);
    strcpy(dest.add(dest_len), src);
    dest
}

/// Concatenate strings with length limit
#[no_mangle]
pub unsafe extern "C" fn strncat(dest: *mut c_char, src: *const c_char, n: size_t) -> *mut c_char {
    if dest.is_null() || src.is_null() {
        return dest;
    }

    let dest_len = crate::strlen(dest as *const u8);
    let mut i = 0;
    while i < n {
        let c = *src.add(i);
        if c == 0 {
            break;
        }
        *dest.add(dest_len + i) = c;
        i += 1;
    }
    *dest.add(dest_len + i) = 0;
    dest
}

/// Compare strings
/// NOTE: We use volatile reads to prevent the compiler from "optimizing"
/// this into a call to strcmp (which would cause infinite recursion).
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn strcmp(s1: *const c_char, s2: *const c_char) -> c_int {
    if s1.is_null() || s2.is_null() {
        return 0;
    }

    let mut i = 0usize;
    loop {
        let c1 = core::ptr::read_volatile(s1.add(i)) as u8;
        let c2 = core::ptr::read_volatile(s2.add(i)) as u8;
        if c1 != c2 || c1 == 0 {
            return (c1 as c_int) - (c2 as c_int);
        }
        i += 1;
    }
}

/// Compare strings with length limit
/// NOTE: We use volatile reads to prevent the compiler from "optimizing"
/// this into a call to strncmp (which would cause infinite recursion).
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn strncmp(s1: *const c_char, s2: *const c_char, n: size_t) -> c_int {
    if s1.is_null() || s2.is_null() || n == 0 {
        return 0;
    }

    let mut i = 0;
    while i < n {
        let c1 = core::ptr::read_volatile(s1.add(i)) as u8;
        let c2 = core::ptr::read_volatile(s2.add(i)) as u8;
        if c1 != c2 || c1 == 0 {
            return (c1 as c_int) - (c2 as c_int);
        }
        i += 1;
    }
    0
}

/// Get string length
/// NOTE: We use volatile reads to prevent the compiler from "optimizing"
/// this into a call to strnlen (which would cause infinite recursion).
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn strnlen(s: *const c_char, maxlen: size_t) -> size_t {
    if s.is_null() {
        return 0;
    }

    let mut len = 0;
    while len < maxlen && core::ptr::read_volatile(s.add(len)) != 0 {
        len += 1;
    }
    len
}

/// Find character in string
/// NOTE: We use volatile reads to prevent compiler optimization.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn strchr(s: *const c_char, c: c_int) -> *mut c_char {
    if s.is_null() {
        return ptr::null_mut();
    }

    let c = c as u8;
    let mut i = 0usize;
    loop {
        let ch = core::ptr::read_volatile(s.add(i)) as u8;
        if ch == c {
            return s.add(i) as *mut c_char;
        }
        if ch == 0 {
            return ptr::null_mut();
        }
        i += 1;
    }
}

/// Find last occurrence of character in string
/// NOTE: We use volatile reads to prevent compiler optimization.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn strrchr(s: *const c_char, c: c_int) -> *mut c_char {
    if s.is_null() {
        return ptr::null_mut();
    }

    let c = c as u8;
    let mut result: *mut c_char = ptr::null_mut();
    let mut i = 0usize;
    loop {
        let ch = core::ptr::read_volatile(s.add(i)) as u8;
        if ch == c {
            result = s.add(i) as *mut c_char;
        }
        if ch == 0 {
            break;
        }
        i += 1;
    }
    result
}

/// Find substring
/// NOTE: We use volatile reads to prevent compiler optimization.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn strstr(haystack: *const c_char, needle: *const c_char) -> *mut c_char {
    if haystack.is_null() || needle.is_null() {
        return ptr::null_mut();
    }

    // Empty needle matches at start
    if core::ptr::read_volatile(needle) == 0 {
        return haystack as *mut c_char;
    }

    let needle_len = crate::strlen(needle as *const u8);
    let haystack_len = crate::strlen(haystack as *const u8);

    if needle_len > haystack_len {
        return ptr::null_mut();
    }

    let mut i = 0;
    while i <= haystack_len - needle_len {
        let mut match_found = true;
        let mut j = 0;
        while j < needle_len {
            let hc = core::ptr::read_volatile(haystack.add(i + j));
            let nc = core::ptr::read_volatile(needle.add(j));
            if hc != nc {
                match_found = false;
                break;
            }
            j += 1;
        }
        if match_found {
            return haystack.add(i) as *mut c_char;
        }
        i += 1;
    }

    ptr::null_mut()
}

/// Duplicate string
#[no_mangle]
pub unsafe extern "C" fn strdup(s: *const c_char) -> *mut c_char {
    if s.is_null() {
        return ptr::null_mut();
    }

    let len = crate::strlen(s as *const u8);
    let ptr = crate::malloc(len + 1) as *mut c_char;
    if !ptr.is_null() {
        ptr::copy_nonoverlapping(s, ptr, len + 1);
    }
    ptr
}

/// Duplicate string with length limit
#[no_mangle]
pub unsafe extern "C" fn strndup(s: *const c_char, n: size_t) -> *mut c_char {
    if s.is_null() {
        return ptr::null_mut();
    }

    let len = strnlen(s, n);
    let ptr = crate::malloc(len + 1) as *mut c_char;
    if !ptr.is_null() {
        ptr::copy_nonoverlapping(s, ptr, len);
        *ptr.add(len) = 0;
    }
    ptr
}

/// Find character in memory
#[no_mangle]
pub unsafe extern "C" fn memchr(s: *const c_void, c: c_int, n: size_t) -> *mut c_void {
    if s.is_null() {
        return ptr::null_mut();
    }

    let s = s as *const u8;
    let c = c as u8;
    for i in 0..n {
        if *s.add(i) == c {
            return s.add(i) as *mut c_void;
        }
    }
    ptr::null_mut()
}

/// Find character in memory (reverse)
#[no_mangle]
pub unsafe extern "C" fn memrchr(s: *const c_void, c: c_int, n: size_t) -> *mut c_void {
    if s.is_null() || n == 0 {
        return ptr::null_mut();
    }

    let s = s as *const u8;
    let c = c as u8;
    let mut i = n;
    while i > 0 {
        i -= 1;
        if *s.add(i) == c {
            return s.add(i) as *mut c_void;
        }
    }
    ptr::null_mut()
}

/// Calculate length of initial segment matching characters
#[no_mangle]
pub unsafe extern "C" fn strspn(s: *const c_char, accept: *const c_char) -> size_t {
    if s.is_null() || accept.is_null() {
        return 0;
    }

    let mut count = 0;
    let mut p = s;
    while *p != 0 {
        let c = *p;
        let mut found = false;
        let mut a = accept;
        while *a != 0 {
            if c == *a {
                found = true;
                break;
            }
            a = a.add(1);
        }
        if !found {
            break;
        }
        count += 1;
        p = p.add(1);
    }
    count
}

/// Calculate length of initial segment not matching characters
#[no_mangle]
pub unsafe extern "C" fn strcspn(s: *const c_char, reject: *const c_char) -> size_t {
    if s.is_null() || reject.is_null() {
        return 0;
    }

    let mut count = 0;
    let mut p = s;
    while *p != 0 {
        let c = *p;
        let mut found = false;
        let mut r = reject;
        while *r != 0 {
            if c == *r {
                found = true;
                break;
            }
            r = r.add(1);
        }
        if found {
            break;
        }
        count += 1;
        p = p.add(1);
    }
    count
}

/// Find first occurrence of any character from accept
#[no_mangle]
pub unsafe extern "C" fn strpbrk(s: *const c_char, accept: *const c_char) -> *mut c_char {
    if s.is_null() || accept.is_null() {
        return ptr::null_mut();
    }

    let mut p = s;
    while *p != 0 {
        let c = *p;
        let mut a = accept;
        while *a != 0 {
            if c == *a {
                return p as *mut c_char;
            }
            a = a.add(1);
        }
        p = p.add(1);
    }
    ptr::null_mut()
}

// ============================================================================
// Character Classification
// ============================================================================

#[no_mangle]
pub extern "C" fn isalpha(c: c_int) -> c_int {
    let c = c as u8;
    if (c >= b'a' && c <= b'z') || (c >= b'A' && c <= b'Z') {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn isdigit(c: c_int) -> c_int {
    let c = c as u8;
    if c >= b'0' && c <= b'9' {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn isalnum(c: c_int) -> c_int {
    if isalpha(c) != 0 || isdigit(c) != 0 {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn isspace(c: c_int) -> c_int {
    let c = c as u8;
    if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == b'\x0b' || c == b'\x0c' {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn isupper(c: c_int) -> c_int {
    let c = c as u8;
    if c >= b'A' && c <= b'Z' {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn islower(c: c_int) -> c_int {
    let c = c as u8;
    if c >= b'a' && c <= b'z' {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn isprint(c: c_int) -> c_int {
    let c = c as u8;
    if c >= 0x20 && c <= 0x7e {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn isxdigit(c: c_int) -> c_int {
    let c = c as u8;
    if (c >= b'0' && c <= b'9') || (c >= b'a' && c <= b'f') || (c >= b'A' && c <= b'F') {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn iscntrl(c: c_int) -> c_int {
    let c = c as u8;
    if c < 0x20 || c == 0x7f {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn isgraph(c: c_int) -> c_int {
    let c = c as u8;
    if c > 0x20 && c <= 0x7e {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ispunct(c: c_int) -> c_int {
    if isgraph(c) != 0 && isalnum(c) == 0 {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn toupper(c: c_int) -> c_int {
    if islower(c) != 0 {
        c - 32
    } else {
        c
    }
}

#[no_mangle]
pub extern "C" fn tolower(c: c_int) -> c_int {
    if isupper(c) != 0 {
        c + 32
    } else {
        c
    }
}

// ============================================================================
// Number Conversion
// ============================================================================

/// Convert string to integer
#[no_mangle]
pub unsafe extern "C" fn atoi(s: *const c_char) -> c_int {
    if s.is_null() {
        return 0;
    }

    let mut i = 0isize;
    let mut result: c_int = 0;
    let mut negative = false;

    // Skip whitespace
    while (*s.offset(i) as u8).is_ascii_whitespace() {
        i += 1;
    }

    // Handle sign
    if *s.offset(i) == b'-' as c_char {
        negative = true;
        i += 1;
    } else if *s.offset(i) == b'+' as c_char {
        i += 1;
    }

    // Convert digits
    while (*s.offset(i) as u8) >= b'0' && (*s.offset(i) as u8) <= b'9' {
        result = result * 10 + ((*s.offset(i) as u8) - b'0') as c_int;
        i += 1;
    }

    if negative {
        -result
    } else {
        result
    }
}

/// Convert string to long
#[no_mangle]
pub unsafe extern "C" fn atol(s: *const c_char) -> crate::c_long {
    atoi(s) as crate::c_long
}

/// Convert string to long long
#[no_mangle]
pub unsafe extern "C" fn atoll(s: *const c_char) -> i64 {
    if s.is_null() {
        return 0;
    }

    let mut i = 0isize;
    let mut result: i64 = 0;
    let mut negative = false;

    // Skip whitespace
    while (*s.offset(i) as u8).is_ascii_whitespace() {
        i += 1;
    }

    // Handle sign
    if *s.offset(i) == b'-' as c_char {
        negative = true;
        i += 1;
    } else if *s.offset(i) == b'+' as c_char {
        i += 1;
    }

    // Convert digits
    while (*s.offset(i) as u8) >= b'0' && (*s.offset(i) as u8) <= b'9' {
        result = result * 10 + ((*s.offset(i) as u8) - b'0') as i64;
        i += 1;
    }

    if negative {
        -result
    } else {
        result
    }
}

/// Convert string to long with base
#[no_mangle]
pub unsafe extern "C" fn strtol(
    s: *const c_char,
    endptr: *mut *mut c_char,
    base: c_int,
) -> crate::c_long {
    if s.is_null() {
        if !endptr.is_null() {
            *endptr = ptr::null_mut();
        }
        return 0;
    }

    let mut i = 0isize;
    let mut result: crate::c_long = 0;
    let mut negative = false;

    // Skip whitespace
    while (*s.offset(i) as u8).is_ascii_whitespace() {
        i += 1;
    }

    // Handle sign
    if *s.offset(i) == b'-' as c_char {
        negative = true;
        i += 1;
    } else if *s.offset(i) == b'+' as c_char {
        i += 1;
    }

    // Determine base
    let actual_base = if base == 0 {
        if *s.offset(i) == b'0' as c_char {
            if *s.offset(i + 1) == b'x' as c_char || *s.offset(i + 1) == b'X' as c_char {
                i += 2;
                16
            } else {
                i += 1;
                8
            }
        } else {
            10
        }
    } else {
        if base == 16 && *s.offset(i) == b'0' as c_char {
            if *s.offset(i + 1) == b'x' as c_char || *s.offset(i + 1) == b'X' as c_char {
                i += 2;
            }
        }
        base
    };

    let start = i;

    // Convert digits
    loop {
        let c = *s.offset(i) as u8;
        let digit = if c >= b'0' && c <= b'9' {
            (c - b'0') as c_int
        } else if c >= b'a' && c <= b'z' {
            (c - b'a' + 10) as c_int
        } else if c >= b'A' && c <= b'Z' {
            (c - b'A' + 10) as c_int
        } else {
            break;
        };

        if digit >= actual_base {
            break;
        }

        result = result * (actual_base as crate::c_long) + (digit as crate::c_long);
        i += 1;
    }

    if !endptr.is_null() {
        if i == start {
            *endptr = s as *mut c_char;
        } else {
            *endptr = s.offset(i) as *mut c_char;
        }
    }

    if negative {
        -result
    } else {
        result
    }
}

/// Convert string to unsigned long with base
#[no_mangle]
pub unsafe extern "C" fn strtoul(
    s: *const c_char,
    endptr: *mut *mut c_char,
    base: c_int,
) -> crate::c_ulong {
    strtol(s, endptr, base) as crate::c_ulong
}

/// Convert string to long long with base
#[no_mangle]
pub unsafe extern "C" fn strtoll(s: *const c_char, endptr: *mut *mut c_char, base: c_int) -> i64 {
    strtol(s, endptr, base) as i64
}

/// Convert string to unsigned long long with base
#[no_mangle]
pub unsafe extern "C" fn strtoull(s: *const c_char, endptr: *mut *mut c_char, base: c_int) -> u64 {
    strtol(s, endptr, base) as u64
}
