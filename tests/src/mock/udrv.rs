//! Mock User-space Driver Framework (UDRV) for testing
//!
//! This module provides a mock implementation of the UDRV framework
//! that can be tested in a standard Rust environment without kernel dependencies.

use std::collections::HashMap;
use std::sync::Mutex;

/// Maximum number of drivers in mock registry
pub const MAX_MOCK_DRIVERS: usize = 64;

/// Maximum number of containers in mock framework
pub const MAX_MOCK_CONTAINERS: usize = 16;

/// Driver ID type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MockDriverId(pub u32);

/// Container ID type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MockContainerId(pub u32);

/// Driver class enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockDriverClass {
    Network,
    Block,
    Character,
    Graphics,
    Input,
    Other,
}

/// Isolation class for drivers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockIsolationClass {
    /// IC0: No isolation (kernel-internal only)
    IC0,
    /// IC1: Mechanism-enforced isolation in kernel space
    IC1,
    /// IC2: Full address space isolation in userspace
    IC2,
}

/// Driver information
#[derive(Debug, Clone)]
pub struct MockDriverInfo {
    pub name: String,
    pub class: MockDriverClass,
    pub isolation: MockIsolationClass,
    pub version: (u8, u8, u8),
}

/// Container state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockContainerState {
    Created,
    Running,
    Paused,
    Stopped,
}

/// Driver container
#[derive(Debug)]
pub struct MockDriverContainer {
    pub id: MockContainerId,
    pub isolation: MockIsolationClass,
    pub state: MockContainerState,
    pub drivers: Vec<MockDriverId>,
}

/// Registry error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockRegistryError {
    NotInitialized,
    TableFull,
    NotFound,
    AlreadyExists,
    InvalidState,
}

/// Container error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockContainerError {
    NotInitialized,
    TableFull,
    NotFound,
    InvalidState,
    DriverNotFound,
}

/// Mock UDRV Framework
pub struct MockUdrvFramework {
    initialized: bool,
    drivers: HashMap<MockDriverId, MockDriverInfo>,
    containers: HashMap<MockContainerId, MockDriverContainer>,
    next_driver_id: u32,
    next_container_id: u32,
}

impl MockUdrvFramework {
    /// Create a new uninitialized framework
    pub fn new() -> Self {
        Self {
            initialized: false,
            drivers: HashMap::new(),
            containers: HashMap::new(),
            next_driver_id: 1,
            next_container_id: 1,
        }
    }

    /// Initialize the framework
    pub fn init(&mut self) {
        self.initialized = true;
        self.drivers.clear();
        self.containers.clear();
        self.next_driver_id = 1;
        self.next_container_id = 1;
    }

    /// Check if framework is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Register a new driver
    pub fn register_driver(&mut self, info: MockDriverInfo) -> Result<MockDriverId, MockRegistryError> {
        if !self.initialized {
            return Err(MockRegistryError::NotInitialized);
        }
        
        if self.drivers.len() >= MAX_MOCK_DRIVERS {
            return Err(MockRegistryError::TableFull);
        }

        // Check for duplicate names
        for existing in self.drivers.values() {
            if existing.name == info.name {
                return Err(MockRegistryError::AlreadyExists);
            }
        }

        let id = MockDriverId(self.next_driver_id);
        self.next_driver_id += 1;
        self.drivers.insert(id, info);
        
        Ok(id)
    }

    /// Unregister a driver
    pub fn unregister_driver(&mut self, id: MockDriverId) -> Result<(), MockRegistryError> {
        if !self.initialized {
            return Err(MockRegistryError::NotInitialized);
        }

        // Check if driver is in any container
        for container in self.containers.values() {
            if container.drivers.contains(&id) {
                return Err(MockRegistryError::InvalidState);
            }
        }

        self.drivers.remove(&id).ok_or(MockRegistryError::NotFound)?;
        Ok(())
    }

    /// Get driver information
    pub fn get_driver_info(&self, id: MockDriverId) -> Option<MockDriverInfo> {
        self.drivers.get(&id).cloned()
    }

    /// List all registered drivers
    pub fn list_drivers(&self) -> Vec<MockDriverId> {
        self.drivers.keys().cloned().collect()
    }

