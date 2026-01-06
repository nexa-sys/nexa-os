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
/// 
/// Aligned with UEFI PI specification phases:
/// - SEC (Security): Trust root establishment, CAR (Cache-as-RAM)
/// - PEI (Pre-EFI Initialization): Memory discovery, chipset init
/// - DXE (Driver Execution Environment): Driver loading, protocol installation
/// - BDS (Boot Device Selection): Boot menu, OS loader selection
/// - RT (Runtime): OS running, UEFI runtime services only
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootPhase {
    // ==================== UEFI SEC Phase ====================
    /// Initial state after power-on/reset (CPU at reset vector 0xFFFF0)
    Reset,
    /// SEC: CPU initialization, CAR setup (16-bit real mode)
    SecCpuInit,
    /// SEC: Trust root validation (Secure Boot key verification)
    SecTrustRoot,
    
    // ==================== UEFI PEI Phase ====================
    /// PEI: Entering protected mode, GDT/IDT setup
    PeiProtectedMode,
    /// PEI: Memory controller initialization
    PeiMemoryInit,
    /// PEI: Memory discovery and testing
    PeiMemoryDiscovery,
    /// PEI: HOB (Hand-Off Block) list creation
    PeiHobCreation,
    
    // ==================== UEFI DXE Phase ====================
    /// DXE: Entering long mode (64-bit)
    DxeLongMode,
    /// DXE: Loading DXE dispatcher
    DxeDispatcher,
    /// DXE: Loading architectural protocols
    DxeArchProtocols,
    /// DXE: VGA/GOP initialization (UI visible from here)
    DxeGopInit,
    /// DXE: Loading platform drivers
    DxePlatformDrivers,
    /// DXE: Console initialization
    DxeConsoleInit,
    /// DXE: PCI enumeration
    DxePciEnumeration,
    /// DXE: Storage driver loading
    DxeStorageDrivers,
    /// DXE: Network driver loading
    DxeNetworkDrivers,
    
    // ==================== UEFI BDS Phase ====================
    /// BDS: Waiting for user input (F2/DEL/F12 timeout)
    BdsWaitingForSetupKey,
    /// BDS: Setup menu active
    BdsSetupMenu,
    /// BDS: Boot device enumeration
    BdsDeviceEnumeration,
    /// BDS: Boot device selection menu
    BdsBootMenu,
    /// BDS: Loading OS loader (BOOTX64.EFI)
    BdsLoadingOsLoader,
    /// BDS: ExitBootServices called - transition to RT
    BdsExitBootServices,
    
    // ==================== UEFI RT Phase ====================
    /// RT: OS running, only runtime services available
    RtRunning,
    /// RT: Runtime services SetVirtualAddressMap called
    RtVirtualMode,
    
    // ==================== Error/Legacy States ====================
    /// No bootable device found
    NoBootableDevice,
    /// Boot failed with error
    Failed,
    
    // ==================== Legacy BIOS Compatibility ====================
    /// Power-On Self-Test in progress (Legacy BIOS)
    PostInProgress,
    /// POST completed successfully (Legacy BIOS)
    PostComplete,
    /// Initializing boot devices (Legacy BIOS)
    InitDevices,
    /// Waiting for user input (Legacy BIOS)
    WaitingForSetupKey,
    /// BIOS/UEFI setup menu (Legacy BIOS)
    SetupMenu,
    /// Selecting boot device (Legacy BIOS)
    BootSelect,
    /// Loading boot loader/OS (Legacy BIOS)
    Loading,
    /// Boot handoff complete, OS running (Legacy BIOS)
    Running,
}

/// Boot menu state for enterprise BIOS/UEFI setup
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootMenuState {
    /// Main menu
    Main,
    /// Boot order configuration
    BootOrder,
    /// Date/Time settings
    DateTime,
    /// Security settings
    Security,
    /// Exit menu
    Exit,
}

/// Key codes for setup menu navigation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupKey {
    F2,
    Delete,
    Escape,
    Enter,
    Up,
    Down,
    F10,  // Save & Exit
    CtrlAltDel,
}

impl Default for BootPhase {
    fn default() -> Self {
        Self::Reset
    }
}

