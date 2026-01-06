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
    /// Pixel mask info (for BitMask pixel format)
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub reserved_mask: u32,
}

/// Graphics Output Protocol
#[derive(Debug, Clone)]
pub struct GraphicsOutputProtocol {
    pub mode_info: GopModeInfo,
    pub framebuffer_base: u64,
    pub framebuffer_size: u64,
    pub current_mode: u32,
    pub max_mode: u32,
    /// Available modes
    pub modes: Vec<GopModeInfo>,
}

impl GraphicsOutputProtocol {
    pub fn new(width: u32, height: u32, fb_base: u64) -> Self {
        let fb_size = (width * height * 4) as u64;  // 32bpp
        
        // BGR pixel masks (common UEFI format)
        let red_mask = 0x00FF0000u32;
        let green_mask = 0x0000FF00u32;
        let blue_mask = 0x000000FFu32;
        let reserved_mask = 0xFF000000u32;
        
        // Define available modes
        let modes = vec![
            GopModeInfo {
                version: 0,
                horizontal_resolution: 640,
                vertical_resolution: 480,
                pixel_format: 1,
                pixels_per_scan_line: 640,
                red_mask,
                green_mask,
                blue_mask,
                reserved_mask,
            },
            GopModeInfo {
                version: 0,
                horizontal_resolution: 800,
                vertical_resolution: 600,
                pixel_format: 1,
                pixels_per_scan_line: 800,
                red_mask,
                green_mask,
                blue_mask,
                reserved_mask,
            },
            GopModeInfo {
                version: 0,
                horizontal_resolution: 1024,
                vertical_resolution: 768,
                pixel_format: 1,
                pixels_per_scan_line: 1024,
                red_mask,
                green_mask,
                blue_mask,
                reserved_mask,
            },
            GopModeInfo {
                version: 0,
                horizontal_resolution: 1280,
                vertical_resolution: 1024,
                pixel_format: 1,
                pixels_per_scan_line: 1280,
                red_mask,
                green_mask,
                blue_mask,
                reserved_mask,
            },
        ];
        
        // Find current mode index
        let current_mode = modes.iter()
            .position(|m| m.horizontal_resolution == width && m.vertical_resolution == height)
            .unwrap_or(2) as u32;  // Default to 1024x768
        
        Self {
            mode_info: GopModeInfo {
                version: 0,
                horizontal_resolution: width,
                vertical_resolution: height,
                pixel_format: 1,  // BGRX (common for UEFI)
                pixels_per_scan_line: width,
                red_mask,
                green_mask,
                blue_mask,
                reserved_mask,
            },
            framebuffer_base: fb_base,
            framebuffer_size: fb_size,
            current_mode,
            max_mode: modes.len() as u32,
            modes,
        }
    }
    
    /// Query mode information
    pub fn query_mode(&self, mode_number: u32) -> Option<&GopModeInfo> {
        self.modes.get(mode_number as usize)
    }
    
    /// Set mode
    pub fn set_mode(&mut self, mode_number: u32) -> bool {
        if let Some(mode) = self.modes.get(mode_number as usize) {
            self.mode_info = mode.clone();
            self.current_mode = mode_number;
            self.framebuffer_size = (mode.horizontal_resolution * mode.vertical_resolution * 4) as u64;
            true
        } else {
            false
        }
    }
    
    /// Blt (Block Transfer) operation
    pub fn blt(&self, memory: &mut [u8], operation: GopBltOperation, 
               src_x: u32, src_y: u32, dst_x: u32, dst_y: u32, 
               width: u32, height: u32, delta: u32) -> bool {
        let fb_base = self.framebuffer_base as usize;
        let stride = self.mode_info.pixels_per_scan_line as usize * 4;
        
        match operation {
            GopBltOperation::BltVideoFill => {
                // Fill rectangle with color from src buffer
                // For now, just return success
                true
            }
            GopBltOperation::BltVideoToBltBuffer => {
                // Copy from framebuffer to buffer
                true
            }
            GopBltOperation::BltBufferToVideo => {
                // Copy from buffer to framebuffer
                true
            }
            GopBltOperation::BltVideoToVideo => {
                // Copy within framebuffer
                true
            }
        }
    }
    
    /// Write GOP protocol to guest memory
    pub fn write_to_memory(&self, memory: &mut [u8], addr: usize) -> usize {
        if addr + 64 > memory.len() {
            return 0;
        }
        
        // GOP Protocol structure:
        // 0x00: QueryMode function pointer
        // 0x08: SetMode function pointer
        // 0x10: Blt function pointer
        // 0x18: Mode pointer
        
        // Write function pointers (these are service call stubs)
        let query_mode_fn = 0x50u64;  // GOP_QUERY_MODE service number
        let set_mode_fn = 0x51u64;    // GOP_SET_MODE service number
        let blt_fn = 0x52u64;          // GOP_BLT service number
        let mode_ptr = (addr + 32) as u64;
        
        memory[addr..addr+8].copy_from_slice(&query_mode_fn.to_le_bytes());
        memory[addr+8..addr+16].copy_from_slice(&set_mode_fn.to_le_bytes());
        memory[addr+16..addr+24].copy_from_slice(&blt_fn.to_le_bytes());
        memory[addr+24..addr+32].copy_from_slice(&mode_ptr.to_le_bytes());
        
        // Write Mode structure at offset 32:
        // 0x00: MaxMode (UINT32)
        // 0x04: Mode (UINT32) - current mode
        // 0x08: Info pointer (to GopModeInfo)
        // 0x10: SizeOfInfo (UINTN)
        // 0x18: FrameBufferBase
        // 0x20: FrameBufferSize
        
        let mode_info_ptr = (addr + 64) as u64;
        
        memory[addr+32..addr+36].copy_from_slice(&self.max_mode.to_le_bytes());
        memory[addr+36..addr+40].copy_from_slice(&self.current_mode.to_le_bytes());
        memory[addr+40..addr+48].copy_from_slice(&mode_info_ptr.to_le_bytes());
        memory[addr+48..addr+56].copy_from_slice(&20u64.to_le_bytes()); // sizeof(GopModeInfo)
        memory[addr+56..addr+64].copy_from_slice(&self.framebuffer_base.to_le_bytes());
        memory[addr+64..addr+72].copy_from_slice(&self.framebuffer_size.to_le_bytes());
        
        // Write GopModeInfo at offset 72
        memory[addr+72..addr+76].copy_from_slice(&self.mode_info.version.to_le_bytes());
        memory[addr+76..addr+80].copy_from_slice(&self.mode_info.horizontal_resolution.to_le_bytes());
        memory[addr+80..addr+84].copy_from_slice(&self.mode_info.vertical_resolution.to_le_bytes());
        memory[addr+84..addr+88].copy_from_slice(&self.mode_info.pixel_format.to_le_bytes());
        memory[addr+88..addr+92].copy_from_slice(&self.mode_info.pixels_per_scan_line.to_le_bytes());
        
        92  // Total bytes written
    }
}

/// GOP Blt Operation types
#[derive(Debug, Clone, Copy)]
pub enum GopBltOperation {
    BltVideoFill,
    BltVideoToBltBuffer,
    BltBufferToVideo,
    BltVideoToVideo,
}

/// Simple File System Protocol
#[derive(Debug, Clone)]
pub struct SimpleFileSystemProtocol {
    /// Revision
    pub revision: u64,
    /// Volume label
    pub volume_label: String,
    /// Files in the filesystem (path -> content)
    pub files: HashMap<String, Vec<u8>>,
    /// Open file handles
    pub open_handles: HashMap<u64, FileHandle>,
    /// Next handle ID
    next_handle: u64,
}

#[derive(Debug, Clone)]
pub struct FileHandle {
    pub path: String,
    pub position: u64,
    pub is_directory: bool,
    pub size: u64,
}

impl SimpleFileSystemProtocol {
    pub fn new() -> Self {
        let mut files = HashMap::new();
        
        // Create a minimal ESP structure
        files.insert("\\".to_string(), Vec::new());  // Root directory
        files.insert("\\EFI".to_string(), Vec::new());
        files.insert("\\EFI\\BOOT".to_string(), Vec::new());
        
        Self {
            revision: 0x00010000,  // 1.0
            volume_label: "EFI System Partition".to_string(),
            files,
            open_handles: HashMap::new(),
            next_handle: 1,
        }
    }
    
    /// Add a file to the filesystem
    pub fn add_file(&mut self, path: &str, content: Vec<u8>) {
        self.files.insert(path.to_string(), content);
    }
    
    /// Open volume (returns root directory handle)
    pub fn open_volume(&mut self) -> u64 {
        let handle = self.next_handle;
        self.next_handle += 1;
        
        self.open_handles.insert(handle, FileHandle {
            path: "\\".to_string(),
            position: 0,
            is_directory: true,
            size: 0,
        });
        
        handle
    }
    
