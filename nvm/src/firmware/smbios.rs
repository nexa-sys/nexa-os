//! SMBIOS (System Management BIOS) Table Generation for NVM Hypervisor
//!
//! This module generates SMBIOS tables for guest VMs, enabling proper
//! system identification and hardware discovery.
//!
//! ## Supported Tables
//!
//! - **Type 0** - BIOS Information
//! - **Type 1** - System Information  
//! - **Type 2** - Baseboard Information
//! - **Type 3** - System Enclosure
//! - **Type 4** - Processor Information
//! - **Type 16** - Physical Memory Array
//! - **Type 17** - Memory Device
//! - **Type 19** - Memory Array Mapped Address
//! - **Type 32** - System Boot Information
//! - **Type 127** - End-of-Table
//!
//! ## Memory Layout
//!
//! ```text
//! 0x000F0000 - Entry Point (SMBIOS 2.x anchor at 16-byte boundary)
//! 0x000F0020 - SMBIOS 3.x Entry Point (64-bit)
//! 0x000F1000 - Table data starts
//! ```

use std::fmt::Write as FmtWrite;

/// SMBIOS Entry Point address (must be on 16-byte boundary in F0000-FFFFF)
pub const SMBIOS_ENTRY_ADDR: u64 = 0xF0000;
/// SMBIOS 3.x Entry Point address
pub const SMBIOS3_ENTRY_ADDR: u64 = 0xF0020;
/// SMBIOS Table data address
pub const SMBIOS_TABLE_ADDR: u64 = 0xF1000;

/// SMBIOS configuration
#[derive(Debug, Clone)]
pub struct SmbiosConfig {
    /// BIOS vendor name
    pub bios_vendor: String,
    /// BIOS version
    pub bios_version: String,
    /// BIOS release date (MM/DD/YYYY)
    pub bios_release_date: String,
    /// System manufacturer
    pub system_manufacturer: String,
    /// System product name
    pub system_product_name: String,
    /// System version
    pub system_version: String,
    /// System serial number
    pub system_serial: String,
    /// System UUID (16 bytes)
    pub system_uuid: [u8; 16],
    /// System SKU
    pub system_sku: String,
    /// System family
    pub system_family: String,
    /// Baseboard manufacturer
    pub board_manufacturer: String,
    /// Baseboard product name
    pub board_product: String,
    /// Baseboard version
    pub board_version: String,
    /// Baseboard serial number
    pub board_serial: String,
    /// Number of CPUs
    pub cpu_count: u32,
    /// CPU model name
    pub cpu_model: String,
    /// CPU frequency in MHz
    pub cpu_freq_mhz: u16,
    /// Number of cores per CPU
    pub cpu_cores: u8,
    /// Number of threads per CPU
    pub cpu_threads: u8,
    /// Total memory in MB
    pub memory_mb: u64,
    /// Memory slots
    pub memory_slots: u8,
}

impl Default for SmbiosConfig {
    fn default() -> Self {
        Self {
            bios_vendor: String::from("NexaOS"),
            bios_version: String::from("NexaBIOS 1.0"),
            bios_release_date: String::from("01/04/2026"),
            system_manufacturer: String::from("NexaOS Team"),
            system_product_name: String::from("NVM Virtual Machine"),
            system_version: String::from("2.0"),
            system_serial: String::from("NEXAVM-0001"),
            system_uuid: [0; 16], // Will be generated
            system_sku: String::from("NVM-ENT"),
            system_family: String::from("Virtual Machine"),
            board_manufacturer: String::from("NexaOS Team"),
            board_product: String::from("NVM Virtual Board"),
            board_version: String::from("1.0"),
            board_serial: String::from("NEXABOARD-0001"),
            cpu_count: 1,
            cpu_model: String::from("NexaOS Virtual CPU"),
            cpu_freq_mhz: 3000,
            cpu_cores: 1,
            cpu_threads: 1,
            memory_mb: 1024,
            memory_slots: 4,
        }
    }
}

