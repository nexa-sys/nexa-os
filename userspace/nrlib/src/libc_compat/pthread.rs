//! pthread compatibility layer
//!
//! Provides pthread mutex, attributes, and thread management functions.

use crate::{c_int, c_ulong, c_void, size_t};
use core::{
    hint::spin_loop,
    mem, ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::types::{
    pthread_attr_t, pthread_mutex_t, pthread_mutexattr_t, pthread_once_t, pthread_t, MutexInner,
    EBUSY, EDEADLK, EPERM, GLIBC_KIND_WORD, MAX_PTHREAD_MUTEXES, MUTEX_LOCKED, MUTEX_MAGIC,
    MUTEX_UNLOCKED, PTHREAD_MUTEX_DEFAULT, PTHREAD_MUTEX_NORMAL, PTHREAD_MUTEX_RECURSIVE,
    PTHREAD_MUTEX_WORDS, PTHREAD_ONCE_DONE, PTHREAD_ONCE_INIT_VALUE, PTHREAD_ONCE_IN_PROGRESS,
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
        buf[i] = if digit < 10 {
            b'0' + digit
        } else {
            b'a' + (digit - 10)
        };
        n >>= 4;
        i += 1;
    }
    buf[..i].reverse();
    &buf[..i]
}

/// Print a hex value to stderr
fn print_hex_u64(val: u64) {
    let mut buf = [0u8; 16];
    let hex = u64_to_hex(val, &mut buf);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, hex.as_ptr() as u64, hex.len() as u64);
}

#[inline(always)]
fn log_mutex(msg: &[u8]) {
    let slot = PTHREAD_MUTEX_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if slot < 0 {
        // Disabled: was 256
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    }
}

