//! TLS (Thread-Local Storage) support for the NexaOS dynamic linker
//!
//! This implements ELF TLS (Thread-Local Storage) variant II for x86_64:
//! - TCB (Thread Control Block) is at FS base
//! - TLS data is located before the TCB
//! - DTV (Dynamic Thread Vector) provides access to each module's TLS block

use core::arch::asm;
use core::ptr;

use crate::constants::MAX_TLS_MODULES;
use crate::syscall::syscall2;

// Constants
const ARCH_SET_FS: u64 = 0x1002;
const SYS_ARCH_PRCTL: u64 = 158;

// ============================================================================
// TLS Module Information
// ============================================================================

#[derive(Clone, Copy)]
pub struct TlsModule {
    /// Module ID (1-based, 0 means unused)
    pub id: u64,
    /// TLS image address in memory (source for initialization)
    pub image: u64,
    /// TLS image size (file size, .tdata)
    pub filesz: u64,
    /// TLS memory size (including BSS, .tbss)
    pub memsz: u64,
    /// TLS alignment
    pub align: u64,
    /// Offset from TP (thread pointer) - negative for variant II
    pub offset: i64,
}

impl TlsModule {
    pub const fn new() -> Self {
        Self {
            id: 0,
            image: 0,
            filesz: 0,
            memsz: 0,
            align: 1,
            offset: 0,
        }
    }
}

// ============================================================================
// Thread Control Block (TCB) for main thread
// ============================================================================

/// TCB structure compatible with nrlib
/// Must match the layout in nrlib/src/libc_compat/pthread.rs
#[repr(C)]
pub struct TcbHeader {
    /// Self pointer (for TLS access via %fs:0)
    pub self_ptr: *mut TcbHeader,
    /// DTV pointer (Dynamic Thread Vector)
    pub dtv: *mut usize,
    /// Padding to match nrlib's ThreadControlBlock layout
    _padding: [u8; 1024 + 128 - 16],
}

// ============================================================================
// Global TLS State
// ============================================================================

pub struct TlsState {
    /// TLS modules
    pub modules: [TlsModule; MAX_TLS_MODULES],
    /// Number of TLS modules
    pub count: usize,
    /// Total static TLS size (sum of all module memsz with alignment)
    pub total_size: usize,
    /// TLS module ID counter
    pub next_id: u64,
    /// TLS block base address (where all TLS data is stored)
    pub tls_block: u64,
    /// TCB address
    pub tcb_addr: u64,
    /// DTV address
    pub dtv_addr: u64,
    /// Initialization done flag
    pub initialized: bool,
}

impl TlsState {
    pub const fn new() -> Self {
        Self {
            modules: [TlsModule::new(); MAX_TLS_MODULES],
            count: 0,
            total_size: 0,
            next_id: 1,
            tls_block: 0,
            tcb_addr: 0,
            dtv_addr: 0,
            initialized: false,
        }
    }
}

pub static mut TLS_STATE: TlsState = TlsState::new();

// Static storage for main thread TCB and TLS
// 16KB should be enough for most programs
const TLS_STATIC_SIZE: usize = 16 * 1024;
static mut TLS_STATIC_BLOCK: [u8; TLS_STATIC_SIZE] = [0u8; TLS_STATIC_SIZE];

// DTV storage (module count + 1 entry per module)
const MAX_DTV_ENTRIES: usize = MAX_TLS_MODULES + 2;
static mut DTV_STORAGE: [usize; MAX_DTV_ENTRIES] = [0; MAX_DTV_ENTRIES];

// ============================================================================
// TLS Functions
// ============================================================================

/// Register a TLS module (called during library loading)
pub unsafe fn register_tls_module(image: u64, filesz: u64, memsz: u64, align: u64) -> u64 {
    if TLS_STATE.count >= MAX_TLS_MODULES {
        return 0;
    }

    let module_id = TLS_STATE.next_id;
    TLS_STATE.next_id += 1;

    let idx = TLS_STATE.count;
    TLS_STATE.modules[idx] = TlsModule {
        id: module_id,
        image,
        filesz,
        memsz,
        align: if align == 0 { 1 } else { align },
        offset: 0, // Will be calculated in setup_main_thread_tls
    };
    TLS_STATE.count += 1;

    module_id
}

/// Align value up to alignment
fn align_up(value: usize, align: usize) -> usize {
    if align == 0 {
        return value;
    }
    (value + align - 1) & !(align - 1)
}