impl BootPhase {
    /// Get the UEFI PI specification phase name
    pub fn uefi_phase_name(&self) -> &'static str {
        match self {
            Self::Reset | Self::SecCpuInit | Self::SecTrustRoot => "SEC",
            Self::PeiProtectedMode | Self::PeiMemoryInit | 
            Self::PeiMemoryDiscovery | Self::PeiHobCreation => "PEI",
            Self::DxeLongMode | Self::DxeDispatcher | Self::DxeArchProtocols |
            Self::DxeGopInit | Self::DxePlatformDrivers | Self::DxeConsoleInit |
            Self::DxePciEnumeration | Self::DxeStorageDrivers | Self::DxeNetworkDrivers => "DXE",
            Self::BdsWaitingForSetupKey | Self::BdsSetupMenu | Self::BdsDeviceEnumeration |
            Self::BdsBootMenu | Self::BdsLoadingOsLoader | Self::BdsExitBootServices => "BDS",
            Self::RtRunning | Self::RtVirtualMode => "RT",
            Self::NoBootableDevice | Self::Failed => "ERROR",
            // Legacy BIOS phases map to approximate UEFI equivalents
            Self::PostInProgress | Self::PostComplete => "POST",
            Self::InitDevices => "DXE",
            Self::WaitingForSetupKey | Self::SetupMenu | Self::BootSelect => "BDS",
            Self::Loading | Self::Running => "RT",
        }
    }
    
    /// Check if this phase supports UI display (GOP available)
    pub fn has_ui_display(&self) -> bool {
        matches!(self,
            Self::DxeGopInit | Self::DxePlatformDrivers | Self::DxeConsoleInit |
            Self::DxePciEnumeration | Self::DxeStorageDrivers | Self::DxeNetworkDrivers |
            Self::BdsWaitingForSetupKey | Self::BdsSetupMenu | Self::BdsDeviceEnumeration |
            Self::BdsBootMenu | Self::BdsLoadingOsLoader | Self::BdsExitBootServices |
            Self::NoBootableDevice | Self::Failed |
            // Legacy phases with display
            Self::WaitingForSetupKey | Self::SetupMenu | Self::BootSelect | Self::Loading
        )
    }
    
    /// Check if this is a good point for JIT state snapshot (NReady!)
    pub fn is_jit_snapshot_point(&self) -> bool {
        matches!(self,
            Self::SecTrustRoot |      // After SEC, before PEI (initial state)
            Self::PeiHobCreation |    // After memory init (memory layout known)
            Self::DxeArchProtocols |  // After core protocols (stable environment)
            Self::BdsExitBootServices // Before OS handoff (final firmware state)
        )
    }
    
    /// Get progress percentage (0-100) for progress bar display
    pub fn progress_percent(&self) -> u8 {
        match self {
            Self::Reset => 0,
            Self::SecCpuInit => 2,
            Self::SecTrustRoot => 5,
            Self::PeiProtectedMode => 8,
            Self::PeiMemoryInit => 12,
            Self::PeiMemoryDiscovery => 18,
            Self::PeiHobCreation => 22,
            Self::DxeLongMode => 25,
            Self::DxeDispatcher => 30,
            Self::DxeArchProtocols => 38,
            Self::DxeGopInit => 45,
            Self::DxePlatformDrivers => 55,
            Self::DxeConsoleInit => 60,
            Self::DxePciEnumeration => 70,
            Self::DxeStorageDrivers => 80,
            Self::DxeNetworkDrivers => 88,
            Self::BdsWaitingForSetupKey => 92,
            Self::BdsSetupMenu => 92,
            Self::BdsDeviceEnumeration => 95,
            Self::BdsBootMenu => 95,
            Self::BdsLoadingOsLoader => 98,
            Self::BdsExitBootServices => 99,
            Self::RtRunning | Self::RtVirtualMode => 100,
            // Legacy phases
            Self::PostInProgress => 10,
            Self::PostComplete => 25,
            Self::InitDevices => 50,
            Self::WaitingForSetupKey => 90,
            Self::SetupMenu => 90,
            Self::BootSelect => 95,
            Self::Loading => 98,
            Self::Running => 100,
            Self::NoBootableDevice | Self::Failed => 100,
        }
    }
}

/// UEFI boot phase UI callback trait
/// 
/// Implement this trait to receive UI updates during UEFI boot.
/// This enables custom boot screen rendering (progress bars, logos, etc.)
pub trait UefiBootUiCallback: Send + Sync {
    /// Called when boot phase changes
    fn on_phase_change(&self, old_phase: BootPhase, new_phase: BootPhase);
    
    /// Called to update progress bar (0-100)
    fn on_progress_update(&self, percent: u8, message: &str);
    