    /// Create a driver container
    pub fn create_container(&mut self, isolation: MockIsolationClass) -> Result<MockContainerId, MockContainerError> {
        if !self.initialized {
            return Err(MockContainerError::NotInitialized);
        }

        if self.containers.len() >= MAX_MOCK_CONTAINERS {
            return Err(MockContainerError::TableFull);
        }

        let id = MockContainerId(self.next_container_id);
        self.next_container_id += 1;

        let container = MockDriverContainer {
            id,
            isolation,
            state: MockContainerState::Created,
            drivers: Vec::new(),
        };

        self.containers.insert(id, container);
        Ok(id)
    }

    /// Spawn a driver in a container
    pub fn spawn_driver(
        &mut self,
        container_id: MockContainerId,
        driver_id: MockDriverId,
    ) -> Result<(), MockContainerError> {
        if !self.initialized {
            return Err(MockContainerError::NotInitialized);
        }

        // Check driver exists
        if !self.drivers.contains_key(&driver_id) {
            return Err(MockContainerError::DriverNotFound);
        }

        // Get container and add driver
        let container = self.containers.get_mut(&container_id)
            .ok_or(MockContainerError::NotFound)?;

        if container.state != MockContainerState::Created && 
           container.state != MockContainerState::Running {
            return Err(MockContainerError::InvalidState);
        }

        container.drivers.push(driver_id);
        container.state = MockContainerState::Running;
        Ok(())
    }

    /// Get container state
    pub fn get_container_state(&self, id: MockContainerId) -> Option<MockContainerState> {
        self.containers.get(&id).map(|c| c.state)
    }

    /// Stop a container
    pub fn stop_container(&mut self, id: MockContainerId) -> Result<(), MockContainerError> {
        let container = self.containers.get_mut(&id)
            .ok_or(MockContainerError::NotFound)?;
        
        container.state = MockContainerState::Stopped;
        container.drivers.clear();
        Ok(())
    }
}

impl Default for MockUdrvFramework {
    fn default() -> Self {
        Self::new()
    }
}

/// Global mock framework for testing (thread-safe)
static MOCK_FRAMEWORK: Mutex<Option<MockUdrvFramework>> = Mutex::new(None);

/// Initialize the global mock framework
pub fn init_global() {
    let mut framework = MOCK_FRAMEWORK.lock().unwrap();
    let mut fw = MockUdrvFramework::new();
    fw.init();
    *framework = Some(fw);
}

