//! Memory subsystem integration tests
//!
//! Tests memory allocation, paging, and physical memory management
//! using the Virtual Machine emulation layer.

use crate::mock::vm::{VirtualMachine, VmConfig, TestHarness};
use crate::mock::hal;
use crate::mock::memory::{PhysicalMemory, MemoryType, MemoryRegion};

/// Physical memory tests
mod physical_memory {
    use super::*;
    
    #[test]
    fn test_physical_memory_creation() {
        let mem = PhysicalMemory::new(64); // 64MB
        
        // Should have memory regions
        let regions = mem.memory_map();
        assert!(!regions.is_empty(), "Should have at least one memory region");
        
        // First region should be usable RAM
        let first = &regions[0];
        assert!(first.region_type.is_usable());
    }
    
    #[test]
    fn test_physical_memory_read_write() {
        let mem = PhysicalMemory::new(16); // 16MB
        
        // Write data at physical address
        let data = b"Hello, NexaOS!";
        mem.write_bytes(0x1000, data);
        
        // Read it back
        let mut buffer = [0u8; 14];
        mem.read_bytes(0x1000, &mut buffer);
        
        assert_eq!(&buffer, data);
    }
    
    #[test]
    fn test_physical_memory_zeroed() {
        let mem = PhysicalMemory::new(4); // 4MB
        
        // Memory should be zeroed initially
        let mut buffer = [0xFFu8; 256];
        mem.read_bytes(0x2000, &mut buffer);
        
        assert!(buffer.iter().all(|&b| b == 0), "Memory should be zeroed");
    }
    
    #[test]
    fn test_physical_memory_boundary() {
        let mem = PhysicalMemory::new(4); // 4MB = 4 * 1024 * 1024 bytes
        
        // Write near the end of memory (should not panic)
        let addr = 4 * 1024 * 1024 - 8;
        mem.write_bytes(addr, &[1, 2, 3, 4, 5, 6, 7, 8]);
        
        let mut buf = [0u8; 8];
        mem.read_bytes(addr, &mut buf);
        assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8]);
    }
    
    #[test]
    fn test_physical_memory_alignment() {
        let mem = PhysicalMemory::new(8); // 8MB
        
        // Write u64 at aligned address
        let value: u64 = 0x123456789ABCDEF0;
        mem.write_bytes(0x1000, &value.to_le_bytes());
        
        let mut buf = [0u8; 8];
        mem.read_bytes(0x1000, &mut buf);
        
        let read_value = u64::from_le_bytes(buf);
        assert_eq!(read_value, value);
    }
}

/// VM memory tests
mod vm_memory {
    use super::*;
    
    #[test]
    fn test_vm_memory_default_size() {
        let vm = VirtualMachine::new();
        
        // Default is 64MB
        let regions = vm.memory().memory_map();
        let total: u64 = regions.iter()
            .filter(|r| r.region_type.is_usable())
            .map(|r| r.size as u64)
            .sum();
        
        // Should be close to 64MB (some may be reserved)
        assert!(total >= 60 * 1024 * 1024, "Should have at least 60MB usable");
    }
    
    #[test]
    fn test_vm_memory_custom_size() {
        let config = VmConfig {
            memory_mb: 128,
            ..VmConfig::default()
        };
        let vm = VirtualMachine::with_config(config);
        
        let regions = vm.memory().memory_map();
        let total: u64 = regions.iter()
            .filter(|r| r.region_type.is_usable())
            .map(|r| r.size as u64)
            .sum();
        
        assert!(total >= 120 * 1024 * 1024, "Should have at least 120MB usable");
    }
    
    #[test]
    fn test_vm_memory_read_write() {
        let vm = VirtualMachine::new();
        
        // Use high-level API
        let test_data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        vm.write_memory(0x100000, &test_data);
        
        let read_data = vm.read_memory(0x100000, 4);
        assert_eq!(read_data, test_data);
    }
    
    #[test]
    fn test_vm_memory_isolation() {
        // Create two VMs and verify memory is isolated
        let vm1 = VirtualMachine::new();
        let vm2 = VirtualMachine::new();
        
        vm1.write_memory(0x1000, &[0xAA, 0xBB, 0xCC, 0xDD]);
        vm2.write_memory(0x1000, &[0x11, 0x22, 0x33, 0x44]);
        
        let data1 = vm1.read_memory(0x1000, 4);
        let data2 = vm2.read_memory(0x1000, 4);
        
        assert_eq!(data1, vec![0xAA, 0xBB, 0xCC, 0xDD]);
        assert_eq!(data2, vec![0x11, 0x22, 0x33, 0x44]);
    }
}

/// Memory mapped I/O tests
mod mmio_tests {
    use super::*;
    
    #[test]
    fn test_mmio_through_hal() {
        let harness = TestHarness::new();
        harness.run(|vm| {
            // LAPIC is typically at 0xFEE00000
            // In emulation, we can test MMIO access patterns
            
            // Read LAPIC ID (offset 0x20)
            let lapic_base: u64 = 0xFEE00000;
            
            // This tests that MMIO infrastructure works
            // Actual LAPIC emulation may or may not be fully implemented
            let lapic_id = hal::mmio_read_u32(lapic_base + 0x20);
            // Just verify no panic
        });
    }
}

/// Boot info tests
mod boot_info_tests {
    use super::*;
    
    #[test]
    fn test_mock_boot_info_creation() {
        let vm = VirtualMachine::new();
        let boot_info = vm.create_boot_info();
        
        // Should have memory map
        assert!(!boot_info.memory_map.is_empty());
        
        // Should have kernel addresses
        assert!(boot_info.kernel_start < boot_info.kernel_end);
        
        // Should have initramfs addresses
        assert!(boot_info.initramfs_start < boot_info.initramfs_end);
    }
    
    #[test]
    fn test_mock_boot_info_memory_regions() {
        let vm = VirtualMachine::new();
        let boot_info = vm.create_boot_info();
        
        // Check that memory regions are properly ordered
        for i in 1..boot_info.memory_map.len() {
            let prev = &boot_info.memory_map[i - 1];
            let curr = &boot_info.memory_map[i];
            
            // Regions should not overlap
            assert!(prev.start + prev.size as u64 <= curr.start, 
                "Memory regions should not overlap");
        }
    }
}
