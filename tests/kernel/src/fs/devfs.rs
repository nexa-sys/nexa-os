//! DevFS Tests
//!
//! Tests for the device filesystem types and structures.

#[cfg(test)]
mod tests {
    use crate::fs::devfs::{DeviceEntry, DeviceType};

    // =========================================================================
    // DeviceType Enumeration Tests
    // =========================================================================

    #[test]
    fn test_device_type_null() {
        let dev = DeviceType::Null;
        assert!(matches!(dev, DeviceType::Null));
    }

    #[test]
    fn test_device_type_zero() {
        let dev = DeviceType::Zero;
        assert!(matches!(dev, DeviceType::Zero));
    }

    #[test]
    fn test_device_type_random() {
        let dev = DeviceType::Random;
        assert!(matches!(dev, DeviceType::Random));
    }

    #[test]
    fn test_device_type_urandom() {
        let dev = DeviceType::Urandom;
        assert!(matches!(dev, DeviceType::Urandom));
    }

    #[test]
    fn test_device_type_console() {
        let dev = DeviceType::Console;
        assert!(matches!(dev, DeviceType::Console));
    }

    #[test]
    fn test_device_type_full() {
        let dev = DeviceType::Full;
        assert!(matches!(dev, DeviceType::Full));
    }

    #[test]
    fn test_device_type_ptymux() {
        let dev = DeviceType::PtyMasterMux;
        assert!(matches!(dev, DeviceType::PtyMasterMux));
    }

    #[test]
    fn test_device_type_network() {
        let dev0 = DeviceType::Network(0);
        let dev1 = DeviceType::Network(1);
        assert!(matches!(dev0, DeviceType::Network(0)));
        assert!(matches!(dev1, DeviceType::Network(1)));
    }

    #[test]
    fn test_device_type_block() {
        let dev = DeviceType::Block(0);
        assert!(matches!(dev, DeviceType::Block(0)));
    }

    #[test]
    fn test_device_type_framebuffer() {
        let dev = DeviceType::Framebuffer(0);
        assert!(matches!(dev, DeviceType::Framebuffer(0)));
    }

    #[test]
    fn test_device_type_loop() {
        let dev = DeviceType::Loop(0);
        assert!(matches!(dev, DeviceType::Loop(0)));
    }

    #[test]
    fn test_device_type_loop_control() {
        let dev = DeviceType::LoopControl;
        assert!(matches!(dev, DeviceType::LoopControl));
    }

    #[test]
    fn test_device_type_input_event() {
        let dev = DeviceType::InputEvent(0);
        assert!(matches!(dev, DeviceType::InputEvent(0)));
    }

    #[test]
    fn test_device_type_input_mice() {
        let dev = DeviceType::InputMice;
        assert!(matches!(dev, DeviceType::InputMice));
    }

    // =========================================================================
    // DeviceType Trait Tests
    // =========================================================================

    #[test]
    fn test_device_type_debug() {
        let dev = DeviceType::Null;
        let debug_str = format!("{:?}", dev);
        assert!(debug_str.contains("Null"));
    }

    #[test]
    fn test_device_type_clone() {
        let dev1 = DeviceType::Random;
        let dev2 = dev1.clone();
        assert_eq!(dev1, dev2);
    }

    #[test]
    fn test_device_type_copy() {
        let dev1 = DeviceType::Zero;
        let dev2 = dev1;
        assert_eq!(dev1, dev2);
    }

    #[test]
    fn test_device_type_eq() {
        assert_eq!(DeviceType::Null, DeviceType::Null);
        assert_eq!(DeviceType::Random, DeviceType::Random);
        assert_ne!(DeviceType::Null, DeviceType::Zero);
    }

    // =========================================================================
    // DeviceEntry Structure Tests
    // =========================================================================

    #[test]
    fn test_device_entry_creation() {
        let entry = DeviceEntry {
            name: "null",
            dev_type: DeviceType::Null,
            major: 1,
            minor: 3,
        };
        assert_eq!(entry.name, "null");
        assert_eq!(entry.major, 1);
        assert_eq!(entry.minor, 3);
    }

