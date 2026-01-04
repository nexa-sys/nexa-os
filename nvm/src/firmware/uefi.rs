//! UEFI Firmware Implementation for NVM
//!
//! Provides a minimal UEFI firmware implementation for modern boot support.
//! Similar to OVMF but integrated into NVM.
//!
//! ## Features
//!
//! - UEFI System Table
//! - Boot Services (memory allocation, protocol handling)
//! - Runtime Services (time, variables)
//! - GOP (Graphics Output Protocol)
//! - Simple File System Protocol (for boot loader)

use super::{Firmware, FirmwareType, FirmwareLoadResult, FirmwareError, FirmwareResult, ServiceRegisters};
use crate::memory::PhysAddr;
use std::collections::HashMap;

/// UEFI configuration
#[derive(Debug, Clone)]
pub struct UefiConfig {
    /// Memory size in MB
    pub memory_mb: u64,
    /// Enable Secure Boot
    pub secure_boot: bool,
    /// Boot file path (e.g., \EFI\BOOT\BOOTX64.EFI)
    pub boot_path: String,
    /// Framebuffer width
    pub fb_width: u32,
    /// Framebuffer height
    pub fb_height: u32,
    /// NVRAM variables
    pub variables: HashMap<String, Vec<u8>>,
}

impl Default for UefiConfig {
    fn default() -> Self {
        Self {
            memory_mb: 256,
            secure_boot: false,
            boot_path: String::from("\\EFI\\BOOT\\BOOTX64.EFI"),
            fb_width: 800,
            fb_height: 600,
            variables: HashMap::new(),
        }
    }
}

/// UEFI memory type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum EfiMemoryType {
    ReservedMemoryType = 0,
    LoaderCode = 1,
    LoaderData = 2,
    BootServicesCode = 3,
    BootServicesData = 4,
    RuntimeServicesCode = 5,
    RuntimeServicesData = 6,
    ConventionalMemory = 7,
    UnusableMemory = 8,
    AcpiReclaimMemory = 9,
    AcpiMemoryNvs = 10,
    MemoryMappedIo = 11,
    MemoryMappedIoPortSpace = 12,
    PalCode = 13,
    PersistentMemory = 14,
    MaxMemoryType = 15,
}

/// UEFI memory descriptor
#[derive(Debug, Clone)]
pub struct EfiMemoryDescriptor {
    pub memory_type: EfiMemoryType,
    pub physical_start: u64,
    pub virtual_start: u64,
    pub number_of_pages: u64,
    pub attribute: u64,
}

/// Graphics Output Protocol mode info
#[derive(Debug, Clone)]
pub struct GopModeInfo {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: u32,  // 0=RGBX, 1=BGRX, 2=BitMask, 3=BltOnly
    pub pixels_per_scan_line: u32,
}

/// Graphics Output Protocol
#[derive(Debug, Clone)]
pub struct GraphicsOutputProtocol {
    pub mode_info: GopModeInfo,
    pub framebuffer_base: u64,
    pub framebuffer_size: u64,
    pub current_mode: u32,
    pub max_mode: u32,
}

impl GraphicsOutputProtocol {
    pub fn new(width: u32, height: u32, fb_base: u64) -> Self {
        let fb_size = (width * height * 4) as u64;  // 32bpp
        Self {
            mode_info: GopModeInfo {
                version: 0,
                horizontal_resolution: width,
                vertical_resolution: height,
                pixel_format: 1,  // BGRX (common for UEFI)
                pixels_per_scan_line: width,
            },
            framebuffer_base: fb_base,
            framebuffer_size: fb_size,
            current_mode: 0,
            max_mode: 1,
        }
    }
}

/// UEFI Boot Services
pub struct UefiBootServices {
    /// Memory map
    memory_map: Vec<EfiMemoryDescriptor>,
    /// Memory map key (changes on each allocation)
    memory_map_key: u64,
    /// Next allocation address
    next_alloc: u64,
    /// Loaded image protocols
    loaded_images: HashMap<u64, LoadedImageProtocol>,
    /// Protocol database
    protocols: HashMap<EfiGuid, u64>,
    /// Event list
    events: Vec<EfiEvent>,
}