/// SMBIOS 2.x Entry Point Structure (31 bytes)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Smbios2EntryPoint {
    /// "_SM_" anchor string
    pub anchor_string: [u8; 4],
    /// Checksum (bytes 0x00-0x0F)
    pub checksum: u8,
    /// Entry point length (0x1F for 2.x)
    pub length: u8,
    /// Major version
    pub major_version: u8,
    /// Minor version
    pub minor_version: u8,
    /// Maximum structure size
    pub max_structure_size: u16,
    /// Entry point revision
    pub entry_point_revision: u8,
    /// Formatted area (5 bytes)
    pub formatted_area: [u8; 5],
    /// "_DMI_" anchor string
    pub intermediate_anchor: [u8; 5],
    /// Intermediate checksum (bytes 0x10-0x1E)
    pub intermediate_checksum: u8,
    /// Structure table length
    pub structure_table_length: u16,
    /// Structure table address
    pub structure_table_address: u32,
    /// Number of structures
    pub number_of_structures: u16,
    /// BCD revision
    pub bcd_revision: u8,
}

impl Smbios2EntryPoint {
    pub fn new(table_length: u16, table_addr: u32, num_structures: u16, max_size: u16) -> Self {
        Self {
            anchor_string: *b"_SM_",
            checksum: 0,
            length: 0x1F,
            major_version: 2,
            minor_version: 8,
            max_structure_size: max_size,
            entry_point_revision: 0,
            formatted_area: [0; 5],
            intermediate_anchor: *b"_DMI_",
            intermediate_checksum: 0,
            structure_table_length: table_length,
            structure_table_address: table_addr,
            number_of_structures: num_structures,
            bcd_revision: 0x28, // 2.8
        }
    }

    pub fn calculate_checksums(&mut self) {
        let bytes = self.to_bytes();
        
        // First checksum: bytes 0x00-0x0F
        let mut sum: u8 = 0;
        for i in 0..0x10 {
            if i != 4 { // Skip checksum byte
                sum = sum.wrapping_add(bytes[i]);
            }
        }
        self.checksum = (!sum).wrapping_add(1);
        
        // Intermediate checksum: bytes 0x10-0x1E
        let bytes = self.to_bytes();
        let mut sum: u8 = 0;
        for i in 0x10..0x1F {
            if i != 0x15 { // Skip intermediate checksum byte
                sum = sum.wrapping_add(bytes[i]);
            }
        }
        self.intermediate_checksum = (!sum).wrapping_add(1);
    }

    pub fn to_bytes(&self) -> [u8; 31] {
        unsafe { std::mem::transmute_copy(self) }
    }
}

/// SMBIOS 3.x Entry Point Structure (24 bytes)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Smbios3EntryPoint {
    /// "_SM3_" anchor string
    pub anchor_string: [u8; 5],
    /// Checksum
    pub checksum: u8,
    /// Entry point length (0x18 for 3.x)
    pub length: u8,
    /// Major version
    pub major_version: u8,
    /// Minor version
    pub minor_version: u8,
    /// Docrev
    pub docrev: u8,
    /// Entry point revision
    pub entry_point_revision: u8,
    /// Reserved
    pub reserved: u8,
    /// Maximum structure table length
    pub structure_table_max_size: u32,
    /// Structure table address (64-bit)
    pub structure_table_address: u64,
}

impl Smbios3EntryPoint {
    pub fn new(table_addr: u64, max_size: u32) -> Self {
        Self {
            anchor_string: *b"_SM3_",
            checksum: 0,
            length: 0x18,
            major_version: 3,
            minor_version: 0,
            docrev: 0,
            entry_point_revision: 1,
            reserved: 0,
            structure_table_max_size: max_size,
            structure_table_address: table_addr,
        }
    }

    pub fn calculate_checksum(&mut self) {
        let bytes = self.to_bytes();
        let sum: u8 = bytes.iter()
            .enumerate()
            .filter(|(i, _)| *i != 5) // Skip checksum byte
            .map(|(_, &b)| b)
            .fold(0u8, |acc, x| acc.wrapping_add(x));
        self.checksum = (!sum).wrapping_add(1);
    }

    pub fn to_bytes(&self) -> [u8; 24] {
        unsafe { std::mem::transmute_copy(self) }
    }
}