#[inline(always)]
fn debug_mutex_event(msg: &[u8]) {
    let slot = PTHREAD_MUTEX_EXTRA_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if slot < 0 {
        // Disabled: was 128
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
        .compare_exchange(
            MUTEX_UNLOCKED,
            MUTEX_LOCKED,
            Ordering::Acquire,
            Ordering::Relaxed,
        )
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

/// Default thread stack size (1MB)
const DEFAULT_STACK_SIZE: usize = 1 * 1024 * 1024;

/// Thread stack guard size
const STACK_GUARD_SIZE: usize = 4096;

/// Clone flags for creating a thread
const CLONE_THREAD_FLAGS: c_int = super::types::CLONE_VM
    | super::types::CLONE_FS
    | super::types::CLONE_FILES
    | super::types::CLONE_SIGHAND
    | super::types::CLONE_THREAD
    | super::types::CLONE_SYSVSEM
    | super::types::CLONE_SETTLS
    | super::types::CLONE_PARENT_SETTID
    | super::types::CLONE_CHILD_CLEARTID;

/// Thread control block - stores thread state
/// This structure is pointed to by the FS register for TLS access
/// Layout must be compatible with musl's pthread structure for std compatibility
/// CRITICAL: Field order determines offsets. Verified layout:
///   offset 0:  self_ptr (8 bytes)
///   offset 8:  dtv (8 bytes)
///   offset 16: prev (8 bytes)
///   offset 24: next (8 bytes)
///   offset 32: sysinfo (8 bytes)
///   offset 40: start_routine (8 bytes)
///   offset 48: arg (8 bytes)
///   offset 56: retval (8 bytes)
///   offset 64: stack_base (8 bytes)
///   offset 72: stack_size (8 bytes)
///   offset 80: tid (8 bytes)
///   offset 88: errno_val (4 bytes)
///   offset 92: flags (4 bytes) - joinable/detached packed
///   offset 96: exited (8 bytes)
///   offset 104: tid_address (8 bytes)
///   offset 112: canary (8 bytes)
///   offset 120: tsd_used (1 byte) + padding (7 bytes)
///   offset 128: tsd (128 * 8 = 1024 bytes) - musl naming
#[repr(C)]
pub(crate) struct ThreadControlBlock {
    /// Self pointer (for TLS access via %fs:0) - offset 0
    self_ptr: *mut ThreadControlBlock,
    /// DTV (Dynamic Thread Vector) for TLS - offset 8
    dtv: *mut usize,
    /// Previous thread in list - offset 16
    prev: *mut ThreadControlBlock,
    /// Next thread in list - offset 24
    next: *mut ThreadControlBlock,
    /// sysinfo - offset 32
    sysinfo: usize,
    /// Start routine - offset 40
    start_routine: extern "C" fn(*mut c_void) -> *mut c_void,
    /// Argument to start routine - offset 48
    arg: *mut c_void,
    /// Return value from thread - offset 56
    retval: *mut c_void,
    /// Stack base address - offset 64
    stack_base: *mut c_void,
    /// Stack size - offset 72
    stack_size: usize,
    /// Thread ID (set after clone) - offset 80
    tid: AtomicUsize,
    /// errno location - offset 88
    errno_val: i32,
    /// Flags: bit 0 = joinable, bit 1 = detached - offset 92
    flags: u32,
    /// Exit flag - offset 96
    exited: AtomicUsize,
    /// TID address for futex wake on exit - offset 104
    tid_address: *mut c_int,
    /// Stack canary - offset 112
    canary: usize,
    /// Whether TSD has been used (for destructor optimization) - offset 120
    pub(crate) tsd_used: bool,
    /// Padding to align tsd to 128 - offset 121
    _pad: [u8; 7],
    /// Thread-specific data (pthread_key values) - offset 128, musl naming
    pub(crate) tsd: [*mut c_void; MAX_TLS_KEYS],
}

impl ThreadControlBlock {
    fn is_joinable(&self) -> bool {
        (self.flags & 1) != 0
    }
    fn set_joinable(&mut self, v: bool) {
        if v {
            self.flags |= 1;
        } else {
            self.flags &= !1;
        }
    }
    fn is_detached(&self) -> bool {
        (self.flags & 2) != 0
    }
    fn set_detached(&mut self, v: bool) {
        if v {
            self.flags |= 2;
        } else {
            self.flags &= !2;
        }
    }
}

/// Maximum TLS keys per thread
const MAX_TLS_KEYS: usize = 128;

/// Maximum number of threads
const MAX_THREADS: usize = 64;

/// Thread table
static mut THREAD_TABLE: [Option<*mut ThreadControlBlock>; MAX_THREADS] = [None; MAX_THREADS];
static THREAD_TABLE_LOCK: AtomicUsize = AtomicUsize::new(0);

/// Static storage for main thread's TCB (avoid dynamic allocation at startup)
#[repr(C, align(16))]
struct MainThreadTcbStorage {
    tcb: ThreadControlBlock,
}

static mut MAIN_THREAD_TCB: MainThreadTcbStorage = MainThreadTcbStorage {
    tcb: ThreadControlBlock {
        self_ptr: ptr::null_mut(),
        dtv: ptr::null_mut(),
        prev: ptr::null_mut(),
        next: ptr::null_mut(),
        sysinfo: 0,
        start_routine: main_thread_dummy_start,
        arg: ptr::null_mut(),
        retval: ptr::null_mut(),
        stack_base: ptr::null_mut(),
        stack_size: 0,
        tid: AtomicUsize::new(0),
        errno_val: 0,
        flags: 1, // joinable
        exited: AtomicUsize::new(0),
        tid_address: ptr::null_mut(),
        canary: 0,
        tsd_used: false,
        _pad: [0; 7],
        tsd: [ptr::null_mut(); MAX_TLS_KEYS],
    },
};

/// Flag to track if main thread TLS has been initialized
static MAIN_TLS_INITIALIZED: AtomicUsize = AtomicUsize::new(0);

/// Dummy start routine for main thread (never called)
extern "C" fn main_thread_dummy_start(_arg: *mut c_void) -> *mut c_void {
    ptr::null_mut()
}

/// Initialize TLS for the main thread.
/// This must be called early in program startup before any TLS operations.
/// It sets up the TCB and FS base register for the main thread.
#[no_mangle]
pub unsafe extern "C" fn __nrlib_init_main_thread_tls() {
    // Debug: print entry
    let msg = b"[nrlib] __nrlib_init_main_thread_tls called\n";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    
    // Only initialize once
    if MAIN_TLS_INITIALIZED
        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    // Get pointer to the static TCB storage
    let tcb_ptr = &mut MAIN_THREAD_TCB.tcb as *mut ThreadControlBlock;

    // Initialize self-pointer (critical for %fs:0 access)
    (*tcb_ptr).self_ptr = tcb_ptr;

    // Get main thread's TID
    let tid = crate::syscall0(crate::SYS_GETTID) as usize;
    (*tcb_ptr).tid.store(tid, Ordering::SeqCst);

    // Initialize TLS data array to null (critical!)
    for i in 0..MAX_TLS_KEYS {
        (*tcb_ptr).tsd[i] = ptr::null_mut();
    }

    // Register in thread table (slot 0 for main thread)
    THREAD_TABLE[0] = Some(tcb_ptr);

    // Set FS base to point to the TCB using arch_prctl
    // ARCH_SET_FS = 0x1002
    let ret = crate::syscall2(crate::SYS_ARCH_PRCTL, 0x1002, tcb_ptr as u64);
    if ret != 0 {
        // Log error but continue - some operations may still work
        let msg = b"[nrlib] WARNING: Failed to set FS base for main thread\n";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    }
    
    // Verify FS base was set correctly
    let fs_check: u64;
    core::arch::asm!("mov {}, fs:0", out(reg) fs_check, options(nostack, preserves_flags, readonly));
    if fs_check != tcb_ptr as u64 {
        let msg = b"[nrlib] ERROR: FS base mismatch after init\n";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    } else {
        let msg = b"[nrlib] TLS init success\n";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    }
}

/// Get the current thread's TCB pointer from FS base.
/// Returns None if TLS is not initialized.
#[inline]
pub unsafe fn get_current_tcb() -> Option<*mut ThreadControlBlock> {
    let tcb_ptr: *mut ThreadControlBlock;
    core::arch::asm!(
        "mov {}, fs:0",
        out(reg) tcb_ptr,
        options(nostack, preserves_flags, readonly)
    );

    if tcb_ptr.is_null() {
        None
    } else {
        Some(tcb_ptr)
    }
}

// ============================================================================
// __tls_get_addr - General Dynamic TLS Access
// ============================================================================

/// TLS index structure used by __tls_get_addr
/// This is the standard ABI structure for General Dynamic TLS model.
#[repr(C)]
pub struct TlsIndex {
    /// TLS module ID (1 = main executable, 2+ = shared libraries)
    pub ti_module: usize,
    /// Offset within the module's TLS block
    pub ti_offset: usize,
}

/// Get the address of a thread-local variable.
/// 
/// This is the General Dynamic TLS model accessor function.
/// Called by code using @tlsgd relocations for dynamic TLS access.
/// 
/// # Safety
/// The TLS index must be valid and point to an initialized TLS block.
/// 
/// # Parameters
/// - `ti`: Pointer to TLS index containing module ID and offset
/// 
/// # Returns
/// Pointer to the thread-local variable
#[no_mangle]
pub unsafe extern "C" fn __tls_get_addr(ti: *const TlsIndex) -> *mut c_void {
    if ti.is_null() {
        return ptr::null_mut();
    }
    
    let ti = &*ti;
    
    // Get the current thread's TCB
    if let Some(tcb) = get_current_tcb() {
        // Get the DTV (Dynamic Thread Vector)
        let dtv = (*tcb).dtv;
        
        if !dtv.is_null() {
            // DTV layout (musl-style):
            //   dtv[0] = generation counter
            //   dtv[module_id] = pointer to TLS block for that module
            //
            // For simplicity in our implementation:
            // - Module 1 (main executable) uses static TLS via TP
            // - Module 2+ (shared libraries) would need dynamic allocation
            
            if ti.ti_module == 0 {
                // Module 0 is invalid
                return ptr::null_mut();
            }
            
            // For static TLS (most common case), compute address directly
            // Static TLS is stored at negative offsets from TP (fs:0)
            if ti.ti_module == 1 {
                // Main executable's TLS - use TP-relative addressing
                // TLS is at negative offset from thread pointer
                let tp = tcb as *mut u8;
                return tp.wrapping_sub(ti.ti_offset) as *mut c_void;
            }
            
            // For dynamic TLS (shared libraries), lookup in DTV
            let tls_block = *dtv.add(ti.ti_module);
            if tls_block != 0 {
                return (tls_block as *mut u8).add(ti.ti_offset) as *mut c_void;
            }
        }
        
        // Fallback for static TLS when DTV is not set up
        // Use direct negative offset from TP
        let tp = tcb as *mut u8;
        return tp.wrapping_sub(ti.ti_offset) as *mut c_void;
    }
    
    // No TCB available - this shouldn't happen in properly initialized programs
    ptr::null_mut()
}

/// Get TLS data for the current thread at the given key index.
/// Returns null if no TCB or key out of range.
#[inline]
pub unsafe fn get_thread_tls_data(key: usize) -> *mut c_void {
    if let Some(tcb) = get_current_tcb() {
        if key < MAX_TLS_KEYS {
            return (*tcb).tsd[key];
        }
    }
    // No TCB or key out of range
    ptr::null_mut()
}

/// Set TLS data for the current thread at the given key index.
/// Falls back to global storage if TLS is not initialized.
#[inline]
pub unsafe fn set_thread_tls_data(key: usize, value: *mut c_void) -> bool {
    if let Some(tcb) = get_current_tcb() {
        if key < MAX_TLS_KEYS {
            (*tcb).tsd[key] = value;
            return true;
        }
    }
    false
}

/// Allocate a thread slot
unsafe fn alloc_thread_slot(tcb: *mut ThreadControlBlock) -> Option<usize> {
    // Acquire lock
    while THREAD_TABLE_LOCK
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spin_loop();
    }

    for i in 0..MAX_THREADS {
        if THREAD_TABLE[i].is_none() {
            THREAD_TABLE[i] = Some(tcb);
            THREAD_TABLE_LOCK.store(0, Ordering::Release);
            return Some(i);
        }
    }

    THREAD_TABLE_LOCK.store(0, Ordering::Release);
    None
}

/// Free a thread slot
unsafe fn free_thread_slot(slot: usize) {
    while THREAD_TABLE_LOCK
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spin_loop();
    }

    if slot < MAX_THREADS {
        THREAD_TABLE[slot] = None;
    }

    THREAD_TABLE_LOCK.store(0, Ordering::Release);
}