    /// Open file relative to directory handle
    pub fn open(&mut self, dir_handle: u64, filename: &str) -> Result<u64, &'static str> {
        let dir = self.open_handles.get(&dir_handle)
            .ok_or("Invalid directory handle")?;
        
        // Build full path
        let full_path = if dir.path == "\\" {
            format!("{}", filename.trim_start_matches('\\'))
        } else {
            format!("{}\\{}", dir.path.trim_end_matches('\\'), filename.trim_start_matches('\\'))
        };
        
        // Normalize path
        let normalized = full_path.replace("/", "\\");
        let normalized = if normalized.starts_with('\\') {
            normalized
        } else {
            format!("\\{}", normalized)
        };
        
        // Check if it's a directory (check for files that start with this path)
        let is_dir = self.files.keys().any(|k| k.starts_with(&format!("{}\\", normalized)));
        
        // Check if file exists
        let file_exists = self.files.contains_key(&normalized);
        
        if !file_exists && !is_dir {
            return Err("File not found");
        }
        
        let size = self.files.get(&normalized).map(|v| v.len() as u64).unwrap_or(0);
        
        let handle = self.next_handle;
        self.next_handle += 1;
        
        self.open_handles.insert(handle, FileHandle {
            path: normalized,
            position: 0,
            is_directory: is_dir,
            size,
        });
        
        Ok(handle)
    }
    
    /// Open file with UEFI style (mode and attributes) 
    pub fn open_with_mode(&mut self, dir_handle: u64, filename: &str, _mode: u64, _attributes: u64) -> Option<u64> {
        self.open(dir_handle, filename).ok()
    }
    
    /// Read file content
    pub fn read(&mut self, handle: u64, buffer_size: usize) -> Result<Vec<u8>, &'static str> {
        let file = self.open_handles.get_mut(&handle)
            .ok_or("Invalid handle")?;
        
        if file.is_directory {
            return Err("Cannot read directory");
        }
        
        let content = self.files.get(&file.path)
            .ok_or("File not found")?;
        let start = file.position as usize;
        let end = (start + buffer_size).min(content.len());
        
        if start >= content.len() {
            return Ok(Vec::new());  // EOF
        }
        
        let data = content[start..end].to_vec();
        file.position = end as u64;
        
        Ok(data)
    }
    
    /// Get file info
    pub fn get_info(&self, handle: u64) -> Result<FileInfo, &'static str> {
        let file = self.open_handles.get(&handle)
            .ok_or("Invalid handle")?;
        
        // For directories, return basic info
        if file.is_directory {
            return Ok(FileInfo {
                size: 0,
                file_size: 0,
                physical_size: 0,
                create_time: EfiTime::default(),
                last_access_time: EfiTime::default(),
                modification_time: EfiTime::default(),
                attribute: 0x10,  // EFI_FILE_DIRECTORY
                file_name: file.path.rsplit('\\').next().unwrap_or("").to_string(),
            });
        }
        
        let content = self.files.get(&file.path)
            .ok_or("File not found")?;
        
        Ok(FileInfo {
            size: content.len() as u64,
            file_size: content.len() as u64,
            physical_size: ((content.len() + 511) / 512 * 512) as u64,
            create_time: EfiTime::default(),
            last_access_time: EfiTime::default(),
            modification_time: EfiTime::default(),
            attribute: 0,  // Regular file
            file_name: file.path.rsplit('\\').next().unwrap_or("").to_string(),
        })
    }
    
    /// Set file position
    pub fn set_position(&mut self, handle: u64, position: u64) -> bool {
        if let Some(file) = self.open_handles.get_mut(&handle) {
            file.position = position;
            true
        } else {
            false
        }
    }
    
    /// Close file handle
    pub fn close(&mut self, handle: u64) -> bool {
        self.open_handles.remove(&handle).is_some()
    }
    
    /// Write protocol to guest memory
    pub fn write_to_memory(&self, memory: &mut [u8], addr: usize) -> usize {
        if addr + 24 > memory.len() {
            return 0;
        }
        
        // SimpleFileSystem Protocol structure:
        // 0x00: Revision (UINT64)
        // 0x08: OpenVolume function pointer
        
        memory[addr..addr+8].copy_from_slice(&self.revision.to_le_bytes());
        memory[addr+8..addr+16].copy_from_slice(&0x60u64.to_le_bytes()); // SFS_OPEN_VOLUME service
        
        16
    }
}

/// File information structure
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub size: u64,
    pub file_size: u64,
    pub physical_size: u64,
    pub create_time: EfiTime,
    pub last_access_time: EfiTime,
    pub modification_time: EfiTime,
    pub attribute: u64,
    pub file_name: String,
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
/// 
/// Provides runtime services available after ExitBootServices.
/// These services remain available to the OS during the RT phase.
pub struct UefiRuntimeServices {
    /// NVRAM variables (name -> (attributes, data))
    variables: HashMap<String, (u32, Vec<u8>)>,
    /// Current time
    time: EfiTime,
    /// Wakeup alarm time (if set)
    wakeup_time: Option<EfiTime>,
    /// Wakeup alarm enabled
    wakeup_enabled: bool,
    /// Reset type for next reset
    pending_reset: Option<ResetType>,
    /// High monotonic count (upper 32 bits)
    high_monotonic_count: u32,
    /// Virtual address map applied
    virtual_mode_active: bool,
    /// Virtual address offset (for pointer conversion)
    virtual_offset: i64,
    /// Capsule update data (if pending)
    pending_capsule: Option<Vec<u8>>,
    /// Whether boot services have been exited
    boot_services_exited: bool,
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
        // Initialize standard UEFI variables
        let mut variables = HashMap::new();
        
        // SecureBoot variable (0 = disabled, 1 = enabled)
        variables.insert(
            "SecureBoot".to_string(),
            (0x06, vec![0x00])  // BS+RT, disabled by default
        );
        
        // SetupMode variable (1 = setup mode, 0 = user mode)
        variables.insert(
            "SetupMode".to_string(),
            (0x06, vec![0x01])  // BS+RT, setup mode
        );
        
        Self {
            variables,
            time: EfiTime::default(),
            wakeup_time: None,
            wakeup_enabled: false,
            pending_reset: None,
            high_monotonic_count: 0,
            virtual_mode_active: false,
            virtual_offset: 0,
            pending_capsule: None,
            boot_services_exited: false,
        }
    }
    
    /// Mark boot services as exited (called by ExitBootServices)
    pub fn exit_boot_services(&mut self) {
        self.boot_services_exited = true;
        // Remove variables with BS (Boot Services) only attribute
        self.variables.retain(|_, (attr, _)| {
            // Keep if has RT (Runtime) attribute (bit 2)
            (*attr & 0x04) != 0
        });
    }
    
    /// Check if boot services have exited
    pub fn are_boot_services_exited(&self) -> bool {
        self.boot_services_exited
    }
    
    /// Get time
    pub fn get_time(&self) -> EfiTime {
        self.time
    }
    
    /// Set time
    pub fn set_time(&mut self, time: EfiTime) {
        self.time = time;
    }
    
    /// Get wakeup time
    pub fn get_wakeup_time(&self) -> (bool, bool, Option<EfiTime>) {
        (self.wakeup_enabled, self.wakeup_time.is_some(), self.wakeup_time)
    }
    
    /// Set wakeup time
    pub fn set_wakeup_time(&mut self, enable: bool, time: Option<EfiTime>) {
        self.wakeup_enabled = enable;
        self.wakeup_time = time;
    }
    
    /// Get variable
    pub fn get_variable(&self, name: &str) -> Option<(u32, &[u8])> {
        self.variables.get(name).map(|(attr, data)| (*attr, data.as_slice()))
    }
    
    /// Set variable
    pub fn set_variable(&mut self, name: &str, attributes: u32, data: Vec<u8>) {
        if data.is_empty() {
            // Empty data = delete variable
            self.variables.remove(name);
        } else {
            self.variables.insert(name.to_string(), (attributes, data));
        }
    }
    
    /// Get next variable name (for enumeration)
    pub fn get_next_variable_name(&self, current: Option<&str>) -> Option<String> {
        let names: Vec<_> = self.variables.keys().collect();
        match current {
            None => names.first().map(|s| (*s).clone()),
            Some(curr) => {
                let pos = names.iter().position(|&n| n == curr)?;
                names.get(pos + 1).map(|s| (*s).clone())
            }
        }
    }
    
    /// Get next high monotonic count
    pub fn get_next_high_monotonic_count(&mut self) -> u32 {
        self.high_monotonic_count = self.high_monotonic_count.wrapping_add(1);
        self.high_monotonic_count
    }
    
    /// Set virtual address map
    /// 
    /// Called by OS to switch runtime services to virtual addressing.
    /// After this call, all runtime service pointers are converted.
    pub fn set_virtual_address_map(&mut self, offset: i64) -> bool {
        if self.virtual_mode_active {
            return false;  // Can only be called once
        }
        self.virtual_mode_active = true;
        self.virtual_offset = offset;
        true
    }
    
    /// Convert pointer (physical to virtual or vice versa)
    pub fn convert_pointer(&self, addr: u64) -> u64 {
        if self.virtual_mode_active {
            (addr as i64 + self.virtual_offset) as u64
        } else {
            addr
        }
    }
    
    /// Check if virtual mode is active
    pub fn is_virtual_mode(&self) -> bool {
        self.virtual_mode_active
    }
    
    /// Request reset
    pub fn reset_system(&mut self, reset_type: ResetType) {
        self.pending_reset = Some(reset_type);
    }
    
    /// Get pending reset (and clear it)
    pub fn take_pending_reset(&mut self) -> Option<ResetType> {
        self.pending_reset.take()
    }
    
    /// Check if reset is pending
    pub fn is_reset_pending(&self) -> bool {
        self.pending_reset.is_some()
    }
    
    /// Update capsule (firmware update mechanism)
    pub fn update_capsule(&mut self, capsule_data: Vec<u8>) -> bool {
        // Basic validation: UEFI capsule header starts with GUID
        if capsule_data.len() < 64 {
            return false;
        }
        self.pending_capsule = Some(capsule_data);
        true
    }
    
    /// Query capsule capabilities
    pub fn query_capsule_capabilities(&self) -> (u64, u32) {
        // Return (MaximumCapsuleSize, ResetType)
        // 64MB max, cold reset required
        (64 * 1024 * 1024, 0)
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
    file_system: SimpleFileSystemProtocol,
    system_table: EfiSystemTable,
    version: String,
    /// Protocol addresses in guest memory
    gop_protocol_addr: u64,
    sfs_protocol_addr: u64,
}