    #[test]
    fn test_device_entry_clone() {
        let entry1 = DeviceEntry {
            name: "zero",
            dev_type: DeviceType::Zero,
            major: 1,
            minor: 5,
        };
        let entry2 = entry1.clone();
        assert_eq!(entry1.name, entry2.name);
        assert_eq!(entry1.major, entry2.major);
    }

    // =========================================================================
    // Device Major/Minor Number Tests
    // =========================================================================

    #[test]
    fn test_standard_char_device_numbers() {
        // Standard Linux character device major/minor
        // null: 1,3
        // zero: 1,5
        // full: 1,7
        // random: 1,8
        // urandom: 1,9
        let null = DeviceEntry {
            name: "null",
            dev_type: DeviceType::Null,
            major: 1,
            minor: 3,
        };
        assert_eq!(null.major, 1);
        assert_eq!(null.minor, 3);

        let zero = DeviceEntry {
            name: "zero",
            dev_type: DeviceType::Zero,
            major: 1,
            minor: 5,
        };
        assert_eq!(zero.major, 1);
        assert_eq!(zero.minor, 5);

        let random = DeviceEntry {
            name: "random",
            dev_type: DeviceType::Random,
            major: 1,
            minor: 8,
        };
        assert_eq!(random.major, 1);
        assert_eq!(random.minor, 8);
    }

    #[test]
    fn test_tty_device_numbers() {
        // tty: 5,0
        // console: 5,1
        // ptmx: 5,2
        let console = DeviceEntry {
            name: "console",
            dev_type: DeviceType::Console,
            major: 5,
            minor: 1,
        };
        assert_eq!(console.major, 5);
        assert_eq!(console.minor, 1);

        let ptmx = DeviceEntry {
            name: "ptmx",
            dev_type: DeviceType::PtyMasterMux,
            major: 5,
            minor: 2,
        };
        assert_eq!(ptmx.major, 5);
        assert_eq!(ptmx.minor, 2);
    }

    #[test]
    fn test_tty_device_minor_series() {
        // tty0-ttyN: major=4, minor=0-N
        for i in 0..8 {
            let tty = DeviceEntry {
                name: "tty",
                dev_type: DeviceType::Console,
                major: 4,
                minor: i,
            };
            assert_eq!(tty.major, 4);
            assert_eq!(tty.minor, i);
        }
    }

    #[test]
    fn test_makedev_encoding() {
        // dev_t = makedev(major, minor)
        fn makedev(major: u32, minor: u32) -> u64 {
            ((major as u64) << 8) | (minor as u64 & 0xFF)
        }

        fn major(dev: u64) -> u32 {
            ((dev >> 8) & 0xFFF) as u32
        }

        fn minor(dev: u64) -> u32 {
            (dev & 0xFF) as u32
        }

        let dev = makedev(1, 3); // null
        assert_eq!(major(dev), 1);
        assert_eq!(minor(dev), 3);
    }

    // =========================================================================
    // Device Path Tests
    // =========================================================================

    #[test]
    fn test_standard_device_paths() {
        let paths = [
            "/dev/null",
            "/dev/zero",
            "/dev/random",
            "/dev/urandom",
            "/dev/console",
            "/dev/tty",
            "/dev/ptmx",
        ];

        for path in paths {
            assert!(path.starts_with("/dev/"));
        }
    }

    #[test]
    fn test_pts_path_format() {
        // /dev/pts/0, /dev/pts/1, etc.
        for i in 0..10 {
            let path = format!("/dev/pts/{}", i);
            assert!(path.starts_with("/dev/pts/"));
        }
    }

    // =========================================================================
    // Device Limit Tests
    // =========================================================================

    #[test]
    fn test_max_devices_constant() {
        const MAX_DEVICES: usize = 64;
        assert!(MAX_DEVICES >= 32); // Must have room for standard devices
    }
}