/// Setup TLS for the main thread
/// Must be called after all libraries are loaded but before calling _start
pub unsafe fn setup_main_thread_tls() {
    if TLS_STATE.initialized {
        return;
    }

    // Calculate total TLS size needed
    // Layout (variant II, x86_64):
    //   [module N TLS] ... [module 1 TLS] [TCB]
    //                                     ^
    //                                     FS base points here
    
    let tcb_size = core::mem::size_of::<TcbHeader>();
    let mut current_offset: usize = 0;
    
    // Calculate offsets for each module (going backwards from TCB)
    for i in 0..TLS_STATE.count {
        let module = &mut TLS_STATE.modules[i];
        let aligned_size = align_up(module.memsz as usize, module.align as usize);
        current_offset += aligned_size;
        // Offset is negative from TP for variant II
        module.offset = -(current_offset as i64);
    }
    
    TLS_STATE.total_size = current_offset;
    
    // Total allocation = TLS data + TCB + DTV
    let total_alloc = current_offset + tcb_size;
    
    if total_alloc > TLS_STATIC_SIZE {
        // TLS block too large - this is a limitation of the current implementation
        return;
    }
    
    // Setup pointers in static block
    // [TLS data (total_size bytes)] [TCB (tcb_size bytes)]
    let block_base = TLS_STATIC_BLOCK.as_mut_ptr() as u64;
    let tcb_addr = block_base + current_offset as u64;
    
    TLS_STATE.tls_block = block_base;
    TLS_STATE.tcb_addr = tcb_addr;
    
    // Initialize TCB
    let tcb = tcb_addr as *mut TcbHeader;
    (*tcb).self_ptr = tcb;
    (*tcb).dtv = DTV_STORAGE.as_mut_ptr();
    
    // Setup DTV
    // DTV[0] = generation count (0 for now)
    // DTV[1] = address of module 1's TLS block
    // DTV[2] = address of module 2's TLS block
    // etc.
    DTV_STORAGE[0] = TLS_STATE.count; // Generation/count
    
    for i in 0..TLS_STATE.count {
        let module = &TLS_STATE.modules[i];
        // Calculate absolute address of this module's TLS block
        let tls_addr = (tcb_addr as i64 + module.offset) as u64;
        DTV_STORAGE[module.id as usize] = tls_addr as usize;
        
        // Initialize TLS data: copy .tdata, zero .tbss
        if module.filesz > 0 {
            ptr::copy_nonoverlapping(
                module.image as *const u8,
                tls_addr as *mut u8,
                module.filesz as usize,
            );
        }
        // Zero the rest (.tbss)
        if module.memsz > module.filesz {
            ptr::write_bytes(
                (tls_addr + module.filesz) as *mut u8,
                0,
                (module.memsz - module.filesz) as usize,
            );
        }
    }
    
    TLS_STATE.dtv_addr = DTV_STORAGE.as_ptr() as u64;
    
    // Set FS base to point to TCB
    syscall2(SYS_ARCH_PRCTL, ARCH_SET_FS, tcb_addr);
    
    TLS_STATE.initialized = true;
}

// ============================================================================
// TLS Access Function (musl/glibc compatible)
// ============================================================================

/// TLS index structure for __tls_get_addr
#[repr(C)]
pub struct TlsIndex {
    pub ti_module: u64, // Module ID (1-based)
    pub ti_offset: u64, // Offset within module's TLS block
}

/// __tls_get_addr - TLS access function (musl/glibc compatible)
/// Used for accessing thread-local variables in dynamically loaded libraries
#[no_mangle]
pub unsafe extern "C" fn __tls_get_addr(ti: *const TlsIndex) -> *mut u8 {
    if ti.is_null() {
        return core::ptr::null_mut();
    }

    let module_id = (*ti).ti_module;
    let offset = (*ti).ti_offset;

    // Get thread pointer (FS base on x86_64)
    let tp: u64;
    asm!("mov {}, fs:0", out(reg) tp, options(nostack, preserves_flags, readonly));

    // For static TLS (variant II, x86_64), TLS data is below TP
    // Find the module and calculate address
    for i in 0..TLS_STATE.count {
        if TLS_STATE.modules[i].id == module_id {
            let tls_offset = TLS_STATE.modules[i].offset;
            return (tp as i64 + tls_offset + offset as i64) as *mut u8;
        }
    }

    // Module not found - return TP + offset as fallback for module 0
    if module_id == 0 {
        return (tp as i64 + offset as i64) as *mut u8;
    }

    core::ptr::null_mut()
}
