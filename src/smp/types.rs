//! SMP Type Definitions
//!
//! This module contains type definitions for SMP (Symmetric Multi-Processing) support,
//! including per-CPU data structures, CPU status, and boot argument types.
//!
//! # Per-CPU Architecture
//!
//! Each CPU (BSP and APs) has dedicated per-CPU resources:
//! - **GDT/TSS**: Per-CPU Global Descriptor Table and Task State Segment (see `gdt.rs`)
//! - **IDT**: Per-CPU Interrupt Descriptor Table (see `interrupts/idt.rs`)
//! - **GS data**: Per-CPU segment data for syscall/interrupt handling
//! - **Stacks**: Kernel stack, double fault stack, error code stack per CPU
//! - **Runtime data**: `CpuData` structure for scheduling and statistics
//!
//! This isolation ensures true SMP safety without shared mutable state contention.

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8};

use crate::acpi;

/// Maximum number of CPUs supported
pub const MAX_CPUS: usize = acpi::MAX_CPUS;

/// Trampoline configuration
pub const TRAMPOLINE_BASE: u64 = 0x8000;
/// Maximum trampoline size: code (~2KB) + APIC mapping (256B) + per-CPU data (1024*24=24KB) + padding
/// Total: ~32KB should be sufficient for 1024 CPUs
pub const TRAMPOLINE_MAX_SIZE: usize = 32 * 1024;
pub const TRAMPOLINE_VECTOR: u8 = (TRAMPOLINE_BASE >> 12) as u8;

/// Per-CPU data structure size: 3 * 8 bytes = 24 bytes (stack_ptr, entry_ptr, arg_ptr)
pub const PER_CPU_DATA_SIZE: usize = 24;

/// AP stack configuration
pub const AP_STACK_SIZE: usize = 16 * 4096;

/// Startup timing constants
pub const STARTUP_WAIT_LOOPS: u64 = 50_000_000; // Increased for reliability
pub const STARTUP_RETRY_MAX: u32 = 3;

/// Per-CPU GS data (same layout as initramfs::GsData)
#[repr(C, align(64))]
#[derive(Copy, Clone)]
pub struct PerCpuGsData(pub [u64; 32]);

impl PerCpuGsData {
    pub const fn new() -> Self {
        Self([0; 32])
    }
}

/// Per-CPU runtime data - isolated to each CPU to avoid cache line contention
#[repr(C, align(64))] // Cache line aligned to prevent false sharing
pub struct CpuData {
    pub cpu_id: u16,  // Supports up to 1024 CPUs
    pub apic_id: u32,
    pub current_pid: AtomicU32, // Currently running process
    pub idle_time: AtomicU64,   // Idle time in ticks
    pub busy_time: AtomicU64,   // Busy time in ticks
    pub reschedule_pending: AtomicBool,
    pub tlb_flush_pending: AtomicBool,
    pub context_switches: AtomicU64,
    pub interrupts_handled: AtomicU64,
}

impl CpuData {
    pub fn new(cpu_id: u16, apic_id: u32) -> Self {
        Self {
            cpu_id,
            apic_id,
            current_pid: AtomicU32::new(0),
            idle_time: AtomicU64::new(0),
            busy_time: AtomicU64::new(0),
            reschedule_pending: AtomicBool::new(false),
            tlb_flush_pending: AtomicBool::new(false),
            context_switches: AtomicU64::new(0),
            interrupts_handled: AtomicU64::new(0),
        }
    }
}

/// CPU status enumeration
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CpuStatus {
    Offline = 0,
    Booting = 1,
    Online = 2,
}

impl CpuStatus {
    pub fn from_atomic(val: u8) -> Self {
        match val {
            1 => CpuStatus::Booting,
            2 => CpuStatus::Online,
            _ => CpuStatus::Offline,
        }
    }
}

/// CPU information structure
#[allow(dead_code)]
pub struct CpuInfo {
    pub apic_id: u32,
    pub acpi_id: u8,
    pub is_bsp: bool,
    pub status: AtomicU8,
    pub startup_attempts: AtomicU32,
    pub last_error: AtomicU32, // Error code from last startup attempt
}

