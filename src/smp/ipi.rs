//! IPI (Inter-Processor Interrupt) Constants and Functions
//!
//! This module provides IPI vector definitions and functions for sending
//! IPIs between CPU cores for reschedule, TLB flush, and other operations.

use core::sync::atomic::Ordering;

use crate::lapic;

use super::state::{CPU_TOTAL, SMP_READY};
use super::types::{cpu_info, CpuStatus, MAX_CPUS};

// ============================================================================
// IPI Vector Constants
// ============================================================================

/// IPI vector for reschedule requests
pub const IPI_RESCHEDULE: u8 = 0xF0;

/// IPI vector for TLB flush requests
pub const IPI_TLB_FLUSH: u8 = 0xF1;

/// IPI vector for function call requests
pub const IPI_CALL_FUNCTION: u8 = 0xF2;

/// IPI vector for halt requests
pub const IPI_HALT: u8 = 0xF3;

// ============================================================================
// IPI Send Functions
// ============================================================================

/// Send reschedule IPI to a specific CPU (supports up to 1024 CPUs)
pub fn send_reschedule_ipi(cpu_id: u16) {
    if !SMP_READY.load(Ordering::Acquire) {
        return;
    }
    let cpu_id = cpu_id as usize;
    if cpu_id >= CPU_TOTAL.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        let info = cpu_info(cpu_id);
        lapic::send_ipi(info.apic_id, IPI_RESCHEDULE);
    }
}

/// Send TLB flush IPI to all CPUs except current
pub fn send_tlb_flush_ipi_all() {
    if !SMP_READY.load(Ordering::Acquire) {
        return;
    }
    let current = super::cpu::current_cpu_id();
    let total = CPU_TOTAL.load(Ordering::Relaxed);
    unsafe {
        for i in 0..total {
            if i == current as usize {
                continue;
            }
            let info = cpu_info(i);
            if CpuStatus::from_atomic(info.status.load(Ordering::Acquire)) == CpuStatus::Online {
                lapic::send_ipi(info.apic_id, IPI_TLB_FLUSH);
            }
        }
    }
}

/// Broadcast IPI to all online CPUs except current
pub fn send_ipi_broadcast(vector: u8) {
    if !SMP_READY.load(Ordering::Acquire) {
        return;
    }
    let current = super::cpu::current_cpu_id();
    let total = CPU_TOTAL.load(Ordering::Relaxed);
    unsafe {
        for i in 0..total {
            if i == current as usize {
                continue;
            }
            let info = cpu_info(i);
            if CpuStatus::from_atomic(info.status.load(Ordering::Acquire)) == CpuStatus::Online {
                lapic::send_ipi(info.apic_id, vector);
            }
        }
    }
}