/// SMBIOS structure header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SmbiosHeader {
    pub structure_type: u8,
    pub length: u8,
    pub handle: u16,
}

impl SmbiosHeader {
    pub fn new(structure_type: u8, length: u8, handle: u16) -> Self {
        Self {
            structure_type,
            length,
            handle,
        }
    }
}

/// SMBIOS table builder
pub struct SmbiosTableBuilder {
    tables: Vec<u8>,
    strings: Vec<String>,
    handle_counter: u16,
    max_structure_size: u16,
    structure_count: u16,
}

impl SmbiosTableBuilder {
    pub fn new() -> Self {
        Self {
            tables: Vec::new(),
            strings: Vec::new(),
            handle_counter: 0,
            max_structure_size: 0,
            structure_count: 0,
        }
    }

    /// Get next handle
    fn next_handle(&mut self) -> u16 {
        let handle = self.handle_counter;
        self.handle_counter += 1;
        handle
    }

    /// Add a string to the string table, return string index (1-based)
    fn add_string(&mut self, s: &str) -> u8 {
        if s.is_empty() {
            return 0;
        }
        self.strings.push(s.to_string());
        self.strings.len() as u8
    }

    /// Flush strings to table
    fn flush_strings(&mut self) {
        for s in &self.strings {
            self.tables.extend_from_slice(s.as_bytes());
            self.tables.push(0); // Null terminator
        }
        if self.strings.is_empty() {
            self.tables.push(0); // At least one null if no strings
        }
        self.tables.push(0); // Double null terminator
        self.strings.clear();
    }

    /// Record structure size for max tracking
    fn record_structure(&mut self, size: u16) {
        if size > self.max_structure_size {
            self.max_structure_size = size;
        }
        self.structure_count += 1;
    }

    /// Add Type 0 - BIOS Information
    pub fn add_bios_info(&mut self, config: &SmbiosConfig) {
        let handle = self.next_handle();
        let start = self.tables.len();
        
        // Header
        let header = SmbiosHeader::new(0, 26, handle); // Type 0, 26 bytes
        self.tables.extend_from_slice(&unsafe { std::mem::transmute::<_, [u8; 4]>(header) });
        
        // Vendor (string 1)
        let vendor_idx = self.add_string(&config.bios_vendor);
        self.tables.push(vendor_idx);
        
        // BIOS Version (string 2)
        let version_idx = self.add_string(&config.bios_version);
        self.tables.push(version_idx);
        
        // BIOS Starting Address Segment
        self.tables.extend_from_slice(&0xE000u16.to_le_bytes());
        
        // BIOS Release Date (string 3)
        let date_idx = self.add_string(&config.bios_release_date);
        self.tables.push(date_idx);
        
        // BIOS ROM Size (64KB units, minus 1)
        self.tables.push(0x0F); // 1MB BIOS ROM
        
        // BIOS Characteristics (8 bytes)
        let characteristics: u64 = 
            (1 << 4)  |  // ISA supported
            (1 << 7)  |  // PCI supported
            (1 << 9)  |  // Plug and Play supported
            (1 << 11) |  // BIOS upgradeable
            (1 << 12) |  // BIOS shadowing allowed
            (1 << 19) |  // EDD supported
            (1 << 23) |  // Boot from CD supported
            (1 << 24) |  // Selectable boot
            (1 << 27) |  // Int 9h 8042 keyboard services
            (1 << 28) |  // Int 14h serial services
            (1 << 29) |  // Int 17h printer services
            (1 << 31);   // Int 10h video services
        self.tables.extend_from_slice(&characteristics.to_le_bytes());
        
        // BIOS Characteristics Extension Bytes (2 bytes)
        self.tables.push(0x03); // ACPI, USB Legacy
        self.tables.push(0x0D); // BIOS Boot Spec, UEFI, Virtual Machine
        
        // System BIOS Major Release
        self.tables.push(1);
        // System BIOS Minor Release
        self.tables.push(0);
        
        // EC Firmware Major Release
        self.tables.push(0xFF);
        // EC Firmware Minor Release
        self.tables.push(0xFF);
        
        // Extended BIOS ROM Size (SMBIOS 3.1+)
        self.tables.extend_from_slice(&0x0001u16.to_le_bytes()); // 16MB
        
        let size = (self.tables.len() - start) as u16;
        self.flush_strings();
        self.record_structure(size);
    }

