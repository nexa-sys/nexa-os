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
pub fn online_cpus() -> usize {
    ONLINE_CPUS.load(Ordering::Acquire)
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