/// Thread entry point - called by clone when ret == 0 (child thread)
/// This function never returns; it calls the user's start routine and then exits.
#[no_mangle]
pub unsafe extern "C" fn __thread_entry() -> ! {
    // Get TLS/TCB pointer from FS base
    let tcb_ptr: *mut ThreadControlBlock;
    core::arch::asm!(
        "mov {}, fs:0",
        out(reg) tcb_ptr,
        options(nostack, preserves_flags)
    );

    let msg = b"[nrlib] __thread_entry: starting thread\n";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);

    // Debug: print tcb_ptr
    let msg = b"[nrlib] __thread_entry: tcb_ptr=0x";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    print_hex_u64(tcb_ptr as u64);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);

    if tcb_ptr.is_null() {
        let msg = b"[nrlib] __thread_entry: ERROR tcb_ptr is NULL!\n";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        crate::syscall1(crate::SYS_EXIT, 1);
        loop {
            spin_loop();
        }
    }

    // Debug: dump raw TCB memory to verify offsets
    {
        let msg = b"[nrlib] __thread_entry: dumping TCB offsets\n";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);

        // Read raw bytes at each offset
        let base = tcb_ptr as *const u8;

        // Offset 64 (start_routine)
        let msg = b"[nrlib] offset64=0x";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        let val64 = *(base.add(64) as *const u64);
        print_hex_u64(val64);
        let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);

        // Offset 72 (arg)
        let msg = b"[nrlib] offset72=0x";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        let val72 = *(base.add(72) as *const u64);
        print_hex_u64(val72);
        let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);

        // Print actual field addresses using offset_of would be ideal, but let's use ptr math
        let msg = b"[nrlib] &tcb.start_routine offset=";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        let tcb = &*tcb_ptr;
        let start_offset = (&tcb.start_routine as *const _ as usize) - (tcb_ptr as usize);
        print_hex_u64(start_offset as u64);
        let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);

        let msg = b"[nrlib] &tcb.arg offset=";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        let arg_offset = (&tcb.arg as *const _ as usize) - (tcb_ptr as usize);
        print_hex_u64(arg_offset as u64);
        let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);
    }

    let tcb = &mut *tcb_ptr;

    // Debug: print start_routine
    {
        let msg: &[u8] = b"[nrlib] __thread_entry: start_routine=0x";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        print_hex_u64(tcb.start_routine as u64);
        let newline: &[u8] = b"\n";
        let _ = crate::syscall3(
            SYS_WRITE_NR,
            2,
            newline.as_ptr() as u64,
            newline.len() as u64,
        );
    }

    // Debug: print arg
    {
        let msg: &[u8] = b"[nrlib] __thread_entry: arg=0x";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        print_hex_u64(tcb.arg as u64);
        let newline: &[u8] = b"\n";
        let _ = crate::syscall3(
            SYS_WRITE_NR,
            2,
            newline.as_ptr() as u64,
            newline.len() as u64,
        );
    }

    // Call the user's start routine
    {
        let msg: &[u8] = b"[nrlib] __thread_entry: calling start_routine at 0x";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        print_hex_u64(tcb.start_routine as u64);
        let msg: &[u8] = b" with arg 0x";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        print_hex_u64(tcb.arg as u64);
        let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);
    }

    // Actually call the start routine
    let start_fn = tcb.start_routine;
    let start_arg = tcb.arg;
    let retval = (start_fn)(start_arg);

    // We reached here - thread function returned
    {
        let msg: &[u8] = b"[nrlib] __thread_entry: start_routine returned 0x";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        print_hex_u64(retval as u64);
        let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);
    }

    tcb.retval = retval;

    // Mark as exited
    tcb.exited.store(1, Ordering::Release);

    // Wake any threads waiting to join
    if !tcb.tid_address.is_null() {
        // Clear the tid address
        *tcb.tid_address = 0;
        // Wake waiters (futex wake)
        crate::syscall6(
            crate::SYS_FUTEX,
            tcb.tid_address as u64,
            super::types::FUTEX_WAKE_OP as u64,
            i32::MAX as u64, // Wake all waiters
            0,
            0,
            0,
        );
    }

    // Exit thread (not process)
    crate::syscall1(crate::SYS_EXIT, 0);

    // Should never reach here
    loop {
        spin_loop();
    }
}

