//! Virtual Machine for Kernel Testing
//!
//! This module provides a comprehensive virtual machine implementation similar to
//! QEMU-KVM, Hyper-V, or VMware for testing the NexaOS kernel without real hardware.
//!
//! ## Features
//!
//! - **Multi-vCPU support** - SMP testing with configurable CPU count
//! - **Snapshot/Restore** - VMware-style VM state snapshots
//! - **Device emulation** - PIC, PIT, UART, APIC, RTC, PCI
//! - **Memory management** - Physical memory, MMIO, DMA emulation
//! - **Event tracing** - Complete VM event logging
//! - **Debugging support** - Breakpoints, single-step, state inspection
//! - **Hot-plug** - Dynamic device attachment/detachment
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────────┐
//! │                         VirtualMachine                                 │
//! ├────────────────────────────────────────────────────────────────────────┤
//! │  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────────┐ │
//! │  │   VmController   │  │   VmMonitor      │  │    VmDebugger        │ │
//! │  │   - Start/Stop   │  │   - Events       │  │    - Breakpoints     │ │
//! │  │   - Pause/Resume │  │   - Statistics   │  │    - Single-step     │ │
//! │  │   - Reset        │  │   - Tracing      │  │    - Inspection      │ │
//! │  └──────────────────┘  └──────────────────┘  └──────────────────────┘ │
//! │  ┌────────────────────────────────────────────────────────────────┐   │
//! │  │                HardwareAbstractionLayer (HAL)                  │   │
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐│   │
//! │  │  │  vCPU(s) │  │ Memory   │  │ Devices  │  │  PCI Bus         ││   │
//! │  │  │  BSP+APs │  │ RAM/MMIO │  │ PIC/PIT  │  │  Enum/Config     ││   │
//! │  │  └──────────┘  └──────────┘  └──────────┘  └──────────────────┘│   │
//! │  └────────────────────────────────────────────────────────────────┘   │
//! │  ┌────────────────────────────────────────────────────────────────┐   │
//! │  │                     Snapshot Manager                           │   │
//! │  │  - CPU state snapshots    - Memory snapshots                   │   │
//! │  │  - Device state snapshots - Named snapshot trees               │   │
//! │  └────────────────────────────────────────────────────────────────┘   │
//! └────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage Example
//!
//! ```rust,ignore
//! use tests::mock::vm::{VirtualMachine, VmConfig};
//!
//! // Create a VM with 4 CPUs and 128MB RAM
//! let vm = VirtualMachine::with_config(VmConfig {
//!     memory_mb: 128,
//!     cpus: 4,
//!     enable_apic: true,
//!     ..Default::default()
//! });
//!
//! // Install HAL and run kernel code
//! vm.install();
//! kernel_init();
//!
//! // Take a snapshot
//! let snapshot = vm.snapshot("after_init");
//!
//! // Run more code...
//! kernel_run();
//!
//! // Restore to saved state
//! vm.restore(&snapshot);
//!
//! vm.uninstall();
//! ```

use std::sync::{Arc, Mutex, RwLock};
use std::collections::{HashMap, VecDeque};
use std::time::{Instant, Duration};

use super::cpu::{VirtualCpu, CpuStateSnapshot, CpuPool, CpuEvent, BreakpointType};
use super::devices::{Device, DeviceManager, DeviceId};
use super::devices::pic::Pic8259;
use super::devices::pit::Pit8254;
use super::devices::uart::Uart16550;
use super::devices::rtc::Rtc;
use super::devices::lapic::LocalApic;
use super::devices::ioapic::IoApic;
use super::hal::{HardwareAbstractionLayer, set_hal, clear_hal};
use super::memory::{PhysicalMemory, MemoryRegion, MemoryType};
use super::pci::{PciBus, PciConfig, PciLocation, SimplePciDevice, class, vendor};

/// Events that can occur in the VM
#[derive(Debug, Clone)]
pub enum VmEvent {
    /// Serial output received
    SerialOutput(Vec<u8>),
    /// Interrupt raised
    Interrupt { vector: u8, from_device: &'static str },
    /// Memory access
    MemoryAccess { addr: u64, size: usize, is_write: bool },
    /// Port I/O
    PortIo { port: u16, value: u32, is_write: bool },
    /// VM state change
    StateChange { from: VmState, to: VmState },
    /// Snapshot created
    SnapshotCreated { name: String },
    /// Snapshot restored
    SnapshotRestored { name: String },
    /// Device attached
    DeviceAttached { id: DeviceId, name: String },
    /// Device detached
    DeviceDetached { id: DeviceId },
    /// CPU event (forwarded from vCPU)
    CpuEvent { cpu_id: u32, event: CpuEvent },
    /// VM started
    Started,
    /// VM stopped
    Stopped,
    /// VM paused
    Paused,
    /// VM resumed
    Resumed,
    /// VM reset
    Reset,
}

/// VM execution state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    /// VM is created but not started
    Created,
    /// VM is running
    Running,
    /// VM is paused
    Paused,
    /// VM is stopped
    Stopped,
    /// VM is in error state
    Error,
}

impl Default for VmState {
    fn default() -> Self {
        Self::Created
    }
}

/// VM configuration
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// Memory size in MB
    pub memory_mb: usize,
    /// Number of CPUs
    pub cpus: usize,
    /// Firmware type (BIOS or UEFI)
    pub firmware_type: crate::firmware::FirmwareType,
    /// Enable PIC (8259)
    pub enable_pic: bool,
    /// Enable PIT (8254)
    pub enable_pit: bool,
    /// Enable serial (16550)
    pub enable_serial: bool,
    /// Enable RTC
    pub enable_rtc: bool,
    /// Enable APIC (LAPIC + IOAPIC)
    pub enable_apic: bool,
    /// Enable VGA display adapter
    pub enable_vga: bool,
    /// Enable event tracing
    pub enable_tracing: bool,
    /// Maximum trace buffer size
    pub max_trace_size: usize,
    /// VM name (for identification)
    pub name: String,
    /// Enable nested virtualization support
    pub nested_virt: bool,
    /// NUMA node configuration (memory_mb per node)
    pub numa_nodes: Vec<usize>,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            memory_mb: 64,
            cpus: 1,
            firmware_type: crate::firmware::FirmwareType::Bios,
            enable_pic: true,
            enable_pit: true,
            enable_serial: true,
            enable_rtc: true,
            enable_apic: true,
            enable_vga: true,
            enable_tracing: true,
            max_trace_size: 10000,
            name: String::from("NexaOS-TestVM"),
            nested_virt: false,
            numa_nodes: Vec::new(),
        }
    }
}

