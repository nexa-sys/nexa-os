//! Kernel Module Taint Flag Tests
//!
//! Tests for Linux-compatible kernel taint flags.

#[cfg(test)]
mod tests {
    use crate::kmod::{TaintFlag};

    // =========================================================================
    // Taint Flag Value Tests
    // =========================================================================

    #[test]
    fn test_taint_flag_proprietary_module() {
        assert_eq!(TaintFlag::ProprietaryModule as u32, 1 << 0);
        assert_eq!(TaintFlag::ProprietaryModule as u32, 1);
    }

    #[test]
    fn test_taint_flag_forced_load() {
        assert_eq!(TaintFlag::ForcedLoad as u32, 1 << 1);
        assert_eq!(TaintFlag::ForcedLoad as u32, 2);
    }

    #[test]
    fn test_taint_flag_smp() {
        assert_eq!(TaintFlag::Smp as u32, 1 << 2);
        assert_eq!(TaintFlag::Smp as u32, 4);
    }

    #[test]
    fn test_taint_flag_forced_unload() {
        assert_eq!(TaintFlag::ForcedUnload as u32, 1 << 3);
        assert_eq!(TaintFlag::ForcedUnload as u32, 8);
    }

    #[test]
    fn test_taint_flag_machine_check() {
        assert_eq!(TaintFlag::MachineCheck as u32, 1 << 4);
        assert_eq!(TaintFlag::MachineCheck as u32, 16);
    }

    #[test]
    fn test_taint_flag_bad_page() {
        assert_eq!(TaintFlag::BadPage as u32, 1 << 5);
        assert_eq!(TaintFlag::BadPage as u32, 32);
    }

    #[test]
    fn test_taint_flag_user_request() {
        assert_eq!(TaintFlag::UserRequest as u32, 1 << 6);
        assert_eq!(TaintFlag::UserRequest as u32, 64);
    }

    #[test]
    fn test_taint_flag_die() {
        assert_eq!(TaintFlag::Die as u32, 1 << 7);
        assert_eq!(TaintFlag::Die as u32, 128);
    }

    #[test]
    fn test_taint_flag_unsigned_module() {
        assert_eq!(TaintFlag::UnsignedModule as u32, 1 << 14);
        assert_eq!(TaintFlag::UnsignedModule as u32, 16384);
    }

    #[test]
    fn test_taint_flag_out_of_tree() {
        assert_eq!(TaintFlag::OutOfTreeModule as u32, 1 << 15);
        assert_eq!(TaintFlag::OutOfTreeModule as u32, 32768);
    }

    #[test]
    fn test_taint_flag_staging_driver() {
        assert_eq!(TaintFlag::StagingDriver as u32, 1 << 16);
        assert_eq!(TaintFlag::StagingDriver as u32, 65536);
    }

    // =========================================================================
    // Taint Flag Character Tests
    // =========================================================================

    #[test]
    fn test_taint_flag_chars() {
        assert_eq!(TaintFlag::ProprietaryModule.as_char(), 'P');
        assert_eq!(TaintFlag::ForcedLoad.as_char(), 'F');
        assert_eq!(TaintFlag::Smp.as_char(), 'S');
        assert_eq!(TaintFlag::ForcedUnload.as_char(), 'R');
        assert_eq!(TaintFlag::MachineCheck.as_char(), 'M');
        assert_eq!(TaintFlag::BadPage.as_char(), 'B');
        assert_eq!(TaintFlag::UserRequest.as_char(), 'U');
        assert_eq!(TaintFlag::Die.as_char(), 'D');
        assert_eq!(TaintFlag::OverriddenAcpiTable.as_char(), 'A');
        assert_eq!(TaintFlag::Warn.as_char(), 'W');
        assert_eq!(TaintFlag::LivePatch.as_char(), 'K');
        assert_eq!(TaintFlag::UnsupportedHardware.as_char(), 'H');
        assert_eq!(TaintFlag::Softlockup.as_char(), 'L');
        assert_eq!(TaintFlag::FirmwareBug.as_char(), 'I');
        assert_eq!(TaintFlag::UnsignedModule.as_char(), 'E');
        assert_eq!(TaintFlag::OutOfTreeModule.as_char(), 'O');
        assert_eq!(TaintFlag::StagingDriver.as_char(), 'C');
        assert_eq!(TaintFlag::RandomizeTampered.as_char(), 'T');
        assert_eq!(TaintFlag::Aux.as_char(), 'X');
    }

