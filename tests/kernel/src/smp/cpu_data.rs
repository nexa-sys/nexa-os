//! SMP CpuData Tests

#[cfg(test)]
mod tests {
    use crate::smp::CpuData;
    use core::sync::atomic::Ordering;

    // =========================================================================
    // CpuData Thread Safety Tests (Simulated)
    // =========================================================================

    #[test]
    fn test_cpu_data_atomic_operations() {
        let cpu_data = CpuData::new(0, 0);
        
        // Test atomic fetch_add on various counters
        cpu_data.interrupts_handled.fetch_add(1, Ordering::SeqCst);
        cpu_data.interrupts_handled.fetch_add(1, Ordering::SeqCst);
        assert_eq!(cpu_data.interrupts_handled.load(Ordering::SeqCst), 2);
        
        cpu_data.syscalls_handled.fetch_add(100, Ordering::SeqCst);
        assert_eq!(cpu_data.syscalls_handled.load(Ordering::SeqCst), 100);
    }

    #[test]
    fn test_cpu_data_atomic_swap() {
        let cpu_data = CpuData::new(0, 0);
        
        // Test atomic swap on reschedule_pending
        cpu_data.reschedule_pending.store(true, Ordering::SeqCst);
        let was_pending = cpu_data.reschedule_pending.swap(false, Ordering::SeqCst);
        assert!(was_pending);
        assert!(!cpu_data.reschedule_pending.load(Ordering::SeqCst));
    }

    #[test]
    fn test_cpu_data_atomic_compare_exchange() {
        let cpu_data = CpuData::new(0, 0);
        
        // Test compare_exchange on current_pid
        cpu_data.current_pid.store(1, Ordering::SeqCst);
        
        let result = cpu_data.current_pid.compare_exchange(
            1, 2, Ordering::SeqCst, Ordering::SeqCst
        );
        assert!(result.is_ok());
        assert_eq!(cpu_data.current_pid.load(Ordering::SeqCst), 2);
        
        // Failed compare_exchange
        let result = cpu_data.current_pid.compare_exchange(
            1, 3, Ordering::SeqCst, Ordering::SeqCst
        );
        assert!(result.is_err());
        assert_eq!(cpu_data.current_pid.load(Ordering::SeqCst), 2);
    }

    // =========================================================================
    // CpuData Interrupt Context Tests
    // =========================================================================