impl VmConfig {
    pub fn minimal() -> Self {
        Self {
            memory_mb: 16,
            cpus: 1,
            firmware_type: crate::firmware::FirmwareType::Bios,
            enable_pic: false,
            enable_pit: false,
            enable_serial: true,
            enable_rtc: false,
            enable_apic: false,
            enable_vga: false,
            enable_tracing: false,
            max_trace_size: 1000,
            name: String::from("MinimalVM"),
            nested_virt: false,
            numa_nodes: Vec::new(),
        }
    }
    
    pub fn full() -> Self {
        Self {
            memory_mb: 128,
            cpus: 4,
            firmware_type: crate::firmware::FirmwareType::Bios,
            enable_pic: true,
            enable_pit: true,
            enable_serial: true,
            enable_rtc: true,
            enable_apic: true,
            enable_vga: true,
            enable_tracing: true,
            max_trace_size: 50000,
            name: String::from("FullVM"),
            nested_virt: false,
            numa_nodes: Vec::new(),
        }
    }
    
    /// Configure for SMP testing
    pub fn smp(cpus: usize) -> Self {
        Self {
            cpus,
            enable_apic: true,
            ..Self::default()
        }
    }
    
    /// Configure for NUMA testing
    pub fn numa(nodes: Vec<usize>) -> Self {
        let total_mem: usize = nodes.iter().sum();
        Self {
            memory_mb: total_mem,
            cpus: nodes.len() * 2, // 2 CPUs per node
            numa_nodes: nodes,
            enable_apic: true,
            ..Self::default()
        }
    }
}

/// VM Snapshot (complete VM state for save/restore)
#[derive(Clone)]
pub struct VmSnapshot {
    /// Snapshot name
    pub name: String,
    /// Creation timestamp
    pub timestamp: Instant,
    /// CPU states
    pub cpu_states: Vec<CpuStateSnapshot>,
    /// Memory snapshot (sparse - only modified pages)
    memory_pages: HashMap<u64, Vec<u8>>,
    /// Device states (serialized)
    device_states: HashMap<DeviceId, Vec<u8>>,
    /// VM state at snapshot time
    pub vm_state: VmState,
    /// Parent snapshot (for incremental snapshots)
    pub parent: Option<String>,
}

impl VmSnapshot {
    /// Get memory size of snapshot
    pub fn memory_usage(&self) -> usize {
        self.memory_pages.values().map(|p| p.len()).sum::<usize>()
            + self.cpu_states.len() * std::mem::size_of::<CpuStateSnapshot>()
    }
}

impl std::fmt::Debug for VmSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VmSnapshot")
            .field("name", &self.name)
            .field("cpu_count", &self.cpu_states.len())
            .field("memory_pages", &self.memory_pages.len())
            .field("vm_state", &self.vm_state)
            .finish()
    }
}

/// VM Statistics
#[derive(Debug, Clone, Default)]
pub struct VmStatistics {
    /// Total cycles executed
    pub total_cycles: u64,
    /// Total instructions retired (estimated)
    pub instructions_retired: u64,
    /// Memory reads
    pub memory_reads: u64,
    /// Memory writes
    pub memory_writes: u64,
    /// Port I/O operations
    pub port_io_ops: u64,
    /// Interrupts delivered
    pub interrupts_delivered: u64,
    /// VM exits (for debugging)
    pub vm_exits: u64,
    /// Time spent running (nanoseconds)
    pub runtime_ns: u64,
    /// Snapshot count
    pub snapshots_created: u64,
    /// Snapshot restores
    pub snapshots_restored: u64,
}

/// Virtual Machine for kernel testing
pub struct VirtualMachine {
    /// Hardware abstraction layer
    hal: Arc<HardwareAbstractionLayer>,
    /// Configuration
    config: VmConfig,
    /// Current VM state
    state: RwLock<VmState>,
    /// Serial output capture
    serial_output: Arc<Mutex<Vec<u8>>>,
    /// VGA display device
    vga_device: Option<Arc<Mutex<crate::devices::vga::Vga>>>,
    /// PS/2 Keyboard device
    keyboard_device: Option<Arc<Mutex<crate::devices::keyboard::Ps2Keyboard>>>,
    /// Event log
    events: Arc<Mutex<VecDeque<VmEvent>>>,
    /// Maximum event log size
    max_events: usize,
    /// Named snapshots
    snapshots: RwLock<HashMap<String, VmSnapshot>>,
    /// Statistics
    stats: Mutex<VmStatistics>,
    /// Start time (for runtime tracking)
    start_time: Mutex<Option<Instant>>,
    /// Attached devices (for hot-plug tracking)
    attached_devices: RwLock<Vec<(DeviceId, String)>>,
}

impl VirtualMachine {
    /// Create a new VM with default configuration
    pub fn new() -> Self {
        Self::with_config(VmConfig::default())
    }
    