    /// Add Type 1 - System Information
    pub fn add_system_info(&mut self, config: &SmbiosConfig) {
        let handle = self.next_handle();
        let start = self.tables.len();
        
        let header = SmbiosHeader::new(1, 27, handle);
        self.tables.extend_from_slice(&unsafe { std::mem::transmute::<_, [u8; 4]>(header) });
        
        // Manufacturer (string 1)
        let mfr_idx = self.add_string(&config.system_manufacturer);
        self.tables.push(mfr_idx);
        
        // Product Name (string 2)
        let prod_idx = self.add_string(&config.system_product_name);
        self.tables.push(prod_idx);
        
        // Version (string 3)
        let ver_idx = self.add_string(&config.system_version);
        self.tables.push(ver_idx);
        
        // Serial Number (string 4)
        let serial_idx = self.add_string(&config.system_serial);
        self.tables.push(serial_idx);
        
        // UUID (16 bytes)
        self.tables.extend_from_slice(&config.system_uuid);
        
        // Wake-up Type
        self.tables.push(0x06); // Power Switch
        
        // SKU Number (string 5)
        let sku_idx = self.add_string(&config.system_sku);
        self.tables.push(sku_idx);
        
        // Family (string 6)
        let family_idx = self.add_string(&config.system_family);
        self.tables.push(family_idx);
        
        let size = (self.tables.len() - start) as u16;
        self.flush_strings();
        self.record_structure(size);
    }

    /// Add Type 2 - Baseboard Information
    pub fn add_baseboard_info(&mut self, config: &SmbiosConfig) {
        let handle = self.next_handle();
        let start = self.tables.len();
        
        let header = SmbiosHeader::new(2, 17, handle);
        self.tables.extend_from_slice(&unsafe { std::mem::transmute::<_, [u8; 4]>(header) });
        
        // Manufacturer (string 1)
        let mfr_idx = self.add_string(&config.board_manufacturer);
        self.tables.push(mfr_idx);
        
        // Product (string 2)
        let prod_idx = self.add_string(&config.board_product);
        self.tables.push(prod_idx);
        
        // Version (string 3)
        let ver_idx = self.add_string(&config.board_version);
        self.tables.push(ver_idx);
        
        // Serial Number (string 4)
        let serial_idx = self.add_string(&config.board_serial);
        self.tables.push(serial_idx);
        
        // Asset Tag (string 5)
        let asset_idx = self.add_string("NEXAVM-ASSET");
        self.tables.push(asset_idx);
        
        // Feature Flags
        self.tables.push(0x09); // Motherboard, Hosting Board
        
        // Location in Chassis (string 6)
        let loc_idx = self.add_string("Slot 1");
        self.tables.push(loc_idx);
        
        // Chassis Handle
        self.tables.extend_from_slice(&0x0003u16.to_le_bytes());
        
        // Board Type
        self.tables.push(0x0A); // Motherboard
        
        // Number of Contained Object Handles
        self.tables.push(0);
        
        let size = (self.tables.len() - start) as u16;
        self.flush_strings();
        self.record_structure(size);
    }

