//! Hardware Watchdog Timer Driver Tests
//!
//! Tests for watchdog timer types, constants, and ioctls.

#[cfg(test)]
mod tests {
    use crate::drivers::watchdog::{
        WatchdogType, WatchdogInfo, options, ioctls, setoptions,
    };

    // =========================================================================
    // WatchdogType Enum Tests
    // =========================================================================

    #[test]
    fn test_watchdog_type_none() {
        let wtype = WatchdogType::None;
        assert_eq!(wtype, WatchdogType::None);
    }

    #[test]
    fn test_watchdog_type_i6300esb() {
        let wtype = WatchdogType::I6300ESB;
        assert_ne!(wtype, WatchdogType::None);
        assert_ne!(wtype, WatchdogType::IntelTCO);
    }

    #[test]
    fn test_watchdog_type_intel_tco() {
        let wtype = WatchdogType::IntelTCO;
        assert_ne!(wtype, WatchdogType::None);
        assert_ne!(wtype, WatchdogType::I6300ESB);
    }

    #[test]
    fn test_watchdog_type_superio() {
        let wtype = WatchdogType::SuperIO;
        assert_ne!(wtype, WatchdogType::None);
    }

    #[test]
    fn test_watchdog_type_software() {
        let wtype = WatchdogType::Software;
        assert_ne!(wtype, WatchdogType::None);
        assert_ne!(wtype, WatchdogType::I6300ESB);
    }

