//! Tests for boot/init.rs - Init system and runlevel management
//!
//! Tests the init system state machine, runlevels, and service management.

use core::mem;

#[cfg(test)]
mod tests {
    use crate::boot::init::{RunLevel, ServiceEntry, INIT_PID};
    use core::mem;

    // =========================================================================
    // INIT_PID Tests
    // =========================================================================

    #[test]
    fn test_init_pid_is_one() {
        // PID 1 is the init process in all Unix-like systems
        assert_eq!(INIT_PID, 1);
    }

    // =========================================================================
    // RunLevel Tests
    // =========================================================================

    #[test]
    fn test_runlevel_values() {
        // System V runlevel conventions
        assert_eq!(RunLevel::Halt as u8, 0);
        assert_eq!(RunLevel::SingleUser as u8, 1);
        assert_eq!(RunLevel::MultiUser as u8, 2);
        assert_eq!(RunLevel::MultiUserNetwork as u8, 3);
        assert_eq!(RunLevel::Unused as u8, 4);
        assert_eq!(RunLevel::MultiUserGUI as u8, 5);
        assert_eq!(RunLevel::Reboot as u8, 6);
    }

    #[test]
    fn test_runlevel_distinct() {
        let levels = [
            RunLevel::Halt,
            RunLevel::SingleUser,
            RunLevel::MultiUser,
            RunLevel::MultiUserNetwork,
            RunLevel::Unused,
            RunLevel::MultiUserGUI,
            RunLevel::Reboot,
        ];
        
        // Each runlevel should have unique value
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i] as u8, levels[j] as u8);
            }
        }
    }

    #[test]
    fn test_runlevel_size() {
        // RunLevel should be 1 byte (u8 repr)
        assert_eq!(mem::size_of::<RunLevel>(), 1);
    }

    #[test]
    fn test_runlevel_copy_clone() {
        let rl = RunLevel::MultiUser;
        let rl2 = rl;
        assert_eq!(rl, rl2);
    }

    #[test]
    fn test_runlevel_equality() {
        assert_eq!(RunLevel::Halt, RunLevel::Halt);
        assert_ne!(RunLevel::Halt, RunLevel::Reboot);
    }

    // =========================================================================
    // ServiceEntry Tests
    // =========================================================================

    #[test]
    fn test_service_entry_name_buffer_size() {
        // Name buffer should be at least 32 bytes
        let entry = ServiceEntry {
            name: [0; 32],
            name_len: 0,
            path: [0; 64],
            path_len: 0,
            pid: None,
            respawn: false,
            runlevels: 0,
            priority: 0,
            tty: None,
        };
        assert_eq!(entry.name.len(), 32);
    }

    #[test]
    fn test_service_entry_path_buffer_size() {
        let entry = ServiceEntry {
            name: [0; 32],
            name_len: 0,
            path: [0; 64],
            path_len: 0,
            pid: None,
            respawn: false,
            runlevels: 0,
            priority: 0,
            tty: None,
        };
        assert_eq!(entry.path.len(), 64);
    }

    #[test]
    fn test_service_entry_runlevel_bitmask() {
        // Runlevels are a bitmask: bit N = runlevel N
        let entry = ServiceEntry {
            name: [0; 32],
            name_len: 4,
            path: [0; 64],
            path_len: 10,
            pid: None,
            respawn: true,
            runlevels: 0b00111110, // Runlevels 1-5
            priority: 10,
            tty: Some(0),
        };
        
        // Test individual runlevel bits
        assert!(entry.runlevels & (1 << 1) != 0, "Runlevel 1 should be set");
        assert!(entry.runlevels & (1 << 2) != 0, "Runlevel 2 should be set");
        assert!(entry.runlevels & (1 << 3) != 0, "Runlevel 3 should be set");
        assert!(entry.runlevels & (1 << 4) != 0, "Runlevel 4 should be set");
        assert!(entry.runlevels & (1 << 5) != 0, "Runlevel 5 should be set");
        assert!(entry.runlevels & (1 << 0) == 0, "Runlevel 0 should not be set");
        assert!(entry.runlevels & (1 << 6) == 0, "Runlevel 6 should not be set");
    }

    #[test]
    fn test_service_entry_respawn_flag() {
        let entry = ServiceEntry {
            name: [0; 32],
            name_len: 0,
            path: [0; 64],
            path_len: 0,
            pid: None,
            respawn: true,
            runlevels: 0,
            priority: 0,
            tty: None,
        };
        assert!(entry.respawn);
    }

    #[test]
    fn test_service_entry_optional_pid() {
        let mut entry = ServiceEntry {
            name: [0; 32],
            name_len: 0,
            path: [0; 64],
            path_len: 0,
            pid: None,
            respawn: false,
            runlevels: 0,
            priority: 0,
            tty: None,
        };
        
        assert!(entry.pid.is_none());
        entry.pid = Some(1234);
        assert_eq!(entry.pid, Some(1234));
    }

    #[test]
    fn test_service_entry_optional_tty() {
        let entry = ServiceEntry {
            name: [0; 32],
            name_len: 0,
            path: [0; 64],
            path_len: 0,
            pid: None,
            respawn: false,
            runlevels: 0,
            priority: 0,
            tty: Some(1),
        };
        assert_eq!(entry.tty, Some(1));
    }

    #[test]
    fn test_service_entry_priority() {
        // Lower priority = starts earlier
        let high_priority = ServiceEntry {
            name: [0; 32],
            name_len: 0,
            path: [0; 64],
            path_len: 0,
            pid: None,
            respawn: false,
            runlevels: 0,
            priority: 1,
            tty: None,
        };
        
        let low_priority = ServiceEntry {
            name: [0; 32],
            name_len: 0,
            path: [0; 64],
            path_len: 0,
            pid: None,
            respawn: false,
            runlevels: 0,
            priority: 99,
            tty: None,
        };
        
        assert!(high_priority.priority < low_priority.priority);
    }

    // =========================================================================
    // inittab Runlevel Bitmask Tests
    // =========================================================================

    #[test]
    fn test_runlevel_bitmask_patterns() {
        // Common inittab patterns
        let all_multi_user: u8 = 0b00111110; // 1,2,3,4,5
        let network_only: u8 = 0b00001000;   // 3 only
        let gui_only: u8 = 0b00100000;       // 5 only
        
        // Check which runlevels are enabled
        fn has_runlevel(mask: u8, level: u8) -> bool {
            mask & (1 << level) != 0
        }
        
        assert!(has_runlevel(all_multi_user, 3));
        assert!(has_runlevel(network_only, 3));
        assert!(!has_runlevel(network_only, 2));
        assert!(has_runlevel(gui_only, 5));
        assert!(!has_runlevel(gui_only, 3));
    }
}