    /// Add Type 3 - System Enclosure/Chassis
    pub fn add_chassis_info(&mut self) {
        let handle = self.next_handle();
        let start = self.tables.len();
        
        let header = SmbiosHeader::new(3, 22, handle);
        self.tables.extend_from_slice(&unsafe { std::mem::transmute::<_, [u8; 4]>(header) });
        
        // Manufacturer (string 1)
        let mfr_idx = self.add_string("NexaOS Team");
        self.tables.push(mfr_idx);
        
        // Type
        self.tables.push(0x01); // Other (Virtual Machine)
        
        // Version (string 2)
        let ver_idx = self.add_string("1.0");
        self.tables.push(ver_idx);
        
        // Serial Number (string 3)
        let serial_idx = self.add_string("NEXACHASSIS-0001");
        self.tables.push(serial_idx);
        
        // Asset Tag (string 4)
        let asset_idx = self.add_string("NEXA-ASSET");
        self.tables.push(asset_idx);
        
        // Boot-up State
        self.tables.push(0x03); // Safe
        
        // Power Supply State
        self.tables.push(0x03); // Safe
        
        // Thermal State
        self.tables.push(0x03); // Safe
        
        // Security Status
        self.tables.push(0x03); // None
        
        // OEM-defined (4 bytes)
        self.tables.extend_from_slice(&0u32.to_le_bytes());
        
        // Height (U)
        self.tables.push(0);
        
        // Number of Power Cords
        self.tables.push(0);
        
        // Contained Element Count
        self.tables.push(0);
        
        // Contained Element Record Length
        self.tables.push(0);
        
        let size = (self.tables.len() - start) as u16;
        self.flush_strings();
        self.record_structure(size);
    }

    /// Add Type 4 - Processor Information
    pub fn add_processor_info(&mut self, config: &SmbiosConfig, processor_index: u32) {
        let handle = self.next_handle();
        let start = self.tables.len();
        
        let header = SmbiosHeader::new(4, 48, handle);
        self.tables.extend_from_slice(&unsafe { std::mem::transmute::<_, [u8; 4]>(header) });
        
        // Socket Designation (string 1)
        let socket = format!("CPU{}", processor_index);
        let socket_idx = self.add_string(&socket);
        self.tables.push(socket_idx);
        
        // Processor Type
        self.tables.push(0x03); // Central Processor
        
        // Processor Family
        self.tables.push(0xB3); // Xeon (or use 0x02 for Unknown)
        
        // Processor Manufacturer (string 2)
        let mfr_idx = self.add_string("NexaOS");
        self.tables.push(mfr_idx);
        
        // Processor ID (8 bytes - CPUID values)
        self.tables.extend_from_slice(&[0x63, 0x06, 0x00, 0x00, 0xFD, 0xAB, 0x8B, 0x07]);
        
        // Processor Version (string 3)
        let ver_idx = self.add_string(&config.cpu_model);
        self.tables.push(ver_idx);
        
        // Voltage
        self.tables.push(0x8C); // 1.2V
        
        // External Clock (MHz)
        self.tables.extend_from_slice(&100u16.to_le_bytes());
        
        // Max Speed (MHz)
        self.tables.extend_from_slice(&config.cpu_freq_mhz.to_le_bytes());
        
        // Current Speed (MHz)
        self.tables.extend_from_slice(&config.cpu_freq_mhz.to_le_bytes());
        
        // Status
        self.tables.push(0x41); // Socket Populated, CPU Enabled
        
        // Processor Upgrade
        self.tables.push(0x06); // None
        
        // L1 Cache Handle
        self.tables.extend_from_slice(&0xFFFFu16.to_le_bytes());
        
        // L2 Cache Handle
        self.tables.extend_from_slice(&0xFFFFu16.to_le_bytes());
        
        // L3 Cache Handle
        self.tables.extend_from_slice(&0xFFFFu16.to_le_bytes());
        
        // Serial Number (string 4)
        let serial_idx = self.add_string("");
        self.tables.push(serial_idx);
        
        // Asset Tag (string 5)
        let asset_idx = self.add_string("");
        self.tables.push(asset_idx);
        
        // Part Number (string 6)
        let part_idx = self.add_string("");
        self.tables.push(part_idx);
        
        // Core Count
        self.tables.push(config.cpu_cores);
        
        // Core Enabled
        self.tables.push(config.cpu_cores);
        
        // Thread Count
        self.tables.push(config.cpu_threads);
        
        // Processor Characteristics
        self.tables.extend_from_slice(&0x04u16.to_le_bytes()); // 64-bit Capable
        
        // Processor Family 2
        self.tables.extend_from_slice(&0x00B3u16.to_le_bytes()); // Xeon
        
        // Core Count 2
        self.tables.extend_from_slice(&(config.cpu_cores as u16).to_le_bytes());
        
        // Core Enabled 2
        self.tables.extend_from_slice(&(config.cpu_cores as u16).to_le_bytes());
        
        // Thread Count 2
        self.tables.extend_from_slice(&(config.cpu_threads as u16).to_le_bytes());
        
        let size = (self.tables.len() - start) as u16;
        self.flush_strings();
        self.record_structure(size);
    }

