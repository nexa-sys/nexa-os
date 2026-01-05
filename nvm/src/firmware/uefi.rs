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
    /// Number of CPUs
    pub cpu_count: u32,
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
            cpu_count: 1,
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
    
    pub const EFI_DEVICE_PATH_PROTOCOL_GUID: Self = Self {
        data1: 0x09576e91,
        data2: 0x6d3f,
        data3: 0x11d2,
        data4: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
    };
    
    pub const EFI_BLOCK_IO_PROTOCOL_GUID: Self = Self {
        data1: 0x0964e5b21,
        data2: 0x6459,
        data3: 0x11d2,
        data4: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
    };
    
    pub const EFI_DISK_IO_PROTOCOL_GUID: Self = Self {
        data1: 0xce345171,
        data2: 0xba0b,
        data3: 0x11d2,
        data4: [0x8e, 0x4f, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
    };
    
    pub const EFI_SIMPLE_TEXT_INPUT_PROTOCOL_GUID: Self = Self {
        data1: 0x387477c1,
        data2: 0x69c7,
        data3: 0x11d2,
        data4: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
    };
    
    pub const EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL_GUID: Self = Self {
        data1: 0x387477c2,
        data2: 0x69c7,
        data3: 0x11d2,
        data4: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
    };
    
    pub const EFI_ACPI_20_TABLE_GUID: Self = Self {
        data1: 0x8868e871,
        data2: 0xe4f1,
        data3: 0x11d3,
        data4: [0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81],
    };
    
    pub const EFI_SMBIOS_TABLE_GUID: Self = Self {
        data1: 0xeb9d2d31,
        data2: 0x2d88,
        data3: 0x11d3,
        data4: [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
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
    
    /// Write memory map to guest memory
    pub fn write_memory_map(&self, memory: &mut [u8], buffer_addr: u64, buffer_size: u64) -> (u64, u64, u64) {
        // EFI_MEMORY_DESCRIPTOR is 40 bytes (with padding)
        const DESC_SIZE: usize = 40;
        let map_size = self.memory_map.len() * DESC_SIZE;
        
        if (buffer_size as usize) < map_size {
            return (0, map_size as u64, 0);
        }
        
        let base = buffer_addr as usize;
        for (i, desc) in self.memory_map.iter().enumerate() {
            let offset = base + i * DESC_SIZE;
            if offset + DESC_SIZE <= memory.len() {
                // Type (4 bytes)
                memory[offset..offset+4].copy_from_slice(&(desc.memory_type as u32).to_le_bytes());
                // Padding (4 bytes)
                memory[offset+4..offset+8].copy_from_slice(&0u32.to_le_bytes());
                // PhysicalStart (8 bytes)
                memory[offset+8..offset+16].copy_from_slice(&desc.physical_start.to_le_bytes());
                // VirtualStart (8 bytes)
                memory[offset+16..offset+24].copy_from_slice(&desc.virtual_start.to_le_bytes());
                // NumberOfPages (8 bytes)
                memory[offset+24..offset+32].copy_from_slice(&desc.number_of_pages.to_le_bytes());
                // Attribute (8 bytes)
                memory[offset+32..offset+40].copy_from_slice(&desc.attribute.to_le_bytes());
            }
        }
        
        (map_size as u64, DESC_SIZE as u64, self.memory_map_key)
    }
    
    /// Free pages
    pub fn free_pages(&mut self, addr: u64, pages: u64) {
        // Remove from memory map
        self.memory_map.retain(|desc| {
            !(desc.physical_start == addr && desc.number_of_pages == pages)
        });
        self.memory_map_key += 1;
    }
    
    /// Allocate pool (convenience wrapper)
    pub fn allocate_pool(&mut self, size: u64, memory_type: EfiMemoryType) -> Option<u64> {
        let pages = (size + 4095) / 4096;
        self.allocate_pages(pages, memory_type)
    }
    
    /// Register a protocol
    pub fn install_protocol(&mut self, handle: u64, guid: EfiGuid, interface: u64) {
        self.protocols.insert(guid, interface);
    }
    
    /// Locate a protocol by GUID
    pub fn locate_protocol(&self, guid: &EfiGuid) -> Option<u64> {
        self.protocols.get(guid).copied()
    }
    
    /// Create event
    pub fn create_event(&mut self, event_type: u32, notify_tpl: u64, notify_function: u64, notify_context: u64) -> u64 {
        let event = EfiEvent {
            event_type,
            notify_tpl,
            notify_function,
            notify_context,
            signaled: false,
        };
        self.events.push(event);
        (self.events.len() - 1) as u64
    }
    
    /// Signal event
    pub fn signal_event(&mut self, event_handle: u64) {
        if let Some(event) = self.events.get_mut(event_handle as usize) {
            event.signaled = true;
        }
    }
    
    /// Close event
    pub fn close_event(&mut self, event_handle: u64) {
        if (event_handle as usize) < self.events.len() {
            // Mark as closed (we don't remove to preserve handles)
            if let Some(event) = self.events.get_mut(event_handle as usize) {
                event.event_type = 0;
            }
        }
    }
    
    /// Register loaded image
    pub fn register_loaded_image(&mut self, handle: u64, image: LoadedImageProtocol) {
        self.loaded_images.insert(handle, image);
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
    
    /// Initialize 64-bit page tables for long mode
    /// 
    /// Creates identity mapping for first 4GB using 2MB pages
    fn init_page_tables(&self, memory: &mut [u8]) -> FirmwareResult<()> {
        // Page tables at 0x100000 (1MB)
        let pml4_addr = 0x100000usize;
        let pdpt_addr = 0x101000usize;
        let pd_addr = 0x102000usize;
        
        // Ensure we have enough memory
        if pd_addr + 0x4000 > memory.len() {
            return Err(FirmwareError::InvalidMemory(
                "Not enough memory for page tables".to_string()
            ));
        }
        
        // Clear page table area
        for i in pml4_addr..(pd_addr + 0x4000) {
            memory[i] = 0;
        }
        
        // PML4[0] -> PDPT (present + writable)
        let pml4e: u64 = pdpt_addr as u64 | 0x03;
        let pml4e_bytes = pml4e.to_le_bytes();
        memory[pml4_addr..pml4_addr + 8].copy_from_slice(&pml4e_bytes);
        
        // Setup PDPT entries pointing to PD tables
        // Map first 4GB (4 PDPT entries, each covering 1GB)
        for i in 0..4 {
            let pdpte: u64 = (pd_addr + i * 0x1000) as u64 | 0x03;
            let offset = pdpt_addr + i * 8;
            let pdpte_bytes = pdpte.to_le_bytes();
            memory[offset..offset + 8].copy_from_slice(&pdpte_bytes);
        }
        
        // Setup PD entries with 2MB pages (PS bit set)
        // Identity map first 4GB
        for gb in 0..4 {
            for i in 0..512 {
                let phys_addr = (gb * 0x40000000) + (i * 0x200000); // 2MB pages
                let pde: u64 = phys_addr as u64 | 0x83; // Present + Writable + PS (2MB page)
                let offset = pd_addr + gb * 0x1000 + i * 8;
                if offset + 8 <= memory.len() {
                    let pde_bytes = pde.to_le_bytes();
                    memory[offset..offset + 8].copy_from_slice(&pde_bytes);
                }
            }
        }
        
        Ok(())
    }
    
    /// Initialize GDT for 64-bit long mode
    fn init_gdt(&self, memory: &mut [u8]) -> FirmwareResult<()> {
        // GDT at 0x80000 (512KB)
        let gdt_addr = 0x80000usize;
        
        if gdt_addr + 0x40 > memory.len() {
            return Err(FirmwareError::InvalidMemory(
                "Not enough memory for GDT".to_string()
            ));
        }
        
        // GDT entries (each 8 bytes)
        let gdt_entries: &[u64] = &[
            0x0000_0000_0000_0000,  // 0x00: Null descriptor
            0x00AF_9A00_0000_FFFF,  // 0x08: 64-bit code segment (DPL=0)
            0x00CF_9200_0000_FFFF,  // 0x10: 64-bit data segment (DPL=0)
            0x00AF_FA00_0000_FFFF,  // 0x18: 64-bit code segment (DPL=3, user)
            0x00CF_F200_0000_FFFF,  // 0x20: 64-bit data segment (DPL=3, user)
            0x0000_0000_0000_0000,  // 0x28: TSS (low) - will be filled at runtime
            0x0000_0000_0000_0000,  // 0x30: TSS (high)
        ];
        
        for (i, &entry) in gdt_entries.iter().enumerate() {
            let offset = gdt_addr + i * 8;
            let entry_bytes = entry.to_le_bytes();
            memory[offset..offset + 8].copy_from_slice(&entry_bytes);
        }
        
        // GDT pointer structure at gdt_addr - 10
        // (GDTR: 2-byte limit + 8-byte base)
        let gdtr_addr = gdt_addr - 10;
        let limit: u16 = (gdt_entries.len() * 8 - 1) as u16;
        let limit_bytes = limit.to_le_bytes();
        memory[gdtr_addr..gdtr_addr + 2].copy_from_slice(&limit_bytes);
        let base_bytes = (gdt_addr as u64).to_le_bytes();
        memory[gdtr_addr + 2..gdtr_addr + 10].copy_from_slice(&base_bytes);
        
        Ok(())
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
    
    /// Initialize ACPI tables for UEFI firmware
    fn init_acpi_tables(&self, memory: &mut [u8]) -> FirmwareResult<()> {
        use super::acpi::{AcpiConfig, AcpiTableGenerator};
        
        let acpi_config = AcpiConfig {
            cpu_count: self.config.cpu_count,
            ..Default::default()
        };
        
        let generator = AcpiTableGenerator::new(acpi_config);
        generator.generate(memory)
            .map_err(|e| FirmwareError::InitializationFailed(format!("ACPI init failed: {}", e)))?;
        Ok(())
    }
    
    /// Initialize SMBIOS tables for UEFI firmware
    fn init_smbios_tables(&self, memory: &mut [u8]) -> FirmwareResult<()> {
        use super::smbios::{SmbiosConfig, SmbiosGenerator};
        
        let smbios_config = SmbiosConfig {
            bios_vendor: String::from("NexaOS"),
            bios_version: String::from("NexaUEFI 1.0"),
            cpu_count: self.config.cpu_count,
            memory_mb: self.config.memory_mb,
            cpu_cores: 1,
            cpu_threads: 1,
            memory_slots: std::cmp::min(4, std::cmp::max(1, (self.config.memory_mb / 4096) as u8 + 1)),
            ..Default::default()
        };
        
        let generator = SmbiosGenerator::new(smbios_config);
        generator.generate(memory)
            .map_err(|e| FirmwareError::InitializationFailed(format!("SMBIOS init failed: {}", e)))?;
        Ok(())
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
        
        // =========== Phase 1: Setup page tables (will be used after mode switch) ===========
        self.init_page_tables(memory)?;
        
        // =========== Phase 2: Setup GDT ===========
        self.init_gdt(memory)?;
        
        // =========== Phase 3: Initialize UEFI tables ===========
        let _system_table_addr = self.init_uefi_tables(memory)?;
        
        // =========== Phase 4: Create UEFI SEC/PEI/DXE bootstrap code ===========
        // Real UEFI starts in 16-bit real mode at reset vector (0xFFFF0)
        // and transitions: Real Mode -> Protected Mode -> Long Mode
        
        // Reset vector at 0xFFFF0 (jumps to SEC entry)
        let reset_vector: &[u8] = &[
            0xEA, 0x00, 0x70, 0x00, 0xF0,  // JMP FAR F000:7000 (SEC entry)
        ];
        let reset_addr = 0xFFFF0usize;
        if reset_addr + reset_vector.len() <= memory.len() {
            memory[reset_addr..reset_addr + reset_vector.len()].copy_from_slice(reset_vector);
        }
        
        // SEC Phase at F000:7000 (linear 0xF7000) - 16-bit real mode code
        // Initializes basic environment, then jumps to PEI
        let sec_code: &[u8] = &[
            // ---- SEC Phase: 16-bit Real Mode ----
            // CLI - disable interrupts
            0xFA,
            // Set up segments for real mode
            0x31, 0xC0,                   // XOR AX, AX
            0x8E, 0xD8,                   // MOV DS, AX
            0x8E, 0xC0,                   // MOV ES, AX
            0x8E, 0xD0,                   // MOV SS, AX
            0xBC, 0x00, 0x7C,             // MOV SP, 0x7C00
            
            // ---- PEI Phase: Switch to 32-bit Protected Mode ----
            // Load GDT (GDTR at DS:0x30 = linear 0x30, GDT table at 0x80000)
            0x0F, 0x01, 0x16, 0x30, 0x00, // LGDT [0x0030]
            
            // Enable Protected Mode (set CR0.PE)
            0x0F, 0x20, 0xC0,             // MOV EAX, CR0
            0x0C, 0x01,                   // OR AL, 1
            0x0F, 0x22, 0xC0,             // MOV CR0, EAX
            
            // Far jump to 32-bit code (flush pipeline, load CS with 32-bit selector)
            0x66, 0xEA,                   // JMP FAR (32-bit)
            0x30, 0x70, 0x00, 0x00,       // Offset: 0x7030
            0x08, 0x00,                   // Selector: 0x08 (32-bit code)
        ];
        let sec_addr = 0xF7000usize;
        if sec_addr + sec_code.len() <= memory.len() {
            memory[sec_addr..sec_addr + sec_code.len()].copy_from_slice(sec_code);
        }
        
        // GDTR structure at linear address 0x30 (DS=0, so LGDT [0x30] reads here)
        // GDT table itself is at 0x80000
        let gdtr_struct: &[u8] = &[
            0x2F, 0x00,                   // Limit: 47 (6 entries * 8 - 1)
            0x00, 0x00, 0x08, 0x00,       // Base: 0x80000
        ];
        memory[0x30..0x30 + gdtr_struct.len()].copy_from_slice(gdtr_struct);
        
        // 32-bit protected mode code at 0x7030
        // This is PEI -> DXE transition: switch from 32-bit to 64-bit
        let pei_code: &[u8] = &[
            // ---- 32-bit Protected Mode ----
            // Set up 32-bit segments
            0x66, 0xB8, 0x10, 0x00,       // MOV AX, 0x10 (32-bit data selector)
            0x8E, 0xD8,                   // MOV DS, AX
            0x8E, 0xC0,                   // MOV ES, AX
            0x8E, 0xD0,                   // MOV SS, AX
            0x8E, 0xE0,                   // MOV FS, AX
            0x8E, 0xE8,                   // MOV GS, AX
            
            // ---- DXE Phase: Switch to 64-bit Long Mode ----
            // Enable PAE in CR4
            0x0F, 0x20, 0xE0,             // MOV EAX, CR4
            0x0D, 0x20, 0x00, 0x00, 0x00, // OR EAX, 0x20 (PAE)
            0x0F, 0x22, 0xE0,             // MOV CR4, EAX
            
            // Load CR3 with PML4 address (page tables at 0x100000)
            0xB8, 0x00, 0x00, 0x10, 0x00, // MOV EAX, 0x100000
            0x0F, 0x22, 0xD8,             // MOV CR3, EAX
            
            // Enable Long Mode in EFER MSR
            0xB9, 0x80, 0x00, 0x00, 0xC0, // MOV ECX, 0xC0000080 (IA32_EFER)
            0x0F, 0x32,                   // RDMSR
            0x0D, 0x00, 0x01, 0x00, 0x00, // OR EAX, 0x100 (LME)
            0x0F, 0x30,                   // WRMSR
            
            // Enable Paging (CR0.PG) - this activates Long Mode
            0x0F, 0x20, 0xC0,             // MOV EAX, CR0
            0x0D, 0x00, 0x00, 0x00, 0x80, // OR EAX, 0x80000000 (PG)
            0x0F, 0x22, 0xC0,             // MOV CR0, EAX
            
            // Far jump to 64-bit code
            0xEA,                         // JMP FAR
            0x00, 0x71, 0x00, 0x00,       // Offset: 0x7100
            0x18, 0x00,                   // Selector: 0x18 (64-bit code)
        ];
        let pei_addr = 0x7030usize;
        if pei_addr + pei_code.len() <= memory.len() {
            memory[pei_addr..pei_addr + pei_code.len()].copy_from_slice(pei_code);
        }
        
        // 64-bit DXE code at 0x7100
        let dxe_code: &[u8] = &[
            // ---- 64-bit Long Mode (DXE) ----
            // Set up 64-bit segments
            0x48, 0x31, 0xC0,             // XOR RAX, RAX
            0x8E, 0xD8,                   // MOV DS, AX
            0x8E, 0xC0,                   // MOV ES, AX
            0x8E, 0xD0,                   // MOV SS, AX
            
            // Set up stack
            0x48, 0xBC,                   // MOV RSP, imm64
            0x00, 0x7C, 0x00, 0x00,       // 0x7C00
            0x00, 0x00, 0x00, 0x00,
            
            // Display "UEFI" on screen (VGA text at 0xB8000)
            0x48, 0xBF,                   // MOV RDI, imm64
            0x00, 0x80, 0x0B, 0x00,       // 0xB8000
            0x00, 0x00, 0x00, 0x00,
            0x66, 0xB8, 0x55, 0x1F,       // MOV AX, 0x1F55 ('U' white on blue) - needs 66 prefix in 64-bit
            0x66, 0x89, 0x07,             // MOV [RDI], AX
            0x66, 0xB8, 0x45, 0x1F,       // MOV AX, 0x1F45 ('E')
            0x66, 0x89, 0x47, 0x02,       // MOV [RDI+2], AX
            0x66, 0xB8, 0x46, 0x1F,       // MOV AX, 0x1F46 ('F')
            0x66, 0x89, 0x47, 0x04,       // MOV [RDI+4], AX
            0x66, 0xB8, 0x49, 0x1F,       // MOV AX, 0x1F49 ('I')
            0x66, 0x89, 0x47, 0x06,       // MOV [RDI+6], AX
            
            // HLT loop (wait for boot device)
            0xF4,                         // HLT
            0xEB, 0xFD,                   // JMP -3
        ];
        let dxe_addr = 0x7100usize;
        if dxe_addr + dxe_code.len() <= memory.len() {
            memory[dxe_addr..dxe_addr + dxe_code.len()].copy_from_slice(dxe_code);
        }
        
        // =========== Phase 5: Initialize VGA buffer ===========
        let vga_base = 0xB8000usize;
        if vga_base + 0x1000 < memory.len() {
            // Clear VGA text buffer
            for i in 0..(80 * 25) {
                memory[vga_base + i * 2] = b' ';
                memory[vga_base + i * 2 + 1] = 0x1F;  // White on blue
            }
        }
        
        // =========== Phase 6: Initialize ACPI tables ===========
        self.init_acpi_tables(memory)?;
        
        // =========== Phase 7: Initialize SMBIOS tables ===========
        self.init_smbios_tables(memory)?;
        
        // Return entry point: CPU starts in 16-bit real mode at reset vector
        Ok(FirmwareLoadResult {
            entry_point: 0xFFFF0,         // Reset vector (real mode)
            stack_pointer: 0x7C00,
            code_segment: 0xF000,         // Real mode segment
            real_mode: true,              // Start in real mode!
        })
    }
    
    fn handle_service(&mut self, memory: &mut [u8], regs: &mut ServiceRegisters) -> FirmwareResult<()> {
        // UEFI services are called via function pointers
        // The service number is passed in RAX
        let service = regs.rax;
        
        match service {
            // ==================== Runtime Services ====================
            // GetTime (0x01)
            0x01 => {
                let time = self.runtime_services.get_time();
                let buffer = regs.rcx as usize;
                if buffer + 16 <= memory.len() {
                    // Write EFI_TIME structure (16 bytes)
                    memory[buffer..buffer+2].copy_from_slice(&time.year.to_le_bytes());
                    memory[buffer+2] = time.month;
                    memory[buffer+3] = time.day;
                    memory[buffer+4] = time.hour;
                    memory[buffer+5] = time.minute;
                    memory[buffer+6] = time.second;
                    memory[buffer+7] = 0; // Pad1
                    memory[buffer+8..buffer+12].copy_from_slice(&time.nanosecond.to_le_bytes());
                    memory[buffer+12..buffer+14].copy_from_slice(&time.timezone.to_le_bytes());
                    memory[buffer+14] = time.daylight;
                    memory[buffer+15] = 0; // Pad2
                    regs.rax = 0;  // EFI_SUCCESS
                } else {
                    regs.rax = 0x8000000000000002;  // EFI_INVALID_PARAMETER
                }
            }
            // SetTime (0x02)
            0x02 => {
                let buffer = regs.rcx as usize;
                if buffer + 16 <= memory.len() {
                    let time = EfiTime {
                        year: u16::from_le_bytes([memory[buffer], memory[buffer+1]]),
                        month: memory[buffer+2],
                        day: memory[buffer+3],
                        hour: memory[buffer+4],
                        minute: memory[buffer+5],
                        second: memory[buffer+6],
                        nanosecond: u32::from_le_bytes([memory[buffer+8], memory[buffer+9], memory[buffer+10], memory[buffer+11]]),
                        timezone: i16::from_le_bytes([memory[buffer+12], memory[buffer+13]]),
                        daylight: memory[buffer+14],
                    };
                    self.runtime_services.set_time(time);
                    regs.rax = 0;
                } else {
                    regs.rax = 0x8000000000000002;
                }
            }
            // GetVariable (0x03)
            0x03 => {
                // Variable name at RCX, GUID at RDX, attributes at R8, data size at R9
                regs.rax = 0x8000000000000005;  // EFI_NOT_FOUND
            }
            // SetVariable (0x04)
            0x04 => {
                regs.rax = 0;  // EFI_SUCCESS
            }
            
            // ==================== Boot Services ====================
            // AllocatePages (0x10)
            0x10 => {
                // RCX = AllocateType, RDX = MemoryType, R8 = Pages, R9 = Memory
                let pages = regs.r8;
                let memory_type = match regs.rdx as u32 {
                    1 => EfiMemoryType::LoaderCode,
                    2 => EfiMemoryType::LoaderData,
                    3 => EfiMemoryType::BootServicesCode,
                    4 => EfiMemoryType::BootServicesData,
                    _ => EfiMemoryType::LoaderData,
                };
                if let Some(addr) = self.boot_services.allocate_pages(pages, memory_type) {
                    // Write address to [R9]
                    let out_ptr = regs.r9 as usize;
                    if out_ptr + 8 <= memory.len() {
                        memory[out_ptr..out_ptr+8].copy_from_slice(&addr.to_le_bytes());
                    }
                    regs.rax = 0;  // EFI_SUCCESS
                } else {
                    regs.rax = 0x8000000000000009;  // EFI_OUT_OF_RESOURCES
                }
            }
            // FreePages (0x11)
            0x11 => {
                let addr = regs.rcx;
                let pages = regs.rdx;
                self.boot_services.free_pages(addr, pages);
                regs.rax = 0;
            }
            // GetMemoryMap (0x12)
            0x12 => {
                // RCX = MemoryMapSize ptr, RDX = MemoryMap buffer, R8 = MapKey ptr,
                // R9 = DescriptorSize ptr, stack = DescriptorVersion ptr
                let size_ptr = regs.rcx as usize;
                let buffer = regs.rdx;
                let key_ptr = regs.r8 as usize;
                let desc_size_ptr = regs.r9 as usize;
                
                // Read requested size
                let requested_size = if size_ptr + 8 <= memory.len() {
                    u64::from_le_bytes([
                        memory[size_ptr], memory[size_ptr+1], memory[size_ptr+2], memory[size_ptr+3],
                        memory[size_ptr+4], memory[size_ptr+5], memory[size_ptr+6], memory[size_ptr+7],
                    ])
                } else { 0 };
                
                let (map_size, desc_size, key) = self.boot_services.write_memory_map(memory, buffer, requested_size);
                
                // Write outputs
                if size_ptr + 8 <= memory.len() {
                    memory[size_ptr..size_ptr+8].copy_from_slice(&map_size.to_le_bytes());
                }
                if key_ptr + 8 <= memory.len() {
                    memory[key_ptr..key_ptr+8].copy_from_slice(&key.to_le_bytes());
                }
                if desc_size_ptr + 8 <= memory.len() {
                    memory[desc_size_ptr..desc_size_ptr+8].copy_from_slice(&desc_size.to_le_bytes());
                }
                
                if map_size > requested_size && requested_size > 0 {
                    regs.rax = 0x8000000000000005;  // EFI_BUFFER_TOO_SMALL
                } else {
                    regs.rax = 0;  // EFI_SUCCESS
                }
            }
            // AllocatePool (0x13)
            0x13 => {
                let memory_type = match regs.rcx as u32 {
                    1 => EfiMemoryType::LoaderCode,
                    2 => EfiMemoryType::LoaderData,
                    _ => EfiMemoryType::LoaderData,
                };
                let size = regs.rdx;
                if let Some(addr) = self.boot_services.allocate_pool(size, memory_type) {
                    let out_ptr = regs.r8 as usize;
                    if out_ptr + 8 <= memory.len() {
                        memory[out_ptr..out_ptr+8].copy_from_slice(&addr.to_le_bytes());
                    }
                    regs.rax = 0;
                } else {
                    regs.rax = 0x8000000000000009;
                }
            }
            // FreePool (0x14)
            0x14 => {
                // Pool memory is tracked with pages
                regs.rax = 0;
            }
            // CreateEvent (0x15)
            0x15 => {
                let event_type = regs.rcx as u32;
                let notify_tpl = regs.rdx;
                let notify_function = regs.r8;
                let notify_context = regs.r9;
                let handle = self.boot_services.create_event(event_type, notify_tpl, notify_function, notify_context);
                // Return handle via stack parameter
                regs.rax = 0;
            }
            // SignalEvent (0x16)
            0x16 => {
                self.boot_services.signal_event(regs.rcx);
                regs.rax = 0;
            }
            // CloseEvent (0x17)
            0x17 => {
                self.boot_services.close_event(regs.rcx);
                regs.rax = 0;
            }
            // ExitBootServices (0x20)
            0x20 => {
                let map_key = regs.rdx;
                match self.boot_services.exit_boot_services(map_key) {
                    Ok(()) => regs.rax = 0,
                    Err(_) => regs.rax = 0x8000000000000002,  // EFI_INVALID_PARAMETER
                }
            }
            // LocateProtocol (0x30)
            0x30 => {
                // RCX = Protocol GUID ptr, RDX = Registration (optional), R8 = Interface ptr
                // For now, return EFI_NOT_FOUND for most protocols
                regs.rax = 0x8000000000000005;  // EFI_NOT_FOUND
            }
            // LocateHandle (0x31)
            0x31 => {
                regs.rax = 0x8000000000000005;  // EFI_NOT_FOUND
            }
            // HandleProtocol (0x32)
            0x32 => {
                // RCX = Handle, RDX = Protocol GUID ptr, R8 = Interface ptr
                regs.rax = 0x8000000000000003;  // EFI_UNSUPPORTED
            }
            // OpenProtocol (0x33)
            0x33 => {
                regs.rax = 0x8000000000000003;
            }
            // CloseProtocol (0x34)
            0x34 => {
                regs.rax = 0;
            }
            
            // ==================== Runtime Services (0x40+) ====================
            // ResetSystem (0x40)
            0x40 => {
                let reset_type = match regs.rcx {
                    0 => ResetType::Cold,
                    1 => ResetType::Warm,
                    2 => ResetType::Shutdown,
                    _ => ResetType::PlatformSpecific,
                };
                self.runtime_services.reset_system(reset_type);
                regs.rax = 0;
            }
            
            // SetVirtualAddressMap (0x41)
            0x41 => {
                // Required for transitioning to virtual mode
                regs.rax = 0;  // EFI_SUCCESS
            }
            
            // ConvertPointer (0x42)
            0x42 => {
                regs.rax = 0;
            }
            
            _ => {
                // Unknown service
                log::debug!("UEFI: Unknown service {:#x}", service);
                regs.rax = 0x8000000000000003;  // EFI_UNSUPPORTED
            }
        }
        
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