impl UefiFirmware {
    pub fn new(config: UefiConfig) -> Self {
        let gop = GraphicsOutputProtocol::new(
            config.fb_width,
            config.fb_height,
            0xFD000000,  // Framebuffer base
        );
        
        let mut file_system = SimpleFileSystemProtocol::new();
        
        // Add default EFI boot file path
        // In real implementation, this would come from the attached disk
        file_system.add_file("\\EFI\\BOOT\\BOOTX64.EFI", Vec::new());
        
        Self {
            boot_services: UefiBootServices::new(config.memory_mb),
            runtime_services: UefiRuntimeServices::new(),
            gop,
            file_system,
            config,
            system_table: EfiSystemTable::default(),
            version: String::from("NexaUEFI 1.0"),
            gop_protocol_addr: 0,
            sfs_protocol_addr: 0,
        }
    }
    
    /// Get GOP
    pub fn gop(&self) -> &GraphicsOutputProtocol {
        &self.gop
    }
    
    /// Get mutable GOP
    pub fn gop_mut(&mut self) -> &mut GraphicsOutputProtocol {
        &mut self.gop
    }
    
    /// Get file system
    pub fn file_system(&self) -> &SimpleFileSystemProtocol {
        &self.file_system
    }
    
    /// Get mutable file system
    pub fn file_system_mut(&mut self) -> &mut SimpleFileSystemProtocol {
        &mut self.file_system
    }
    
    /// Add EFI file to the virtual filesystem
    pub fn add_efi_file(&mut self, path: &str, content: Vec<u8>) {
        self.file_system.add_file(path, content);
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
            // Use selector 0x08 (64-bit kernel code segment, DPL=0)
            0xEA,                         // JMP FAR
            0x00, 0x71, 0x00, 0x00,       // Offset: 0x7100
            0x08, 0x00,                   // Selector: 0x08 (64-bit code, DPL=0)
        ];
        let pei_addr = 0x7030usize;
        if pei_addr + pei_code.len() <= memory.len() {
            memory[pei_addr..pei_addr + pei_code.len()].copy_from_slice(pei_code);
        }
        
        // 64-bit DXE code at 0x7100
        // Complete UEFI DXE→BDS implementation
        // 
        // DXE (Driver Execution Environment):
        //   - Initialize console, display boot logo
        //   - Set up UEFI System Table at 0x200000
        //   - Install Boot Services and Runtime Services
        //
        // BDS (Boot Device Selection):
        //   - Enumerate boot devices
        //   - Load EFI application from ESP
        //   - Transfer control to OS loader
        //
        // Memory Layout:
        //   0x7100      - DXE entry point (64-bit long mode)
        //   0x7200      - BDS boot device selection code
        //   0x7300      - EFI application loader
        //   0x7400      - Service call dispatcher
        //   0x80000     - GDT (set up by PEI)
        //   0x100000    - Page tables (PML4)
        //   0x200000    - UEFI System Table
        //   0x201000    - Boot Services Table
        //   0x202000    - Runtime Services Table
        //   0x210000    - UEFI service call vector
        //   0x300000    - EFI application load address
        //   0xB8000     - VGA text buffer
        //   0xFD000000  - GOP framebuffer
        
        let dxe_code: &[u8] = &[
            // ========== DXE Phase: 64-bit Long Mode Entry ==========
            // Set up 64-bit data segments
            0x48, 0x31, 0xC0,             // XOR RAX, RAX
            0x8E, 0xD8,                   // MOV DS, AX
            0x8E, 0xC0,                   // MOV ES, AX
            0x8E, 0xD0,                   // MOV SS, AX
            
            // Set up stack at 0x80000 (512KB, grows down)
            0x48, 0xBC,                   // MOV RSP, imm64
            0x00, 0x00, 0x08, 0x00,       // 0x80000
            0x00, 0x00, 0x00, 0x00,
            
            // ========== DXE: Clear VGA Screen ==========
            // Use REP STOSQ for efficient fill (500 qwords = 4000 bytes)
            0x48, 0xBF,                   // MOV RDI, imm64
            0x00, 0x80, 0x0B, 0x00,       // 0xB8000
            0x00, 0x00, 0x00, 0x00,
            0x48, 0xB8,                   // MOV RAX, imm64
            0x20, 0x1F, 0x20, 0x1F,       // 0x1F201F201F201F20 (space + blue bg)
            0x20, 0x1F, 0x20, 0x1F,
            0xB9, 0xF4, 0x01, 0x00, 0x00, // MOV ECX, 500
            0xFC,                         // CLD
            0xF3, 0x48, 0xAB,             // REP STOSQ
            
            // ========== DXE: Display "NexaUEFI" Boot Banner ==========
            // Row 0: "NexaUEFI 1.0" (centered at column 34)
            0x48, 0xBF,                   // MOV RDI, imm64
            0x44, 0x80, 0x0B, 0x00,       // 0xB8000 + 34*2 = 0xB8044
            0x00, 0x00, 0x00, 0x00,
            // "Nexa" = 0x1F611F781F651F4E
            0x48, 0xB8,                   // MOV RAX, imm64
            0x4E, 0x1F, 0x65, 0x1F,       // 'N' 0x1F 'e' 0x1F
            0x78, 0x1F, 0x61, 0x1F,       // 'x' 0x1F 'a' 0x1F
            0x48, 0x89, 0x07,             // MOV [RDI], RAX
            // "UEFI" = 0x1F491F461F451F55
            0x48, 0xB8,                   // MOV RAX, imm64
            0x55, 0x1F, 0x45, 0x1F,       // 'U' 0x1F 'E' 0x1F
            0x46, 0x1F, 0x49, 0x1F,       // 'F' 0x1F 'I' 0x1F
            0x48, 0x89, 0x47, 0x08,       // MOV [RDI+8], RAX
            // " 1.0" = 0x1F301F2E1F311F20
            0x48, 0xB8,                   // MOV RAX, imm64
            0x20, 0x1F, 0x31, 0x1F,       // ' ' 0x1F '1' 0x1F
            0x2E, 0x1F, 0x30, 0x1F,       // '.' 0x1F '0' 0x1F
            0x48, 0x89, 0x47, 0x10,       // MOV [RDI+16], RAX
            
            // ========== DXE: Initialize UEFI System Table ==========
            // System Table at 0x200000
            // Write EFI_SYSTEM_TABLE_SIGNATURE "IBI SYST" = 0x5453595320494249
            0x48, 0xBF,                   // MOV RDI, imm64
            0x00, 0x00, 0x20, 0x00,       // 0x200000
            0x00, 0x00, 0x00, 0x00,
            0x48, 0xB8,                   // MOV RAX, imm64
            0x49, 0x42, 0x49, 0x20,       // "IBI "
            0x53, 0x59, 0x53, 0x54,       // "SYST"
            0x48, 0x89, 0x07,             // MOV [RDI], RAX (signature)
            // Revision: 0x0002004E (UEFI 2.78)
            0xB8, 0x4E, 0x00, 0x02, 0x00, // MOV EAX, 0x0002004E
            0x89, 0x47, 0x08,             // MOV [RDI+8], EAX
            // Header size: 120 bytes
            0xB8, 0x78, 0x00, 0x00, 0x00, // MOV EAX, 120
            0x89, 0x47, 0x0C,             // MOV [RDI+12], EAX
            // Boot Services pointer at offset 0x60 -> 0x201000
            0x48, 0xB8,                   // MOV RAX, imm64
            0x00, 0x10, 0x20, 0x00,       // 0x201000
            0x00, 0x00, 0x00, 0x00,
            0x48, 0x89, 0x47, 0x60,       // MOV [RDI+0x60], RAX
            // Runtime Services pointer at offset 0x58 -> 0x202000
            0x48, 0xB8,                   // MOV RAX, imm64
            0x00, 0x20, 0x20, 0x00,       // 0x202000
            0x00, 0x00, 0x00, 0x00,
            0x48, 0x89, 0x47, 0x58,       // MOV [RDI+0x58], RAX
            
            // ========== DXE: Install UEFI Service Call Vector ==========
            // Service call handler at 0x210000
            // When guest calls OUT 0xE9, <service_num>, we trap to hypervisor
            // This is a VMCALL/hypercall mechanism for UEFI services
            0x48, 0xBF,                   // MOV RDI, imm64
            0x00, 0x00, 0x21, 0x00,       // 0x210000
            0x00, 0x00, 0x00, 0x00,
            // Write service dispatcher code
            // INT3 (0xCC) triggers service dispatch in hypervisor
            0xC6, 0x07, 0xCC,             // MOV BYTE [RDI], 0xCC (INT3)
            0xC6, 0x47, 0x01, 0xC3,       // MOV BYTE [RDI+1], 0xC3 (RET)
            
            // ========== Display "Booting..." on Row 2 ==========
            0x48, 0xBF,                   // MOV RDI, imm64
            0x40, 0x81, 0x0B, 0x00,       // 0xB8000 + 160*2 = 0xB8140 (row 2)
            0x00, 0x00, 0x00, 0x00,
            // "Boot" = 0x1F741F6F1F6F1F42
            0x48, 0xB8,                   // MOV RAX, imm64
            0x42, 0x0E, 0x6F, 0x0E,       // 'B' 0x0E 'o' 0x0E (yellow on black)
            0x6F, 0x0E, 0x74, 0x0E,       // 'o' 0x0E 't' 0x0E
            0x48, 0x89, 0x07,             // MOV [RDI], RAX
            // "ing." = 0x1F2E1F671F6E1F69
            0x48, 0xB8,                   // MOV RAX, imm64
            0x69, 0x0E, 0x6E, 0x0E,       // 'i' 0x0E 'n' 0x0E
            0x67, 0x0E, 0x2E, 0x0E,       // 'g' 0x0E '.' 0x0E
            0x48, 0x89, 0x47, 0x08,       // MOV [RDI+8], RAX
            // ".." for animation
            0x48, 0xB8,                   // MOV RAX, imm64
            0x2E, 0x0E, 0x2E, 0x0E,       // '.' 0x0E '.' 0x0E
            0x20, 0x0E, 0x20, 0x0E,       // ' ' 0x0E ' ' 0x0E
            0x48, 0x89, 0x47, 0x10,       // MOV [RDI+16], RAX
            
            // ========== BDS Phase: Jump to Boot Device Selection ==========
            // DXE code is 239 bytes, JMP at offset 234 (0x71EA)
            // Target 0x7200, rel32 = 0x7200 - (0x71EA + 5) = 0x11
            0xE9, 0x11, 0x00, 0x00, 0x00, // JMP rel32 to 0x7200
        ];
        let dxe_addr = 0x7100usize;
        if dxe_addr + dxe_code.len() <= memory.len() {
            memory[dxe_addr..dxe_addr + dxe_code.len()].copy_from_slice(dxe_code);
        }
        
