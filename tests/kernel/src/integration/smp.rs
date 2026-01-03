//! SMP Integration Tests
//!
//! Tests for Symmetric Multi-Processing subsystem using the virtual machine layer.
//! These tests verify per-CPU data isolation, inter-processor communication,
//! and multi-core scheduling correctness.

use crate::mock::vm::{VirtualMachine, VmConfig};
use crate::mock::cpu::VirtualCpu;
use std::sync::{Arc, Barrier};
use std::thread;

// ============================================================================
// Per-CPU Data Isolation Tests
// ============================================================================

#[test]
fn test_smp_cpu_id_isolation() {
    // Each CPU should have a unique ID
    let config = VmConfig {
        cpus: 4,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    vm.install();
    
    let hal = vm.hal();
    
    // Verify we have 4 CPUs
    let cpus = hal.cpus.read().unwrap();
    assert_eq!(cpus.len(), 4);
    
    // Verify unique IDs
    for (i, cpu) in cpus.iter().enumerate() {
        assert_eq!(cpu.id as usize, i, "CPU ID should match index");
    }
    
    vm.uninstall();
}

#[test]
fn test_smp_bsp_vs_ap() {
    // BSP (CPU 0) should not be halted, APs should start halted
    let config = VmConfig {
        cpus: 4,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    vm.install();
    
    let hal = vm.hal();
    let cpus = hal.cpus.read().unwrap();
    
    // BSP not halted
    assert!(!cpus[0].is_halted(), "BSP should not be halted");
    
    // APs start halted
    for i in 1..4 {
        assert!(cpus[i].is_halted(), "AP {} should start halted", i);
    }
    
    vm.uninstall();
}

#[test]
fn test_smp_cpu_state_isolation() {
    // Each CPU should have independent state
    let config = VmConfig {
        cpus: 4,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    vm.install();
    
    let hal = vm.hal();
    
    // Set different CR3 for each CPU
    {
        let cpus = hal.cpus.read().unwrap();
        for (i, cpu) in cpus.iter().enumerate() {
            cpu.write_cr3(0x1000 * (i as u64 + 1));
        }
    }
    
    // Verify each CPU has its own CR3
    {
        let cpus = hal.cpus.read().unwrap();
        for (i, cpu) in cpus.iter().enumerate() {
            let expected = 0x1000 * (i as u64 + 1);
            assert_eq!(cpu.read_cr3(), expected, "CPU {} CR3 should be {:#x}", i, expected);
        }
    }
    
    vm.uninstall();
}

// ============================================================================
// CPU Switching Tests
// ============================================================================

#[test]
fn test_smp_cpu_switching() {
    let config = VmConfig {
        cpus: 4,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    vm.install();
    
    let hal = vm.hal();
    
    // Start on CPU 0
    assert_eq!(hal.cpu().id, 0);
    
    // Switch to CPU 2
    hal.switch_cpu(2);
    assert_eq!(hal.cpu().id, 2);
    
    // Switch to CPU 3
    hal.switch_cpu(3);
    assert_eq!(hal.cpu().id, 3);
    
    // Switch back to CPU 0
    hal.switch_cpu(0);
    assert_eq!(hal.cpu().id, 0);
    
    vm.uninstall();
}

#[test]
fn test_smp_state_preserved_across_switch() {
    let config = VmConfig {
        cpus: 2,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    vm.install();
    
    let hal = vm.hal();
    
    // Set state on CPU 0
    hal.switch_cpu(0);
    hal.cpu().write_cr3(0x1000);
    hal.cpu().write_msr(0xC0000100, 0x12345678); // FS_BASE
    
    // Switch to CPU 1 and set different state
    hal.switch_cpu(1);
    hal.cpu().wake(); // Wake AP
    hal.cpu().write_cr3(0x2000);
    hal.cpu().write_msr(0xC0000100, 0xABCDEF00);
    
    // Verify CPU 1 state
    assert_eq!(hal.cpu().read_cr3(), 0x2000);
    assert_eq!(hal.cpu().read_msr(0xC0000100), 0xABCDEF00);
    
    // Switch back to CPU 0 and verify state preserved
    hal.switch_cpu(0);
    assert_eq!(hal.cpu().read_cr3(), 0x1000);
    assert_eq!(hal.cpu().read_msr(0xC0000100), 0x12345678);
    
    vm.uninstall();
}

// ============================================================================
// TSC (Time Stamp Counter) Tests
// ============================================================================

#[test]
fn test_smp_tsc_independent() {
    let config = VmConfig {
        cpus: 2,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    vm.install();
    
    let hal = vm.hal();
    
    // Read TSC on CPU 0
    hal.switch_cpu(0);
    let tsc0_before = hal.cpu().rdtsc();
    
    // Advance CPU 0's TSC
    hal.cpu().advance_cycles(1000);
    let tsc0_after = hal.cpu().rdtsc();
    
    // Switch to CPU 1 and read its TSC
    hal.switch_cpu(1);
    hal.cpu().wake();
    let tsc1 = hal.cpu().rdtsc();
    
    // CPU 0's TSC should have advanced
    assert!(tsc0_after > tsc0_before, "CPU 0 TSC should advance");
    
    // TSCs are read independently (they may or may not be synchronized)
    eprintln!("CPU 0 TSC: {} -> {}", tsc0_before, tsc0_after);
    eprintln!("CPU 1 TSC: {}", tsc1);
    
    vm.uninstall();
}

// ============================================================================
// Interrupt Flag Tests
// ============================================================================

#[test]
fn test_smp_interrupt_flag_isolation() {
    let config = VmConfig {
        cpus: 2,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    vm.install();
    
    let hal = vm.hal();
    
    // Enable interrupts on CPU 0
    hal.switch_cpu(0);
    hal.cpu().enable_interrupts();
    assert!(hal.cpu().interrupts_enabled());
    
    // CPU 1 should still have interrupts disabled
    hal.switch_cpu(1);
    hal.cpu().wake();
    assert!(!hal.cpu().interrupts_enabled(), "CPU 1 should start with IF=0");
    
    // Enable on CPU 1
    hal.cpu().enable_interrupts();
    assert!(hal.cpu().interrupts_enabled());
    
    // CPU 0 should still be enabled
    hal.switch_cpu(0);
    assert!(hal.cpu().interrupts_enabled());
    
    vm.uninstall();
}

// ============================================================================
// Halt State Tests
// ============================================================================

#[test]
fn test_smp_halt_wake_cycle() {
    let config = VmConfig {
        cpus: 2,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    vm.install();
    
    let hal = vm.hal();
    
    // CPU 1 starts halted
    hal.switch_cpu(1);
    assert!(hal.cpu().is_halted());
    
    // Wake CPU 1
    hal.cpu().wake();
    assert!(!hal.cpu().is_halted());
    
    // Halt CPU 1 again
    hal.cpu().halt();
    assert!(hal.cpu().is_halted());
    
    // Wake again
    hal.cpu().wake();
    assert!(!hal.cpu().is_halted());
    
    vm.uninstall();
}

// ============================================================================
// Multi-CPU Concurrent Access Tests
// ============================================================================

#[test]
fn test_smp_concurrent_memory_access() {
    let config = VmConfig {
        memory_mb: 64,
        cpus: 4,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    let memory = vm.memory();
    
    // Each CPU writes to a different memory location
    let barrier = Arc::new(Barrier::new(4));
    let mut handles = vec![];
    
    for cpu_id in 0..4u32 {
        let memory = Arc::clone(&memory);
        let barrier = Arc::clone(&barrier);
        
        handles.push(thread::spawn(move || {
            barrier.wait();
            
            // Each CPU writes to its own page
            let addr = (cpu_id as u64 + 1) * 0x1000;
            memory.write_u64(addr, cpu_id as u64 * 0x1111_1111);
            
            // Read back
            let value = memory.read_u64(addr);
            assert_eq!(value, cpu_id as u64 * 0x1111_1111,
                "CPU {} memory write should be preserved", cpu_id);
        }));
    }
    
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
}

#[test]
fn test_smp_shared_memory_access() {
    let config = VmConfig {
        memory_mb: 64,
        cpus: 4,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    let memory = vm.memory();
    
    // All CPUs increment a shared counter
    let barrier = Arc::new(Barrier::new(4));
    let counter_addr = 0x10000u64;
    
    // Initialize counter
    memory.write_u64(counter_addr, 0);
    
    let mut handles = vec![];
    
    for _cpu_id in 0..4 {
        let memory = Arc::clone(&memory);
        let barrier = Arc::clone(&barrier);
        
        handles.push(thread::spawn(move || {
            barrier.wait();
            
            // Note: This is NOT atomic! Just testing memory access
            for _ in 0..100 {
                let val = memory.read_u64(counter_addr);
                memory.write_u64(counter_addr, val + 1);
            }
        }));
    }
    
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
    
    // Due to races, counter will likely be less than 400
    let final_val = memory.read_u64(counter_addr);
    eprintln!("Shared counter (expected races): {}", final_val);
    assert!(final_val > 0, "Counter should have been incremented");
}

// ============================================================================
// MSR Per-CPU Tests
// ============================================================================

#[test]
fn test_smp_msr_per_cpu() {
    let config = VmConfig {
        cpus: 4,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    vm.install();
    
    let hal = vm.hal();
    
    use crate::mock::cpu::msr;
    
    // Set different FS_BASE for each CPU
    for cpu_id in 0..4u32 {
        hal.switch_cpu(cpu_id);
        if cpu_id > 0 {
            hal.cpu().wake();
        }
        hal.cpu().write_msr(msr::IA32_FS_BASE, 0x7F00_0000_0000 + cpu_id as u64 * 0x1000);
    }
    
    // Verify each CPU has its own value
    for cpu_id in 0..4u32 {
        hal.switch_cpu(cpu_id);
        let fs_base = hal.cpu().read_msr(msr::IA32_FS_BASE);
        let expected = 0x7F00_0000_0000 + cpu_id as u64 * 0x1000;
        assert_eq!(fs_base, expected, "CPU {} FS_BASE should be {:#x}", cpu_id, expected);
    }
    
    vm.uninstall();
}

// ============================================================================
// CPUID Per-CPU Tests
// ============================================================================

#[test]
fn test_smp_cpuid_apic_id() {
    let config = VmConfig {
        cpus: 4,
        ..VmConfig::default()
    };
    
    let vm = VirtualMachine::with_config(config);
    vm.install();
    
    let hal = vm.hal();
    
    // CPUID leaf 1, EBX[31:24] contains initial APIC ID
    for cpu_id in 0..4u32 {
        hal.switch_cpu(cpu_id);
        if cpu_id > 0 {
            hal.cpu().wake();
        }
        
        let (_, ebx, _, _) = hal.cpu().cpuid(1, 0);
        let apic_id = (ebx >> 24) & 0xFF;
        
        // APIC ID should match CPU ID in our emulation
        assert_eq!(apic_id, cpu_id, "CPU {} APIC ID should be {}", cpu_id, cpu_id);
    }
    
    vm.uninstall();
}