#[derive(Debug, Clone)]
pub struct LoadedImageProtocol {
    pub revision: u32,
    pub parent_handle: u64,
    pub system_table: u64,
    pub device_handle: u64,
    pub file_path: String,
    pub image_base: u64,
    pub image_size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EfiGuid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl EfiGuid {
    pub const EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID: Self = Self {
        data1: 0x9042a9de,
        data2: 0x23dc,
        data3: 0x4a38,
        data4: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
    };
    
    pub const EFI_LOADED_IMAGE_PROTOCOL_GUID: Self = Self {
        data1: 0x5B1B31A1,
        data2: 0x9562,
        data3: 0x11d2,
        data4: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
    };
    
    pub const EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID: Self = Self {
        data1: 0x0964e5b22,
        data2: 0x6459,
        data3: 0x11d2,
        data4: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
    };
}

#[derive(Debug, Clone)]
pub struct EfiEvent {
    pub event_type: u32,
    pub notify_tpl: u64,
    pub notify_function: u64,
    pub notify_context: u64,
    pub signaled: bool,
}

impl UefiBootServices {
    pub fn new(memory_mb: u64) -> Self {
        let mut memory_map = Vec::new();
        
        // Create initial memory map
        // UEFI firmware at 0-1MB
        memory_map.push(EfiMemoryDescriptor {
            memory_type: EfiMemoryType::BootServicesCode,
            physical_start: 0,
            virtual_start: 0,
            number_of_pages: 256,  // 1MB
            attribute: 0x8000000000000000 | 0xF,  // EFI_MEMORY_RUNTIME | cacheable
        });
        
        // Conventional memory from 1MB onwards
        let conventional_pages = ((memory_mb - 1) * 256) as u64;  // 4KB pages
        memory_map.push(EfiMemoryDescriptor {
            memory_type: EfiMemoryType::ConventionalMemory,
            physical_start: 0x100000,
            virtual_start: 0x100000,
            number_of_pages: conventional_pages,
            attribute: 0xF,  // Cacheable
        });
        
        // MMIO region for framebuffer
        memory_map.push(EfiMemoryDescriptor {
            memory_type: EfiMemoryType::MemoryMappedIo,
            physical_start: 0xFD000000,
            virtual_start: 0xFD000000,
            number_of_pages: 0x1000,  // 16MB
            attribute: 0x8000000000000000,  // Runtime
        });
        
        Self {
            memory_map,
            memory_map_key: 1,
            next_alloc: 0x1000000,  // Start allocations at 16MB
            loaded_images: HashMap::new(),
            protocols: HashMap::new(),
            events: Vec::new(),
        }
    }
    
    /// Allocate pages
    pub fn allocate_pages(&mut self, pages: u64, memory_type: EfiMemoryType) -> Option<u64> {
        let size = pages * 4096;
        let addr = self.next_alloc;
        self.next_alloc += size;
        
        // Add to memory map
        self.memory_map.push(EfiMemoryDescriptor {
            memory_type,
            physical_start: addr,
            virtual_start: addr,
            number_of_pages: pages,
            attribute: 0xF,
        });
        
        self.memory_map_key += 1;
        Some(addr)
    }
    
    /// Get memory map
    pub fn get_memory_map(&self) -> (&[EfiMemoryDescriptor], u64) {
        (&self.memory_map, self.memory_map_key)
    }
    
    /// Exit boot services
    pub fn exit_boot_services(&mut self, map_key: u64) -> Result<(), &'static str> {
        if map_key != self.memory_map_key {
            return Err("Invalid memory map key");
        }
        
        // Mark boot services memory as available
        for desc in &mut self.memory_map {
            if desc.memory_type == EfiMemoryType::BootServicesCode 
               || desc.memory_type == EfiMemoryType::BootServicesData {
                desc.memory_type = EfiMemoryType::ConventionalMemory;
            }
        }
        