/// Check if global framework is initialized
pub fn is_global_initialized() -> bool {
    MOCK_FRAMEWORK.lock().unwrap()
        .as_ref()
        .map(|f| f.is_initialized())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_framework_init() {
        let mut framework = MockUdrvFramework::new();
        assert!(!framework.is_initialized());
        
        framework.init();
        assert!(framework.is_initialized());
    }

    #[test]
    fn test_global_framework_init() {
        init_global();
        assert!(is_global_initialized());
    }

    #[test]
    fn test_register_driver() {
        let mut framework = MockUdrvFramework::new();
        framework.init();

        let info = MockDriverInfo {
            name: "test_net_driver".to_string(),
            class: MockDriverClass::Network,
            isolation: MockIsolationClass::IC2,
            version: (1, 0, 0),
        };

        let id = framework.register_driver(info).unwrap();
        assert_eq!(id, MockDriverId(1));
        
        let drivers = framework.list_drivers();
        assert_eq!(drivers.len(), 1);
        assert!(drivers.contains(&id));
    }

    #[test]
    fn test_register_driver_not_initialized() {
        let mut framework = MockUdrvFramework::new();
        
        let info = MockDriverInfo {
            name: "test_driver".to_string(),
            class: MockDriverClass::Block,
            isolation: MockIsolationClass::IC1,
            version: (1, 0, 0),
        };

        let result = framework.register_driver(info);
        assert_eq!(result, Err(MockRegistryError::NotInitialized));
    }

    #[test]
    fn test_register_duplicate_driver() {
        let mut framework = MockUdrvFramework::new();
        framework.init();

        let info1 = MockDriverInfo {
            name: "same_name".to_string(),
            class: MockDriverClass::Network,
            isolation: MockIsolationClass::IC2,
            version: (1, 0, 0),
        };

        let info2 = MockDriverInfo {
            name: "same_name".to_string(),
            class: MockDriverClass::Block,
            isolation: MockIsolationClass::IC1,
            version: (2, 0, 0),
        };

        let _ = framework.register_driver(info1).unwrap();
        let result = framework.register_driver(info2);
        assert_eq!(result, Err(MockRegistryError::AlreadyExists));
    }

    #[test]
    fn test_unregister_driver() {
        let mut framework = MockUdrvFramework::new();
        framework.init();

        let info = MockDriverInfo {
            name: "to_remove".to_string(),
            class: MockDriverClass::Character,
            isolation: MockIsolationClass::IC2,
            version: (1, 0, 0),
        };

        let id = framework.register_driver(info).unwrap();
        assert_eq!(framework.list_drivers().len(), 1);

        framework.unregister_driver(id).unwrap();
        assert_eq!(framework.list_drivers().len(), 0);
    }

    #[test]
    fn test_create_container() {
        let mut framework = MockUdrvFramework::new();
        framework.init();

        let id = framework.create_container(MockIsolationClass::IC2).unwrap();
        assert_eq!(id, MockContainerId(1));

        let state = framework.get_container_state(id);
        assert_eq!(state, Some(MockContainerState::Created));
    }

    #[test]
    fn test_spawn_driver_in_container() {
        let mut framework = MockUdrvFramework::new();
        framework.init();

        let driver_info = MockDriverInfo {
            name: "net_driver".to_string(),
            class: MockDriverClass::Network,
            isolation: MockIsolationClass::IC2,
            version: (1, 0, 0),
        };

        let driver_id = framework.register_driver(driver_info).unwrap();
        let container_id = framework.create_container(MockIsolationClass::IC2).unwrap();

        framework.spawn_driver(container_id, driver_id).unwrap();

        let state = framework.get_container_state(container_id);
        assert_eq!(state, Some(MockContainerState::Running));
    }

    #[test]
    fn test_spawn_nonexistent_driver() {
        let mut framework = MockUdrvFramework::new();
        framework.init();

        let container_id = framework.create_container(MockIsolationClass::IC2).unwrap();
        let fake_driver_id = MockDriverId(999);

        let result = framework.spawn_driver(container_id, fake_driver_id);
        assert_eq!(result, Err(MockContainerError::DriverNotFound));
    }

    #[test]
    fn test_stop_container() {
        let mut framework = MockUdrvFramework::new();
        framework.init();

        let container_id = framework.create_container(MockIsolationClass::IC2).unwrap();
        
        framework.stop_container(container_id).unwrap();
        
        let state = framework.get_container_state(container_id);
        assert_eq!(state, Some(MockContainerState::Stopped));
    }

    #[test]
    fn test_cannot_unregister_active_driver() {
        let mut framework = MockUdrvFramework::new();
        framework.init();

        let driver_info = MockDriverInfo {
            name: "active_driver".to_string(),
            class: MockDriverClass::Block,
            isolation: MockIsolationClass::IC2,
            version: (1, 0, 0),
        };

        let driver_id = framework.register_driver(driver_info).unwrap();
        let container_id = framework.create_container(MockIsolationClass::IC2).unwrap();
        framework.spawn_driver(container_id, driver_id).unwrap();

        // Should fail because driver is in a container
        let result = framework.unregister_driver(driver_id);
        assert_eq!(result, Err(MockRegistryError::InvalidState));
    }

    #[test]
    fn test_get_driver_info() {
        let mut framework = MockUdrvFramework::new();
        framework.init();

        let info = MockDriverInfo {
            name: "info_test".to_string(),
            class: MockDriverClass::Graphics,
            isolation: MockIsolationClass::IC2,
            version: (2, 1, 3),
        };

        let id = framework.register_driver(info.clone()).unwrap();
        
        let retrieved = framework.get_driver_info(id).unwrap();
        assert_eq!(retrieved.name, "info_test");
        assert_eq!(retrieved.class, MockDriverClass::Graphics);
        assert_eq!(retrieved.version, (2, 1, 3));
    }
}