    /// Called when a driver is loaded during DXE
    fn on_driver_loaded(&self, driver_name: &str, load_time_us: u64);
    
    /// Called when boot timeout ticks (for countdown display)
    fn on_timeout_tick(&self, remaining_secs: u8);
    
    /// Called when entering setup menu
    fn on_setup_enter(&self);
    
    /// Called before ExitBootServices (chance to save JIT state)
    fn on_exit_boot_services(&self);
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
    /// Boot timeout remaining (seconds)
    pub boot_timeout_secs: u8,
    /// Whether setup key was pressed
    pub setup_requested: bool,
    /// Boot menu state (if in setup)
    pub menu_state: Option<BootMenuState>,
    /// Last error message
    pub last_error: Option<String>,
    /// Whether Ctrl+Alt+Del was pressed
    pub reboot_requested: bool,
    /// UEFI-specific: Current DXE driver being loaded
    pub current_driver: Option<String>,
    /// UEFI-specific: DXE driver load times (name -> microseconds)
    pub driver_load_times: Vec<(String, u64)>,
    /// JIT snapshot taken at this phase
    pub jit_snapshot_phase: Option<BootPhase>,
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
            boot_timeout_secs: 3,  // ESXi-style 3 second timeout
            setup_requested: false,
            menu_state: None,
            last_error: None,
            reboot_requested: false,
            current_driver: None,
            driver_load_times: Vec::new(),
            jit_snapshot_phase: None,
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
            rflags: 0x0000_0202,  // Reserved bit 1 + IF (enable interrupts)
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
    /// UI callback for boot progress (optional)
    ui_callback: RwLock<Option<Arc<dyn UefiBootUiCallback>>>,
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
            ui_callback: RwLock::new(None),
        }
    }
    
    /// Set UI callback for boot progress updates
    pub fn set_ui_callback(&self, callback: Arc<dyn UefiBootUiCallback>) {
        *self.ui_callback.write().unwrap() = Some(callback);
    }
    
    /// Clear UI callback
    pub fn clear_ui_callback(&self) {
        *self.ui_callback.write().unwrap() = None;
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
                    cpu_count,
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
                    cpu_count,
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
        
        // NOTE: Do NOT add fake boot devices here!
        // The VM should call add_boot_device() only for actually attached disks
        // This allows proper "No bootable device" detection
        
        self.post_code(0x50);  // Device enumeration complete
        
        // Phase: Wait for setup key (2-3 second timeout for F2/DEL)
        self.set_phase(BootPhase::WaitingForSetupKey);
        
        // Initialize boot timeout countdown
        {
            let mut state = self.state.write().unwrap();
            state.boot_timeout_secs = 3;  // Enterprise standard: 3 seconds
        }
        
        // Boot device selection happens after setup key wait timeout
        // or when user presses a boot key
        
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
                // UEFI starts in real mode at reset vector, just like BIOS
                // The firmware code itself handles the transition:
                // Real Mode (SEC) -> Protected Mode (PEI) -> Long Mode (DXE)
                FirmwareBootContext {
                    entry_point: result.entry_point,  // 0xFFFF0 (reset vector)
                    stack_pointer: result.stack_pointer,
                    code_segment: result.code_segment, // 0xF000 (real mode)
                    data_segment: 0x0000,
                    real_mode: result.real_mode,       // true - starts in real mode
                    // Real mode CR0: no paging, no protected mode
                    cr0: 0x0000_0010,  // ET bit only
                    cr3: 0,
                    cr4: 0,
                    efer: 0,
                    rflags: 0x0000_0002,  // Reserved bit 1
                    gdt_base: 0,
                    gdt_limit: 0,
                    idt_base: 0,
                    idt_limit: 0x3FF,  // Real mode IVT
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
    
    /// Proceed to boot after setup key wait timeout
    /// Returns true if boot can proceed, false if no bootable device
    pub fn proceed_to_boot(&self) -> bool {
        if !self.has_bootable_device() {
            self.set_no_bootable_device();
            return false;
        }
        
        // Select first bootable device
        {
            let mut state = self.state.write().unwrap();
            state.boot_device_index = Some(0);
            state.boot_attempt += 1;
        }
        
        self.post_code(0x60);  // Loading boot sector
        self.set_phase(BootPhase::Loading);
        
        true
    }
    
    /// Check if we're in setup key wait phase
    pub fn is_waiting_for_setup_key(&self) -> bool {
        matches!(self.get_phase(), BootPhase::WaitingForSetupKey)
    }
    
    /// Enter BIOS/UEFI setup
    pub fn enter_setup(&self) {
        let mut state = self.state.write().unwrap();
        state.phase = BootPhase::SetupMenu;
        state.setup_requested = true;
        state.menu_state = Some(BootMenuState::Main);
    }
    
    /// Get current boot phase
    pub fn get_phase(&self) -> BootPhase {
        self.state.read().unwrap().phase
    }
    
    /// Set boot phase with UI callback notification
    fn set_phase(&self, phase: BootPhase) {
        let old_phase = {
            let mut state = self.state.write().unwrap();
            let old = state.phase;
            state.phase = phase;
            old
        };
        
        // Notify UI callback
        if let Some(ref callback) = *self.ui_callback.read().unwrap() {
            callback.on_phase_change(old_phase, phase);
            callback.on_progress_update(phase.progress_percent(), phase.uefi_phase_name());
        }
        
        // Check if this is a JIT snapshot point
        if phase.is_jit_snapshot_point() {
            let mut state = self.state.write().unwrap();
            state.jit_snapshot_phase = Some(phase);
            log::info!("[Firmware] JIT snapshot point: {:?} ({})", phase, phase.uefi_phase_name());
        }
    }
    
    /// Set boot phase for UEFI with detailed sub-phase tracking
    pub fn set_uefi_phase(&self, phase: BootPhase, driver_name: Option<&str>) {
        // Update driver tracking
        if let Some(name) = driver_name {
            let mut state = self.state.write().unwrap();
            state.current_driver = Some(name.to_string());
        }
        
        self.set_phase(phase);
    }
    
    /// Record DXE driver load time
    pub fn record_driver_load(&self, driver_name: &str, load_time_us: u64) {
        {
            let mut state = self.state.write().unwrap();
            state.driver_load_times.push((driver_name.to_string(), load_time_us));
            state.current_driver = None;
        }
        
        // Notify UI callback
        if let Some(ref callback) = *self.ui_callback.read().unwrap() {
            callback.on_driver_loaded(driver_name, load_time_us);
        }
    }
    
    /// Notify ExitBootServices hook (for JIT state saving)
    pub fn notify_exit_boot_services(&self) {
        self.set_phase(BootPhase::BdsExitBootServices);
        
        if let Some(ref callback) = *self.ui_callback.read().unwrap() {
            callback.on_exit_boot_services();
        }
        
        log::info!("[Firmware] ExitBootServices - transitioning to RT phase");
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
        {
            let mut state = self.state.write().unwrap();
            state.last_error = Some(error.to_string());
        }
        self.error_log.lock().unwrap().push(error.to_string());
    }
    
    /// Handle setup key press (F2/DEL)
    pub fn request_setup(&self) {
        let mut state = self.state.write().unwrap();
        state.setup_requested = true;
        state.phase = BootPhase::SetupMenu;
        state.menu_state = Some(BootMenuState::Main);
    }
    
    /// Handle Ctrl+Alt+Del
    pub fn request_reboot(&self) {
        let mut state = self.state.write().unwrap();
        state.reboot_requested = true;
    }
    
    /// Check if reboot was requested
    pub fn is_reboot_requested(&self) -> bool {
        self.state.read().unwrap().reboot_requested
    }
    
    /// Check if setup was requested
    pub fn is_setup_requested(&self) -> bool {
        self.state.read().unwrap().setup_requested
    }
    
    /// Set no bootable device state
    pub fn set_no_bootable_device(&self) {
        let mut state = self.state.write().unwrap();
        state.phase = BootPhase::NoBootableDevice;
        state.last_error = Some("No bootable device found".to_string());
    }
    
    /// Set boot sector invalid state (disk exists but no valid bootloader)
    pub fn set_boot_sector_invalid(&self, device_name: &str) {
        let mut state = self.state.write().unwrap();
        state.phase = BootPhase::NoBootableDevice;
        state.last_error = Some(format!(
            "No operating system found on {}. Missing bootloader or invalid boot sector.", 
            device_name
        ));
    }
    
    /// Check for bootable devices (just presence, not boot sector validity)
    pub fn has_bootable_device(&self) -> bool {
        let devices = self.boot_devices.read().unwrap();
        devices.iter().any(|d| d.bootable)
    }
    
    /// Validate boot sector from disk image
    /// 
    /// For BIOS: Check MBR signature (0x55 0xAA at offset 510-511)
    /// For UEFI: Check GPT signature or EFI bootloader path
    /// 
    /// Returns: (is_valid, error_message)
    pub fn validate_boot_sector(&self, boot_sector: &[u8], firmware_type: FirmwareType) -> (bool, Option<String>) {
        if boot_sector.len() < 512 {
            return (false, Some("Boot sector too small (< 512 bytes)".to_string()));
        }
        
        match firmware_type {
            FirmwareType::Bios => {
                // Check MBR signature: bytes 510-511 must be 0x55 0xAA
                let sig_55 = boot_sector[510];
                let sig_aa = boot_sector[511];
                
                if sig_55 == 0x55 && sig_aa == 0xAA {
                    // Check if partition table has at least one bootable partition
                    // or if there's code in the boot sector (non-zero first bytes)
                    let has_code = boot_sector[0..446].iter().any(|&b| b != 0);
                    if has_code {
                        (true, None)
                    } else {
                        // MBR signature present but no boot code
                        (false, Some("Disk has MBR signature but no boot code".to_string()))
                    }
                } else {
                    (false, Some(format!(
                        "Invalid MBR signature: expected 0x55AA, found 0x{:02X}{:02X}",
                        sig_55, sig_aa
                    )))
                }
            }
            FirmwareType::Uefi | FirmwareType::UefiSecure => {
                // For UEFI, check for GPT signature "EFI PART" at LBA 1 (offset 512)
                // Or check protective MBR (0xEE partition type)
                let sig_55 = boot_sector[510];
                let sig_aa = boot_sector[511];
                
                if sig_55 == 0x55 && sig_aa == 0xAA {
                    // Check for protective MBR (GPT indicator)
                    // Partition type 0xEE at offset 450 indicates GPT
                    let partition_type = boot_sector[450];
                    if partition_type == 0xEE {
                        // Protective MBR found, this is a GPT disk
                        // Actual EFI boot file check would need filesystem access
                        (true, None)
                    } else {
                        // Legacy MBR, might be hybrid or legacy boot
                        let has_code = boot_sector[0..446].iter().any(|&b| b != 0);
                        if has_code {
                            (true, None)
                        } else {
                            (false, Some("No EFI System Partition found".to_string()))
                        }
                    }
                } else {
                    (false, Some("Invalid boot sector signature".to_string()))
                }
            }
        }
    }
    
    /// Get error message for boot failure
    pub fn get_boot_error_message(&self, firmware_type: FirmwareType) -> Vec<String> {
        let state = self.state.read().unwrap();
        let mut lines = Vec::new();
        
        match firmware_type {
            FirmwareType::Bios => {
                lines.push(String::new());
                lines.push(String::from("================================================================================"));
                if let Some(ref error) = state.last_error {
                    if error.contains("Invalid MBR") {
                        lines.push(String::from("              MISSING OPERATING SYSTEM"));
                    } else if error.contains("no boot code") {
                        lines.push(String::from("              DISK BOOT FAILURE"));
                    } else {
                        lines.push(String::from("            OPERATING SYSTEM NOT FOUND"));
                    }
                } else {
                    lines.push(String::from("            OPERATING SYSTEM NOT FOUND"));
                }
                lines.push(String::from("================================================================================"));
                lines.push(String::new());
                if let Some(ref error) = state.last_error {
                    lines.push(error.clone());
                }
                lines.push(String::new());
                lines.push(String::from("Please insert a bootable disk and press any key..."));
                lines.push(String::from("Or press F2 to enter BIOS Setup"));
            }
            FirmwareType::Uefi | FirmwareType::UefiSecure => {
                lines.push(String::new());
                lines.push(String::from("================================================================================"));
                lines.push(String::from("              NO BOOTABLE DEVICE FOUND"));
                lines.push(String::from("================================================================================"));
                lines.push(String::new());
                if let Some(ref error) = state.last_error {
                    lines.push(error.clone());
                } else {
                    lines.push(String::from("No EFI bootloader found on any device."));
                }
                lines.push(String::new());
                lines.push(String::from("The system could not find a valid boot device."));
                lines.push(String::from("Please verify:"));
                lines.push(String::from("  - A bootable disk is attached"));
                lines.push(String::from("  - The disk contains a valid EFI System Partition"));
                lines.push(String::from("  - \\EFI\\BOOT\\BOOTX64.EFI exists on the ESP"));
                lines.push(String::new());
                lines.push(String::from("[F2] Enter UEFI Setup    [F12] Boot Menu    [Ctrl+Alt+Del] Reboot"));
            }
        }
        
        lines
    }
    
    /// Get boot timeout remaining
    pub fn get_boot_timeout(&self) -> u8 {
        self.state.read().unwrap().boot_timeout_secs
    }
    
    /// Decrement boot timeout
    pub fn tick_boot_timeout(&self) -> u8 {
        let mut state = self.state.write().unwrap();
        if state.boot_timeout_secs > 0 {
            state.boot_timeout_secs -= 1;
        }
        state.boot_timeout_secs
    }
    
    /// Generate boot prompt text for display
    pub fn get_boot_prompt(&self, firmware_type: FirmwareType) -> Vec<String> {
        let state = self.state.read().unwrap();
        let mut lines = Vec::new();
        
        match firmware_type {
            FirmwareType::Bios => {
                lines.push(String::new());
                lines.push(format!("Press F2 or DEL to enter BIOS Setup ({} sec)", state.boot_timeout_secs));
                lines.push(String::from("Press F12 for Boot Menu"));
                lines.push(String::new());
            }
            FirmwareType::Uefi | FirmwareType::UefiSecure => {
                lines.push(String::new());
                lines.push(format!("Press F2 or DEL to enter UEFI Setup ({} sec)", state.boot_timeout_secs));
                lines.push(String::from("Press F12 for Boot Device Selection"));
                if matches!(firmware_type, FirmwareType::UefiSecure) {
                    lines.push(String::from("Secure Boot: ENABLED"));
                }
                lines.push(String::new());
            }
        }
        
        lines
    }
    
    /// Generate no boot device error message
    pub fn get_no_boot_device_message(&self, firmware_type: FirmwareType) -> Vec<String> {
        let mut lines = Vec::new();
        
        match firmware_type {
            FirmwareType::Bios => {
                lines.push(String::new());
                lines.push(String::from("================================================================================"));
                lines.push(String::from("                         BOOT DEVICE NOT FOUND"));
                lines.push(String::from("================================================================================"));
                lines.push(String::new());
                lines.push(String::from("No bootable device detected. Please verify:"));
                lines.push(String::from("  - Boot media is properly connected"));
                lines.push(String::from("  - Boot order is correctly configured in BIOS Setup"));
                lines.push(String::from("  - Boot device contains a valid operating system"));
                lines.push(String::new());
                lines.push(String::from("Press F2 to enter BIOS Setup"));
                lines.push(String::from("Press Ctrl+Alt+Del to reboot"));
                lines.push(String::new());
                lines.push(String::from("Strike the F1 key to retry boot, F2 for setup utility"));
            }
            FirmwareType::Uefi | FirmwareType::UefiSecure => {
                lines.push(String::new());
                lines.push(String::from("================================================================================"));
                lines.push(String::from("                       NO BOOTABLE DEVICE FOUND"));
                lines.push(String::from("================================================================================"));
                lines.push(String::new());
                lines.push(String::from("  The system could not find any bootable devices."));
                lines.push(String::new());
                lines.push(String::from("  Please ensure that:"));
                lines.push(String::from("    1. A bootable medium is connected (HDD, SSD, USB, CD/DVD)"));
                lines.push(String::from("    2. The boot device contains a valid EFI boot loader"));
                lines.push(String::from("    3. Secure Boot is disabled if using unsigned boot media"));
                lines.push(String::new());
                lines.push(String::from("  Options:"));
                lines.push(String::from("    [F2]            Enter UEFI Setup"));
                lines.push(String::from("    [F12]           Select Boot Device"));
                lines.push(String::from("    [Ctrl+Alt+Del]  Restart System"));
                lines.push(String::new());
            }
        }
        
        lines
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
        
        // UEFI starts in real mode at reset vector, firmware code transitions to long mode
        // This matches real hardware behavior (SEC phase is 16-bit real mode)
        assert!(context.real_mode);
        assert_eq!(context.code_segment, 0xF000);  // Real mode segment
        assert_eq!(context.entry_point, 0xFFFF0);  // Reset vector
        // CR0, EFER, CR4 start at their reset values - firmware code will set them
        assert_eq!(context.cr0, 0x0000_0010);  // ET bit only
        assert_eq!(context.efer, 0);            // Not in long mode yet
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