        // ========== BDS Phase Code at 0x7200 ==========
        // Boot Device Selection - enumerate devices, load EFI app
        //
        // BDS Flow:
        // 1. Display "Searching for boot device..."
        // 2. Check if EFI image exists at 0x300000 (pre-loaded by hypervisor)
        // 3. If MZ header found, parse PE and jump to entry
        // 4. If not found, display "No bootable device" and halt
        //
        let bds_code: &[u8] = &[
            // ========== BDS: Display Status Message ==========
            0x48, 0xBF,                   // MOV RDI, imm64
            0x80, 0x82, 0x0B, 0x00,       // 0xB8280 (row 4)
            0x00, 0x00, 0x00, 0x00,
            0x48, 0xB8,                   // MOV RAX, "BDS:"
            0x42, 0x0A, 0x44, 0x0A, 0x53, 0x0A, 0x3A, 0x0A,
            0x48, 0x89, 0x07,             // MOV [RDI], RAX
            0x48, 0xB8,                   // MOV RAX, " Sca"
            0x20, 0x0A, 0x53, 0x0A, 0x63, 0x0A, 0x61, 0x0A,
            0x48, 0x89, 0x47, 0x08,       // MOV [RDI+8], RAX
            0x48, 0xB8,                   // MOV RAX, "nnin"
            0x6E, 0x0A, 0x6E, 0x0A, 0x69, 0x0A, 0x6E, 0x0A,
            0x48, 0x89, 0x47, 0x10,       // MOV [RDI+16], RAX
            0x48, 0xB8,                   // MOV RAX, "g..."
            0x67, 0x0A, 0x2E, 0x0A, 0x2E, 0x0A, 0x2E, 0x0A,
            0x48, 0x89, 0x47, 0x18,       // MOV [RDI+24], RAX
            
            // ========== BDS: Check for EFI Image at 0x300000 ==========
            0x48, 0xBE,                   // MOV RSI, imm64 (EFI base)
            0x00, 0x00, 0x30, 0x00,       // 0x300000
            0x00, 0x00, 0x00, 0x00,
            0x66, 0x8B, 0x06,             // MOV AX, [RSI] (MZ magic)
            0x66, 0x3D, 0x4D, 0x5A,       // CMP AX, 0x5A4D ("MZ")
            0x75, 0x70,                   // JNE no_boot (relative +112 bytes)
            
            // ========== Valid MZ - Display "Found EFI" ==========
            0x48, 0xBF,                   // MOV RDI, imm64
            0xC0, 0x83, 0x0B, 0x00,       // 0xB83C0 (row 6)
            0x00, 0x00, 0x00, 0x00,
            0x48, 0xB8,                   // MOV RAX, "Foun"
            0x46, 0x0F, 0x6F, 0x0F, 0x75, 0x0F, 0x6E, 0x0F,
            0x48, 0x89, 0x07,             // MOV [RDI], RAX
            0x48, 0xB8,                   // MOV RAX, "d EF"
            0x64, 0x0F, 0x20, 0x0F, 0x45, 0x0F, 0x46, 0x0F,
            0x48, 0x89, 0x47, 0x08,       // MOV [RDI+8], RAX
            0x48, 0xB8,                   // MOV RAX, "I im"
            0x49, 0x0F, 0x20, 0x0F, 0x69, 0x0F, 0x6D, 0x0F,
            0x48, 0x89, 0x47, 0x10,       // MOV [RDI+16], RAX
            0x48, 0xB8,                   // MOV RAX, "age!"
            0x61, 0x0F, 0x67, 0x0F, 0x65, 0x0F, 0x21, 0x0F,
            0x48, 0x89, 0x47, 0x18,       // MOV [RDI+24], RAX
            
            // ========== Parse PE Header ==========
            // RSI = 0x300000 (image base)
            0x8B, 0x46, 0x3C,             // MOV EAX, [RSI+0x3C] (e_lfanew)
            0x48, 0x01, 0xF0,             // ADD RAX, RSI (PE header addr)
            // Verify "PE\0\0" signature
            0x81, 0x38, 0x50, 0x45, 0x00, 0x00, // CMP DWORD [RAX], 0x00004550
            0x75, 0x38,                   // JNE no_boot (relative +56 bytes)
            
            // Get entry point RVA from PE32+ optional header
            // OptionalHeader at PE+0x18, AddressOfEntryPoint at OptHdr+0x10
            0x8B, 0x58, 0x28,             // MOV EBX, [RAX+0x28] (EntryPointRVA)
            0x48, 0x01, 0xF3,             // ADD RBX, RSI (absolute entry point)
            
            // ========== Display "Jumping..." ==========
            0x48, 0xBF,                   // MOV RDI, imm64
            0x00, 0x85, 0x0B, 0x00,       // 0xB8500 (row 8)
            0x00, 0x00, 0x00, 0x00,
            0x48, 0xB8,                   // MOV RAX, "Jump"
            0x4A, 0x0B, 0x75, 0x0B, 0x6D, 0x0B, 0x70, 0x0B,
            0x48, 0x89, 0x07,             // MOV [RDI], RAX
            0x48, 0xB8,                   // MOV RAX, "ing!"
            0x69, 0x0B, 0x6E, 0x0B, 0x67, 0x0B, 0x21, 0x0B,
            0x48, 0x89, 0x47, 0x08,       // MOV [RDI+8], RAX
            
            // ========== Call EFI Entry Point ==========
            // MS x64 ABI: RCX=ImageHandle, RDX=SystemTable
            0x48, 0xC7, 0xC1, 0x01, 0x00, 0x00, 0x00, // MOV RCX, 1
            0x48, 0xBA,                   // MOV RDX, imm64
            0x00, 0x00, 0x20, 0x00,       // 0x200000 (SystemTable)
            0x00, 0x00, 0x00, 0x00,
            0xFF, 0xD3,                   // CALL RBX (entry point)
            
            // If EFI returns, halt
            0xF4,                         // HLT
            0xEB, 0xFD,                   // JMP -3
            
            // ========== No Boot Device Handler ==========
            // (This label is at offset 0x70 from JNE, i.e. 0x7270)
            0x48, 0xBF,                   // MOV RDI, imm64
            0x40, 0x86, 0x0B, 0x00,       // 0xB8640 (row 10)
            0x00, 0x00, 0x00, 0x00,
            0x48, 0xB8,                   // MOV RAX, "No b"
            0x4E, 0x0C, 0x6F, 0x0C, 0x20, 0x0C, 0x62, 0x0C,
            0x48, 0x89, 0x07,             // MOV [RDI], RAX
            0x48, 0xB8,                   // MOV RAX, "oota"
            0x6F, 0x0C, 0x6F, 0x0C, 0x74, 0x0C, 0x61, 0x0C,
            0x48, 0x89, 0x47, 0x08,       // MOV [RDI+8], RAX
            0x48, 0xB8,                   // MOV RAX, "ble "
            0x62, 0x0C, 0x6C, 0x0C, 0x65, 0x0C, 0x20, 0x0C,
            0x48, 0x89, 0x47, 0x10,       // MOV [RDI+16], RAX
            0x48, 0xB8,                   // MOV RAX, "devi"
            0x64, 0x0C, 0x65, 0x0C, 0x76, 0x0C, 0x69, 0x0C,
            0x48, 0x89, 0x47, 0x18,       // MOV [RDI+24], RAX
            0x48, 0xB8,                   // MOV RAX, "ce!!"
            0x63, 0x0C, 0x65, 0x0C, 0x21, 0x0C, 0x21, 0x0C,
            0x48, 0x89, 0x47, 0x20,       // MOV [RDI+32], RAX
            
            // Display "Press F2 for setup..."
            0x48, 0xBF,                   // MOV RDI, imm64
            0xC0, 0x87, 0x0B, 0x00,       // 0xB87C0 (row 12)
            0x00, 0x00, 0x00, 0x00,
            0x48, 0xB8,                   // MOV RAX, "Pres"
            0x50, 0x0F, 0x72, 0x0F, 0x65, 0x0F, 0x73, 0x0F,
            0x48, 0x89, 0x07,             // MOV [RDI], RAX
            0x48, 0xB8,                   // MOV RAX, "s F2"
            0x73, 0x0F, 0x20, 0x0F, 0x46, 0x0F, 0x32, 0x0F,
            0x48, 0x89, 0x47, 0x08,       // MOV [RDI+8], RAX
            0x48, 0xB8,                   // MOV RAX, " for"
            0x20, 0x0F, 0x66, 0x0F, 0x6F, 0x0F, 0x72, 0x0F,
            0x48, 0x89, 0x47, 0x10,       // MOV [RDI+16], RAX
            0x48, 0xB8,                   // MOV RAX, " set"
            0x20, 0x0F, 0x73, 0x0F, 0x65, 0x0F, 0x74, 0x0F,
            0x48, 0x89, 0x47, 0x18,       // MOV [RDI+24], RAX
            0x48, 0xB8,                   // MOV RAX, "up.."
            0x75, 0x0F, 0x70, 0x0F, 0x2E, 0x0F, 0x2E, 0x0F,
            0x48, 0x89, 0x47, 0x20,       // MOV [RDI+32], RAX
            
            // HLT loop
            0xF4,                         // HLT
            0xEB, 0xFD,                   // JMP -3
        ];
        