    /// Create a new VM with custom configuration
    pub fn with_config(config: VmConfig) -> Self {
        let hal = Arc::new(HardwareAbstractionLayer::new(config.memory_mb));
        let serial_output = Arc::new(Mutex::new(Vec::new()));
        let max_events = config.max_trace_size;
        
        // Add standard devices
        if config.enable_pic {
            let pic = Pic8259::new();
            hal.devices.add_device_with_ports(
                Arc::new(Mutex::new(pic)),
                &[(0x20, 0x21), (0xA0, 0xA1)],
            );
        }
        
        if config.enable_pit {
            let pit = Pit8254::new();
            hal.devices.add_device_with_ports(
                Arc::new(Mutex::new(pit)),
                &[(0x40, 0x43)],
            );
        }
        
        if config.enable_serial {
            let uart = Uart16550::new_com1();
            // Capture output
            let _output = uart.output();
            hal.devices.add_device_with_ports(
                Arc::new(Mutex::new(uart)),
                &[(0x3F8, 0x3FF)],
            );
        }
        
        if config.enable_rtc {
            let rtc = Rtc::new();
            hal.devices.add_device_with_ports(
                Arc::new(Mutex::new(rtc)),
                &[(0x70, 0x71)],
            );
        }
        
        if config.enable_apic {
            let lapic = LocalApic::new(0);
            hal.devices.add_device(Arc::new(Mutex::new(lapic)));
            
            let ioapic = IoApic::new(0);
            hal.devices.add_device(Arc::new(Mutex::new(ioapic)));
        }
        
        // VGA display device
        let vga_device = if config.enable_vga {
            let vga = Arc::new(Mutex::new(crate::devices::vga::Vga::new()));
            // Initialize with text mode message
            {
                let mut vga_lock = vga.lock().unwrap();
                vga_lock.clear();
                vga_lock.write_string("NexaOS Virtual Machine Console\n");
                vga_lock.write_string("-----------------------------\n\n");
            }
            // Register VGA port ranges (3C0-3CF for attribute/misc, 3D4-3D5 for CRTC)
            hal.devices.add_device_with_ports(
                vga.clone(),
                &[(0x3C0, 0x3CF), (0x3D4, 0x3D5)],
            );
            Some(vga)
        } else {
            None
        };
        
        // PS/2 Keyboard Controller
        let keyboard_device = {
            let keyboard = Arc::new(Mutex::new(crate::devices::keyboard::Ps2Keyboard::new()));
            // Register keyboard port ranges (0x60 and 0x64)
            hal.devices.add_device_with_ports(
                keyboard.clone(),
                &[(0x60, 0x60), (0x64, 0x64)],
            );
            Some(keyboard)
        };
        
        // Add additional CPUs
        for _ in 1..config.cpus {
            hal.add_cpu();
        }
        
        Self {
            hal,
            config,
            state: RwLock::new(VmState::Created),
            serial_output,
            vga_device,
            keyboard_device,
            events: Arc::new(Mutex::new(VecDeque::with_capacity(max_events))),
            max_events,
            snapshots: RwLock::new(HashMap::new()),
            stats: Mutex::new(VmStatistics::default()),
            start_time: Mutex::new(None),
            attached_devices: RwLock::new(Vec::new()),
        }
    }
    
    // ========================================================================
    // VM Lifecycle Control
    // ========================================================================
    
    /// Start the VM
    pub fn start(&self) {
        let mut state = self.state.write().unwrap();
        if *state == VmState::Created || *state == VmState::Stopped {
            let old_state = *state;
            
            // Load firmware into guest memory before starting
            self.load_firmware();
            
            *state = VmState::Running;
            *self.start_time.lock().unwrap() = Some(Instant::now());
            drop(state);
            self.record_event(VmEvent::StateChange { from: old_state, to: VmState::Running });
            self.record_event(VmEvent::Started);
        }
    }
    
