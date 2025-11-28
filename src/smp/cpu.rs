//! CPU Management Functions
//!
//! This module provides functions for querying and managing CPU state,
//! including getting current CPU ID, CPU count, and per-CPU data.

use core::sync::atomic::Ordering;

use crate::lapic;

use super::state::{CPU_TOTAL, ONLINE_CPUS, SMP_READY};
use super::types::{cpu_data, cpu_info, CpuData};

/// Get the total number of CPUs detected
pub fn cpu_count() -> usize {
    CPU_TOTAL.load(Ordering::SeqCst)
}

/// Get the number of CPUs currently online
/// Uses trampoline status array for accurate count after SMP initialization
pub fn online_cpus() -> usize {
    // During boot, ONLINE_CPUS atomic may not be fully updated by all AP cores yet.
    // Use current_online() which reads from trampoline status array for accuracy.
    if SMP_READY.load(Ordering::Acquire) {
        // After SMP is ready, read the authoritative count from trampoline
        current_online()
    } else {
        // Before SMP init, only BSP is online
        ONLINE_CPUS.load(Ordering::Acquire)
    }
}

/// Get current CPU ID from LAPIC (supports up to 1024 CPUs)
pub fn current_cpu_id() -> u16 {
    if !SMP_READY.load(Ordering::Acquire) {
        return 0;
    }
    let apic_id = lapic::current_apic_id();
    unsafe {
        for i in 0..CPU_TOTAL.load(Ordering::Relaxed) {
            let info = cpu_info(i);
            if info.apic_id == apic_id {
                return i as u16;
            }
        }
    }
    0
}

/// Get per-CPU data for current CPU
pub fn current_cpu_data() -> Option<&'static CpuData> {
    if !SMP_READY.load(Ordering::Acquire) {
        return None;
    }
    let cpu_id = current_cpu_id() as usize;
    if cpu_id < CPU_TOTAL.load(Ordering::Relaxed) {
        unsafe { Some(cpu_data(cpu_id)) }
    } else {
        None
    }
}

/// Count the number of CPUs currently online
/// Uses trampoline status array which is reliably accessible from all cores
pub fn current_online() -> usize {
    unsafe {
        let total = CPU_TOTAL.load(Ordering::SeqCst);
        let mut online = 0;
        for idx in 0..total {
            // Use trampoline status array instead of dynamically allocated CpuInfo
            // BSP (idx=0) is always online if we got here
            if idx == 0 {
                online += 1;
            } else {
                let status = super::trampoline::get_cpu_status_from_trampoline(idx);
                if status == super::trampoline::CPU_STATUS_ONLINE {
                    online += 1;
                }
            }
        }
        online
    }
}

/// Get the GS_DATA pointer for the current CPU
/// 
/// Returns the address of the GS_DATA structure that should be used by the current CPU.
/// - BSP (CPU 0) uses the static `initramfs::GS_DATA` before SMP init
/// - After SMP init, each CPU uses its own per-CPU GS_DATA
/// 
/// # Safety
/// This function returns a raw pointer. The caller must ensure proper synchronization
/// when accessing the GS_DATA structure.
pub fn current_gs_data_ptr() -> *mut u64 {
    if !SMP_READY.load(Ordering::Acquire) {
        // Before SMP init, only BSP is running, use static GS_DATA
        return unsafe { 
            core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *mut u64 
        };
    }
    
    let cpu_id = current_cpu_id() as usize;
    
    // Try to get per-CPU GS_DATA pointer
    match super::alloc::get_gs_data_ptr(cpu_id) {
        Ok(ptr) => ptr as *mut u64,
        Err(_) => {
            // Fallback to static GS_DATA (this shouldn't happen in normal operation)
            unsafe { 
                core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *mut u64 
            }
        }
    }
}

/// Get the GS_DATA pointer for a specific CPU
/// 
/// Returns the address of the GS_DATA structure for the specified CPU index.
/// 
/// # Arguments
/// * `cpu_index` - The CPU index (0 for BSP, 1+ for APs)
/// 
/// # Safety
/// This function returns a raw pointer. The caller must ensure proper synchronization
/// when accessing the GS_DATA structure.
pub fn gs_data_ptr_for_cpu(cpu_index: usize) -> *mut u64 {
    if cpu_index == 0 && !SMP_READY.load(Ordering::Acquire) {
        // BSP before SMP init
        return unsafe { 
            core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *mut u64 
        };
    }
    
    // Try to get per-CPU GS_DATA pointer
    match super::alloc::get_gs_data_ptr(cpu_index) {
        Ok(ptr) => ptr as *mut u64,
        Err(_) => {
            // Fallback to static GS_DATA
            unsafe { 
                core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *mut u64 
            }
        }
    }
}