        let bds_addr = 0x7200usize;
        if bds_addr + bds_code.len() <= memory.len() {
            memory[bds_addr..bds_addr + bds_code.len()].copy_from_slice(bds_code);
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
                    Ok(()) => {
                        // Notify runtime services that boot services have exited
                        self.runtime_services.exit_boot_services();
                        log::info!("[UEFI] ExitBootServices called - transitioning to RT phase");
                        regs.rax = 0;
                    }
                    Err(_) => regs.rax = 0x8000000000000002,  // EFI_INVALID_PARAMETER
                }
            }
            // LocateProtocol (0x30)
            0x30 => {
                // RCX = Protocol GUID ptr, RDX = Registration (optional), R8 = Interface ptr
                let guid_ptr = regs.rcx as usize;
                let interface_ptr = regs.r8 as usize;
                
                if guid_ptr + 16 <= memory.len() && interface_ptr + 8 <= memory.len() {
                    // Read GUID from memory
                    let guid_bytes: [u8; 16] = memory[guid_ptr..guid_ptr+16].try_into().unwrap_or([0; 16]);
                    
                    // Check for known protocols
                    // GOP GUID: 9042a9de-23dc-4a38-96fb-7aded080516a
                    let gop_guid: [u8; 16] = [
                        0xde, 0xa9, 0x42, 0x90, 0xdc, 0x23, 0x38, 0x4a,
                        0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a,
                    ];
                    // Simple File System GUID: 0964e5b22-6459-11d2-8e39-00a0c969723b
                    let sfs_guid: [u8; 16] = [
                        0x22, 0x5b, 0x4e, 0x96, 0x59, 0x64, 0xd2, 0x11,
                        0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b,
                    ];
                    
                    if guid_bytes == gop_guid {
                        // Return GOP protocol interface address
                        // Write GOP structure to a known location and return pointer
                        let gop_addr: u64 = 0x90000;  // GOP protocol structure address
                        self.gop.write_to_memory(memory, gop_addr as usize);
                        memory[interface_ptr..interface_ptr+8].copy_from_slice(&gop_addr.to_le_bytes());
                        regs.rax = 0;  // EFI_SUCCESS
                    } else if guid_bytes == sfs_guid {
                        // Return Simple File System protocol interface address
                        let sfs_addr: u64 = 0x91000;  // SFS protocol structure address
                        // Write SFS protocol structure
                        // For now, just write the OpenVolume function pointer (service 0x60)
                        // EFI_SIMPLE_FILE_SYSTEM_PROTOCOL has Revision + OpenVolume
                        let sfs_struct: [u8; 16] = [
                            // Revision (0x00010000 = 1.0)
                            0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
                            // OpenVolume function pointer (magic value triggers service 0x60)
                            0x60, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00,
                        ];
                        let sfs_offset = sfs_addr as usize;
                        if sfs_offset + 16 <= memory.len() {
                            memory[sfs_offset..sfs_offset+16].copy_from_slice(&sfs_struct);
                        }
                        memory[interface_ptr..interface_ptr+8].copy_from_slice(&sfs_addr.to_le_bytes());
                        regs.rax = 0;  // EFI_SUCCESS
                    } else {
                        // Unknown protocol
                        regs.rax = 0x8000000000000005;  // EFI_NOT_FOUND
                    }
                } else {
                    regs.rax = 0x8000000000000002;  // EFI_INVALID_PARAMETER
                }
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
                // RCX = MemoryMapSize, RDX = DescriptorSize, R8 = DescriptorVersion, R9 = VirtualMap
                // The OS calls this to switch runtime services to virtual addressing
                let _map_size = regs.rcx;
                let _desc_size = regs.rdx;
                let _desc_version = regs.r8;
                let _virtual_map = regs.r9;
                
                // For simplicity, we assume identity mapping (no offset)
                // A real implementation would parse the virtual map and calculate offsets
                if self.runtime_services.set_virtual_address_map(0) {
                    log::info!("[UEFI] SetVirtualAddressMap called - entering virtual mode");
                    regs.rax = 0;  // EFI_SUCCESS
                } else {
                    // Can only be called once
                    regs.rax = 0x8000000000000003;  // EFI_UNSUPPORTED
                }
            }
            
            // ConvertPointer (0x42)
            0x42 => {
                // RCX = DebugDisposition, RDX = Address ptr
                let addr_ptr = regs.rdx as usize;
                if addr_ptr + 8 <= memory.len() {
                    let addr = u64::from_le_bytes([
                        memory[addr_ptr], memory[addr_ptr+1], memory[addr_ptr+2], memory[addr_ptr+3],
                        memory[addr_ptr+4], memory[addr_ptr+5], memory[addr_ptr+6], memory[addr_ptr+7],
                    ]);
                    let converted = self.runtime_services.convert_pointer(addr);
                    memory[addr_ptr..addr_ptr+8].copy_from_slice(&converted.to_le_bytes());
                    regs.rax = 0;
                } else {
                    regs.rax = 0x8000000000000002;
                }
            }
            
            // GetWakeupTime (0x43)
            0x43 => {
                // RCX = Enabled ptr, RDX = Pending ptr, R8 = Time ptr
                let enabled_ptr = regs.rcx as usize;
                let pending_ptr = regs.rdx as usize;
                let time_ptr = regs.r8 as usize;
                
                let (enabled, pending, time) = self.runtime_services.get_wakeup_time();
                
                if enabled_ptr + 1 <= memory.len() {
                    memory[enabled_ptr] = enabled as u8;
                }
                if pending_ptr + 1 <= memory.len() {
                    memory[pending_ptr] = pending as u8;
                }
                if let Some(t) = time {
                    if time_ptr + 16 <= memory.len() {
                        memory[time_ptr..time_ptr+2].copy_from_slice(&t.year.to_le_bytes());
                        memory[time_ptr+2] = t.month;
                        memory[time_ptr+3] = t.day;
                        memory[time_ptr+4] = t.hour;
                        memory[time_ptr+5] = t.minute;
                        memory[time_ptr+6] = t.second;
                        memory[time_ptr+7] = 0;
                        memory[time_ptr+8..time_ptr+12].copy_from_slice(&t.nanosecond.to_le_bytes());
                        memory[time_ptr+12..time_ptr+14].copy_from_slice(&t.timezone.to_le_bytes());
                        memory[time_ptr+14] = t.daylight;
                        memory[time_ptr+15] = 0;
                    }
                }
                regs.rax = 0;
            }
            
            // SetWakeupTime (0x44)
            0x44 => {
                // RCX = Enable, RDX = Time ptr
                let enable = regs.rcx != 0;
                let time_ptr = regs.rdx as usize;
                
                let time = if enable && time_ptr + 16 <= memory.len() {
                    Some(EfiTime {
                        year: u16::from_le_bytes([memory[time_ptr], memory[time_ptr+1]]),
                        month: memory[time_ptr+2],
                        day: memory[time_ptr+3],
                        hour: memory[time_ptr+4],
                        minute: memory[time_ptr+5],
                        second: memory[time_ptr+6],
                        nanosecond: u32::from_le_bytes([
                            memory[time_ptr+8], memory[time_ptr+9],
                            memory[time_ptr+10], memory[time_ptr+11]
                        ]),
                        timezone: i16::from_le_bytes([memory[time_ptr+12], memory[time_ptr+13]]),
                        daylight: memory[time_ptr+14],
                    })
                } else {
                    None
                };
                
                self.runtime_services.set_wakeup_time(enable, time);
                regs.rax = 0;
            }
            
            // GetNextHighMonotonicCount (0x45)
            0x45 => {
                // RCX = HighCount ptr
                let count_ptr = regs.rcx as usize;
                let count = self.runtime_services.get_next_high_monotonic_count();
                if count_ptr + 4 <= memory.len() {
                    memory[count_ptr..count_ptr+4].copy_from_slice(&count.to_le_bytes());
                    regs.rax = 0;
                } else {
                    regs.rax = 0x8000000000000002;
                }
            }
            
            // QueryCapsuleCapabilities (0x46)
            0x46 => {
                // RCX = CapsuleHeaderArray, RDX = CapsuleCount, R8 = MaximumCapsuleSize ptr, R9 = ResetType ptr
                let max_size_ptr = regs.r8 as usize;
                let reset_type_ptr = regs.r9 as usize;
                
                let (max_size, reset_type) = self.runtime_services.query_capsule_capabilities();
                
                if max_size_ptr + 8 <= memory.len() {
                    memory[max_size_ptr..max_size_ptr+8].copy_from_slice(&max_size.to_le_bytes());
                }
                if reset_type_ptr + 4 <= memory.len() {
                    memory[reset_type_ptr..reset_type_ptr+4].copy_from_slice(&reset_type.to_le_bytes());
                }
                regs.rax = 0;
            }
            
            // GetNextVariableName (0x47)
            0x47 => {
                // RCX = VariableNameSize ptr, RDX = VariableName buffer, R8 = VendorGuid ptr
                // For simplicity, return EFI_NOT_FOUND (enumeration not implemented)
                regs.rax = 0x8000000000000005;  // EFI_NOT_FOUND
            }
            
            // ==================== GOP Protocol Services (0x50+) ====================
            // GOP.QueryMode (0x50)
            0x50 => {
                // RCX = Mode number, RDX = SizeOfInfo ptr, R8 = Info ptr
                let mode_num = regs.rcx as u32;
                if let Some(mode_info) = self.gop.query_mode(mode_num) {
                    // Write mode info to buffer
                    let info_ptr = regs.r8 as usize;
                    if info_ptr + 36 <= memory.len() {
                        // EFI_GRAPHICS_OUTPUT_MODE_INFORMATION
                        memory[info_ptr..info_ptr+4].copy_from_slice(&mode_info.version.to_le_bytes());
                        memory[info_ptr+4..info_ptr+8].copy_from_slice(&mode_info.horizontal_resolution.to_le_bytes());
                        memory[info_ptr+8..info_ptr+12].copy_from_slice(&mode_info.vertical_resolution.to_le_bytes());
                        memory[info_ptr+12..info_ptr+16].copy_from_slice(&(mode_info.pixel_format as u32).to_le_bytes());
                        // Pixel bitmask (16 bytes)
                        memory[info_ptr+16..info_ptr+20].copy_from_slice(&mode_info.red_mask.to_le_bytes());
                        memory[info_ptr+20..info_ptr+24].copy_from_slice(&mode_info.green_mask.to_le_bytes());
                        memory[info_ptr+24..info_ptr+28].copy_from_slice(&mode_info.blue_mask.to_le_bytes());
                        memory[info_ptr+28..info_ptr+32].copy_from_slice(&mode_info.reserved_mask.to_le_bytes());
                        memory[info_ptr+32..info_ptr+36].copy_from_slice(&mode_info.pixels_per_scan_line.to_le_bytes());
                        
                        // Write size to size ptr
                        let size_ptr = regs.rdx as usize;
                        if size_ptr + 8 <= memory.len() {
                            memory[size_ptr..size_ptr+8].copy_from_slice(&36u64.to_le_bytes());
                        }
                        regs.rax = 0;  // EFI_SUCCESS
                    } else {
                        regs.rax = 0x8000000000000002;  // EFI_INVALID_PARAMETER
                    }
                } else {
                    regs.rax = 0x8000000000000002;  // EFI_INVALID_PARAMETER
                }
            }
            
            // GOP.SetMode (0x51)
            0x51 => {
                // RCX = Mode number
                let mode_num = regs.rcx as u32;
                if self.gop.set_mode(mode_num) {
                    regs.rax = 0;  // EFI_SUCCESS
                } else {
                    regs.rax = 0x8000000000000003;  // EFI_UNSUPPORTED
                }
            }
            
            // GOP.Blt (0x52)
            0x52 => {
                // RCX = BltBuffer, RDX = BltOperation, R8 = SourceX, R9 = SourceY
                // Stack: DestX, DestY, Width, Height, Delta
                let blt_buffer = regs.rcx;
                let operation = regs.rdx as u32;
                let source_x = regs.r8 as u32;
                let source_y = regs.r9 as u32;
                
                // For now, we support basic BLT operations
                let op = match operation {
                    0 => GopBltOperation::BltVideoFill,
                    1 => GopBltOperation::BltVideoToBltBuffer,
                    2 => GopBltOperation::BltBufferToVideo,
                    3 => GopBltOperation::BltVideoToVideo,
                    _ => {
                        regs.rax = 0x8000000000000002;
                        return Ok(());
                    }
                };
                
                // Basic implementation - copy between buffer and framebuffer
                let fb_base = self.gop.framebuffer_base as usize;
                let fb_size = self.gop.framebuffer_size as usize;
                
                match op {
                    GopBltOperation::BltVideoFill => {
                        // Fill video with single pixel from buffer
                        if blt_buffer as usize + 4 <= memory.len() && fb_base + fb_size <= memory.len() {
                            let pixel = u32::from_le_bytes([
                                memory[blt_buffer as usize],
                                memory[blt_buffer as usize + 1],
                                memory[blt_buffer as usize + 2],
                                memory[blt_buffer as usize + 3],
                            ]);
                            // Fill entire framebuffer (simplified)
                            for offset in (0..fb_size).step_by(4) {
                                memory[fb_base + offset..fb_base + offset + 4]
                                    .copy_from_slice(&pixel.to_le_bytes());
                            }
                        }
                        regs.rax = 0;
                    }
                    _ => {
                        // Other BLT operations - basic success for now
                        regs.rax = 0;
                    }
                }
            }
            
            // GOP.GetMode (0x53) - Get current mode info
            0x53 => {
                // RCX = Mode info output ptr
                let info_ptr = regs.rcx as usize;
                if info_ptr + 48 <= memory.len() {
                    // EFI_GRAPHICS_OUTPUT_PROTOCOL_MODE
                    memory[info_ptr..info_ptr+4].copy_from_slice(&self.gop.max_mode.to_le_bytes());
                    memory[info_ptr+4..info_ptr+8].copy_from_slice(&self.gop.current_mode.to_le_bytes());
                    // Mode info pointer (would be set during protocol install)
                    memory[info_ptr+8..info_ptr+16].copy_from_slice(&0u64.to_le_bytes());
                    // SizeOfInfo
                    memory[info_ptr+16..info_ptr+24].copy_from_slice(&36u64.to_le_bytes());
                    // FrameBufferBase
                    memory[info_ptr+24..info_ptr+32].copy_from_slice(&self.gop.framebuffer_base.to_le_bytes());
                    // FrameBufferSize
                    memory[info_ptr+32..info_ptr+40].copy_from_slice(&self.gop.framebuffer_size.to_le_bytes());
                    regs.rax = 0;
                } else {
                    regs.rax = 0x8000000000000002;
                }
            }
            
            // ==================== Simple File System Protocol Services (0x60+) ====================
            // SFS.OpenVolume (0x60)
            0x60 => {
                // RCX = FileSystem handle (ignored, we have single volume)
                // RDX = Root directory handle output ptr
                let handle = self.file_system.open_volume();
                let out_ptr = regs.rdx as usize;
                if out_ptr + 8 <= memory.len() {
                    memory[out_ptr..out_ptr+8].copy_from_slice(&handle.to_le_bytes());
                    regs.rax = 0;  // EFI_SUCCESS
                } else {
                    regs.rax = 0x8000000000000002;
                }
            }
            
            // File.Open (0x61)
            0x61 => {
                // RCX = Parent handle, RDX = New handle output ptr, R8 = Filename ptr, R9 = OpenMode
                let parent_handle = regs.rcx;
                let new_handle_ptr = regs.rdx as usize;
                let filename_ptr = regs.r8 as usize;
                let _open_mode = regs.r9;
                
                // Read filename (UTF-16 LE, null terminated)
                let mut filename = String::new();
                let mut pos = filename_ptr;
                while pos + 2 <= memory.len() {
                    let ch = u16::from_le_bytes([memory[pos], memory[pos + 1]]);
                    if ch == 0 {
                        break;
                    }
                    if let Some(c) = char::from_u32(ch as u32) {
                        filename.push(c);
                    }
                    pos += 2;
                }
                
                match self.file_system.open(parent_handle, &filename) {
                    Ok(handle) => {
                        if new_handle_ptr + 8 <= memory.len() {
                            memory[new_handle_ptr..new_handle_ptr+8].copy_from_slice(&handle.to_le_bytes());
                            regs.rax = 0;
                        } else {
                            regs.rax = 0x8000000000000002;
                        }
                    }
                    Err(_) => {
                        regs.rax = 0x8000000000000005;  // EFI_NOT_FOUND
                    }
                }
            }
            
            // File.Close (0x62)
            0x62 => {
                let handle = regs.rcx;
                self.file_system.close(handle);
                regs.rax = 0;
            }
            
            // File.Read (0x63)
            0x63 => {
                // RCX = Handle, RDX = BufferSize ptr, R8 = Buffer
                let handle = regs.rcx;
                let size_ptr = regs.rdx as usize;
                let buffer_ptr = regs.r8 as usize;
                
                // Read requested size
                let requested_size = if size_ptr + 8 <= memory.len() {
                    u64::from_le_bytes([
                        memory[size_ptr], memory[size_ptr+1], memory[size_ptr+2], memory[size_ptr+3],
                        memory[size_ptr+4], memory[size_ptr+5], memory[size_ptr+6], memory[size_ptr+7],
                    ]) as usize
                } else {
                    regs.rax = 0x8000000000000002;
                    return Ok(());
                };
                
                match self.file_system.read(handle, requested_size) {
                    Ok(data) => {
                        let actual_size = data.len();
                        if buffer_ptr + actual_size <= memory.len() {
                            memory[buffer_ptr..buffer_ptr + actual_size].copy_from_slice(&data);
                            // Update size with actual bytes read
                            memory[size_ptr..size_ptr+8].copy_from_slice(&(actual_size as u64).to_le_bytes());
                            regs.rax = 0;
                        } else {
                            regs.rax = 0x8000000000000002;
                        }
                    }
                    Err(_) => {
                        regs.rax = 0x8000000000000007;  // EFI_DEVICE_ERROR
                    }
                }
            }
            
            // File.Write (0x64) - Basic stub for EFI write
            0x64 => {
                // Not fully implemented - ESP is typically read-only during boot
                regs.rax = 0x8000000000000003;  // EFI_UNSUPPORTED
            }
            
            // File.GetPosition (0x65)
            0x65 => {
                let handle = regs.rcx;
                let pos_ptr = regs.rdx as usize;
                
                if let Some(fh) = self.file_system.open_handles.get(&handle) {
                    if pos_ptr + 8 <= memory.len() {
                        memory[pos_ptr..pos_ptr+8].copy_from_slice(&fh.position.to_le_bytes());
                        regs.rax = 0;
                    } else {
                        regs.rax = 0x8000000000000002;
                    }
                } else {
                    regs.rax = 0x8000000000000005;  // EFI_NOT_FOUND
                }
            }
            
            // File.SetPosition (0x66)
            0x66 => {
                let handle = regs.rcx;
                let position = regs.rdx;
                self.file_system.set_position(handle, position);
                regs.rax = 0;
            }
            
            // File.GetInfo (0x67)
            0x67 => {
                // RCX = Handle, RDX = InfoType GUID ptr, R8 = BufferSize ptr, R9 = Buffer
                let handle = regs.rcx;
                let buffer_size_ptr = regs.r8 as usize;
                let buffer_ptr = regs.r9 as usize;
                
                match self.file_system.get_info(handle) {
                    Ok(info) => {
                        // EFI_FILE_INFO minimum size: 80 bytes + filename
                        let info_size = 80 + (info.file_name.len() + 1) * 2;
                        
                        // Check buffer size
                        let available = if buffer_size_ptr + 8 <= memory.len() {
                            u64::from_le_bytes([
                                memory[buffer_size_ptr], memory[buffer_size_ptr+1],
                                memory[buffer_size_ptr+2], memory[buffer_size_ptr+3],
                                memory[buffer_size_ptr+4], memory[buffer_size_ptr+5],
                                memory[buffer_size_ptr+6], memory[buffer_size_ptr+7],
                            ]) as usize
                        } else {
                            regs.rax = 0x8000000000000002;
                            return Ok(());
                        };
                        
                        if available < info_size {
                            // Return needed size
                            memory[buffer_size_ptr..buffer_size_ptr+8]
                                .copy_from_slice(&(info_size as u64).to_le_bytes());
                            regs.rax = 0x8000000000000005;  // EFI_BUFFER_TOO_SMALL
                            return Ok(());
                        }
                        
                        if buffer_ptr + info_size <= memory.len() {
                            // Write EFI_FILE_INFO
                            memory[buffer_ptr..buffer_ptr+8].copy_from_slice(&info.size.to_le_bytes());
                            memory[buffer_ptr+8..buffer_ptr+16].copy_from_slice(&info.file_size.to_le_bytes());
                            memory[buffer_ptr+16..buffer_ptr+24].copy_from_slice(&info.physical_size.to_le_bytes());
                            // Create/LastAccess/Modification times (EFI_TIME = 16 bytes each)
                            memory[buffer_ptr+24..buffer_ptr+72].fill(0);  // Zero times for now
                            memory[buffer_ptr+72..buffer_ptr+80].copy_from_slice(&info.attribute.to_le_bytes());
                            
                            // Write filename (UTF-16 LE)
                            let mut fname_offset = buffer_ptr + 80;
                            for ch in info.file_name.encode_utf16() {
                                if fname_offset + 2 <= memory.len() {
                                    memory[fname_offset..fname_offset+2].copy_from_slice(&ch.to_le_bytes());
                                    fname_offset += 2;
                                }
                            }
                            // Null terminator
                            if fname_offset + 2 <= memory.len() {
                                memory[fname_offset..fname_offset+2].copy_from_slice(&0u16.to_le_bytes());
                            }
                            
                            regs.rax = 0;
                        } else {
                            regs.rax = 0x8000000000000002;
                        }
                    }
                    Err(_) => {
                        regs.rax = 0x8000000000000005;
                    }
                }
            }
            
            // ==================== LocateProtocol Enhancement (0x30) ====================
            // Already handled above, but we need to support GOP and SFS protocols
            
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
    
    #[test]
    fn test_gop_modes() {
        let gop = GraphicsOutputProtocol::new(800, 600, 0xFD000000);
        
        // Test mode query
        assert!(gop.query_mode(0).is_some());
        assert!(gop.query_mode(1).is_some());
        assert!(gop.query_mode(2).is_some());
        assert!(gop.query_mode(3).is_some());
        assert!(gop.query_mode(4).is_none());
        
        // Verify mode 0 is 640x480
        let mode0 = gop.query_mode(0).unwrap();
        assert_eq!(mode0.horizontal_resolution, 640);
        assert_eq!(mode0.vertical_resolution, 480);
        
        // Verify mode 2 is 1024x768
        let mode2 = gop.query_mode(2).unwrap();
        assert_eq!(mode2.horizontal_resolution, 1024);
        assert_eq!(mode2.vertical_resolution, 768);
    }
    
    #[test]
    fn test_gop_set_mode() {
        let mut gop = GraphicsOutputProtocol::new(800, 600, 0xFD000000);
        
        // Should be able to set valid modes
        assert!(gop.set_mode(0));
        assert_eq!(gop.current_mode, 0);
        assert_eq!(gop.mode_info.horizontal_resolution, 640);
        
        assert!(gop.set_mode(2));
        assert_eq!(gop.current_mode, 2);
        assert_eq!(gop.mode_info.horizontal_resolution, 1024);
        
        // Invalid mode should fail
        assert!(!gop.set_mode(99));
    }
    
    #[test]
    fn test_gop_framebuffer() {
        let gop = GraphicsOutputProtocol::new(1024, 768, 0xFD000000);
        
        assert_eq!(gop.framebuffer_base, 0xFD000000);
        // 1024 * 768 * 4 bytes per pixel
        assert_eq!(gop.framebuffer_size, 1024 * 768 * 4);
    }
    
    #[test]
    fn test_simple_file_system_basic() {
        let mut sfs = SimpleFileSystemProtocol::new();
        
        // Add a test file
        let test_content = vec![0x4D, 0x5A, 0x90, 0x00];  // MZ header
        sfs.add_file("\\EFI\\BOOT\\BOOTX64.EFI", test_content.clone());
        
        // Open volume
        let root_handle = sfs.open_volume();
        assert!(root_handle != 0);
        
        // Open the file
        let file_handle = sfs.open(root_handle, "\\EFI\\BOOT\\BOOTX64.EFI").unwrap();
        assert!(file_handle != 0);
        
        // Read file content
        let data = sfs.read(file_handle, 1024).unwrap();
        assert_eq!(data, test_content);
        
        // Close handles
        sfs.close(file_handle);
        sfs.close(root_handle);
    }
    
    #[test]
    fn test_simple_file_system_get_info() {
        let mut sfs = SimpleFileSystemProtocol::new();
        
        let test_content = vec![0x00; 1024];
        sfs.add_file("\\TEST.BIN", test_content);
        
        let root = sfs.open_volume();
        let file = sfs.open(root, "\\TEST.BIN").unwrap();
        
        let info = sfs.get_info(file).unwrap();
        assert_eq!(info.file_size, 1024);
        assert!(info.file_name.contains("TEST.BIN"));
    }
    
    #[test]
    fn test_simple_file_system_position() {
        let mut sfs = SimpleFileSystemProtocol::new();
        
        let test_content: Vec<u8> = (0u8..=255).collect();
        sfs.add_file("\\DATA.BIN", test_content);
        
        let root = sfs.open_volume();
        let file = sfs.open(root, "\\DATA.BIN").unwrap();
        
        // Read first 10 bytes
        let data1 = sfs.read(file, 10).unwrap();
        assert_eq!(data1, (0u8..10).collect::<Vec<u8>>());
        
        // Position should have advanced
        let pos = sfs.open_handles.get(&file).unwrap().position;
        assert_eq!(pos, 10);
        
        // Set position to 100
        sfs.set_position(file, 100);
        
        // Read next 10 bytes
        let data2 = sfs.read(file, 10).unwrap();
        assert_eq!(data2, (100u8..110).collect::<Vec<u8>>());
    }
    
    #[test]
    fn test_simple_file_system_directory() {
        let mut sfs = SimpleFileSystemProtocol::new();
        
        sfs.add_file("\\EFI\\BOOT\\BOOTX64.EFI", vec![0xAA, 0xBB]);
        sfs.add_file("\\EFI\\BOOT\\BOOTIA32.EFI", vec![0xCC, 0xDD]);
        
        let root = sfs.open_volume();
        
        // Open directory
        let efi_dir = sfs.open(root, "\\EFI").unwrap();
        
        // Verify it's a directory
        let info = sfs.get_info(efi_dir).unwrap();
        assert!(info.attribute & 0x10 != 0);  // EFI_FILE_DIRECTORY
    }
    
    #[test]
    fn test_uefi_file_system_integration() {
        let mut uefi = UefiFirmware::new(UefiConfig::default());
        
        // Add an EFI file
        let efi_binary = vec![0x4D, 0x5A, 0x90, 0x00, 0x03, 0x00, 0x00, 0x00];
        uefi.add_efi_file("\\EFI\\CUSTOM\\APP.EFI", efi_binary.clone());
        
        // Verify we can access it through file_system
        let root = uefi.file_system_mut().open_volume();
        let file = uefi.file_system_mut().open(root, "\\EFI\\CUSTOM\\APP.EFI").unwrap();
        let data = uefi.file_system_mut().read(file, 1024).unwrap();
        
        assert_eq!(data, efi_binary);
    }
    
    #[test]
    fn test_uefi_gop_service() {
        let mut uefi = UefiFirmware::new(UefiConfig::default());
        let mut memory = vec![0u8; 0x200000];
        
        // Initialize UEFI tables
        uefi.load(&mut memory).unwrap();
        
        let mut regs = ServiceRegisters::default();
        
        // Test GOP.QueryMode (0x50)
        regs.rax = 0x50;
        regs.rcx = 0;  // Mode 0
        regs.rdx = 0x1000;  // Size output
        regs.r8 = 0x1100;   // Info output
        
        uefi.handle_service(&mut memory, &mut regs).unwrap();
        assert_eq!(regs.rax, 0);  // EFI_SUCCESS
        
        // Verify mode info was written
        let width = u32::from_le_bytes([
            memory[0x1104], memory[0x1105], memory[0x1106], memory[0x1107]
        ]);
        assert_eq!(width, 640);  // Mode 0 is 640x480
    }
    
    #[test]
    fn test_uefi_locate_gop_protocol() {
        let mut uefi = UefiFirmware::new(UefiConfig::default());
        let mut memory = vec![0u8; 0x200000];
        
        uefi.load(&mut memory).unwrap();
        
        // Write GOP GUID to memory
        let gop_guid: [u8; 16] = [
            0xde, 0xa9, 0x42, 0x90, 0xdc, 0x23, 0x38, 0x4a,
            0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a,
        ];
        memory[0x2000..0x2010].copy_from_slice(&gop_guid);
        
        let mut regs = ServiceRegisters::default();
        regs.rax = 0x30;  // LocateProtocol
        regs.rcx = 0x2000;  // GUID ptr
        regs.rdx = 0;       // Registration (unused)
        regs.r8 = 0x2100;   // Interface output ptr
        
        uefi.handle_service(&mut memory, &mut regs).unwrap();
        assert_eq!(regs.rax, 0);  // EFI_SUCCESS
        
        // Verify interface pointer was written
        let interface_addr = u64::from_le_bytes([
            memory[0x2100], memory[0x2101], memory[0x2102], memory[0x2103],
            memory[0x2104], memory[0x2105], memory[0x2106], memory[0x2107],
        ]);
        assert!(interface_addr > 0);
    }
    
    #[test]
    fn test_uefi_locate_sfs_protocol() {
        let mut uefi = UefiFirmware::new(UefiConfig::default());
        let mut memory = vec![0u8; 0x200000];
        
        uefi.load(&mut memory).unwrap();
        
        // Write SFS GUID to memory
        let sfs_guid: [u8; 16] = [
            0x22, 0x5b, 0x4e, 0x96, 0x59, 0x64, 0xd2, 0x11,
            0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b,
        ];
        memory[0x2000..0x2010].copy_from_slice(&sfs_guid);
        
        let mut regs = ServiceRegisters::default();
        regs.rax = 0x30;  // LocateProtocol
        regs.rcx = 0x2000;  // GUID ptr
        regs.rdx = 0;
        regs.r8 = 0x2100;   // Interface output ptr
        
        uefi.handle_service(&mut memory, &mut regs).unwrap();
        assert_eq!(regs.rax, 0);  // EFI_SUCCESS
    }
}