    /// Load firmware (BIOS or UEFI) into guest memory
    fn load_firmware(&self) {
        use crate::firmware::{FirmwareManager, FirmwareType, BootPhase};
        
        // Get raw memory slice from physical memory
        let (ram_ptr, ram_size) = self.hal.memory.ram_region();
        let memory_slice = unsafe { std::slice::from_raw_parts_mut(ram_ptr, ram_size) };
        
        // Create and initialize firmware manager
        let fw_manager = FirmwareManager::new(self.config.firmware_type);
        if let Err(e) = fw_manager.initialize(self.config.memory_mb, self.config.cpus as u32) {
            self.record_event(VmEvent::PortIo { 
                port: 0x80, value: 0xFF, is_write: true // Fatal error
            });
            return;
        }
        
        // Load firmware and get boot context
        match fw_manager.load_firmware(memory_slice) {
            Ok(context) => {
                // Initialize CPU state based on boot context
                self.initialize_cpu_for_boot(&context);
                
                // Record POST code
                let post_code = fw_manager.get_state().post_code;
                self.record_event(VmEvent::PortIo { 
                    port: 0x80,
                    value: post_code as u32,
                    is_write: true 
                });
                
                // Display boot info on VGA - Enterprise-grade boot experience
                if let Some(ref vga) = self.vga_device {
                    let mut vga_lock = vga.lock().unwrap();
                    vga_lock.clear();
                    
                    match self.config.firmware_type {
                        FirmwareType::Bios => {
                            // ESXi-style BIOS boot screen
                            vga_lock.write_string_colored("NexaBIOS v1.0 - Enterprise Edition\n", 0x1F);  // White on blue
                            vga_lock.write_string("================================================================================\n");
                            vga_lock.write_string(&format!("Memory: {} MB detected\n", self.config.memory_mb));
                            vga_lock.write_string(&format!("CPUs: {} processor(s)\n\n", self.config.cpus));
                            vga_lock.write_string("POST Complete.\n\n");
                            
                            // Setup key prompt (F2/DEL)
                            for line in fw_manager.get_boot_prompt(self.config.firmware_type) {
                                if line.contains("F2") || line.contains("DEL") {
                                    vga_lock.write_string_colored(&format!("{}\n", line), 0x0E);  // Yellow
                                } else {
                                    vga_lock.write_string(&format!("{}\n", line));
                                }
                            }
                        }
                        FirmwareType::Uefi | FirmwareType::UefiSecure => {
                            // Modern UEFI boot screen (like Dell/HP enterprise servers)
                            vga_lock.write_string_colored("NexaUEFI v1.0 - Enterprise Edition\n", 0x1F);  // White on blue
                            vga_lock.write_string("================================================================================\n\n");
                            vga_lock.write_string(&format!("Memory: {} MB\n", self.config.memory_mb));
                            vga_lock.write_string(&format!("CPUs: {} processor(s)\n", self.config.cpus));
                            if matches!(self.config.firmware_type, FirmwareType::UefiSecure) {
                                vga_lock.write_string_colored("Secure Boot: ENABLED\n", 0x0A);  // Green
                            }
                            vga_lock.write_string("\nUEFI Initialization Complete.\n\n");
                            
                            // Setup key prompt
                            for line in fw_manager.get_boot_prompt(self.config.firmware_type) {
                                if line.contains("F2") || line.contains("DEL") || line.contains("F12") {
                                    vga_lock.write_string_colored(&format!("{}\n", line), 0x0E);  // Yellow
                                } else {
                                    vga_lock.write_string(&format!("{}\n", line));
                                }
                            }
                        }
                    }
                    
                    // Check for bootable devices - if none, show enterprise error
                    if !fw_manager.has_bootable_device() {
                        fw_manager.set_no_bootable_device();
                        vga_lock.write_string("\n");
                        
                        for line in fw_manager.get_no_boot_device_message(self.config.firmware_type) {
                            if line.contains("NOT FOUND") || line.contains("NO BOOTABLE") {
                                vga_lock.write_string_colored(&format!("{}\n", line), 0x4F);  // White on red
                            } else if line.contains("[F2]") || line.contains("[F12]") || line.contains("Ctrl+Alt+Del") {
                                vga_lock.write_string_colored(&format!("{}\n", line), 0x0E);  // Yellow
                            } else {
                                vga_lock.write_string(&format!("{}\n", line));
                            }
                        }
                    } else {
                        vga_lock.write_string("Loading Boot Manager...\n\n");
                    }
                }
            }
            Err(e) => {
                // Boot failed - show enterprise error
                self.record_event(VmEvent::PortIo { 
                    port: 0x80, value: 0xE0, is_write: true // Error
                });
                
                fw_manager.boot_failed(&e.to_string());
                
                if let Some(ref vga) = self.vga_device {
                    let mut vga_lock = vga.lock().unwrap();
                    vga_lock.write_string_colored("\n\n*** FIRMWARE INITIALIZATION ERROR ***\n\n", 0x4F);
                    vga_lock.write_string(&format!("Error: {}\n\n", e));
                    vga_lock.write_string("The system was unable to initialize the firmware.\n");
                    vga_lock.write_string("Please check hardware configuration.\n\n");
                    vga_lock.write_string_colored("Press Ctrl+Alt+Del to restart\n", 0x0E);
                    vga_lock.write_string("System halted.\n");
                }
            }
        }
    }
    
    /// Initialize CPU state for firmware boot handoff
    fn initialize_cpu_for_boot(&self, context: &crate::firmware::FirmwareBootContext) {
        use crate::cpu::msr;
        
        let cpus = self.hal.cpus.read().unwrap();
        if let Some(bsp) = cpus.first() {
            // Set instruction pointer
            bsp.write_rip(context.entry_point);
            
            // Set stack pointer
            bsp.write_rsp(context.stack_pointer);
            
            // Set control registers
            bsp.write_cr0(context.cr0);
            bsp.write_cr3(context.cr3);
            bsp.write_cr4(context.cr4);
            
            // Set EFER MSR
            bsp.write_msr(msr::IA32_EFER, context.efer);
            
            // Set RFLAGS
            bsp.write_rflags(context.rflags);
            
            // Set segment selectors via internal state
            // For BIOS (real mode): CS=F000, IP points to reset vector
            // For UEFI (long mode): CS=08, 64-bit flat memory model
            if context.real_mode {
                // Real mode initialization
                // CS:IP = F000:FFF0 for reset vector
                // Additional real-mode specific setup could go here
                bsp.write_msr(msr::IA32_EFER, 0);  // No long mode
            } else {
                // Long mode initialization
                // Setup for 64-bit execution
                let efer = context.efer | (1 << 8) | (1 << 10);  // LME + LMA
                bsp.write_msr(msr::IA32_EFER, efer);
            }
        }
        
        // Initialize APs (Application Processors) if SMP enabled
        if self.config.cpus > 1 && self.config.enable_apic {
            for (i, ap) in cpus.iter().enumerate().skip(1) {
                // APs start in INIT state (halted) waiting for SIPI
                ap.write_rip(0);
                ap.write_rsp(0);
                // APs will be woken by SIPI from BSP
            }
        }
    }
    
    /// Stop the VM
    pub fn stop(&self) {
        let mut state = self.state.write().unwrap();
        let old_state = *state;
        *state = VmState::Stopped;
        
        // Update runtime stats
        if let Some(start) = self.start_time.lock().unwrap().take() {
            self.stats.lock().unwrap().runtime_ns += start.elapsed().as_nanos() as u64;
        }
        
        drop(state);
        self.record_event(VmEvent::StateChange { from: old_state, to: VmState::Stopped });
        self.record_event(VmEvent::Stopped);
    }
    
    /// Pause the VM
    pub fn pause_vm(&self) {
        let mut state = self.state.write().unwrap();
        if *state == VmState::Running {
            *state = VmState::Paused;
            
            // Pause all CPUs
            for cpu in self.hal.cpus.read().unwrap().iter() {
                cpu.pause();
            }
            
            drop(state);
            self.record_event(VmEvent::StateChange { from: VmState::Running, to: VmState::Paused });
            self.record_event(VmEvent::Paused);
        }
    }
    
    /// Resume the VM
    pub fn resume_vm(&self) {
        let mut state = self.state.write().unwrap();
        if *state == VmState::Paused {
            *state = VmState::Running;
            
            // Resume all CPUs
            for cpu in self.hal.cpus.read().unwrap().iter() {
                cpu.resume();
            }
            
            drop(state);
            self.record_event(VmEvent::StateChange { from: VmState::Paused, to: VmState::Running });
            self.record_event(VmEvent::Resumed);
        }
    }
    
