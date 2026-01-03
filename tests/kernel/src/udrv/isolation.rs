//! Tests for udrv/isolation.rs - Isolation Classes
//!
//! Tests the differentiated isolation classes (IC0/IC1/IC2)
//! based on HongMeng microkernel design.

use core::mem;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::udrv::isolation::IsolationClass;

    // =========================================================================
    // IsolationClass Enum Tests
    // =========================================================================

    #[test]
    fn test_isolation_class_values() {
        // Three isolation classes as per HongMeng design
        assert_eq!(IsolationClass::IC0 as u8, 0);
        assert_eq!(IsolationClass::IC1 as u8, 1);
        assert_eq!(IsolationClass::IC2 as u8, 2);
    }

    #[test]
    fn test_isolation_class_size() {
        // Should be 1 byte for efficient storage
        assert_eq!(mem::size_of::<IsolationClass>(), 1);
    }

    #[test]
    fn test_isolation_class_ordering() {
        // IC0 < IC1 < IC2 (by isolation strength)
        assert!(IsolationClass::IC0 < IsolationClass::IC1);
        assert!(IsolationClass::IC1 < IsolationClass::IC2);
        assert!(IsolationClass::IC0 < IsolationClass::IC2);
    }

    // =========================================================================
    // Security Level Tests
    // =========================================================================

    #[test]
    fn test_security_levels() {
        // Higher class = higher security
        let ic0 = IsolationClass::IC0.security_level();
        let ic1 = IsolationClass::IC1.security_level();
        let ic2 = IsolationClass::IC2.security_level();
        
        assert!(ic0 < ic1, "IC1 should be more secure than IC0");
        assert!(ic1 < ic2, "IC2 should be more secure than IC1");
    }

    #[test]
    fn test_security_level_values() {
        assert_eq!(IsolationClass::IC0.security_level(), 0);
        assert_eq!(IsolationClass::IC1.security_level(), 1);
        assert_eq!(IsolationClass::IC2.security_level(), 2);
    }

    // =========================================================================
    // IPC Latency Tests
    // =========================================================================

    #[test]
    fn test_ipc_latency_ordering() {
        // Higher isolation = higher IPC cost
        let ic0_lat = IsolationClass::IC0.ipc_latency_cycles();
        let ic1_lat = IsolationClass::IC1.ipc_latency_cycles();
        let ic2_lat = IsolationClass::IC2.ipc_latency_cycles();
        
        assert!(ic0_lat < ic1_lat, "IC0 should have lower IPC latency than IC1");
        assert!(ic1_lat < ic2_lat, "IC1 should have lower IPC latency than IC2");
    }

    #[test]
    fn test_ipc_latency_values() {
        // As documented in the design
        assert_eq!(IsolationClass::IC0.ipc_latency_cycles(), 18);
        assert_eq!(IsolationClass::IC1.ipc_latency_cycles(), 500);
        assert_eq!(IsolationClass::IC2.ipc_latency_cycles(), 1000);
    }

    // =========================================================================
    // Access Control Tests
    // =========================================================================

    #[test]
    fn test_can_access_same_class() {
        // Same class can always access same class
        assert!(IsolationClass::IC0.can_access(IsolationClass::IC0));
        assert!(IsolationClass::IC1.can_access(IsolationClass::IC1));
        assert!(IsolationClass::IC2.can_access(IsolationClass::IC2));
    }

    #[test]
    fn test_can_access_higher_class() {
        // Lower can access higher (less privileged accessing more isolated)
        assert!(IsolationClass::IC0.can_access(IsolationClass::IC1));
        assert!(IsolationClass::IC0.can_access(IsolationClass::IC2));
        assert!(IsolationClass::IC1.can_access(IsolationClass::IC2));
    }

    #[test]
    fn test_cannot_access_lower_class() {
        // Higher cannot access lower (security principle)
        assert!(!IsolationClass::IC1.can_access(IsolationClass::IC0));
        assert!(!IsolationClass::IC2.can_access(IsolationClass::IC0));
        assert!(!IsolationClass::IC2.can_access(IsolationClass::IC1));
    }

    // =========================================================================
    // Clone/Copy/Eq Tests
    // =========================================================================

    #[test]
    fn test_isolation_class_copy() {
        let ic = IsolationClass::IC1;
        let ic2 = ic; // Copy
        assert_eq!(ic, ic2);
    }

    #[test]
    fn test_isolation_class_clone() {
        let ic = IsolationClass::IC2;
        let ic2 = ic.clone();
        assert_eq!(ic, ic2);
    }

    #[test]
    fn test_isolation_class_equality() {
        assert_eq!(IsolationClass::IC0, IsolationClass::IC0);
        assert_ne!(IsolationClass::IC0, IsolationClass::IC1);
        assert_ne!(IsolationClass::IC1, IsolationClass::IC2);
    }
}