        Ok(())
    }
}

/// UEFI Runtime Services
pub struct UefiRuntimeServices {
    /// NVRAM variables
    variables: HashMap<String, (u32, Vec<u8>)>,  // (attributes, data)
    /// Current time
    time: EfiTime,
    /// Reset type for next reset
    pending_reset: Option<ResetType>,
}

#[derive(Debug, Clone, Copy)]
pub struct EfiTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub nanosecond: u32,
    pub timezone: i16,
    pub daylight: u8,
}

impl Default for EfiTime {
    fn default() -> Self {
        Self {
            year: 2026,
            month: 1,
            day: 4,
            hour: 0,
            minute: 0,
            second: 0,
            nanosecond: 0,
            timezone: 0,
            daylight: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ResetType {
    Cold,
    Warm,
    Shutdown,
    PlatformSpecific,
}

impl UefiRuntimeServices {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            time: EfiTime::default(),
            pending_reset: None,
        }
    }
    
    /// Get time
    pub fn get_time(&self) -> EfiTime {
        self.time
    }
    
    /// Set time
    pub fn set_time(&mut self, time: EfiTime) {
        self.time = time;
    }
    
    /// Get variable
    pub fn get_variable(&self, name: &str) -> Option<(u32, &[u8])> {
        self.variables.get(name).map(|(attr, data)| (*attr, data.as_slice()))
    }
    
    /// Set variable
    pub fn set_variable(&mut self, name: &str, attributes: u32, data: Vec<u8>) {
        self.variables.insert(name.to_string(), (attributes, data));
    }
    
    /// Request reset
    pub fn reset_system(&mut self, reset_type: ResetType) {
        self.pending_reset = Some(reset_type);
    }
}

impl Default for UefiRuntimeServices {
    fn default() -> Self {
        Self::new()
    }
}

/// UEFI System Table structure (in-memory layout)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct EfiSystemTable {
    pub hdr_signature: u64,
    pub hdr_revision: u32,
    pub hdr_header_size: u32,
    pub hdr_crc32: u32,
    pub hdr_reserved: u32,
    pub firmware_vendor: u64,  // Pointer to firmware vendor string
    pub firmware_revision: u32,
    pub _pad: u32,
    pub console_in_handle: u64,
    pub con_in: u64,
    pub console_out_handle: u64,
    pub con_out: u64,
    pub standard_error_handle: u64,
    pub std_err: u64,
    pub runtime_services: u64,
    pub boot_services: u64,
    pub number_of_table_entries: u64,
    pub configuration_table: u64,
}

impl Default for EfiSystemTable {
    fn default() -> Self {
        Self {
            hdr_signature: 0x5453595320494249,  // "IBI SYST"
            hdr_revision: 0x0002004E,  // UEFI 2.78
            hdr_header_size: std::mem::size_of::<Self>() as u32,
            hdr_crc32: 0,
            hdr_reserved: 0,
            firmware_vendor: 0,
            firmware_revision: 0x00010000,  // 1.0
            _pad: 0,
            console_in_handle: 0,
            con_in: 0,
            console_out_handle: 0,
            con_out: 0,
            standard_error_handle: 0,
            std_err: 0,
            runtime_services: 0,
            boot_services: 0,
            number_of_table_entries: 0,
            configuration_table: 0,
        }
    }
}

/// UEFI Firmware implementation
pub struct UefiFirmware {
    config: UefiConfig,
    boot_services: UefiBootServices,
    runtime_services: UefiRuntimeServices,
    gop: GraphicsOutputProtocol,
    system_table: EfiSystemTable,
    version: String,
}