    /// Add Type 16 - Physical Memory Array
    pub fn add_memory_array(&mut self, config: &SmbiosConfig) -> u16 {
        let handle = self.next_handle();
        let start = self.tables.len();
        
        let header = SmbiosHeader::new(16, 23, handle);
        self.tables.extend_from_slice(&unsafe { std::mem::transmute::<_, [u8; 4]>(header) });
        
        // Location
        self.tables.push(0x03); // System Board
        
        // Use
        self.tables.push(0x03); // System Memory
        
        // Memory Error Correction
        self.tables.push(0x03); // None
        
        // Maximum Capacity (KB)
        let max_kb = (config.memory_mb * 1024) as u32;
        self.tables.extend_from_slice(&max_kb.to_le_bytes());
        
        // Memory Error Information Handle
        self.tables.extend_from_slice(&0xFFFEu16.to_le_bytes()); // Not Provided
        
        // Number of Memory Devices
        self.tables.extend_from_slice(&(config.memory_slots as u16).to_le_bytes());
        
        // Extended Maximum Capacity (bytes, SMBIOS 2.7+)
        let max_bytes = config.memory_mb * 1024 * 1024;
        self.tables.extend_from_slice(&max_bytes.to_le_bytes());
        
        let size = (self.tables.len() - start) as u16;
        self.flush_strings();
        self.record_structure(size);
        
        handle
    }

    /// Add Type 17 - Memory Device
    pub fn add_memory_device(&mut self, config: &SmbiosConfig, array_handle: u16, slot: u8) {
        let handle = self.next_handle();
        let start = self.tables.len();
        
        let header = SmbiosHeader::new(17, 92, handle);
        self.tables.extend_from_slice(&unsafe { std::mem::transmute::<_, [u8; 4]>(header) });
        
        // Physical Memory Array Handle
        self.tables.extend_from_slice(&array_handle.to_le_bytes());
        
        // Memory Error Information Handle
        self.tables.extend_from_slice(&0xFFFEu16.to_le_bytes());
        
        // Total Width (bits)
        self.tables.extend_from_slice(&64u16.to_le_bytes());
        
        // Data Width (bits)
        self.tables.extend_from_slice(&64u16.to_le_bytes());
        
        // Size (MB, or 0x7FFF for extended)
        let size_mb = config.memory_mb / config.memory_slots as u64;
        if size_mb > 0x7FFF {
            self.tables.extend_from_slice(&0x7FFFu16.to_le_bytes());
        } else {
            self.tables.extend_from_slice(&(size_mb as u16).to_le_bytes());
        }
        
        // Form Factor
        self.tables.push(0x09); // DIMM
        
        // Device Set
        self.tables.push(0);
        
        // Device Locator (string 1)
        let loc = format!("DIMM{}", slot);
        let loc_idx = self.add_string(&loc);
        self.tables.push(loc_idx);
        
        // Bank Locator (string 2)
        let bank_idx = self.add_string("Bank 0");
        self.tables.push(bank_idx);
        
        // Memory Type
        self.tables.push(0x1A); // DDR4
        
        // Type Detail
        self.tables.extend_from_slice(&0x0080u16.to_le_bytes()); // Synchronous
        
        // Speed (MT/s)
        self.tables.extend_from_slice(&3200u16.to_le_bytes());
        
        // Manufacturer (string 3)
        let mfr_idx = self.add_string("NexaOS");
        self.tables.push(mfr_idx);
        
        // Serial Number (string 4)
        let serial_idx = self.add_string("");
        self.tables.push(serial_idx);
        
        // Asset Tag (string 5)
        let asset_idx = self.add_string("");
        self.tables.push(asset_idx);
        
        // Part Number (string 6)
        let part_idx = self.add_string("NEXAMEM-DDR4");
        self.tables.push(part_idx);
        
        // Attributes (Rank)
        self.tables.push(0x01); // Single Rank
        
        // Extended Size (MB, SMBIOS 2.7+)
        self.tables.extend_from_slice(&(size_mb as u32).to_le_bytes());
        
        // Configured Memory Speed (MT/s)
        self.tables.extend_from_slice(&3200u16.to_le_bytes());
        
        // Minimum Voltage (mV)
        self.tables.extend_from_slice(&1200u16.to_le_bytes());
        
        // Maximum Voltage (mV)
        self.tables.extend_from_slice(&1200u16.to_le_bytes());
        
        // Configured Voltage (mV)
        self.tables.extend_from_slice(&1200u16.to_le_bytes());
        
        // Memory Technology
        self.tables.push(0x03); // DRAM
        
        // Memory Operating Mode Capability
        self.tables.extend_from_slice(&0x0004u16.to_le_bytes()); // Volatile
        
        // Firmware Version (string 7)
        let fw_idx = self.add_string("");
        self.tables.push(fw_idx);
        
        // Module Manufacturer ID
        self.tables.extend_from_slice(&0x0000u16.to_le_bytes());
        
        // Module Product ID
        self.tables.extend_from_slice(&0x0000u16.to_le_bytes());
        
        // Memory Subsystem Controller Manufacturer ID
        self.tables.extend_from_slice(&0x0000u16.to_le_bytes());
        
        // Memory Subsystem Controller Product ID
        self.tables.extend_from_slice(&0x0000u16.to_le_bytes());
        
        // Non-volatile Size
        self.tables.extend_from_slice(&0u64.to_le_bytes());
        
        // Volatile Size
        self.tables.extend_from_slice(&(size_mb * 1024 * 1024).to_le_bytes());
        
        // Cache Size
        self.tables.extend_from_slice(&0u64.to_le_bytes());
        
        // Logical Size
        self.tables.extend_from_slice(&0u64.to_le_bytes());
        
        // Extended Speed
        self.tables.extend_from_slice(&3200u32.to_le_bytes());
        
        // Extended Configured Memory Speed
        self.tables.extend_from_slice(&3200u32.to_le_bytes());
        
        let size = (self.tables.len() - start) as u16;
        self.flush_strings();
        self.record_structure(size);
    }

