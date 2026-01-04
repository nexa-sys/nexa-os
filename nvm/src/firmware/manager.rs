//! Enterprise-Grade Firmware Manager for NVM Hypervisor
//!
//! This module provides ESXi-level firmware management capabilities:
//! - Unified BIOS/UEFI boot path
//! - Proper CPU state initialization for each firmware type
//! - Boot device enumeration and selection
//! - Firmware state machine with proper boot phases
//! - Hot-plug device support during boot
//!
//! ## Architecture (ESXi-style)
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────────┐
//! │                        FirmwareManager                                   │
//! ├──────────────────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────┐  ┌─────────────────┐  ┌────────────────────────┐   │
//! │  │  Boot Phase     │  │  CPU State      │  │   Boot Device          │   │
//! │  │  State Machine  │  │  Initializer    │  │   Manager              │   │
//! │  └─────────────────┘  └─────────────────┘  └────────────────────────┘   │
//! │                                                                          │
//! │  Boot Phases:                                                            │
//! │  ┌──────┐   ┌──────┐   ┌──────┐   ┌──────┐   ┌──────┐   ┌──────┐       │
//! │  │Reset │ → │POST  │ → │Init  │ → │Boot  │ → │Load  │ → │Run   │       │
//! │  │      │   │      │   │Devs  │   │Select│   │      │   │      │       │
//! │  └──────┘   └──────┘   └──────┘   └──────┘   └──────┘   └──────┘       │
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```

use super::{
    Firmware, FirmwareType, FirmwareLoadResult, FirmwareError, FirmwareResult,
    Bios, BiosConfig, UefiFirmware, UefiConfig, ServiceRegisters,
};
use std::sync::{Arc, Mutex, RwLock};
use std::collections::HashMap;

/// Boot phase in the firmware lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootPhase {
    /// Initial state after power-on/reset
    Reset,
    /// Power-On Self-Test in progress
    PostInProgress,
    /// POST completed successfully
    PostComplete,
    /// Initializing boot devices
    InitDevices,
    /// Selecting boot device
    BootSelect,
    /// Loading boot loader/OS
    Loading,
    /// Boot handoff complete, OS running
    Running,
    /// Boot failed
    Failed,
}

impl Default for BootPhase {
    fn default() -> Self {
        Self::Reset
    }
}

/// Firmware state for monitoring and debugging
#[derive(Debug, Clone)]
pub struct FirmwareState {
    /// Current boot phase
    pub phase: BootPhase,
    /// POST code (0x00-0xFF)
    pub post_code: u8,
    /// POST error code (0 = no error)
    pub post_error: u16,
    /// Current boot attempt number
    pub boot_attempt: u32,
    /// Selected boot device index
    pub boot_device_index: Option<usize>,
    /// Firmware type in use
    pub firmware_type: FirmwareType,
    /// Memory detected (KB)
    pub memory_detected_kb: u64,
    /// CPU count detected
    pub cpu_count: u32,
    /// Boot time (milliseconds since reset)
    pub boot_time_ms: u64,
}

impl Default for FirmwareState {
    fn default() -> Self {
        Self {
            phase: BootPhase::Reset,
            post_code: 0x00,
            post_error: 0,
            boot_attempt: 0,
            boot_device_index: None,
            firmware_type: FirmwareType::Bios,
            memory_detected_kb: 0,
            cpu_count: 1,
            boot_time_ms: 0,
        }
    }
}

/// Boot device descriptor
#[derive(Debug, Clone)]
pub struct BootDevice {
    /// Device identifier
    pub id: String,
    /// Device type (hdd, cdrom, network, etc.)
    pub device_type: BootDeviceType,
    /// Device priority (lower = higher priority)
    pub priority: u32,
    /// Device is bootable
    pub bootable: bool,
    /// Device description
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootDeviceType {
    HardDisk,
    CdRom,
    Floppy,
    Network,
    Usb,
    Nvme,
}

/// CPU initialization context for firmware handoff
#[derive(Debug, Clone)]
pub struct FirmwareBootContext {
    /// Entry point address (RIP for 64-bit, IP for 16-bit)
    pub entry_point: u64,
    /// Stack pointer
    pub stack_pointer: u64,
    /// Code segment (CS)
    pub code_segment: u16,
    /// Data segment (DS = ES = SS for BIOS)
    pub data_segment: u16,
    /// Whether to start in real mode (BIOS) or protected/long mode (UEFI)
    pub real_mode: bool,
    /// Control registers
    pub cr0: u64,
    pub cr3: u64,
    pub cr4: u64,
    /// EFER MSR value
    pub efer: u64,
    /// Initial RFLAGS
    pub rflags: u64,
    /// GDT base (for UEFI)
    pub gdt_base: u64,
    pub gdt_limit: u16,
    /// IDT base (for UEFI)
    pub idt_base: u64,
    pub idt_limit: u16,
}

impl Default for FirmwareBootContext {
    fn default() -> Self {
        Self {
            entry_point: 0xFFFF0,
            stack_pointer: 0x7C00,
            code_segment: 0xF000,
            data_segment: 0x0000,
            real_mode: true,
            cr0: 0x0000_0010,  // ET bit set (x87 present)
            cr3: 0,
            cr4: 0,
            efer: 0,
            rflags: 0x0000_0002,  // Reserved bit 1 always set
            gdt_base: 0,
            gdt_limit: 0,
            idt_base: 0,
            idt_limit: 0x3FF,  // Real mode IVT limit
        }
    }
}

/// Enterprise-grade Firmware Manager
/// 
/// Manages the complete boot process similar to ESXi/vSphere:
/// - Unified BIOS/UEFI management
/// - Proper CPU state initialization
/// - Boot device enumeration
/// - State machine for boot phases
pub struct FirmwareManager {
    /// Active firmware instance
    firmware: Mutex<Option<Box<dyn Firmware>>>,
    /// Firmware configuration
    firmware_type: RwLock<FirmwareType>,
    /// Current firmware state
    state: RwLock<FirmwareState>,
    /// Boot devices
    boot_devices: RwLock<Vec<BootDevice>>,
    /// Boot sector cache (for quick retry)
    boot_sector_cache: Mutex<Option<[u8; 512]>>,
    /// POST code history
    post_history: Mutex<Vec<(u8, u64)>>,  // (code, timestamp_ms)
    /// Error log
    error_log: Mutex<Vec<String>>,
}

impl FirmwareManager {
    /// Create a new firmware manager
    pub fn new(firmware_type: FirmwareType) -> Self {
        Self {
            firmware: Mutex::new(None),
            firmware_type: RwLock::new(firmware_type),
            state: RwLock::new(FirmwareState {
                firmware_type,
                ..Default::default()
            }),
            boot_devices: RwLock::new(Vec::new()),
            boot_sector_cache: Mutex::new(None),
            post_history: Mutex::new(Vec::new()),
            error_log: Mutex::new(Vec::new()),
        }
    }
    
    /// Initialize firmware for the specified memory size
    pub fn initialize(&self, memory_mb: usize, cpu_count: u32) -> FirmwareResult<()> {
        let firmware_type = *self.firmware_type.read().unwrap();
        
        // Update state
        {
            let mut state = self.state.write().unwrap();
            state.phase = BootPhase::Reset;
            state.memory_detected_kb = (memory_mb * 1024) as u64;
            state.cpu_count = cpu_count;
            state.boot_attempt = 0;
        }
        
        // Create firmware instance
        let firmware: Box<dyn Firmware> = match firmware_type {
            FirmwareType::Bios => {
                let config = BiosConfig {
                    memory_kb: std::cmp::min(memory_mb * 1024, 640) as u32,
                    extended_memory_kb: ((memory_mb * 1024) as u32).saturating_sub(1024),
                    num_hard_disks: 1,
                    num_floppies: 0,
                    boot_order: vec![0x80, 0x00],
                    serial_enabled: true,
                    com_port: 0x3F8,
                };
                Box::new(Bios::new(config))
            }
            FirmwareType::Uefi | FirmwareType::UefiSecure => {
                let config = UefiConfig {
                    memory_mb: memory_mb as u64,
                    secure_boot: matches!(firmware_type, FirmwareType::UefiSecure),
                    boot_path: String::from("\\EFI\\BOOT\\BOOTX64.EFI"),
                    fb_width: 1024,
                    fb_height: 768,
                    variables: HashMap::new(),
                };
                Box::new(UefiFirmware::new(config))
            }
        };
        
        *self.firmware.lock().unwrap() = Some(firmware);
        self.post_code(0x01);  // Power-on
        
        Ok(())
    }
    
    /// Load firmware into guest memory and get CPU boot context
    pub fn load_firmware(&self, memory: &mut [u8]) -> FirmwareResult<FirmwareBootContext> {
        // Phase: POST in progress
        self.set_phase(BootPhase::PostInProgress);
        self.post_code(0x10);  // Memory test start
        
        // Get firmware and load
        let mut firmware_guard = self.firmware.lock().unwrap();
        let firmware = firmware_guard.as_mut()
            .ok_or_else(|| FirmwareError::InitializationFailed("Firmware not initialized".into()))?;
        
        self.post_code(0x20);  // CPU init
        
        let result = firmware.load(memory)?;
        
        self.post_code(0x30);  // POST complete
        self.set_phase(BootPhase::PostComplete);
        
        // Create boot context based on firmware type
        let firmware_type = *self.firmware_type.read().unwrap();
        let context = self.create_boot_context(firmware_type, &result);
        
        // Phase: Init devices
        self.set_phase(BootPhase::InitDevices);
        self.post_code(0x40);  // PCI enumeration
        
        // Add default boot device
        self.add_boot_device(BootDevice {
            id: "hd0".into(),
            device_type: BootDeviceType::HardDisk,
            priority: 0,
            bootable: true,
            description: "Primary Hard Disk".into(),
        });
        
        self.post_code(0x50);  // Boot device init complete
        self.set_phase(BootPhase::BootSelect);
        
        // Select boot device
        {
            let mut state = self.state.write().unwrap();
            state.boot_device_index = Some(0);
            state.boot_attempt += 1;
        }
        
        self.post_code(0x60);  // Loading boot sector
        self.set_phase(BootPhase::Loading);
        
        Ok(context)
    }
    
    /// Create CPU boot context for the specified firmware type
    fn create_boot_context(&self, firmware_type: FirmwareType, result: &FirmwareLoadResult) -> FirmwareBootContext {
        match firmware_type {
            FirmwareType::Bios => {
                // Real mode boot context (16-bit)
                FirmwareBootContext {
                    entry_point: result.entry_point,
                    stack_pointer: result.stack_pointer,
                    code_segment: result.code_segment,
                    data_segment: 0x0000,
                    real_mode: true,
                    // Real mode CR0: no paging, no protected mode
                    cr0: 0x0000_0010,  // ET bit only
                    cr3: 0,
                    cr4: 0,
                    efer: 0,
                    rflags: 0x0000_0002,  // Reserved bit 1
                    gdt_base: 0,
                    gdt_limit: 0,
                    idt_base: 0,
                    idt_limit: 0x3FF,  // Real mode IVT: 256 * 4 bytes - 1
                }
            }
            FirmwareType::Uefi | FirmwareType::UefiSecure => {
                // 64-bit long mode boot context
                FirmwareBootContext {
                    entry_point: result.entry_point,
                    stack_pointer: result.stack_pointer,
                    code_segment: 0x08,  // 64-bit code segment selector
                    data_segment: 0x10,  // 64-bit data segment selector
                    real_mode: false,
                    // Long mode CR0: PE + PG + ET + NE + WP + AM
                    cr0: 0x8005_0033,
                    cr3: 0x0010_0000,  // Page tables at 1MB
                    // CR4: PAE + PGE + OSFXSR + OSXMMEXCPT + OSXSAVE
                    cr4: 0x0000_06A0,
                    // EFER: LME + LMA + SCE + NXE
                    efer: 0x0000_0D01,
                    rflags: 0x0000_0002,
                    gdt_base: 0x0008_0000,  // GDT at 512KB
                    gdt_limit: 0x2F,        // 6 entries (null + code64 + data64 + code32 + data32 + tss)
                    idt_base: 0x0008_1000,  // IDT at 512KB + 4KB
                    idt_limit: 0x0FFF,      // 256 entries * 16 bytes
                }
            }
        }
    }
    
    /// Handle firmware service call
    pub fn handle_service(&self, memory: &mut [u8], regs: &mut ServiceRegisters) -> FirmwareResult<()> {
        let mut firmware_guard = self.firmware.lock().unwrap();
        if let Some(ref mut firmware) = *firmware_guard {
            firmware.handle_service(memory, regs)
        } else {
            Err(FirmwareError::InitializationFailed("Firmware not loaded".into()))
        }
    }
    
    /// Add a boot device
    pub fn add_boot_device(&self, device: BootDevice) {
        let mut devices = self.boot_devices.write().unwrap();
        devices.push(device);
        // Sort by priority
        devices.sort_by_key(|d| d.priority);
    }
    
    /// Get current boot phase
    pub fn get_phase(&self) -> BootPhase {
        self.state.read().unwrap().phase
    }
    
    /// Set boot phase
    fn set_phase(&self, phase: BootPhase) {
        self.state.write().unwrap().phase = phase;
    }
    
    /// Update POST code
    pub fn post_code(&self, code: u8) {
        let mut state = self.state.write().unwrap();
        state.post_code = code;
        drop(state);
        
        let mut history = self.post_history.lock().unwrap();
        history.push((code, 0));  // TODO: Add actual timestamp
    }
    
    /// Get firmware state
    pub fn get_state(&self) -> FirmwareState {
        self.state.read().unwrap().clone()
    }
    
    /// Get POST code history
    pub fn get_post_history(&self) -> Vec<(u8, u64)> {
        self.post_history.lock().unwrap().clone()
    }
    
    /// Reset firmware
    pub fn reset(&self) {
        if let Some(ref mut firmware) = *self.firmware.lock().unwrap() {
            firmware.reset();
        }
        
        let mut state = self.state.write().unwrap();
        state.phase = BootPhase::Reset;
        state.post_code = 0x00;
        state.post_error = 0;
        
        self.post_history.lock().unwrap().clear();
    }
    
    /// Mark boot as complete
    pub fn boot_complete(&self) {
        self.set_phase(BootPhase::Running);
        self.post_code(0x00);  // POST complete, running
    }
    
    /// Mark boot as failed
    pub fn boot_failed(&self, error: &str) {
        self.set_phase(BootPhase::Failed);
        self.error_log.lock().unwrap().push(error.to_string());
    }
    
    /// Get firmware version string
    pub fn version(&self) -> String {
        if let Some(ref firmware) = *self.firmware.lock().unwrap() {
            firmware.version().to_string()
        } else {
            "Not initialized".to_string()
        }
    }
    
    /// Set boot sector data (for testing without disk)
    pub fn set_boot_sector(&self, data: &[u8]) {
        if data.len() >= 512 {
            let mut sector = [0u8; 512];
            sector.copy_from_slice(&data[..512]);
            *self.boot_sector_cache.lock().unwrap() = Some(sector);
        }
    }
    
    /// Get available boot devices
    pub fn get_boot_devices(&self) -> Vec<BootDevice> {
        self.boot_devices.read().unwrap().clone()
    }
    
    /// Get firmware type
    pub fn get_firmware_type(&self) -> FirmwareType {
        *self.firmware_type.read().unwrap()
    }
    
    /// Switch firmware type (requires re-initialization)
    pub fn set_firmware_type(&self, firmware_type: FirmwareType) {
        *self.firmware_type.write().unwrap() = firmware_type;
        *self.firmware.lock().unwrap() = None;
    }
}

impl Default for FirmwareManager {
    fn default() -> Self {
        Self::new(FirmwareType::Bios)
    }
}

/// POST code descriptions (for debugging)
pub fn post_code_description(code: u8) -> &'static str {
    match code {
        0x00 => "System running / POST complete",
        0x01 => "Power-on",
        0x02 => "CPU init",
        0x03 => "CPU test",
        0x10 => "Memory test start",
        0x11 => "Memory test in progress",
        0x12 => "Memory test complete",
        0x20 => "CPU mode switch",
        0x21 => "CPU protected mode",
        0x22 => "CPU long mode",
        0x30 => "POST complete",
        0x40 => "PCI enumeration",
        0x41 => "PCI device init",
        0x50 => "Boot device init",
        0x51 => "Boot device ready",
        0x60 => "Loading boot sector",
        0x61 => "Boot sector loaded",
        0x62 => "Executing boot loader",
        0x70 => "Loading OS kernel",
        0x80..=0x8F => "System init",
        0x90..=0x9F => "Device drivers loading",
        0xA0..=0xAF => "Services starting",
        0xE0..=0xEF => "Error detected",
        0xFF => "Fatal error",
        _ => "Unknown POST code",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_firmware_manager_creation() {
        let manager = FirmwareManager::new(FirmwareType::Bios);
        assert_eq!(manager.get_firmware_type(), FirmwareType::Bios);
        assert_eq!(manager.get_phase(), BootPhase::Reset);
    }
    
    #[test]
    fn test_firmware_initialization() {
        let manager = FirmwareManager::new(FirmwareType::Bios);
        manager.initialize(64, 1).unwrap();
        
        let state = manager.get_state();
        assert_eq!(state.memory_detected_kb, 64 * 1024);
        assert_eq!(state.cpu_count, 1);
    }
    
    #[test]
    fn test_bios_boot_context() {
        let manager = FirmwareManager::new(FirmwareType::Bios);
        manager.initialize(64, 1).unwrap();
        
        let mut memory = vec![0u8; 64 * 1024 * 1024];
        let context = manager.load_firmware(&mut memory).unwrap();
        
        assert!(context.real_mode);
        assert_eq!(context.code_segment, 0xF000);
        assert_eq!(context.entry_point, 0xFFFF0);
    }
    
    #[test]
    fn test_uefi_boot_context() {
        let manager = FirmwareManager::new(FirmwareType::Uefi);
        manager.initialize(256, 1).unwrap();
        
        let mut memory = vec![0u8; 256 * 1024 * 1024];
        let context = manager.load_firmware(&mut memory).unwrap();
        
        assert!(!context.real_mode);
        assert_eq!(context.code_segment, 0x08);
        // EFER should have LME and LMA set
        assert!(context.efer & 0x0100 != 0);  // LME
        assert!(context.efer & 0x0400 != 0);  // LMA
    }
    
    #[test]
    fn test_post_codes() {
        let manager = FirmwareManager::new(FirmwareType::Bios);
        manager.initialize(64, 1).unwrap();
        
        let state = manager.get_state();
        assert_eq!(state.post_code, 0x01);  // Power-on code
    }
    
    #[test]
    fn test_boot_device_management() {
        let manager = FirmwareManager::new(FirmwareType::Bios);
        
        manager.add_boot_device(BootDevice {
            id: "hd0".into(),
            device_type: BootDeviceType::HardDisk,
            priority: 1,
            bootable: true,
            description: "Hard Disk".into(),
        });
        
        manager.add_boot_device(BootDevice {
            id: "cdrom0".into(),
            device_type: BootDeviceType::CdRom,
            priority: 0,
            bootable: true,
            description: "CD-ROM".into(),
        });
        
        let devices = manager.get_boot_devices();
        assert_eq!(devices.len(), 2);
        // Should be sorted by priority
        assert_eq!(devices[0].id, "cdrom0");
        assert_eq!(devices[1].id, "hd0");
    }
}
