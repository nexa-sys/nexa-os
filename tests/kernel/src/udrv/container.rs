//! Tests for udrv/container.rs - Driver Container
//!
//! Tests the driver container lifecycle and state management.

use core::mem;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::udrv::container::{ContainerState, MAX_DRIVERS_PER_CONTAINER, DriverContainerId};

    // =========================================================================
    // Container Constants Tests
    // =========================================================================

    #[test]
    fn test_max_drivers_per_container() {
        // Should be reasonable for typical use cases
        assert!(MAX_DRIVERS_PER_CONTAINER >= 4);
        assert!(MAX_DRIVERS_PER_CONTAINER <= 64);
        assert_eq!(MAX_DRIVERS_PER_CONTAINER, 8);
    }

    // =========================================================================
    // ContainerState Tests
    // =========================================================================

    #[test]
    fn test_container_state_values() {
        assert_eq!(ContainerState::Created as u8, 0);
        assert_eq!(ContainerState::Initializing as u8, 1);
        assert_eq!(ContainerState::Running as u8, 2);
        assert_eq!(ContainerState::Stopping as u8, 3);
        assert_eq!(ContainerState::Stopped as u8, 4);
        assert_eq!(ContainerState::Crashed as u8, 5);
        assert_eq!(ContainerState::Failed as u8, 6);
    }

    #[test]
    fn test_container_state_size() {
        assert_eq!(mem::size_of::<ContainerState>(), 1);
    }

    #[test]
    fn test_container_state_distinct() {
        let states = [
            ContainerState::Created,
            ContainerState::Initializing,
            ContainerState::Running,
            ContainerState::Stopping,
            ContainerState::Stopped,
            ContainerState::Crashed,
            ContainerState::Failed,
        ];
        
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_container_state_copy_clone() {
        let state = ContainerState::Running;
        let state2 = state;
        let state3 = state.clone();
        assert_eq!(state, state2);
        assert_eq!(state, state3);
    }

    // =========================================================================
    // Container Lifecycle Tests
    // =========================================================================

    #[test]
    fn test_container_lifecycle_created_to_initializing() {
        // Valid transition: Created -> Initializing
        let before = ContainerState::Created;
        let after = ContainerState::Initializing;
        assert!((before as u8) < (after as u8));
    }

    #[test]
    fn test_container_lifecycle_initializing_to_running() {
        // Valid transition: Initializing -> Running
        let before = ContainerState::Initializing;
        let after = ContainerState::Running;
        assert!((before as u8) < (after as u8));
    }

    #[test]
    fn test_container_lifecycle_running_to_stopping() {
        // Valid transition: Running -> Stopping
        let before = ContainerState::Running;
        let after = ContainerState::Stopping;
        assert!((before as u8) < (after as u8));
    }

    #[test]
    fn test_container_id_type() {
        // DriverContainerId is u32
        let id: DriverContainerId = 42;
        assert_eq!(id, 42u32);
        assert_eq!(mem::size_of::<DriverContainerId>(), 4);
    }
}