/// Create a new thread
#[no_mangle]
pub unsafe extern "C" fn pthread_create(
    thread: *mut pthread_t,
    attr: *const pthread_attr_t,
    start_routine: extern "C" fn(*mut c_void) -> *mut c_void,
    arg: *mut c_void,
) -> c_int {
    let msg = b"[nrlib] pthread_create called\n";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);

    if thread.is_null() {
        return crate::EINVAL;
    }

    // Get stack size from attributes or use default
    let stack_size = if !attr.is_null() {
        DEFAULT_STACK_SIZE // TODO: parse from attr
    } else {
        DEFAULT_STACK_SIZE
    };

    let total_stack = stack_size + STACK_GUARD_SIZE;

    // Allocate stack using mmap
    let stack = crate::syscall6(
        crate::SYS_MMAP,
        0,
        total_stack as u64,
        (super::types::PROT_READ | super::types::PROT_WRITE) as u64,
        (super::types::MAP_PRIVATE | super::types::MAP_ANONYMOUS) as u64,
        u64::MAX, // -1 for anonymous
        0,
    );

    // Debug: print mmap result
    let msg = b"[nrlib] pthread_create: mmap returned stack=0x";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    print_hex_u64(stack);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);

    if stack == u64::MAX || stack == 0 {
        let msg = b"[nrlib] pthread_create: mmap failed\n";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        return crate::ENOMEM;
    }

    // Calculate TCB location at top of stack
    // Stack layout (high to low):
    //   +---------------------+ <- stack + total_stack
    //   |  Thread Control     |
    //   |  Block (TCB)        |
    //   +---------------------+ <- tcb_addr
    //   |  [padding]          |
    //   +---------------------+ <- child_stack (16-byte aligned)
    //   |                     |
    //   |  Thread stack       |
    //   |  (grows downward)   |
    //   |                     |
    //   +---------------------+ <- stack + STACK_GUARD_SIZE
    //   |  Guard page         |
    //   +---------------------+ <- stack

    let stack_top = stack + total_stack as u64;
    let tcb_size = mem::size_of::<ThreadControlBlock>() as u64;
    let tcb_addr = (stack_top - tcb_size) & !0xF; // 16-byte aligned
    let tcb_ptr = tcb_addr as *mut ThreadControlBlock;

    // Debug: print tcb_addr
    let msg = b"[nrlib] pthread_create: tcb_addr=0x";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
    print_hex_u64(tcb_addr);
    let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);

    // Initialize TCB
    let tcb = &mut *tcb_ptr;
    tcb.self_ptr = tcb_ptr; // Self pointer for TLS access (%fs:0)
    tcb.dtv = ptr::null_mut(); // No dynamic TLS for now
    tcb.prev = ptr::null_mut();
    tcb.next = ptr::null_mut();
    tcb.sysinfo = 0;
    tcb.start_routine = start_routine;
    tcb.arg = arg;
    tcb.retval = ptr::null_mut();
    tcb.stack_base = stack as *mut c_void;
    tcb.stack_size = total_stack;
    tcb.tid = AtomicUsize::new(0);
    tcb.errno_val = 0;
    tcb.flags = 1; // joinable = true, detached = false
    tcb.exited = AtomicUsize::new(0);
    tcb.tid_address = ptr::null_mut();
    tcb.canary = 0; // TODO: randomize for security
    tcb.tsd_used = false;
    tcb._pad = [0; 7];
    // CRITICAL: Initialize TLS data array to null pointers
    // Without this, TLS access in new threads would return garbage values
    for i in 0..MAX_TLS_KEYS {
        tcb.tsd[i] = ptr::null_mut();
    }

    // Debug: verify we stored arg correctly
    {
        let msg = b"[nrlib] pthread_create: arg stored=0x";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        print_hex_u64(tcb.arg as u64);
        let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);

        // Print actual offset of arg field
        let msg = b"[nrlib] pthread_create: &tcb.arg offset=";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        let arg_offset = (&tcb.arg as *const _ as usize) - (tcb_ptr as usize);
        print_hex_u64(arg_offset as u64);
        let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);
    }

    // Initialize TLS data to null
    for i in 0..MAX_TLS_KEYS {
        tcb.tsd[i] = ptr::null_mut();
    }

    // Set TID address for CLONE_CHILD_CLEARTID
    let tid_storage = &mut tcb.tid as *mut AtomicUsize as *mut c_int;
    tcb.tid_address = tid_storage;

    // Allocate a thread slot
    let slot = match alloc_thread_slot(tcb_ptr) {
        Some(s) => s,
        None => {
            // Free the stack
            crate::syscall2(crate::SYS_MUNMAP, stack, total_stack as u64);
            return crate::EAGAIN;
        }
    };

    // Calculate child stack pointer (16-byte aligned, below TCB)
    // Leave space for a return address (though it won't be used)
    let child_stack = (tcb_addr - 8) & !0xF;

    // Set up the stack so when the child thread starts, __thread_entry gets called
    // Put the return address (entry point) on the stack
    let stack_frame = child_stack as *mut u64;
    *stack_frame = __thread_entry as u64; // Return address

    let msg = b"[nrlib] pthread_create: calling clone\n";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);

    // Clone flags for thread creation
    let clone_flags = CLONE_THREAD_FLAGS as u64;

    // Call clone syscall
    // Arguments:
    //   - flags: CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | ...
    //   - stack: child_stack (will be used as RSP)
    //   - parent_tid: not used (NULL)
    //   - child_tid: address to store TID and clear on exit
    //   - tls: TCB address (set as FS base)
    let ret = crate::syscall5(
        crate::SYS_CLONE,
        clone_flags,
        child_stack,
        0,                  // parent_tid - not needed
        tid_storage as u64, // child_tid for CLONE_CHILD_CLEARTID
        tcb_addr,           // TLS pointer = TCB address
    );

    if ret == u64::MAX || ret as i64 == -1 {
        let msg = b"[nrlib] pthread_create: clone failed\n";
        let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);
        // Clean up
        free_thread_slot(slot);
        crate::syscall2(crate::SYS_MUNMAP, stack, total_stack as u64);
        crate::refresh_errno_from_kernel();
        return crate::EAGAIN;
    }

    if ret == 0 {
        // We are the child thread!
        // This code path should not execute in the parent's flow.
        // The child will start with a fresh stack and should call __thread_entry.
        // However, because clone() returns here, we need to jump to the entry point.
        __thread_entry();
        // Never returns
    }

    // Parent: ret is child TID
    tcb.tid.store(ret as usize, Ordering::Release);
    *thread = ret as pthread_t;

    let msg = b"[nrlib] pthread_create: thread created with TID ";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);

    // Print TID
    let mut tid_buf = [0u8; 20];
    let mut tid_val = ret;
    let mut i = 0;
    if tid_val == 0 {
        tid_buf[0] = b'0';
        i = 1;
    } else {
        while tid_val > 0 && i < 20 {
            tid_buf[19 - i] = b'0' + (tid_val % 10) as u8;
            tid_val /= 10;
            i += 1;
        }
    }
    let tid_str = if ret == 0 {
        &tid_buf[0..1]
    } else {
        &tid_buf[20 - i..20]
    };
    let _ = crate::syscall3(
        SYS_WRITE_NR,
        2,
        tid_str.as_ptr() as u64,
        tid_str.len() as u64,
    );
    let _ = crate::syscall3(SYS_WRITE_NR, 2, b"\n".as_ptr() as u64, 1);

    0
}

