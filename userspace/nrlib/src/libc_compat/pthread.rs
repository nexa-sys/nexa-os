//! pthread compatibility layer
//!
//! Provides pthread mutex, attributes, and thread management functions.

use crate::{c_int, c_ulong, c_void, size_t};
use core::{
    hint::spin_loop,
    mem,
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::types::{
    pthread_attr_t, pthread_mutex_t, pthread_mutexattr_t, pthread_once_t, pthread_t,
    MutexInner, EBUSY, EDEADLK, EPERM, GLIBC_KIND_WORD, MAX_PTHREAD_MUTEXES,
    MUTEX_LOCKED, MUTEX_MAGIC, MUTEX_UNLOCKED, PTHREAD_MUTEX_DEFAULT,
    PTHREAD_MUTEX_NORMAL, PTHREAD_MUTEX_RECURSIVE, PTHREAD_MUTEX_WORDS, PTHREAD_ONCE_DONE,
    PTHREAD_ONCE_IN_PROGRESS, PTHREAD_ONCE_INIT_VALUE,
};

/// Trace function entry (logs to stderr)
/// Disabled by default for clean output
macro_rules! trace_fn {
    ($name:expr) => {
        // crate::debug_log_message(concat!("[nrlib] ", $name, "\n").as_bytes());
    };
}

static PTHREAD_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
static PTHREAD_MUTEX_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
static PTHREAD_MUTEX_EXTRA_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

const SYS_WRITE_NR: u64 = 1;
const MUTEX_INNER_SIZE: usize = mem::size_of::<MutexInner>();

// ============================================================================
// Mutex Pool Management
// ============================================================================

#[repr(align(16))]
#[derive(Copy, Clone)]
struct MutexSlot {
    bytes: [u8; MUTEX_INNER_SIZE],
}

impl MutexSlot {
    const fn new() -> Self {
        Self {
            bytes: [0; MUTEX_INNER_SIZE],
        }
    }

    fn as_mut_ptr(&mut self) -> *mut MutexInner {
        self.bytes.as_mut_ptr() as *mut MutexInner
    }

    fn as_ptr(&self) -> *const MutexInner {
        self.bytes.as_ptr() as *const MutexInner
    }

    fn reset(&mut self) {
        self.bytes = [0; MUTEX_INNER_SIZE];
    }
}

static mut MUTEX_POOL: [MutexSlot; MAX_PTHREAD_MUTEXES] = [MutexSlot::new(); MAX_PTHREAD_MUTEXES];
static mut MUTEX_POOL_USED: [bool; MAX_PTHREAD_MUTEXES] = [false; MAX_PTHREAD_MUTEXES];

unsafe fn mutex_word_ptr(mutex: *mut pthread_mutex_t, index: usize) -> *mut usize {
    (*mutex).data.as_mut_ptr().add(index)
}

unsafe fn mutex_get_inner(mutex: *mut pthread_mutex_t) -> Option<*mut MutexInner> {
    let word0 = *mutex_word_ptr(mutex, 0);
    if word0 == 0 {
        None
    } else {
        Some(word0 as *mut MutexInner)
    }
}

unsafe fn mutex_set_inner(mutex: *mut pthread_mutex_t, inner: *mut MutexInner) {
    *mutex_word_ptr(mutex, 0) = inner as usize;
    *mutex_word_ptr(mutex, 1) = MUTEX_MAGIC;
    *mutex_word_ptr(mutex, 2) = (*inner).kind as usize;
    *mutex_word_ptr(mutex, 3) = 0;
    *mutex_word_ptr(mutex, 4) = 0;
}

unsafe fn mutex_is_initialized(mutex: *mut pthread_mutex_t) -> bool {
    *mutex_word_ptr(mutex, 1) == MUTEX_MAGIC
}

unsafe fn detect_static_kind(mutex: *mut pthread_mutex_t) -> c_int {
    let word = (*mutex).data[GLIBC_KIND_WORD];
    let kind = (word & 0xFFFF_FFFF) as c_int;
    if kind == PTHREAD_MUTEX_RECURSIVE {
        PTHREAD_MUTEX_RECURSIVE
    } else {
        PTHREAD_MUTEX_DEFAULT
    }
}

unsafe fn alloc_mutex_inner(kind: c_int) -> Result<*mut MutexInner, c_int> {
    for idx in 0..MAX_PTHREAD_MUTEXES {
        if !MUTEX_POOL_USED[idx] {
            MUTEX_POOL_USED[idx] = true;
            let slot = &mut MUTEX_POOL[idx];
            let inner_ptr = slot.as_mut_ptr();
            ptr::write(inner_ptr, MutexInner::new(kind));
            return Ok(inner_ptr);
        }
    }

    crate::set_errno(crate::ENOMEM);
    Err(crate::ENOMEM)
}

unsafe fn free_mutex_inner(inner: *mut MutexInner) {
    if inner.is_null() {
        return;
    }

    for idx in 0..MAX_PTHREAD_MUTEXES {
        let slot_ptr = MUTEX_POOL[idx].as_ptr() as *const MutexInner;
        if slot_ptr == inner as *const MutexInner {
            MUTEX_POOL[idx].reset();
            MUTEX_POOL_USED[idx] = false;
            return;
        }
    }
}

unsafe fn ensure_mutex_inner(mutex: *mut pthread_mutex_t) -> Result<*mut MutexInner, c_int> {
    if mutex_is_initialized(mutex) {
        if let Some(inner) = mutex_get_inner(mutex) {
            return Ok(inner);
        }
        return Err(crate::EINVAL);
    }

    let kind = detect_static_kind(mutex);
    let inner = alloc_mutex_inner(kind)?;
    (*inner).kind = kind;
    mutex_set_inner(mutex, inner);
    Ok(inner)
}

// ============================================================================
// Debug Logging Helpers
// ============================================================================

fn u64_to_hex(mut n: u64, buf: &mut [u8]) -> &[u8] {
    if n == 0 {
        buf[0] = b'0';
        return &buf[0..1];
    }
    let mut i = 0;
    while n > 0 && i < buf.len() {
        let digit = (n & 0xF) as u8;
        buf[i] = if digit < 10 { b'0' + digit } else { b'a' + (digit - 10) };
        n >>= 4;
        i += 1;
    }
    buf[..i].reverse();
    &buf[..i]
}

#[inline(always)]
fn log_mutex(msg: &[u8]) {
    let slot = PTHREAD_MUTEX_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if slot < 0 {  // Disabled: was 256
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    }
}

#[inline(always)]
fn debug_mutex_event(msg: &[u8]) {
    let slot = PTHREAD_MUTEX_EXTRA_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if slot < 0 {  // Disabled: was 128
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    }
}

#[allow(dead_code)]
fn log_mutex_state(tag: &[u8], words: &[usize; PTHREAD_MUTEX_WORDS]) {
    let mut buf = [0u8; 192];
    let mut pos = 0usize;

    let copy_tag = core::cmp::min(tag.len(), buf.len());
    buf[..copy_tag].copy_from_slice(&tag[..copy_tag]);
    pos += copy_tag;

    for (idx, word) in words.iter().enumerate() {
        if pos + 5 >= buf.len() {
            break;
        }
        buf[pos] = b' ';
        buf[pos + 1] = b'w';
        buf[pos + 2] = b'0' + (idx as u8);
        buf[pos + 3] = b'=';
        buf[pos + 4] = b'0';
        buf[pos + 5] = b'x';
        pos += 6;

        if pos >= buf.len() {
            break;
        }

        let mut tmp = [0u8; 16];
        let hex = u64_to_hex(*word as u64, &mut tmp);
        let available = core::cmp::min(hex.len(), buf.len() - pos);
        buf[pos..pos + available].copy_from_slice(&hex[..available]);
        pos += available;
    }

    if pos < buf.len() {
        buf[pos] = b'\n';
        pos += 1;
    }

    let _ = crate::syscall3(SYS_WRITE_NR, 2, buf.as_ptr() as u64, pos as u64);
}

#[allow(dead_code)]
fn log_mutex_kind(tag: &[u8], kind: c_int) {
    let mut buf = [0u8; 64];
    let mut pos = 0usize;

    let copy_tag = core::cmp::min(tag.len(), buf.len());
    buf[..copy_tag].copy_from_slice(&tag[..copy_tag]);
    pos += copy_tag;

    if pos + 4 >= buf.len() {
        let _ = crate::syscall3(SYS_WRITE_NR, 2, buf.as_ptr() as u64, pos as u64);
        return;
    }

    buf[pos] = b' ';
    buf[pos + 1] = b'k';
    buf[pos + 2] = b'i';
    buf[pos + 3] = b'n';
    buf[pos + 4] = b'd';
    buf[pos + 5] = b'=';
    pos += 6;

    if pos < buf.len() {
        let mut tmp = [0u8; 16];
        let hex = u64_to_hex(kind as u64, &mut tmp);
        let available = core::cmp::min(hex.len(), buf.len() - pos);
        buf[pos..pos + available].copy_from_slice(&hex[..available]);
        pos += available;
    }

    if pos < buf.len() {
        buf[pos] = b'\n';
        pos += 1;
    }

    let _ = crate::syscall3(SYS_WRITE_NR, 2, buf.as_ptr() as u64, pos as u64);
}

// ============================================================================
// Thread Attribute Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_init(_attr: *mut pthread_attr_t) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_destroy(_attr: *mut pthread_attr_t) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_setstacksize(
    _attr: *mut pthread_attr_t,
    _stacksize: size_t,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_setguardsize(
    _attr: *mut pthread_attr_t,
    _guardsize: size_t,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_getguardsize(
    _attr: *const pthread_attr_t,
    guardsize: *mut size_t,
) -> c_int {
    if !guardsize.is_null() {
        *guardsize = 0;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_attr_getstack(
    _attr: *const pthread_attr_t,
    stackaddr: *mut *mut c_void,
    stacksize: *mut size_t,
) -> c_int {
    if !stackaddr.is_null() {
        *stackaddr = ptr::null_mut();
    }
    if !stacksize.is_null() {
        *stacksize = 0;
    }
    0
}

// ============================================================================
// Mutex Attribute Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn pthread_mutexattr_init(attr: *mut pthread_mutexattr_t) -> c_int {
    if attr.is_null() {
        return crate::EINVAL;
    }
    debug_mutex_event(b"[nrlib] pthread_mutexattr_init enter\n");
    (*attr).set_kind(PTHREAD_MUTEX_DEFAULT);
    for slot in 1..(*attr).data.len() {
        (*attr).data[slot] = 0;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutexattr_destroy(attr: *mut pthread_mutexattr_t) -> c_int {
    if attr.is_null() {
        return crate::EINVAL;
    }
    debug_mutex_event(b"[nrlib] pthread_mutexattr_destroy enter\n");
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutexattr_settype(
    attr: *mut pthread_mutexattr_t,
    kind: c_int,
) -> c_int {
    if attr.is_null() {
        return crate::EINVAL;
    }

    crate::debug_log_message(b"[nrlib] pthread_mutexattr_settype called\n");

    if kind == PTHREAD_MUTEX_NORMAL || kind == PTHREAD_MUTEX_RECURSIVE {
        crate::debug_log_message(b"[nrlib] pthread_mutexattr_settype setting kind\n");
        (*attr).set_kind(kind);
        crate::debug_log_message(b"[nrlib] pthread_mutexattr_settype returning 0\n");
        0
    } else {
        crate::debug_log_message(b"[nrlib] pthread_mutexattr_settype returning EINVAL\n");
        crate::EINVAL
    }
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutexattr_gettype(
    attr: *const pthread_mutexattr_t,
    kind_out: *mut c_int,
) -> c_int {
    if attr.is_null() || kind_out.is_null() {
        return crate::EINVAL;
    }
    debug_mutex_event(b"[nrlib] pthread_mutexattr_gettype enter\n");

    *kind_out = (*attr).kind();
    0
}

// ============================================================================
// Mutex Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn pthread_mutex_init(
    mutex: *mut pthread_mutex_t,
    attr: *const pthread_mutexattr_t,
) -> c_int {
    if mutex.is_null() {
        return crate::EINVAL;
    }

    let kind = if attr.is_null() {
        PTHREAD_MUTEX_DEFAULT
    } else {
        (*attr).kind()
    };

    let inner = match alloc_mutex_inner(kind) {
        Ok(inner) => inner,
        Err(err) => return err,
    };
    (*inner).kind = kind;
    mutex_set_inner(mutex, inner);
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutex_destroy(mutex: *mut pthread_mutex_t) -> c_int {
    if mutex.is_null() {
        return crate::EINVAL;
    }

    if let Some(inner) = mutex_get_inner(mutex) {
        if (*inner).state.load(Ordering::Acquire) == MUTEX_LOCKED {
            return EBUSY;
        }

        free_mutex_inner(inner);
    }

    (*mutex).data = [0; PTHREAD_MUTEX_WORDS];
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutex_lock(mutex: *mut pthread_mutex_t) -> c_int {
    if mutex.is_null() {
        return crate::EINVAL;
    }

    let inner = match ensure_mutex_inner(mutex) {
        Ok(inner) => inner,
        Err(err) => return err,
    };

    let tid = crate::getpid() as c_ulong;
    let kind = (*inner).kind;

    if kind == PTHREAD_MUTEX_RECURSIVE && (*inner).owner == tid {
        (*inner).recursion = (*inner).recursion.saturating_add(1);
        return 0;
    }

    if kind != PTHREAD_MUTEX_RECURSIVE
        && (*inner).owner == tid
        && (*inner).state.load(Ordering::Acquire) == MUTEX_LOCKED
    {
        return EDEADLK;
    }

    let mut spins = 0usize;
    const MAX_SPINS: usize = 1_000_000;
    while (*inner)
        .state
        .compare_exchange(MUTEX_UNLOCKED, MUTEX_LOCKED, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spins += 1;
        if spins > MAX_SPINS {
            return EBUSY;
        }
        spin_loop();
    }

    (*inner).owner = tid;
    (*inner).recursion = 1;
    0
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutex_trylock(mutex: *mut pthread_mutex_t) -> c_int {
    if mutex.is_null() {
        return crate::EINVAL;
    }

    let inner = match ensure_mutex_inner(mutex) {
        Ok(inner) => inner,
        Err(err) => return err,
    };

    let tid = crate::getpid() as c_ulong;
    let kind = (*inner).kind;

    if kind == PTHREAD_MUTEX_RECURSIVE && (*inner).owner == tid {
        (*inner).recursion = (*inner).recursion.saturating_add(1);
        return 0;
    }

    match (*inner).state.compare_exchange(
        MUTEX_UNLOCKED,
        MUTEX_LOCKED,
        Ordering::Acquire,
        Ordering::Relaxed,
    ) {
        Ok(_) => {
            (*inner).owner = tid;
            (*inner).recursion = 1;
            0
        }
        Err(_) => EBUSY,
    }
}

#[no_mangle]
pub unsafe extern "C" fn pthread_mutex_unlock(mutex: *mut pthread_mutex_t) -> c_int {
    if mutex.is_null() {
        return crate::EINVAL;
    }

    let inner = match ensure_mutex_inner(mutex) {
        Ok(inner) => inner,
        Err(err) => return err,
    };

    if (*inner).state.load(Ordering::Acquire) == MUTEX_UNLOCKED {
        return crate::EINVAL;
    }

    let tid = crate::getpid() as c_ulong;
    if (*inner).owner != tid {
        return EPERM;
    }

    if (*inner).kind == PTHREAD_MUTEX_RECURSIVE {
        if (*inner).recursion > 1 {
            (*inner).recursion -= 1;
            return 0;
        }
    }

    (*inner).owner = 0;
    (*inner).recursion = 0;
    (*inner).state.store(MUTEX_UNLOCKED, Ordering::Release);
    0
}

// ============================================================================
// Alias Functions (glibc compatibility)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn __pthread_mutex_init(
    mutex: *mut pthread_mutex_t,
    attr: *const pthread_mutexattr_t,
) -> c_int {
    pthread_mutex_init(mutex, attr)
}

#[no_mangle]
pub unsafe extern "C" fn __pthread_mutex_destroy(mutex: *mut pthread_mutex_t) -> c_int {
    pthread_mutex_destroy(mutex)
}

#[no_mangle]
pub unsafe extern "C" fn __pthread_mutex_lock(mutex: *mut pthread_mutex_t) -> c_int {
    pthread_mutex_lock(mutex)
}

#[no_mangle]
pub unsafe extern "C" fn __pthread_mutex_trylock(mutex: *mut pthread_mutex_t) -> c_int {
    pthread_mutex_trylock(mutex)
}

#[no_mangle]
pub unsafe extern "C" fn __pthread_mutex_unlock(mutex: *mut pthread_mutex_t) -> c_int {
    pthread_mutex_unlock(mutex)
}

// ============================================================================
// Thread Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn pthread_self() -> pthread_t {
    let slot = PTHREAD_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if slot < 32 {
        let msg = b"[nrlib] pthread_self\n";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    }
    1 // Always return 1 for single-threaded
}

#[no_mangle]
pub unsafe extern "C" fn pthread_getattr_np(
    _thread: pthread_t,
    _attr: *mut pthread_attr_t,
) -> c_int {
    trace_fn!("pthread_getattr_np");
    0
}

// ============================================================================
// pthread_once Support
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn pthread_once(
    once_control: *mut pthread_once_t,
    init_routine: Option<unsafe extern "C" fn()>,
) -> c_int {
    trace_fn!("pthread_once");
    
    let routine_addr = if let Some(f) = init_routine {
        f as *const () as u64
    } else {
        0
    };
    
    let diag_msg = b"[nrlib] pthread_once called with routine @ 0x";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, diag_msg.as_ptr() as u64, diag_msg.len() as u64);
    
    for i in 0..16 {
        let shift = (15 - i) * 4;
        let nibble = ((routine_addr >> shift) & 0xF) as u8;
        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
        let _ = crate::syscall3(SYS_WRITE_NR, 2, &ch as *const u8 as u64, 1);
    }
    let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);
    
    if once_control.is_null() {
        return crate::EINVAL;
    }
    
    let init = match init_routine {
        Some(f) => f,
        None => return crate::EINVAL,
    };
    
    let control = &*once_control;
    
    // Fast path: already initialized
    if control.state.load(Ordering::Acquire) == PTHREAD_ONCE_DONE {
        return 0;
    }
    
    // Try to be the thread that initializes
    match control.state.compare_exchange(
        PTHREAD_ONCE_INIT_VALUE,
        PTHREAD_ONCE_IN_PROGRESS,
        Ordering::Acquire,
        Ordering::Acquire,
    ) {
        Ok(_) => {
            let diag_msg = b"[nrlib] pthread_once: Calling init routine\n";
            let _ = crate::syscall3(SYS_WRITE_NR, 2, diag_msg.as_ptr() as u64, diag_msg.len() as u64);
            
            init();
            
            let diag_msg = b"[nrlib] pthread_once: Init routine completed\n";
            let _ = crate::syscall3(SYS_WRITE_NR, 2, diag_msg.as_ptr() as u64, diag_msg.len() as u64);
            
            control.state.store(PTHREAD_ONCE_DONE, Ordering::Release);
            0
        }
        Err(PTHREAD_ONCE_DONE) => {
            0
        }
        Err(_) => {
            let diag_msg = b"[nrlib] pthread_once: Waiting for init from another thread\n";
            let _ = crate::syscall3(SYS_WRITE_NR, 2, diag_msg.as_ptr() as u64, diag_msg.len() as u64);
            
            let mut spin_count = 0u32;
            loop {
                if control.state.load(Ordering::Acquire) != PTHREAD_ONCE_IN_PROGRESS {
                    break;
                }
                spin_count += 1;
                if spin_count > 100000 {
                    let hang_msg = b"[nrlib] WARNING: pthread_once init timeout - possible hang\n";
                    let _ = crate::syscall3(SYS_WRITE_NR, 2, hang_msg.as_ptr() as u64, hang_msg.len() as u64);
                    break;
                }
                spin_loop();
            }
            
            let diag_msg = b"[nrlib] pthread_once: Init completed by other thread\n";
            let _ = crate::syscall3(SYS_WRITE_NR, 2, diag_msg.as_ptr() as u64, diag_msg.len() as u64);
            
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn __pthread_once(
    once_control: *mut pthread_once_t,
    init_routine: Option<unsafe extern "C" fn()>,
) -> c_int {
    pthread_once(once_control, init_routine)
}