    /// Add Type 32 - System Boot Information
    pub fn add_boot_info(&mut self) {
        let handle = self.next_handle();
        let start = self.tables.len();
        
        let header = SmbiosHeader::new(32, 11, handle);
        self.tables.extend_from_slice(&unsafe { std::mem::transmute::<_, [u8; 4]>(header) });
        
        // Reserved (6 bytes)
        self.tables.extend_from_slice(&[0u8; 6]);
        
        // Boot Status
        self.tables.push(0); // No errors
        
        let size = (self.tables.len() - start) as u16;
        self.flush_strings();
        self.record_structure(size);
    }

    /// Add Type 127 - End-of-Table
    pub fn add_end_of_table(&mut self) {
        let handle = self.next_handle();
        
        let header = SmbiosHeader::new(127, 4, handle);
        self.tables.extend_from_slice(&unsafe { std::mem::transmute::<_, [u8; 4]>(header) });
        self.tables.push(0);
        self.tables.push(0);
        
        self.structure_count += 1;
    }

    /// Build complete SMBIOS tables
    pub fn build(self) -> SmbiosTables {
        SmbiosTables {
            data: self.tables,
            structure_count: self.structure_count,
            max_structure_size: self.max_structure_size,
        }
    }
}

impl Default for SmbiosTableBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Complete SMBIOS tables
pub struct SmbiosTables {
    pub data: Vec<u8>,
    pub structure_count: u16,
    pub max_structure_size: u16,
}

/// Complete SMBIOS generator
pub struct SmbiosGenerator {
    config: SmbiosConfig,
}

impl SmbiosGenerator {
    pub fn new(config: SmbiosConfig) -> Self {
        Self { config }
    }

    /// Generate all SMBIOS tables and write to guest memory
    pub fn generate(&self, memory: &mut [u8]) -> Result<(), &'static str> {
        let mut builder = SmbiosTableBuilder::new();
        