impl CpuInfo {
    pub fn new(apic_id: u32, acpi_id: u8, is_bsp: bool) -> Self {
        let initial = if is_bsp {
            CpuStatus::Online
        } else {
            CpuStatus::Offline
        } as u8;
        Self {
            apic_id,
            acpi_id,
            is_bsp,
            status: AtomicU8::new(initial),
            startup_attempts: AtomicU32::new(0),
            last_error: AtomicU32::new(0),
        }
    }
}

/// AP boot arguments passed to each AP core
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ApBootArgs {
    pub cpu_index: u32,
    pub apic_id: u32,
}

impl ApBootArgs {
    pub const fn new() -> Self {
        Self {
            cpu_index: 0,
            apic_id: 0,
        }
    }
}

/// Per-CPU trampoline data structure (matches assembly layout)
/// Layout: [stack_ptr:8][entry_ptr:8][arg_ptr:8] = 24 bytes per CPU
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct PerCpuTrampolineData {
    pub stack_ptr: u64,     // Offset 0: Stack top address
    pub entry_ptr: u64,     // Offset 8: Entry function address
    pub arg_ptr: u64,       // Offset 16: Boot arguments pointer
}

impl PerCpuTrampolineData {
    pub const fn new() -> Self {
        Self {
            stack_ptr: 0,
            entry_ptr: 0,
            arg_ptr: 0,
        }
    }
}

/// Aligned AP stack structure
#[repr(C, align(16))] // x86_64 ABI requires 16-byte stack alignment
pub struct AlignedApStack(pub [u8; AP_STACK_SIZE]);

// ============================================================================
// Global static data (minimal - most data is dynamically allocated)
// ============================================================================

/// Static fallback count for BSP-only operation when dynamic alloc unavailable
pub const STATIC_CPU_COUNT: usize = 4;

/// GS data - small static array for early boot, rest is dynamic
pub static mut AP_GS_DATA: [PerCpuGsData; STATIC_CPU_COUNT] = [PerCpuGsData::new(); STATIC_CPU_COUNT];

/// Debug: AP arrival flags (non-zero = arrived) - keep static for atomic access
pub static AP_ARRIVED: [AtomicU32; MAX_CPUS] = {
    const INIT: AtomicU32 = AtomicU32::new(0);
    [INIT; MAX_CPUS]
};

/// AP stacks - small static array for early boot, rest is dynamic
/// Only allocate for STATIC_CPU_COUNT - 1 APs statically
pub static mut AP_STACKS: [AlignedApStack; STATIC_CPU_COUNT - 1] =
    unsafe { MaybeUninit::<[AlignedApStack; STATIC_CPU_COUNT - 1]>::zeroed().assume_init() };

/// Per-CPU data array - small static array for early boot
pub static mut CPU_DATA: [MaybeUninit<CpuData>; STATIC_CPU_COUNT] =
    unsafe { MaybeUninit::<[MaybeUninit<CpuData>; STATIC_CPU_COUNT]>::uninit().assume_init() };

/// CPU information array - small static array for early boot
pub static mut CPU_INFOS: [MaybeUninit<CpuInfo>; STATIC_CPU_COUNT] =
    unsafe { MaybeUninit::<[MaybeUninit<CpuInfo>; STATIC_CPU_COUNT]>::uninit().assume_init() };

/// AP boot arguments - small static array
pub static mut AP_BOOT_ARGS: [ApBootArgs; STATIC_CPU_COUNT] = [ApBootArgs::new(); STATIC_CPU_COUNT];

/// BSP APIC ID
pub static mut BSP_APIC_ID: u32 = 0;

// ============================================================================
// Helper functions for accessing global data
// ============================================================================

/// Get CPU data by index (unsafe - caller must ensure index is valid)
pub unsafe fn cpu_data(idx: usize) -> &'static CpuData {
    CPU_DATA[idx].assume_init_ref()
}

/// Get CPU info by index (unsafe - caller must ensure index is valid)
pub unsafe fn cpu_info(idx: usize) -> &'static CpuInfo {
    CPU_INFOS[idx].assume_init_ref()
}