/// Join a thread
#[no_mangle]
pub unsafe extern "C" fn pthread_join(thread: pthread_t, retval: *mut *mut c_void) -> c_int {
    let msg = b"[nrlib] pthread_join called\n";
    let _ = crate::syscall3(SYS_WRITE_NR, 2, msg.as_ptr() as u64, msg.len() as u64);

    // Find the thread in our table
    while THREAD_TABLE_LOCK
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spin_loop();
    }

    let mut found_tcb: *mut ThreadControlBlock = ptr::null_mut();
    let mut found_slot: usize = MAX_THREADS;

    for i in 0..MAX_THREADS {
        if let Some(tcb) = THREAD_TABLE[i] {
            if (*tcb).tid.load(Ordering::Acquire) == thread as usize {
                found_tcb = tcb;
                found_slot = i;
                break;
            }
        }
    }

    THREAD_TABLE_LOCK.store(0, Ordering::Release);

    if found_tcb.is_null() {
        return crate::ESRCH;
    }

    let tcb = &*found_tcb;

    if tcb.is_detached() {
        return crate::EINVAL;
    }

    // Wait for thread to exit using futex
    while tcb.exited.load(Ordering::Acquire) == 0 {
        // Wait on the tid address
        crate::syscall6(
            crate::SYS_FUTEX,
            tcb.tid_address as u64,
            super::types::FUTEX_WAIT_OP as u64,
            tcb.tid.load(Ordering::Acquire) as u64,
            0, // No timeout
            0,
            0,
        );
    }

    // Get return value
    if !retval.is_null() {
        *retval = (*found_tcb).retval;
    }

    // Free resources
    let stack_base = (*found_tcb).stack_base as u64;
    let stack_size = (*found_tcb).stack_size as u64;

    free_thread_slot(found_slot);

    // Unmap the stack
    crate::syscall2(crate::SYS_MUNMAP, stack_base, stack_size);

    0
}