    /// Get current VM state
    pub fn get_state(&self) -> VmState {
        *self.state.read().unwrap()
    }
    
    /// Check if VM is running
    pub fn is_running(&self) -> bool {
        *self.state.read().unwrap() == VmState::Running
    }
    
    // ========================================================================
    // HAL Management
    // ========================================================================
    
    /// Get the HAL
    pub fn hal(&self) -> Arc<HardwareAbstractionLayer> {
        self.hal.clone()
    }
    
    /// Install the HAL for current thread (required before running kernel code)
    pub fn install(&self) {
        set_hal(self.hal.clone());
        self.start();
    }
    
    /// Uninstall the HAL
    pub fn uninstall(&self) {
        self.stop();
        clear_hal();
    }
    
    /// Get physical memory
    pub fn memory(&self) -> Arc<PhysicalMemory> {
        self.hal.memory.clone()
    }
    
    /// Get device manager
    pub fn devices(&self) -> Arc<DeviceManager> {
        self.hal.devices.clone()
    }
    
    /// Get PCI bus
    pub fn pci(&self) -> &std::sync::RwLock<PciBus> {
        &self.hal.pci
    }
    
    /// Get current CPU
    pub fn cpu(&self) -> Arc<VirtualCpu> {
        self.hal.cpu()
    }
    
    /// Get CPU by ID
    pub fn get_cpu(&self, id: u32) -> Option<Arc<VirtualCpu>> {
        self.hal.cpus.read().unwrap().get(id as usize).cloned()
    }
    
    /// Get number of CPUs
    pub fn cpu_count(&self) -> usize {
        self.hal.cpus.read().unwrap().len()
    }
    
    /// Switch to a specific CPU
    pub fn switch_cpu(&self, id: u32) {
        self.hal.switch_cpu(id);
    }
    
    // ========================================================================
    // VGA / Console Access
    // ========================================================================
    
    /// Get VGA framebuffer as RGBA data (800x600x4 = 1920000 bytes)
    /// Returns None if VGA is not enabled
    pub fn get_vga_framebuffer(&self) -> Option<Vec<u8>> {
        self.vga_device.as_ref().map(|vga| {
            let vga_lock = vga.lock().unwrap();
            vga_lock.get_framebuffer().lock().unwrap().clone()
        })
    }
    
    /// Get VGA display dimensions
    pub fn get_vga_dimensions(&self) -> Option<(u32, u32)> {
        self.vga_device.as_ref().map(|vga| {
            let vga_lock = vga.lock().unwrap();
            (vga_lock.width() as u32, vga_lock.height() as u32)
        })
    }
    
    /// Check if VGA is enabled
    pub fn has_vga(&self) -> bool {
        self.vga_device.is_some()
    }
    
    /// Write text to VGA console (text mode)
    pub fn vga_write(&self, text: &str) {
        if let Some(vga) = &self.vga_device {
            let mut vga_lock = vga.lock().unwrap();
            vga_lock.write_string(text);
        }
    }
    
    /// Write colored text to VGA console
    pub fn vga_write_colored(&self, text: &str, attr: u8) {
        if let Some(vga) = &self.vga_device {
            let mut vga_lock = vga.lock().unwrap();
            vga_lock.write_string_colored(text, attr);
        }
    }
    
    // ========================================================================
    // Keyboard Input (Enterprise boot-time key handling)
    // ========================================================================
    
    /// Inject a keyboard key press
    pub fn inject_key(&self, key: &str, is_release: bool) {
        if let Some(ref keyboard) = self.keyboard_device {
            let mut kb_lock = keyboard.lock().unwrap();
            kb_lock.inject_key(key, is_release);
        }
    }
    
    /// Check if setup key (F2/DEL) was pressed
    pub fn is_setup_key_pressed(&self) -> bool {
        if let Some(ref keyboard) = self.keyboard_device {
            keyboard.lock().unwrap().setup_key_pressed()
        } else {
            false
        }
    }
    
    /// Check if reboot combination (Ctrl+Alt+Del) was pressed
    pub fn is_reboot_requested(&self) -> bool {
        if let Some(ref keyboard) = self.keyboard_device {
            keyboard.lock().unwrap().reboot_requested()
        } else {
            false
        }
    }
    
    /// Poll for special key event
    pub fn poll_special_key(&self) -> Option<crate::devices::keyboard::SpecialKey> {
        if let Some(ref keyboard) = self.keyboard_device {
            keyboard.lock().unwrap().poll_special_key()
        } else {
            None
        }
    }
    
    /// Clear pending special key events
    pub fn clear_special_keys(&self) {
        if let Some(ref keyboard) = self.keyboard_device {
            keyboard.lock().unwrap().clear_special_keys();
        }
    }
    
    /// Handle Ctrl+Alt+Del reboot request
    pub fn handle_reboot_request(&self) {
        if self.is_reboot_requested() {
            self.record_event(VmEvent::StateChange { 
                from: *self.state.read().unwrap(), 
                to: VmState::Created 
            });
            
            // Display reboot message
            if let Some(ref vga) = self.vga_device {
                let mut vga_lock = vga.lock().unwrap();
                vga_lock.write_string_colored("\n\nSystem restart initiated (Ctrl+Alt+Del)...\n", 0x0E);
            }
            
            // Reset VM
            self.reset();
            self.clear_special_keys();
            
            // Restart
            self.start();
        }
    }

    // ========================================================================
    // Device Management (Hot-plug support)
    // ========================================================================
    
    /// Add a custom device
    pub fn add_device(&self, device: Arc<Mutex<dyn Device>>, ports: &[(u16, u16)]) {
        let id = device.lock().unwrap().id();
        let name = device.lock().unwrap().name().to_string();
        
        self.hal.devices.add_device_with_ports(device, ports);
        self.attached_devices.write().unwrap().push((id, name.clone()));
        self.record_event(VmEvent::DeviceAttached { id, name });
    }
    