    #[test]
    fn test_nested_interrupt_context() {
        let cpu_data = CpuData::new(0, 0);
        
        // Enter interrupt context multiple times (nested interrupts)
        cpu_data.enter_interrupt();
        cpu_data.enter_interrupt();
        
        assert!(cpu_data.in_interrupt_context());
        assert_eq!(cpu_data.preempt_count.load(Ordering::Relaxed), 2);
        
        // Leave once
        cpu_data.leave_interrupt();
        assert!(!cpu_data.in_interrupt_context()); // in_interrupt is bool, not nested
        assert_eq!(cpu_data.preempt_count.load(Ordering::Relaxed), 1);
        
        // Leave again
        cpu_data.leave_interrupt();
        assert_eq!(cpu_data.preempt_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_reschedule_cleared_on_leave() {
        let cpu_data = CpuData::new(0, 0);
        
        cpu_data.enter_interrupt();
        cpu_data.reschedule_pending.store(true, Ordering::SeqCst);
        
        let needs_resched = cpu_data.leave_interrupt();
        
        // leave_interrupt should atomically swap the flag
        assert!(needs_resched);
        assert!(!cpu_data.reschedule_pending.load(Ordering::SeqCst));
    }

    #[test]
    fn test_no_reschedule_when_not_pending() {
        let cpu_data = CpuData::new(0, 0);
        
        cpu_data.enter_interrupt();
        // Don't set reschedule_pending
        
        let needs_resched = cpu_data.leave_interrupt();
        assert!(!needs_resched);
    }

    // =========================================================================
    // CpuData Statistics Accumulation
    // =========================================================================

    #[test]
    fn test_statistics_accumulation() {
        let cpu_data = CpuData::new(0, 0);
        
        // Simulate many context switches
        for i in 0..100 {
            cpu_data.record_context_switch(i % 2 == 0);
        }
        
        assert_eq!(cpu_data.context_switches.load(Ordering::Relaxed), 100);
        assert_eq!(cpu_data.voluntary_switches.load(Ordering::Relaxed), 50);
        assert_eq!(cpu_data.preemptions.load(Ordering::Relaxed), 50);
    }

    #[test]
    fn test_idle_busy_time_accumulation() {
        let cpu_data = CpuData::new(0, 0);
        
        // Simulate time accumulation
        cpu_data.idle_time.fetch_add(1000, Ordering::Relaxed);
        cpu_data.busy_time.fetch_add(3000, Ordering::Relaxed);
        
        let idle = cpu_data.idle_time.load(Ordering::Relaxed);
        let busy = cpu_data.busy_time.load(Ordering::Relaxed);
        
        assert_eq!(idle, 1000);
        assert_eq!(busy, 3000);
        
        // Calculate utilization
        let total = idle + busy;
        let utilization = (busy * 100) / total;
        assert_eq!(utilization, 75);
    }

    #[test]
    fn test_ipi_tracking() {
        let cpu_data = CpuData::new(0, 0);
        
        // Simulate IPI activity
        cpu_data.ipi_sent.fetch_add(10, Ordering::Relaxed);
        cpu_data.ipi_received.fetch_add(5, Ordering::Relaxed);
        
        assert_eq!(cpu_data.ipi_sent.load(Ordering::Relaxed), 10);
        assert_eq!(cpu_data.ipi_received.load(Ordering::Relaxed), 5);
    }

    // =========================================================================
    // CpuData TSC and Timing Tests
    // =========================================================================

    #[test]
    fn test_last_tick_tsc_tracking() {
        let cpu_data = CpuData::new(0, 0);
        
        // Simulate TSC values (monotonically increasing)
        let tsc_values = [1000u64, 2000, 3000, 4000, 5000];
        
        for &tsc in &tsc_values {
            cpu_data.last_tick_tsc.store(tsc, Ordering::Relaxed);
        }
        
        assert_eq!(cpu_data.last_tick_tsc.load(Ordering::Relaxed), 5000);
    }

    #[test]
    fn test_local_tick_overflow() {
        let cpu_data = CpuData::new(0, 0);
        
        // Set to near max value
        cpu_data.local_tick.store(u64::MAX - 1, Ordering::Relaxed);
        
        // Increment should wrap
        cpu_data.local_tick.fetch_add(1, Ordering::Relaxed);
        assert_eq!(cpu_data.local_tick.load(Ordering::Relaxed), u64::MAX);
        
        cpu_data.local_tick.fetch_add(1, Ordering::Relaxed);
        assert_eq!(cpu_data.local_tick.load(Ordering::Relaxed), 0); // Wrapped
    }

    // =========================================================================
    // CpuData Current PID Tests
    // =========================================================================

    #[test]
    fn test_current_pid_tracking() {
        let cpu_data = CpuData::new(0, 0);
        
        // Initial PID is 0 (idle)
        assert_eq!(cpu_data.current_pid.load(Ordering::Relaxed), 0);
        
        // Switch to process 1
        cpu_data.current_pid.store(1, Ordering::Relaxed);
        assert_eq!(cpu_data.current_pid.load(Ordering::Relaxed), 1);
        
        // Switch to process 42
        cpu_data.current_pid.store(42, Ordering::Relaxed);
        assert_eq!(cpu_data.current_pid.load(Ordering::Relaxed), 42);
        
        // Back to idle
        cpu_data.current_pid.store(0, Ordering::Relaxed);
        assert_eq!(cpu_data.current_pid.load(Ordering::Relaxed), 0);
    }

    // =========================================================================
    // CpuData TLB Flush Tests
    // =========================================================================

    #[test]
    fn test_tlb_flush_pending() {
        let cpu_data = CpuData::new(0, 0);
        
        assert!(!cpu_data.tlb_flush_pending.load(Ordering::Relaxed));
        
        cpu_data.tlb_flush_pending.store(true, Ordering::Relaxed);
        assert!(cpu_data.tlb_flush_pending.load(Ordering::Relaxed));
        
        // Clear after handling
        cpu_data.tlb_flush_pending.store(false, Ordering::Relaxed);
        assert!(!cpu_data.tlb_flush_pending.load(Ordering::Relaxed));
    }
}
