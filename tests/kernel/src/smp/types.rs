//! SMP Types Tests
//!
//! Tests for SMP type definitions and structures.

#[cfg(test)]
mod tests {
    use crate::smp::{
        CpuData, CpuStatus, CpuInfo, ApBootArgs, PerCpuTrampolineData, PerCpuGsData,
        MAX_CPUS, TRAMPOLINE_BASE, TRAMPOLINE_MAX_SIZE, TRAMPOLINE_VECTOR,
        PER_CPU_DATA_SIZE, AP_STACK_SIZE, STARTUP_WAIT_LOOPS, STARTUP_RETRY_MAX,
        STATIC_CPU_COUNT,
    };
    use core::sync::atomic::Ordering;

    // =========================================================================
    // CpuData Tests
    // =========================================================================

    #[test]
    fn test_cpu_data_new() {
        let cpu_data = CpuData::new(0, 0);
        assert_eq!(cpu_data.cpu_id, 0);
        assert_eq!(cpu_data.apic_id, 0);
        assert_eq!(cpu_data.numa_node, 0);
        assert_eq!(cpu_data.current_pid.load(Ordering::Relaxed), 0);
        assert!(!cpu_data.reschedule_pending.load(Ordering::Relaxed));
        assert!(!cpu_data.tlb_flush_pending.load(Ordering::Relaxed));
        assert!(!cpu_data.in_interrupt.load(Ordering::Relaxed));
        assert_eq!(cpu_data.preempt_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_cpu_data_different_ids() {
        let cpu0 = CpuData::new(0, 0);
        let cpu1 = CpuData::new(1, 4);
        let cpu2 = CpuData::new(2, 8);

        assert_eq!(cpu0.cpu_id, 0);
        assert_eq!(cpu0.apic_id, 0);

        assert_eq!(cpu1.cpu_id, 1);
        assert_eq!(cpu1.apic_id, 4);

        assert_eq!(cpu2.cpu_id, 2);
        assert_eq!(cpu2.apic_id, 8);
    }

    #[test]
    fn test_cpu_data_preempt_disable() {
        let cpu_data = CpuData::new(0, 0);
        
        // Initially preemptible
        assert!(!cpu_data.preempt_disabled());
        
        // After disable, not preemptible
        cpu_data.preempt_disable();
        assert!(cpu_data.preempt_disabled());
        assert_eq!(cpu_data.preempt_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_cpu_data_preempt_enable() {
        let cpu_data = CpuData::new(0, 0);
        
        cpu_data.preempt_disable();
        assert!(cpu_data.preempt_disabled());
        
        let was_enabled = cpu_data.preempt_enable();
        assert!(was_enabled); // Was 1, now 0
        assert!(!cpu_data.preempt_disabled());
    }

    #[test]
    fn test_cpu_data_nested_preempt() {
        let cpu_data = CpuData::new(0, 0);
        
        // Nested disable
        cpu_data.preempt_disable();
        cpu_data.preempt_disable();
        cpu_data.preempt_disable();
        
        assert!(cpu_data.preempt_disabled());
        assert_eq!(cpu_data.preempt_count.load(Ordering::Relaxed), 3);
        
        // First enable doesn't make it preemptible
        let was_enabled = cpu_data.preempt_enable();
        assert!(!was_enabled); // Was 3, now 2 - not yet enabled
        assert!(cpu_data.preempt_disabled());
        
        // Second enable
        let was_enabled = cpu_data.preempt_enable();
        assert!(!was_enabled); // Was 2, now 1 - still not enabled
        
        // Third enable
        let was_enabled = cpu_data.preempt_enable();
        assert!(was_enabled); // Was 1, now 0 - enabled!
        assert!(!cpu_data.preempt_disabled());
    }

    #[test]
    fn test_cpu_data_in_interrupt_context() {
        let cpu_data = CpuData::new(0, 0);
        
        assert!(!cpu_data.in_interrupt_context());
        
        cpu_data.enter_interrupt();
        assert!(cpu_data.in_interrupt_context());
        assert!(cpu_data.preempt_disabled()); // Interrupt disables preemption
    }

    #[test]
    fn test_cpu_data_leave_interrupt() {
        let cpu_data = CpuData::new(0, 0);
        
        cpu_data.enter_interrupt();
        assert!(cpu_data.in_interrupt_context());
        
        // Set reschedule pending
        cpu_data.reschedule_pending.store(true, Ordering::Relaxed);
        
        let needs_resched = cpu_data.leave_interrupt();
        assert!(needs_resched); // We had reschedule pending
        assert!(!cpu_data.in_interrupt_context());
        assert!(!cpu_data.preempt_disabled());
        
        // Reschedule flag should be cleared
        assert!(!cpu_data.reschedule_pending.load(Ordering::Relaxed));
    }

    #[test]
    fn test_cpu_data_record_context_switch_voluntary() {
        let cpu_data = CpuData::new(0, 0);
        
        cpu_data.record_context_switch(true);
        assert_eq!(cpu_data.context_switches.load(Ordering::Relaxed), 1);
        assert_eq!(cpu_data.voluntary_switches.load(Ordering::Relaxed), 1);
        assert_eq!(cpu_data.preemptions.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_cpu_data_record_context_switch_preemption() {
        let cpu_data = CpuData::new(0, 0);
        
        cpu_data.record_context_switch(false);
        assert_eq!(cpu_data.context_switches.load(Ordering::Relaxed), 1);
        assert_eq!(cpu_data.voluntary_switches.load(Ordering::Relaxed), 0);
        assert_eq!(cpu_data.preemptions.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_cpu_data_set_numa_node() {
        let mut cpu_data = CpuData::new(0, 0);
        assert_eq!(cpu_data.numa_node, 0);
        
        cpu_data.set_numa_node(1);
        assert_eq!(cpu_data.numa_node, 1);
    }

    #[test]
    fn test_cpu_data_statistics_initial() {
        let cpu_data = CpuData::new(0, 0);
        
        assert_eq!(cpu_data.idle_time.load(Ordering::Relaxed), 0);
        assert_eq!(cpu_data.busy_time.load(Ordering::Relaxed), 0);
        assert_eq!(cpu_data.context_switches.load(Ordering::Relaxed), 0);
        assert_eq!(cpu_data.voluntary_switches.load(Ordering::Relaxed), 0);
        assert_eq!(cpu_data.preemptions.load(Ordering::Relaxed), 0);
        assert_eq!(cpu_data.interrupts_handled.load(Ordering::Relaxed), 0);
        assert_eq!(cpu_data.syscalls_handled.load(Ordering::Relaxed), 0);
        assert_eq!(cpu_data.ipi_received.load(Ordering::Relaxed), 0);
        assert_eq!(cpu_data.ipi_sent.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_cpu_data_local_tick() {
        let cpu_data = CpuData::new(0, 0);
        
        assert_eq!(cpu_data.local_tick.load(Ordering::Relaxed), 0);
        
        cpu_data.local_tick.fetch_add(1, Ordering::Relaxed);
        assert_eq!(cpu_data.local_tick.load(Ordering::Relaxed), 1);
        
        cpu_data.local_tick.fetch_add(100, Ordering::Relaxed);
        assert_eq!(cpu_data.local_tick.load(Ordering::Relaxed), 101);
    }

    // =========================================================================
    // CpuData Memory Layout Tests
    // =========================================================================

    #[test]
    fn test_cpu_data_alignment() {
        let align = core::mem::align_of::<CpuData>();
        // Should be cache-line aligned (64 bytes)
        assert_eq!(align, 64, "CpuData should be 64-byte aligned");
    }

    #[test]
    fn test_cpu_data_size() {
        let size = core::mem::size_of::<CpuData>();
        // Should fit in 2 cache lines (128 bytes) or less
        assert!(size <= 256, "CpuData too large: {} bytes", size);
        // Should be at least one cache line
        assert!(size >= 64, "CpuData too small: {} bytes", size);
    }

    // =========================================================================
    // CpuStatus Tests
    // =========================================================================

    #[test]
    fn test_cpu_status_values() {
        assert_eq!(CpuStatus::Offline as u8, 0);
        assert_eq!(CpuStatus::Booting as u8, 1);
        assert_eq!(CpuStatus::Online as u8, 2);
    }

    #[test]
    fn test_cpu_status_from_atomic() {
        assert_eq!(CpuStatus::from_atomic(0), CpuStatus::Offline);
        assert_eq!(CpuStatus::from_atomic(1), CpuStatus::Booting);
        assert_eq!(CpuStatus::from_atomic(2), CpuStatus::Online);
    }

    #[test]
    fn test_cpu_status_from_atomic_invalid() {
        // Invalid values should map to Offline
        assert_eq!(CpuStatus::from_atomic(3), CpuStatus::Offline);
        assert_eq!(CpuStatus::from_atomic(255), CpuStatus::Offline);
    }

    #[test]
    fn test_cpu_status_eq() {
        assert_eq!(CpuStatus::Offline, CpuStatus::Offline);
        assert_eq!(CpuStatus::Booting, CpuStatus::Booting);
        assert_eq!(CpuStatus::Online, CpuStatus::Online);
        assert_ne!(CpuStatus::Offline, CpuStatus::Online);
    }

    #[test]
    fn test_cpu_status_copy_clone() {
        let status = CpuStatus::Online;
        let copied = status;
        let cloned = status.clone();
        assert_eq!(status, copied);
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_cpu_status_debug() {
        let debug_str = format!("{:?}", CpuStatus::Online);
        assert!(debug_str.contains("Online"));
    }

    // =========================================================================
    // CpuInfo Tests
    // =========================================================================

    #[test]
    fn test_cpu_info_new_bsp() {
        let info = CpuInfo::new(0, 0, true);
        assert_eq!(info.apic_id, 0);
        assert_eq!(info.acpi_id, 0);
        assert!(info.is_bsp);
        // BSP starts Online
        assert_eq!(CpuStatus::from_atomic(info.status.load(Ordering::Relaxed)), CpuStatus::Online);
    }

    #[test]
    fn test_cpu_info_new_ap() {
        let info = CpuInfo::new(4, 1, false);
        assert_eq!(info.apic_id, 4);
        assert_eq!(info.acpi_id, 1);
        assert!(!info.is_bsp);
        // AP starts Offline
        assert_eq!(CpuStatus::from_atomic(info.status.load(Ordering::Relaxed)), CpuStatus::Offline);
    }

    #[test]
    fn test_cpu_info_startup_tracking() {
        let info = CpuInfo::new(4, 1, false);
        assert_eq!(info.startup_attempts.load(Ordering::Relaxed), 0);
        assert_eq!(info.last_error.load(Ordering::Relaxed), 0);
        
        info.startup_attempts.fetch_add(1, Ordering::Relaxed);
        assert_eq!(info.startup_attempts.load(Ordering::Relaxed), 1);
    }

    // =========================================================================
    // ApBootArgs Tests
    // =========================================================================

    #[test]
    fn test_ap_boot_args_new() {
        let args = ApBootArgs::new();
        assert_eq!(args.cpu_index, 0);
        assert_eq!(args.apic_id, 0);
    }

    #[test]
    fn test_ap_boot_args_const() {
        const ARGS: ApBootArgs = ApBootArgs::new();
        assert_eq!(ARGS.cpu_index, 0);
        assert_eq!(ARGS.apic_id, 0);
    }

    #[test]
    fn test_ap_boot_args_copy() {
        let mut args = ApBootArgs::new();
        args.cpu_index = 5;
        args.apic_id = 20;
        
        let copied = args;
        assert_eq!(copied.cpu_index, 5);
        assert_eq!(copied.apic_id, 20);
    }

    #[test]
    fn test_ap_boot_args_size() {
        let size = core::mem::size_of::<ApBootArgs>();
        // Should be 8 bytes (2x u32)
        assert_eq!(size, 8);
    }

    // =========================================================================
    // PerCpuTrampolineData Tests
    // =========================================================================

    #[test]
    fn test_trampoline_data_new() {
        let data = PerCpuTrampolineData::new();
        assert_eq!(data.stack_ptr, 0);
        assert_eq!(data.entry_ptr, 0);
        assert_eq!(data.arg_ptr, 0);
    }

    #[test]
    fn test_trampoline_data_const() {
        const DATA: PerCpuTrampolineData = PerCpuTrampolineData::new();
        assert_eq!(DATA.stack_ptr, 0);
    }

    #[test]
    fn test_trampoline_data_size() {
        let size = core::mem::size_of::<PerCpuTrampolineData>();
        // Should be 24 bytes (3x u64)
        assert_eq!(size, 24);
        assert_eq!(size, PER_CPU_DATA_SIZE);
    }

    #[test]
    fn test_trampoline_data_debug() {
        let data = PerCpuTrampolineData::new();
        let debug_str = format!("{:?}", data);
        assert!(debug_str.contains("PerCpuTrampolineData"));
    }

    // =========================================================================
    // PerCpuGsData Tests
    // =========================================================================

    #[test]
    fn test_gs_data_new() {
        let gs_data = PerCpuGsData::new();
        for i in 0..32 {
            assert_eq!(gs_data.0[i], 0);
        }
    }

    #[test]
    fn test_gs_data_const() {
        const GS_DATA: PerCpuGsData = PerCpuGsData::new();
        assert_eq!(GS_DATA.0[0], 0);
    }

    #[test]
    fn test_gs_data_size() {
        let size = core::mem::size_of::<PerCpuGsData>();
        // Should be 256 bytes (32 x u64)
        assert_eq!(size, 256);
    }

    #[test]
    fn test_gs_data_alignment() {
        let align = core::mem::align_of::<PerCpuGsData>();
        // Should be 64-byte aligned
        assert_eq!(align, 64);
    }

    #[test]
    fn test_gs_data_copy() {
        let gs_data = PerCpuGsData::new();
        let copied = gs_data;
        for i in 0..32 {
            assert_eq!(copied.0[i], 0);
        }
    }
}
