//! Firmware Emulation for NVM Hypervisor
//!
//! This module provides BIOS and UEFI firmware implementations for virtual machines.
//! Unlike QEMU which uses SeaBIOS/OVMF, NVM includes its own lightweight firmware
//! to enable VM boot without external dependencies.
//!
//! ## Features
//!
//! - **BIOS (Legacy)**: Traditional PC BIOS with INT services
//! - **UEFI**: Modern firmware with GOP, runtime services
//! - **ACPI**: Complete ACPI tables (RSDP, RSDT, XSDT, FADT, DSDT, MADT, etc.)
//! - **SMBIOS**: System Management BIOS tables (Type 0-127)
//! - **VGA Font**: Built-in 8x16 bitmap font for console display
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                        Firmware Layer                                │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────┐      ┌──────────────────────────┐              │
//! │  │   Legacy BIOS   │      │     UEFI Firmware        │              │
//! │  │                 │      │                          │              │
//! │  │  - POST         │      │  - System Table          │              │
//! │  │  - IVT          │      │  - Boot Services         │              │
//! │  │  - INT 10h/13h  │      │  - Runtime Services      │              │
//! │  │  - Boot loader  │      │  - GOP (Graphics)        │              │
//! │  └─────────────────┘      └──────────────────────────┘              │
//! │  ┌─────────────────────────────────────────────────────────────┐    │
//! │  │                    System Tables                             │    │
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐     │    │
//! │  │  │   ACPI   │  │  SMBIOS  │  │    MP    │  │   E820   │     │    │
//! │  │  │  Tables  │  │  Tables  │  │  Tables  │  │ Memory   │     │    │
//! │  │  └──────────┘  └──────────┘  └──────────┘  └──────────┘     │    │
//! │  └─────────────────────────────────────────────────────────────┘    │
//! │  ┌─────────────────────────────────────────────────────────────┐    │
//! │  │                   VGA ROM Font (8x16 bitmap)                 │    │
//! │  └─────────────────────────────────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```

pub mod bios;
pub mod uefi;
pub mod font;
pub mod manager;
pub mod acpi;
pub mod smbios;

pub use bios::{Bios, BiosConfig, BiosServices};
pub use uefi::{UefiFirmware, UefiConfig, UefiBootServices, UefiRuntimeServices};
pub use font::{VgaFont, get_vga_font, FONT_WIDTH, FONT_HEIGHT};
pub use manager::{FirmwareManager, FirmwareBootContext, FirmwareState, BootPhase, BootMenuState, SetupKey};
pub use acpi::{AcpiConfig, AcpiTableGenerator, Rsdp, Fadt, Facs, MadtBuilder, DsdtBuilder};
pub use smbios::{SmbiosConfig, SmbiosGenerator, Smbios2EntryPoint, Smbios3EntryPoint};

use crate::memory::PhysAddr;

/// Firmware type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareType {
    /// Legacy BIOS (SeaBIOS compatible)
    Bios,
    /// UEFI firmware (OVMF compatible)
    Uefi,
    /// UEFI with Secure Boot enabled
    UefiSecure,
}

impl Default for FirmwareType {
    fn default() -> Self {
        Self::Bios
    }
}

/// Firmware loading result
pub struct FirmwareLoadResult {
    /// Entry point address for jumping to firmware
    pub entry_point: PhysAddr,
    /// Stack pointer initial value
    pub stack_pointer: PhysAddr,
    /// Code segment (for real mode)
    pub code_segment: u16,
    /// Whether VM should start in real mode
    pub real_mode: bool,
}

/// Firmware initialization error
#[derive(Debug, Clone)]
pub enum FirmwareError {
    /// Failed to initialize firmware
    InitializationFailed(String),
    /// Invalid memory configuration
    InvalidMemory(String),
    /// Firmware image not found
    NotFound(String),
    /// Boot device not found
    NoBootDevice,
    /// Boot loader failed
    BootFailed(String),
}

impl std::fmt::Display for FirmwareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitializationFailed(msg) => write!(f, "Firmware init failed: {}", msg),
            Self::InvalidMemory(msg) => write!(f, "Invalid memory: {}", msg),
            Self::NotFound(msg) => write!(f, "Firmware not found: {}", msg),
            Self::NoBootDevice => write!(f, "No bootable device found"),
            Self::BootFailed(msg) => write!(f, "Boot failed: {}", msg),
        }
    }
}

impl std::error::Error for FirmwareError {}

pub type FirmwareResult<T> = Result<T, FirmwareError>;

/// Common firmware trait
pub trait Firmware: Send + Sync {
    /// Get firmware type
    fn firmware_type(&self) -> FirmwareType;
    
    /// Load firmware into guest memory
    fn load(&mut self, memory: &mut [u8]) -> FirmwareResult<FirmwareLoadResult>;
    
    /// Handle firmware service call (INT for BIOS, SMI for UEFI)
    fn handle_service(&mut self, memory: &mut [u8], registers: &mut ServiceRegisters) -> FirmwareResult<()>;
    
    /// Reset firmware state
    fn reset(&mut self);
    
    /// Get firmware name/version string
    fn version(&self) -> &str;
}

/// Registers passed to firmware service calls
#[derive(Debug, Clone, Default)]
pub struct ServiceRegisters {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub rflags: u64,
    pub rip: u64,
    pub cs: u16,
    pub ds: u16,
    pub es: u16,
    pub ss: u16,
}