        // Type 0 - BIOS Information
        builder.add_bios_info(&self.config);
        
        // Type 1 - System Information
        builder.add_system_info(&self.config);
        
        // Type 2 - Baseboard Information
        builder.add_baseboard_info(&self.config);
        
        // Type 3 - Chassis Information
        builder.add_chassis_info();
        
        // Type 4 - Processor Information (one per CPU)
        for i in 0..self.config.cpu_count {
            builder.add_processor_info(&self.config, i);
        }
        
        // Type 16 - Physical Memory Array
        let array_handle = builder.add_memory_array(&self.config);
        
        // Type 17 - Memory Device (one per slot)
        for slot in 0..self.config.memory_slots {
            builder.add_memory_device(&self.config, array_handle, slot);
        }
        
        // Type 32 - System Boot Information
        builder.add_boot_info();
        
        // Type 127 - End-of-Table
        builder.add_end_of_table();
        
        let tables = builder.build();
        
        // Write table data
        let table_addr = SMBIOS_TABLE_ADDR as usize;
        let table_end = table_addr + tables.data.len();
        if table_end > memory.len() {
            return Err("SMBIOS tables exceed memory bounds");
        }
        memory[table_addr..table_end].copy_from_slice(&tables.data);
        
        // Write SMBIOS 2.x entry point
        let mut entry2 = Smbios2EntryPoint::new(
            tables.data.len() as u16,
            SMBIOS_TABLE_ADDR as u32,
            tables.structure_count,
            tables.max_structure_size,
        );
        entry2.calculate_checksums();
        
        let entry2_addr = SMBIOS_ENTRY_ADDR as usize;
        memory[entry2_addr..entry2_addr + 31].copy_from_slice(&entry2.to_bytes());
        
        // Write SMBIOS 3.x entry point
        let mut entry3 = Smbios3EntryPoint::new(
            SMBIOS_TABLE_ADDR,
            tables.data.len() as u32,
        );
        entry3.calculate_checksum();
        
        let entry3_addr = SMBIOS3_ENTRY_ADDR as usize;
        memory[entry3_addr..entry3_addr + 24].copy_from_slice(&entry3.to_bytes());
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smbios2_entry_checksum() {
        let mut entry = Smbios2EntryPoint::new(100, SMBIOS_TABLE_ADDR as u32, 10, 50);
        entry.calculate_checksums();
        
        let bytes = entry.to_bytes();
        
        // Verify first 16 bytes checksum
        let sum: u8 = bytes[..16].iter().fold(0u8, |acc, &x| acc.wrapping_add(x));
        assert_eq!(sum, 0);
        
        // Verify intermediate checksum (bytes 16-30)
        let sum: u8 = bytes[16..31].iter().fold(0u8, |acc, &x| acc.wrapping_add(x));
        assert_eq!(sum, 0);
    }

    #[test]
    fn test_smbios3_entry_checksum() {
        let mut entry = Smbios3EntryPoint::new(SMBIOS_TABLE_ADDR, 1000);
        entry.calculate_checksum();
        
        let bytes = entry.to_bytes();
        let sum: u8 = bytes.iter().fold(0u8, |acc, &x| acc.wrapping_add(x));
        assert_eq!(sum, 0);
    }

    #[test]
    fn test_smbios_generator() {
        let config = SmbiosConfig {
            cpu_count: 2,
            memory_mb: 2048,
            memory_slots: 2,
            ..Default::default()
        };
        
        let generator = SmbiosGenerator::new(config);
        let mut memory = vec![0u8; 0x200000]; // 2MB
        
        generator.generate(&mut memory).unwrap();
        
        // Verify SMBIOS 2.x signature
        assert_eq!(&memory[SMBIOS_ENTRY_ADDR as usize..SMBIOS_ENTRY_ADDR as usize + 4], b"_SM_");
        
        // Verify SMBIOS 3.x signature
        assert_eq!(&memory[SMBIOS3_ENTRY_ADDR as usize..SMBIOS3_ENTRY_ADDR as usize + 5], b"_SM3_");
    }
}