    #[test]
    fn test_taint_chars_are_unique() {
        let chars = [
            TaintFlag::ProprietaryModule.as_char(),
            TaintFlag::ForcedLoad.as_char(),
            TaintFlag::Smp.as_char(),
            TaintFlag::ForcedUnload.as_char(),
            TaintFlag::MachineCheck.as_char(),
            TaintFlag::BadPage.as_char(),
            TaintFlag::UserRequest.as_char(),
            TaintFlag::Die.as_char(),
            TaintFlag::OverriddenAcpiTable.as_char(),
            TaintFlag::Warn.as_char(),
            TaintFlag::LivePatch.as_char(),
            TaintFlag::UnsupportedHardware.as_char(),
            TaintFlag::Softlockup.as_char(),
            TaintFlag::FirmwareBug.as_char(),
            TaintFlag::UnsignedModule.as_char(),
            TaintFlag::OutOfTreeModule.as_char(),
            TaintFlag::StagingDriver.as_char(),
            TaintFlag::RandomizeTampered.as_char(),
            TaintFlag::Aux.as_char(),
        ];
        
        // All characters should be unique
        for i in 0..chars.len() {
            for j in (i+1)..chars.len() {
                assert_ne!(chars[i], chars[j], "Taint chars {} and {} overlap", i, j);
            }
        }
    }

    // =========================================================================
    // Taint Flag Bit Tests
    // =========================================================================

    #[test]
    fn test_taint_flags_are_power_of_two() {
        let flags = [
            TaintFlag::ProprietaryModule as u32,
            TaintFlag::ForcedLoad as u32,
            TaintFlag::Smp as u32,
            TaintFlag::ForcedUnload as u32,
            TaintFlag::MachineCheck as u32,
            TaintFlag::BadPage as u32,
            TaintFlag::UserRequest as u32,
            TaintFlag::Die as u32,
            TaintFlag::OverriddenAcpiTable as u32,
            TaintFlag::Warn as u32,
            TaintFlag::LivePatch as u32,
            TaintFlag::UnsupportedHardware as u32,
            TaintFlag::Softlockup as u32,
            TaintFlag::FirmwareBug as u32,
            TaintFlag::UnsignedModule as u32,
            TaintFlag::OutOfTreeModule as u32,
            TaintFlag::StagingDriver as u32,
            TaintFlag::RandomizeTampered as u32,
            TaintFlag::Aux as u32,
        ];
        
        for flag in flags.iter() {
            assert!(flag.is_power_of_two(), "Flag {} is not power of two", flag);
        }
    }

    #[test]
    fn test_taint_flags_no_overlap() {
        let flags = [
            TaintFlag::ProprietaryModule as u32,
            TaintFlag::ForcedLoad as u32,
            TaintFlag::Smp as u32,
            TaintFlag::ForcedUnload as u32,
            TaintFlag::MachineCheck as u32,
            TaintFlag::BadPage as u32,
            TaintFlag::UserRequest as u32,
            TaintFlag::Die as u32,
            TaintFlag::OverriddenAcpiTable as u32,
            TaintFlag::Warn as u32,
            TaintFlag::LivePatch as u32,
            TaintFlag::UnsupportedHardware as u32,
            TaintFlag::Softlockup as u32,
            TaintFlag::FirmwareBug as u32,
            TaintFlag::UnsignedModule as u32,
            TaintFlag::OutOfTreeModule as u32,
            TaintFlag::StagingDriver as u32,
            TaintFlag::RandomizeTampered as u32,
            TaintFlag::Aux as u32,
        ];
        
        // No two flags should share any bits
        for i in 0..flags.len() {
            for j in (i+1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0, "Flags {} and {} overlap", i, j);
            }
        }
    }

    #[test]
    fn test_taint_flag_combination() {
        let combined = TaintFlag::ProprietaryModule as u32 | 
                       TaintFlag::UnsignedModule as u32 |
                       TaintFlag::OutOfTreeModule as u32;
        
        // Can check individual flags
        assert!(combined & (TaintFlag::ProprietaryModule as u32) != 0);
        assert!(combined & (TaintFlag::UnsignedModule as u32) != 0);
        assert!(combined & (TaintFlag::OutOfTreeModule as u32) != 0);
        assert!(combined & (TaintFlag::ForcedLoad as u32) == 0);
    }

    // =========================================================================
    // TaintFlag Derive Traits Tests
    // =========================================================================

    #[test]
    fn test_taint_flag_copy() {
        let flag = TaintFlag::ProprietaryModule;
        let copied = flag;
        assert_eq!(flag, copied);
    }

    #[test]
    fn test_taint_flag_clone() {
        let flag = TaintFlag::UnsignedModule;
        let cloned = flag.clone();
        assert_eq!(flag, cloned);
    }

    #[test]
    fn test_taint_flag_eq() {
        assert_eq!(TaintFlag::ProprietaryModule, TaintFlag::ProprietaryModule);
        assert_ne!(TaintFlag::ProprietaryModule, TaintFlag::ForcedLoad);
    }

    #[test]
    fn test_taint_flag_debug() {
        let flag = TaintFlag::ProprietaryModule;
        let debug_str = format!("{:?}", flag);
        assert!(debug_str.contains("ProprietaryModule"));
    }
}