/// Detach a thread
#[no_mangle]
pub unsafe extern "C" fn pthread_detach(thread: pthread_t) -> c_int {
    while THREAD_TABLE_LOCK
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spin_loop();
    }

    for i in 0..MAX_THREADS {
        if let Some(tcb) = THREAD_TABLE[i] {
            if (*tcb).tid.load(Ordering::Acquire) == thread as usize {
                (*tcb).set_detached(true);
                THREAD_TABLE_LOCK.store(0, Ordering::Release);
                return 0;
            }
        }
    }

    THREAD_TABLE_LOCK.store(0, Ordering::Release);
    crate::ESRCH
}

/// Exit current thread
#[no_mangle]
pub unsafe extern "C" fn pthread_exit(retval: *mut c_void) -> ! {
    // TODO: Find current thread's TCB and set return value
    // For now, just exit
    crate::syscall1(crate::SYS_EXIT, 0);
    loop {
        spin_loop();
    }
}

#[no_mangle]
pub unsafe extern "C" fn pthread_self() -> pthread_t {
    // In musl ABI, pthread_self returns the TCB pointer (from FS base)
    // which is what Rust std expects for TLS access
    let tcb_ptr: usize;
    core::arch::asm!(
        "mov {}, fs:0",
        out(reg) tcb_ptr,
        options(nostack, preserves_flags)
    );

    // If we have a valid TCB, return it
    // Otherwise fall back to TID
    if tcb_ptr != 0 {
        tcb_ptr as pthread_t
    } else {
        // Fallback for main thread or threads without TCB
        crate::syscall0(crate::SYS_GETTID) as pthread_t
    }
}