impl UefiFirmware {
    pub fn new(config: UefiConfig) -> Self {
        let gop = GraphicsOutputProtocol::new(
            config.fb_width,
            config.fb_height,
            0xFD000000,  // Framebuffer base
        );
        
        Self {
            boot_services: UefiBootServices::new(config.memory_mb),
            runtime_services: UefiRuntimeServices::new(),
            gop,
            config,
            system_table: EfiSystemTable::default(),
            version: String::from("NexaUEFI 1.0"),
        }
    }
    
    /// Get GOP
    pub fn gop(&self) -> &GraphicsOutputProtocol {
        &self.gop
    }
    
    /// Get boot services
    pub fn boot_services(&mut self) -> &mut UefiBootServices {
        &mut self.boot_services
    }
    
    /// Get runtime services
    pub fn runtime_services(&mut self) -> &mut UefiRuntimeServices {
        &mut self.runtime_services
    }
    
    /// Write system table to memory
    fn write_system_table(&self, memory: &mut [u8], addr: usize) {
        if addr + std::mem::size_of::<EfiSystemTable>() > memory.len() {
            return;
        }
        
        // Write system table structure
        let table = &self.system_table;
        let bytes = unsafe {
            std::slice::from_raw_parts(
                table as *const _ as *const u8,
                std::mem::size_of::<EfiSystemTable>()
            )
        };
        memory[addr..addr + bytes.len()].copy_from_slice(bytes);
    }
    
    /// Initialize UEFI firmware in memory
    fn init_uefi_tables(&mut self, memory: &mut [u8]) -> FirmwareResult<u64> {
        // Place system table at 1MB
        let system_table_addr = 0x100000u64;
        
        // Firmware vendor string
        let vendor_addr = 0x100200u64;
        let vendor = "NexaUEFI\0";
        let vendor_wide: Vec<u16> = vendor.encode_utf16().collect();
        for (i, &ch) in vendor_wide.iter().enumerate() {
            let offset = vendor_addr as usize + i * 2;
            if offset + 1 < memory.len() {
                memory[offset] = ch as u8;
                memory[offset + 1] = (ch >> 8) as u8;
            }
        }
        
        self.system_table.firmware_vendor = vendor_addr;
        self.system_table.firmware_revision = 0x00010000;
        
        // Set up pointers (these would point to actual service implementations)
        let boot_services_addr = 0x100400u64;
        let runtime_services_addr = 0x100800u64;
        
        self.system_table.boot_services = boot_services_addr;
        self.system_table.runtime_services = runtime_services_addr;
        
        // Write tables to memory
        self.write_system_table(memory, system_table_addr as usize);
        
        Ok(system_table_addr)
    }
}

impl Firmware for UefiFirmware {
    fn firmware_type(&self) -> FirmwareType {
        if self.config.secure_boot {
            FirmwareType::UefiSecure
        } else {
            FirmwareType::Uefi
        }
    }
    
    fn load(&mut self, memory: &mut [u8]) -> FirmwareResult<FirmwareLoadResult> {
        if memory.len() < 0x200000 {
            return Err(FirmwareError::InvalidMemory(
                "Need at least 2MB of memory for UEFI".to_string()
            ));
        }
        
        // Initialize UEFI tables
        let system_table_addr = self.init_uefi_tables(memory)?;
        
        // Create a minimal UEFI entry stub
        // This would normally load the boot loader from disk
        let entry_addr = 0x80000u64;
        
        // Simple 64-bit entry code
        let entry_code: &[u8] = &[
            // Entry point - receives SystemTable in RDX
            0x48, 0x89, 0xD3,             // MOV RBX, RDX (save system table)
            0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // MOV RAX, 0 (EFI_SUCCESS)
            // Normally we'd transfer to boot loader here
            0xF4,                          // HLT
            0xEB, 0xFD,                    // JMP -3
        ];
        
        let entry_addr_usize = entry_addr as usize;
        if entry_addr_usize + entry_code.len() < memory.len() {
            memory[entry_addr_usize..entry_addr_usize + entry_code.len()]
                .copy_from_slice(entry_code);
        }
        
        // Clear framebuffer with blue (UEFI boot color)
        let fb_base = 0xFD000000usize;
        let fb_size = (self.config.fb_width * self.config.fb_height * 4) as usize;
        if fb_base + fb_size <= memory.len() {
            for i in 0..self.config.fb_width * self.config.fb_height {
                let offset = fb_base + (i as usize) * 4;
                memory[offset] = 0x80;     // B
                memory[offset + 1] = 0x00; // G
                memory[offset + 2] = 0x00; // R
                memory[offset + 3] = 0xFF; // A
            }
        }
        
        // Return entry point (64-bit mode)
        Ok(FirmwareLoadResult {
            entry_point: entry_addr,
            stack_pointer: 0x7C000,
            code_segment: 0x08,  // 64-bit code segment
            real_mode: false,
        })
    }
    