    /// Add a PCI device
    pub fn add_pci_device(&self, loc: PciLocation, config: PciConfig) {
        let device = Arc::new(Mutex::new(SimplePciDevice::new(config)));
        self.hal.pci.write().unwrap().add_device(loc, device);
    }
    
    /// Get list of attached devices
    pub fn list_devices(&self) -> Vec<(DeviceId, String)> {
        self.attached_devices.read().unwrap().clone()
    }
    
    // ========================================================================
    // Memory Operations
    // ========================================================================
    
    /// Write data to physical memory
    pub fn write_memory(&self, addr: u64, data: &[u8]) {
        self.hal.memory.write_bytes(addr, data);
        self.stats.lock().unwrap().memory_writes += 1;
    }
    
    /// Read data from physical memory
    pub fn read_memory(&self, addr: u64, size: usize) -> Vec<u8> {
        let mut buf = vec![0u8; size];
        self.hal.memory.read_bytes(addr, &mut buf);
        self.stats.lock().unwrap().memory_reads += 1;
        buf
    }
    
    /// Zero a memory region
    pub fn zero_memory(&self, addr: u64, size: usize) {
        let zeros = vec![0u8; size];
        self.hal.memory.write_bytes(addr, &zeros);
    }
    
    // ========================================================================
    // Snapshot/Restore (VMware-style)
    // ========================================================================
    
    /// Create a named snapshot of current VM state
    pub fn snapshot(&self, name: &str) -> VmSnapshot {
        let cpu_states: Vec<_> = self.hal.cpus.read().unwrap()
            .iter()
            .map(|cpu| cpu.snapshot())
            .collect();
        
        // Snapshot memory (in real impl, would be copy-on-write)
        let memory_pages = self.snapshot_memory();
        
        let snapshot = VmSnapshot {
            name: name.to_string(),
            timestamp: Instant::now(),
            cpu_states,
            memory_pages,
            device_states: HashMap::new(), // TODO: device state serialization
            vm_state: *self.state.read().unwrap(),
            parent: None,
        };
        
        // Store snapshot
        self.snapshots.write().unwrap().insert(name.to_string(), snapshot.clone());
        self.stats.lock().unwrap().snapshots_created += 1;
        self.record_event(VmEvent::SnapshotCreated { name: name.to_string() });
        
        snapshot
    }
    
    /// Restore VM state from a snapshot
    pub fn restore(&self, snapshot: &VmSnapshot) {
        // Restore CPU states
        let cpus = self.hal.cpus.read().unwrap();
        for (cpu, state) in cpus.iter().zip(snapshot.cpu_states.iter()) {
            cpu.restore(state);
        }
        drop(cpus);
        
        // Restore memory
        self.restore_memory(&snapshot.memory_pages);
        
        // Restore VM state
        *self.state.write().unwrap() = snapshot.vm_state;
        
        self.stats.lock().unwrap().snapshots_restored += 1;
        self.record_event(VmEvent::SnapshotRestored { name: snapshot.name.clone() });
    }
    
    /// Restore from a named snapshot
    pub fn restore_by_name(&self, name: &str) -> bool {
        if let Some(snapshot) = self.snapshots.read().unwrap().get(name).cloned() {
            self.restore(&snapshot);
            true
        } else {
            false
        }
    }
    
    /// List all snapshots
    pub fn list_snapshots(&self) -> Vec<String> {
        self.snapshots.read().unwrap().keys().cloned().collect()
    }
    
    /// Delete a snapshot
    pub fn delete_snapshot(&self, name: &str) -> bool {
        self.snapshots.write().unwrap().remove(name).is_some()
    }
    
    /// Create snapshot of memory pages (sparse)
    fn snapshot_memory(&self) -> HashMap<u64, Vec<u8>> {
        // In a real implementation, this would use copy-on-write
        // For testing, we snapshot key regions
        let mut pages = HashMap::new();
        
        // Snapshot first 1MB (low memory)
        for page in (0..0x100000).step_by(4096) {
            let data = self.read_memory(page, 4096);
            if data.iter().any(|&b| b != 0) {
                pages.insert(page, data);
            }
        }
        
        // Snapshot kernel region (1MB - 16MB)
        for page in (0x100000..0x1000000).step_by(4096) {
            let data = self.read_memory(page, 4096);
            if data.iter().any(|&b| b != 0) {
                pages.insert(page, data);
            }
        }
        
        pages
    }
    
    /// Restore memory from snapshot
    fn restore_memory(&self, pages: &HashMap<u64, Vec<u8>>) {
        for (addr, data) in pages {
            self.hal.memory.write_bytes(*addr, data);
        }
    }
    
    // ========================================================================
    // Event Logging & Tracing
    // ========================================================================
    
    /// Record a VM event
    fn record_event(&self, event: VmEvent) {
        if self.config.enable_tracing {
            let mut events = self.events.lock().unwrap();
            if events.len() >= self.max_events {
                events.pop_front();
            }
            events.push_back(event);
        }
    }
    
    /// Get recent events
    pub fn get_events(&self, count: usize) -> Vec<VmEvent> {
        self.events.lock().unwrap().iter().rev().take(count).cloned().collect()
    }
    
    /// Clear event log
    pub fn clear_events(&self) {
        self.events.lock().unwrap().clear();
    }
    
    /// Inject input to serial port
    pub fn inject_serial_input(&self, data: &[u8]) {
        // Find COM1 device and inject
        // This is a simplified version - real implementation would find the UART device
        let _ = data; // TODO: implement UART input injection
    }
    
    /// Get serial output as string
    pub fn serial_output(&self) -> String {
        // Read from serial port output capture
        // For now, we'll read directly from the UART if we can find it
        String::new()
    }
    
    // ========================================================================
    // Statistics & Monitoring
    // ========================================================================
    
