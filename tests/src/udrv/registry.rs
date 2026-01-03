//! Tests for udrv/registry.rs - Driver Registry
//!
//! Tests the driver registration and metadata management.

use core::mem;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::udrv::registry::{DriverClass, DriverVersion, DriverId, MAX_DRIVERS};

    // =========================================================================
    // Registry Constants Tests
    // =========================================================================

    #[test]
    fn test_max_drivers() {
        // Should match MAX_UDRV_DRIVERS (64)
        assert_eq!(MAX_DRIVERS, 64);
    }

    // =========================================================================
    // DriverId Tests
    // =========================================================================

    #[test]
    fn test_driver_id_type() {
        let id: DriverId = 123;
        assert_eq!(id, 123u32);
        assert_eq!(mem::size_of::<DriverId>(), 4);
    }

    // =========================================================================
    // DriverVersion Tests
    // =========================================================================

    #[test]
    fn test_driver_version_new() {
        let ver = DriverVersion::new(1, 2, 3);
        assert_eq!(ver.major, 1);
        assert_eq!(ver.minor, 2);
        assert_eq!(ver.patch, 3);
    }

    #[test]
    fn test_driver_version_default() {
        let ver = DriverVersion::default();
        assert_eq!(ver.major, 0);
        assert_eq!(ver.minor, 0);
        assert_eq!(ver.patch, 0);
    }

    #[test]
    fn test_driver_version_size() {
        // Should be 3 bytes for major.minor.patch
        assert_eq!(mem::size_of::<DriverVersion>(), 3);
    }

    #[test]
    fn test_driver_version_copy() {
        let ver = DriverVersion::new(2, 5, 10);
        let ver2 = ver;
        assert_eq!(ver.major, ver2.major);
        assert_eq!(ver.minor, ver2.minor);
        assert_eq!(ver.patch, ver2.patch);
    }

    // =========================================================================
    // DriverClass Tests
    // =========================================================================

    #[test]
    fn test_driver_class_values() {
        assert_eq!(DriverClass::Network as u8, 0);
        assert_eq!(DriverClass::Block as u8, 1);
        assert_eq!(DriverClass::Char as u8, 2);
        assert_eq!(DriverClass::Input as u8, 3);
        assert_eq!(DriverClass::Display as u8, 4);
        assert_eq!(DriverClass::Audio as u8, 5);
        assert_eq!(DriverClass::Usb as u8, 6);
        assert_eq!(DriverClass::Pci as u8, 7);
    }

    #[test]
    fn test_driver_class_size() {
        assert_eq!(mem::size_of::<DriverClass>(), 1);
    }

    #[test]
    fn test_driver_class_distinct() {
        let classes = [
            DriverClass::Network,
            DriverClass::Block,
            DriverClass::Char,
            DriverClass::Input,
            DriverClass::Display,
            DriverClass::Audio,
            DriverClass::Usb,
            DriverClass::Pci,
        ];
        
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_driver_class_copy_clone() {
        let class = DriverClass::Network;
        let class2 = class;
        let class3 = class.clone();
        assert_eq!(class, class2);
        assert_eq!(class, class3);
    }

    // =========================================================================
    // Driver Class Use Case Tests
    // =========================================================================

    #[test]
    fn test_common_driver_classes_exist() {
        // Ensure common driver types are supported
        let _net = DriverClass::Network;
        let _blk = DriverClass::Block;
        let _chr = DriverClass::Char;
        let _inp = DriverClass::Input;
    }
}