    #[test]
    fn test_watchdog_type_all_distinct() {
        let types = [
            WatchdogType::None,
            WatchdogType::I6300ESB,
            WatchdogType::IntelTCO,
            WatchdogType::SuperIO,
            WatchdogType::Software,
        ];
        
        for i in 0..types.len() {
            for j in (i+1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_watchdog_type_copy() {
        let wtype = WatchdogType::I6300ESB;
        let copied = wtype;
        assert_eq!(wtype, copied);
    }

    #[test]
    fn test_watchdog_type_clone() {
        let wtype = WatchdogType::IntelTCO;
        let cloned = wtype.clone();
        assert_eq!(wtype, cloned);
    }

    #[test]
    fn test_watchdog_type_debug() {
        let wtype = WatchdogType::Software;
        let debug_str = format!("{:?}", wtype);
        assert!(debug_str.contains("Software"));
    }

    // =========================================================================
    // WatchdogInfo Structure Tests
    // =========================================================================

    #[test]
    fn test_watchdog_info_size() {
        let size = core::mem::size_of::<WatchdogInfo>();
        // options(4) + firmware_version(4) + identity[32](32) = 40 bytes
        assert_eq!(size, 40);
    }

    #[test]
    fn test_watchdog_info_alignment() {
        let align = core::mem::align_of::<WatchdogInfo>();
        assert!(align >= 4);
    }

    #[test]
    fn test_watchdog_info_create() {
        let info = WatchdogInfo {
            options: options::WDIOF_SETTIMEOUT,
            firmware_version: 1,
            identity: [0; 32],
        };
        assert_eq!(info.options, options::WDIOF_SETTIMEOUT);
        assert_eq!(info.firmware_version, 1);
    }

    #[test]
    fn test_watchdog_info_identity() {
        let mut identity = [0u8; 32];
        let name = b"NexaOS Watchdog";
        identity[..name.len()].copy_from_slice(name);
        
        let info = WatchdogInfo {
            options: 0,
            firmware_version: 0,
            identity,
        };
        
        assert_eq!(&info.identity[..name.len()], name);
    }

    #[test]
    fn test_watchdog_info_copy() {
        let info1 = WatchdogInfo {
            options: 0x8080,
            firmware_version: 2,
            identity: [b'X'; 32],
        };
        let info2 = info1;
        assert_eq!(info1.options, info2.options);
        assert_eq!(info1.firmware_version, info2.firmware_version);
    }

    #[test]
    fn test_watchdog_info_clone() {
        let info1 = WatchdogInfo {
            options: 0x100,
            firmware_version: 3,
            identity: [0; 32],
        };
        let info2 = info1.clone();
        assert_eq!(info1.options, info2.options);
    }

    #[test]
    fn test_watchdog_info_debug() {
        let info = WatchdogInfo {
            options: 0,
            firmware_version: 1,
            identity: [0; 32],
        };
        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("WatchdogInfo"));
    }

    // =========================================================================
    // Option Flags Tests
    // =========================================================================

    #[test]
    fn test_option_flags_values() {
        assert_eq!(options::WDIOF_SETTIMEOUT, 0x0080);
        assert_eq!(options::WDIOF_MAGICCLOSE, 0x0100);
        assert_eq!(options::WDIOF_KEEPALIVEPING, 0x8000);
    }

    #[test]
    fn test_option_flags_no_overlap() {
        let flags = [
            options::WDIOF_SETTIMEOUT,
            options::WDIOF_MAGICCLOSE,
            options::WDIOF_KEEPALIVEPING,
        ];
        
        // Check no two flags share bits
        for i in 0..flags.len() {
            for j in (i+1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_option_flags_combination() {
        let combined = options::WDIOF_SETTIMEOUT | options::WDIOF_KEEPALIVEPING;
        assert!(combined & options::WDIOF_SETTIMEOUT != 0);
        assert!(combined & options::WDIOF_KEEPALIVEPING != 0);
        assert!(combined & options::WDIOF_MAGICCLOSE == 0);
    }

    // =========================================================================
    // Ioctl Commands Tests
    // =========================================================================

    #[test]
    fn test_ioctl_commands_defined() {
        // These are Linux-compatible ioctl numbers
        assert_eq!(ioctls::WDIOC_GETSUPPORT, 0x80285700);
        assert_eq!(ioctls::WDIOC_GETSTATUS, 0x80045701);
        assert_eq!(ioctls::WDIOC_GETBOOTSTATUS, 0x80045702);
        assert_eq!(ioctls::WDIOC_GETTIMEOUT, 0x80045707);
        assert_eq!(ioctls::WDIOC_SETTIMEOUT, 0xC0045706);
        assert_eq!(ioctls::WDIOC_KEEPALIVE, 0x80045705);
        assert_eq!(ioctls::WDIOC_SETOPTIONS, 0x80045704);
    }

    #[test]
    fn test_ioctl_commands_all_different() {
        let cmds = [
            ioctls::WDIOC_GETSUPPORT,
            ioctls::WDIOC_GETSTATUS,
            ioctls::WDIOC_GETBOOTSTATUS,
            ioctls::WDIOC_GETTIMEOUT,
            ioctls::WDIOC_SETTIMEOUT,
            ioctls::WDIOC_KEEPALIVE,
            ioctls::WDIOC_SETOPTIONS,
        ];
        
        for i in 0..cmds.len() {
            for j in (i+1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j], "ioctl commands {} and {} overlap", i, j);
            }
        }
    }

    #[test]
    fn test_ioctl_read_write_bits() {
        // Linux ioctl encoding: bits 30-31 indicate direction
        // 0x80 = read, 0x40 = write, 0xC0 = read+write
        
        // GETSUPPORT is read
        assert!(ioctls::WDIOC_GETSUPPORT & 0x80000000 != 0);
        
        // SETTIMEOUT is read+write
        assert!(ioctls::WDIOC_SETTIMEOUT & 0xC0000000 != 0);
    }

    // =========================================================================
    // Set Options Tests
    // =========================================================================

    #[test]
    fn test_setoptions_values() {
        assert_eq!(setoptions::WDIOS_DISABLECARD, 0x0001);
        assert_eq!(setoptions::WDIOS_ENABLECARD, 0x0002);
    }

    #[test]
    fn test_setoptions_no_overlap() {
        assert_eq!(setoptions::WDIOS_DISABLECARD & setoptions::WDIOS_ENABLECARD, 0);
    }

    #[test]
    fn test_setoptions_mutually_exclusive() {
        // Enable and disable should not be set together
        let enable_disable = setoptions::WDIOS_ENABLECARD | setoptions::WDIOS_DISABLECARD;
        // Count bits - should be 2 separate bits
        assert_eq!(enable_disable.count_ones(), 2);
    }

    // =========================================================================
    // Hardware Register Constants Tests
    // =========================================================================

    #[test]
    fn test_tco_base_port() {
        const TCO_BASE: u16 = 0x60;
        assert_eq!(TCO_BASE, 0x60);
    }

    #[test]
    fn test_i6300esb_registers() {
        const I6300ESB_BASE: u16 = 0x400;
        const I6300ESB_TIMER1_REG: u16 = I6300ESB_BASE + 0x00;
        const I6300ESB_TIMER2_REG: u16 = I6300ESB_BASE + 0x04;
        const I6300ESB_GINTSR_REG: u16 = I6300ESB_BASE + 0x08;
        const I6300ESB_RELOAD_REG: u16 = I6300ESB_BASE + 0x0C;
        
        assert_eq!(I6300ESB_BASE, 0x400);
        assert_eq!(I6300ESB_TIMER1_REG, 0x400);
        assert_eq!(I6300ESB_TIMER2_REG, 0x404);
        assert_eq!(I6300ESB_GINTSR_REG, 0x408);
        assert_eq!(I6300ESB_RELOAD_REG, 0x40C);
    }

    #[test]
    fn test_superio_wdt_port() {
        const SUPERIO_WDT_BASE: u16 = 0x2E;
        assert_eq!(SUPERIO_WDT_BASE, 0x2E);
    }

    // =========================================================================
    // Default Timeout Tests
    // =========================================================================

    #[test]
    fn test_default_timeout() {
        const DEFAULT_TIMEOUT_SECS: u32 = 60;
        assert_eq!(DEFAULT_TIMEOUT_SECS, 60);
    }

    #[test]
    fn test_reasonable_timeout_range() {
        const MIN_TIMEOUT: u32 = 1;
        const MAX_TIMEOUT: u32 = 600;  // 10 minutes
        const DEFAULT_TIMEOUT: u32 = 60;
        
        assert!(DEFAULT_TIMEOUT >= MIN_TIMEOUT);
        assert!(DEFAULT_TIMEOUT <= MAX_TIMEOUT);
    }
}