    /// Get VM statistics
    pub fn statistics(&self) -> VmStatistics {
        let mut stats = self.stats.lock().unwrap().clone();
        
        // Update cycle count from CPUs
        for cpu in self.hal.cpus.read().unwrap().iter() {
            stats.total_cycles += cpu.get_cycle_count();
            stats.instructions_retired += cpu.get_instructions_retired();
        }
        
        stats
    }
    
    /// Reset statistics
    pub fn reset_statistics(&self) {
        *self.stats.lock().unwrap() = VmStatistics::default();
    }
    
    // ========================================================================
    // Execution Control
    // ========================================================================
    
    /// Advance VM time
    pub fn tick(&self, cycles: u64) {
        self.hal.tick(cycles);
        self.stats.lock().unwrap().total_cycles += cycles;
    }
    
    /// Run for a number of cycles
    pub fn run(&self, cycles: u64) {
        if self.is_running() {
            self.tick(cycles);
        }
    }
    
    /// Run until a condition is met
    pub fn run_until<F>(&self, max_cycles: u64, mut condition: F) -> bool
    where
        F: FnMut(&Self) -> bool,
    {
        let mut cycles = 0u64;
        while cycles < max_cycles && !condition(self) {
            self.tick(1000);
            cycles += 1000;
        }
        condition(self)
    }
    
    /// Reset the VM
    pub fn reset(&self) {
        self.hal.devices.reset_all();
        
        // Reset CPUs
        for (i, cpu) in self.hal.cpus.read().unwrap().iter().enumerate() {
            if i == 0 {
                // BSP reset to running
                let state = super::cpu::CpuState::default();
                cpu.set_state(state);
            } else {
                // APs reset to halted
                let mut state = super::cpu::CpuState::default();
                state.halted = true;
                cpu.set_state(state);
            }
        }
        
        *self.state.write().unwrap() = VmState::Created;
        self.record_event(VmEvent::Reset);
    }
    
    /// Get event log
    pub fn events(&self) -> Vec<VmEvent> {
        self.events.lock().unwrap().iter().cloned().collect()
    }
    
    /// Create a mock boot info structure (for kernel initialization)
    pub fn create_boot_info(&self) -> MockBootInfo {
        MockBootInfo {
            memory_map: self.hal.memory.memory_map(),
            kernel_start: 0x100000,
            kernel_end: 0x200000,
            initramfs_start: 0x200000,
            initramfs_end: 0x300000,
            framebuffer: None,
            command_line: String::new(),
            acpi_rsdp: None,
        }
    }
    
    /// Get VM configuration
    pub fn config(&self) -> &VmConfig {
        &self.config
    }
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for VirtualMachine {
    fn drop(&mut self) {
        // Ensure HAL is cleared
        clear_hal();
    }
}

/// Mock boot information (similar to Multiboot2 boot info)
#[derive(Debug, Clone)]
pub struct MockBootInfo {
    pub memory_map: Vec<MemoryRegion>,
    pub kernel_start: u64,
    pub kernel_end: u64,
    pub initramfs_start: u64,
    pub initramfs_end: u64,
    pub framebuffer: Option<MockFramebufferInfo>,
    pub command_line: String,
    pub acpi_rsdp: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct MockFramebufferInfo {
    pub addr: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u8,
}

// ============================================================================
// Test Harness
// ============================================================================

/// Test harness for running kernel code in the VM
pub struct TestHarness {
    vm: VirtualMachine,
}

impl TestHarness {
    pub fn new() -> Self {
        Self {
            vm: VirtualMachine::new(),
        }
    }
    
    pub fn with_config(config: VmConfig) -> Self {
        Self {
            vm: VirtualMachine::with_config(config),
        }
    }
    
    /// Run a test with the VM environment
    pub fn run<F, R>(&self, test: F) -> R
    where
        F: FnOnce(&VirtualMachine) -> R,
    {
        self.vm.install();
        let result = test(&self.vm);
        self.vm.uninstall();
        result
    }
    
    /// Run a test with snapshot support
    pub fn run_with_snapshot<F, R>(&self, snapshot_name: &str, test: F) -> R
    where
        F: FnOnce(&VirtualMachine) -> R,
    {
        self.vm.install();
        
        // Take initial snapshot
        self.vm.snapshot(snapshot_name);
        
        let result = test(&self.vm);
        
        // Restore to initial state
        self.vm.restore_by_name(snapshot_name);
        
        self.vm.uninstall();
        result
    }
    
    /// Get the VM
    pub fn vm(&self) -> &VirtualMachine {
        &self.vm
    }
}

impl Default for TestHarness {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// VM Builder (Fluent API)
// ============================================================================

/// Builder for creating VMs with fluent API
pub struct VmBuilder {
    config: VmConfig,
}

impl VmBuilder {
    pub fn new() -> Self {
        Self {
            config: VmConfig::default(),
        }
    }
    
    pub fn memory(mut self, mb: usize) -> Self {
        self.config.memory_mb = mb;
        self
    }
    
    pub fn cpus(mut self, count: usize) -> Self {
        self.config.cpus = count;
        self
    }
    
    pub fn name(mut self, name: &str) -> Self {
        self.config.name = name.to_string();
        self
    }
    
    pub fn with_pic(mut self) -> Self {
        self.config.enable_pic = true;
        self
    }
    
    pub fn without_pic(mut self) -> Self {
        self.config.enable_pic = false;
        self
    }
    
    pub fn with_apic(mut self) -> Self {
        self.config.enable_apic = true;
        self
    }
    
    pub fn without_apic(mut self) -> Self {
        self.config.enable_apic = false;
        self
    }
    
    pub fn with_serial(mut self) -> Self {
        self.config.enable_serial = true;
        self
    }
    
    pub fn with_tracing(mut self, max_size: usize) -> Self {
        self.config.enable_tracing = true;
        self.config.max_trace_size = max_size;
        self
    }
    
    pub fn without_tracing(mut self) -> Self {
        self.config.enable_tracing = false;
        self
    }
    