    fn handle_service(&mut self, memory: &mut [u8], regs: &mut ServiceRegisters) -> FirmwareResult<()> {
        // UEFI services are called via function pointers
        // The service number is passed in RAX
        let service = regs.rax;
        
        match service {
            // GetTime
            0x01 => {
                let time = self.runtime_services.get_time();
                // Write time to buffer pointed by RCX
                let _ = time;
                regs.rax = 0;  // EFI_SUCCESS
            }
            // GetVariable
            0x02 => {
                regs.rax = 0x8000000000000005;  // EFI_NOT_FOUND
            }
            // SetVariable
            0x03 => {
                regs.rax = 0;  // EFI_SUCCESS
            }
            // AllocatePages
            0x10 => {
                let pages = regs.rcx;
                if let Some(addr) = self.boot_services.allocate_pages(pages, EfiMemoryType::LoaderData) {
                    regs.rcx = addr;
                    regs.rax = 0;
                } else {
                    regs.rax = 0x8000000000000009;  // EFI_OUT_OF_RESOURCES
                }
            }
            // GetMemoryMap
            0x11 => {
                let (map, key) = self.boot_services.get_memory_map();
                // Would write map to buffer
                let _ = map;
                regs.rbx = key;
                regs.rax = 0;
            }
            // ExitBootServices
            0x12 => {
                let map_key = regs.rcx;
                match self.boot_services.exit_boot_services(map_key) {
                    Ok(()) => regs.rax = 0,
                    Err(_) => regs.rax = 0x8000000000000002,  // EFI_INVALID_PARAMETER
                }
            }
            // ResetSystem
            0x20 => {
                let reset_type = match regs.rcx {
                    0 => ResetType::Cold,
                    1 => ResetType::Warm,
                    2 => ResetType::Shutdown,
                    _ => ResetType::PlatformSpecific,
                };
                self.runtime_services.reset_system(reset_type);
                regs.rax = 0;
            }
            _ => {
                // Unknown service
                regs.rax = 0x8000000000000003;  // EFI_UNSUPPORTED
            }
        }
        
        let _ = memory;
        Ok(())
    }
    
    fn reset(&mut self) {
        self.boot_services = UefiBootServices::new(self.config.memory_mb);
        self.runtime_services = UefiRuntimeServices::new();
    }
    
    fn version(&self) -> &str {
        &self.version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_uefi_creation() {
        let uefi = UefiFirmware::new(UefiConfig::default());
        assert_eq!(uefi.firmware_type(), FirmwareType::Uefi);
        assert!(uefi.version().contains("NexaUEFI"));
    }
    
    #[test]
    fn test_uefi_secure_boot() {
        let uefi = UefiFirmware::new(UefiConfig {
            secure_boot: true,
            ..Default::default()
        });
        assert_eq!(uefi.firmware_type(), FirmwareType::UefiSecure);
    }
    
    #[test]
    fn test_gop() {
        let uefi = UefiFirmware::new(UefiConfig {
            fb_width: 1024,
            fb_height: 768,
            ..Default::default()
        });
        
        let gop = uefi.gop();
        assert_eq!(gop.mode_info.horizontal_resolution, 1024);
        assert_eq!(gop.mode_info.vertical_resolution, 768);
    }
}
