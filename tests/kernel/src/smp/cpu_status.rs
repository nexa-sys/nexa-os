//! SMP CpuStatus Tests

#[cfg(test)]
mod tests {
    use crate::smp::CpuStatus;
    use core::sync::atomic::{AtomicU8, Ordering};

    // =========================================================================
    // CpuStatus Value Tests
    // =========================================================================

    #[test]
    fn test_status_offline() {
        let status = CpuStatus::Offline;
        assert_eq!(status as u8, 0);
    }

    #[test]
    fn test_status_booting() {
        let status = CpuStatus::Booting;
        assert_eq!(status as u8, 1);
    }

    #[test]
    fn test_status_online() {
        let status = CpuStatus::Online;
        assert_eq!(status as u8, 2);
    }

    // =========================================================================
    // CpuStatus State Machine Tests
    // =========================================================================

    #[test]
    fn test_valid_transitions() {
        // Offline -> Booting
        let status = AtomicU8::new(CpuStatus::Offline as u8);
        status.store(CpuStatus::Booting as u8, Ordering::SeqCst);
        assert_eq!(CpuStatus::from_atomic(status.load(Ordering::SeqCst)), CpuStatus::Booting);
        
        // Booting -> Online
        status.store(CpuStatus::Online as u8, Ordering::SeqCst);
        assert_eq!(CpuStatus::from_atomic(status.load(Ordering::SeqCst)), CpuStatus::Online);
    }

    #[test]
    fn test_offline_to_online_transition() {
        // Online -> Offline (hotplug)
        let status = AtomicU8::new(CpuStatus::Online as u8);
        status.store(CpuStatus::Offline as u8, Ordering::SeqCst);
        assert_eq!(CpuStatus::from_atomic(status.load(Ordering::SeqCst)), CpuStatus::Offline);
    }

    #[test]
    fn test_booting_failure() {
        // Booting -> Offline (boot failed)
        let status = AtomicU8::new(CpuStatus::Booting as u8);
        status.store(CpuStatus::Offline as u8, Ordering::SeqCst);
        assert_eq!(CpuStatus::from_atomic(status.load(Ordering::SeqCst)), CpuStatus::Offline);
    }

    // =========================================================================
    // CpuStatus Atomic Operations
    // =========================================================================

    #[test]
    fn test_atomic_status_compare_exchange() {
        let status = AtomicU8::new(CpuStatus::Offline as u8);
        
        // Try to transition from Offline to Booting
        let result = status.compare_exchange(
            CpuStatus::Offline as u8,
            CpuStatus::Booting as u8,
            Ordering::SeqCst,
            Ordering::SeqCst
        );
        
        assert!(result.is_ok());
        assert_eq!(CpuStatus::from_atomic(status.load(Ordering::SeqCst)), CpuStatus::Booting);
    }

    #[test]
    fn test_atomic_status_compare_exchange_failure() {
        let status = AtomicU8::new(CpuStatus::Online as u8);
        
        // Try to transition from Offline to Booting (should fail - already Online)
        let result = status.compare_exchange(
            CpuStatus::Offline as u8,
            CpuStatus::Booting as u8,
            Ordering::SeqCst,
            Ordering::SeqCst
        );
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), CpuStatus::Online as u8);
    }

    // =========================================================================
    // CpuStatus from_atomic Edge Cases
    // =========================================================================

    #[test]
    fn test_from_atomic_all_invalid_values() {
        // Test all invalid values (3-255)
        for i in 3u8..=255 {
            assert_eq!(CpuStatus::from_atomic(i), CpuStatus::Offline,
                "Invalid value {} should map to Offline", i);
        }
    }

    #[test]
    fn test_from_atomic_boundary_values() {
        // Just below valid
        assert_eq!(CpuStatus::from_atomic(0), CpuStatus::Offline);
        // Valid range
        assert_eq!(CpuStatus::from_atomic(1), CpuStatus::Booting);
        assert_eq!(CpuStatus::from_atomic(2), CpuStatus::Online);
        // Just above valid
        assert_eq!(CpuStatus::from_atomic(3), CpuStatus::Offline);
    }
}