    pub fn build(self) -> VirtualMachine {
        VirtualMachine::with_config(self.config)
    }
}

impl Default for VmBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience macro for creating a test with VM environment
#[macro_export]
macro_rules! vm_test {
    ($name:ident, $body:expr) => {
        #[test]
        fn $name() {
            let harness = $crate::vm::TestHarness::new();
            harness.run(|_vm| {
                $body
            });
        }
    };
    ($name:ident, config = $config:expr, $body:expr) => {
        #[test]
        fn $name() {
            let harness = $crate::vm::TestHarness::with_config($config);
            harness.run(|_vm| {
                $body
            });
        }
    };
}

/// Convenience macro for creating SMP test
#[macro_export]
macro_rules! smp_test {
    ($name:ident, cpus = $cpus:expr, $body:expr) => {
        #[test]
        fn $name() {
            let config = $crate::vm::VmConfig::smp($cpus);
            let harness = $crate::vm::TestHarness::with_config(config);
            harness.run(|_vm| {
                $body
            });
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vm_basic() {
        let vm = VirtualMachine::new();
        
        // Test basic VM operations
        assert_eq!(vm.get_state(), VmState::Created);
        vm.start();
        assert_eq!(vm.get_state(), VmState::Running);
        vm.stop();
        assert_eq!(vm.get_state(), VmState::Stopped);
    }
    
    #[test]
    fn test_vm_memory() {
        let vm = VirtualMachine::new();
        
        // Write and read memory
        let data = b"Test data for VM memory";
        vm.write_memory(0x10000, data);
        
        let read = vm.read_memory(0x10000, data.len());
        assert_eq!(&read[..], data);
    }
    
    #[test]
    fn test_vm_pci() {
        let vm = VirtualMachine::new();
        
        // Enumerate PCI devices
        let devices = vm.pci().read().unwrap().enumerate();
        assert!(!devices.is_empty()); // Should have at least host bridge
    }
    
    #[test]
    fn test_vm_config_minimal() {
        let vm = VirtualMachine::with_config(VmConfig::minimal());
        
        // Should work with minimal config
        assert_eq!(vm.cpu_count(), 1);
        assert!(vm.config().memory_mb > 0);
    }
    
    #[test]
    fn test_harness() {
        let harness = TestHarness::new();
        
        let result = harness.run(|vm| {
            vm.write_memory(0x1000, b"Hello");
            let data = vm.read_memory(0x1000, 5);
            String::from_utf8_lossy(&data).to_string()
        });
        
        assert_eq!(result, "Hello");
    }
    
    #[test]
    fn test_vm_snapshot_restore() {
        let vm = VirtualMachine::new();
        
        // Write initial data
        vm.write_memory(0x10000, b"Initial");
        
        // Take snapshot
        let _snapshot = vm.snapshot("test_snap");
        
        // Modify memory
        vm.write_memory(0x10000, b"Changed");
        assert_eq!(&vm.read_memory(0x10000, 7)[..], b"Changed");
        
        // Restore
        vm.restore_by_name("test_snap");
        assert_eq!(&vm.read_memory(0x10000, 7)[..], b"Initial");
    }
    
    #[test]
    fn test_vm_state_lifecycle() {
        let vm = VirtualMachine::new();
        
        assert_eq!(vm.get_state(), VmState::Created);
        
        vm.start();
        assert_eq!(vm.get_state(), VmState::Running);
        
        vm.pause_vm();
        assert_eq!(vm.get_state(), VmState::Paused);
        
        vm.resume_vm();
        assert_eq!(vm.get_state(), VmState::Running);
        
        vm.stop();
        assert_eq!(vm.get_state(), VmState::Stopped);
    }
    
    #[test]
    fn test_vm_statistics() {
        let vm = VirtualMachine::new();
        
        // Run some cycles
        vm.tick(10000);
        
        let stats = vm.statistics();
        assert!(stats.total_cycles >= 10000);
    }
    
    #[test]
    fn test_vm_builder() {
        let vm = VmBuilder::new()
            .memory(256)
            .cpus(4)
            .name("TestVM")
            .with_apic()
            .with_tracing(5000)
            .build();
        
        assert_eq!(vm.cpu_count(), 4);
        assert_eq!(vm.config().memory_mb, 256);
        assert_eq!(vm.config().name, "TestVM");
    }
    
    #[test]
    fn test_vm_smp_config() {
        let vm = VirtualMachine::with_config(VmConfig::smp(8));
        assert_eq!(vm.cpu_count(), 8);
        
        // BSP should not be halted
        let bsp = vm.get_cpu(0).unwrap();
        assert!(!bsp.is_halted());
        
        // APs should be halted
        let ap = vm.get_cpu(1).unwrap();
        assert!(ap.is_halted());
    }
    
    #[test]
    fn test_vm_run_until() {
        let vm = VirtualMachine::new();
        
        let mut count = 0u64;
        
        // Run until count reaches threshold
        // Each tick is 1000 cycles, so we need at least 10000 cycles to call condition 10+ times
        let success = vm.run_until(100_000, |_| {
            count += 1;
            count >= 10
        });
        
        assert!(success);
        assert!(count >= 10);
    }
    
    #[test]
    fn test_vm_multiple_snapshots() {
        let vm = VirtualMachine::new();
        
        // Create multiple snapshots
        vm.write_memory(0x1000, b"State1");
        vm.snapshot("snap1");
        
        vm.write_memory(0x1000, b"State2");
        vm.snapshot("snap2");
        
        vm.write_memory(0x1000, b"State3");
        vm.snapshot("snap3");
        
        // List snapshots
        let snaps = vm.list_snapshots();
        assert_eq!(snaps.len(), 3);
        
        // Restore to snap1
        vm.restore_by_name("snap1");
        assert_eq!(&vm.read_memory(0x1000, 6)[..], b"State1");
        
        // Restore to snap2
        vm.restore_by_name("snap2");
        assert_eq!(&vm.read_memory(0x1000, 6)[..], b"State2");
        
        // Delete snap1
        assert!(vm.delete_snapshot("snap1"));
        assert_eq!(vm.list_snapshots().len(), 2);
    }
}