#[no_mangle]
pub unsafe extern "C" fn pthread_getattr_np(
    _thread: pthread_t,
    _attr: *mut pthread_attr_t,
) -> c_int {
    trace_fn!("pthread_getattr_np");
    0
}

/// Set thread name (stub - NexaOS doesn't support thread names yet)
#[no_mangle]
pub unsafe extern "C" fn pthread_setname_np(_thread: pthread_t, _name: *const i8) -> c_int {
    trace_fn!("pthread_setname_np");
    // Silently succeed - thread naming is a nice-to-have feature
    0
}

/// Get thread name (stub)
#[no_mangle]
pub unsafe extern "C" fn pthread_getname_np(
    _thread: pthread_t,
    name: *mut i8,
    len: size_t,
) -> c_int {
    trace_fn!("pthread_getname_np");
    // Return empty string
    if !name.is_null() && len > 0 {
        *name = 0;
    }
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
    let _ = crate::syscall3(
        SYS_WRITE_NR,
        2,
        diag_msg.as_ptr() as u64,
        diag_msg.len() as u64,
    );

    for i in 0..16 {
        let shift = (15 - i) * 4;
        let nibble = ((routine_addr >> shift) & 0xF) as u8;
        let ch = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + nibble - 10
        };
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
            let _ = crate::syscall3(
                SYS_WRITE_NR,
                2,
                diag_msg.as_ptr() as u64,
                diag_msg.len() as u64,
            );

            init();

            let diag_msg = b"[nrlib] pthread_once: Init routine completed\n";
            let _ = crate::syscall3(
                SYS_WRITE_NR,
                2,
                diag_msg.as_ptr() as u64,
                diag_msg.len() as u64,
            );

            control.state.store(PTHREAD_ONCE_DONE, Ordering::Release);
            0
        }
        Err(PTHREAD_ONCE_DONE) => 0,
        Err(_) => {
            let diag_msg = b"[nrlib] pthread_once: Waiting for init from another thread\n";
            let _ = crate::syscall3(
                SYS_WRITE_NR,
                2,
                diag_msg.as_ptr() as u64,
                diag_msg.len() as u64,
            );

            let mut spin_count = 0u32;
            loop {
                if control.state.load(Ordering::Acquire) != PTHREAD_ONCE_IN_PROGRESS {
                    break;
                }
                spin_count += 1;
                if spin_count > 100000 {
                    let hang_msg = b"[nrlib] WARNING: pthread_once init timeout - possible hang\n";
                    let _ = crate::syscall3(
                        SYS_WRITE_NR,
                        2,
                        hang_msg.as_ptr() as u64,
                        hang_msg.len() as u64,
                    );
                    break;
                }
                spin_loop();
            }

            let diag_msg = b"[nrlib] pthread_once: Init completed by other thread\n";
            let _ = crate::syscall3(
                SYS_WRITE_NR,
                2,
                diag_msg.as_ptr() as u64,
                diag_msg.len() as u64,
            );

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
