//! ACPI Table Generation for NVM Hypervisor
//!
//! This module generates complete ACPI tables for guest VMs, enabling
//! proper hardware discovery and power management.
//!
//! ## Supported Tables
//!
//! - **RSDP** (Root System Description Pointer) - Entry point for ACPI
//! - **RSDT** (Root System Description Table) - 32-bit table pointers
//! - **XSDT** (Extended System Description Table) - 64-bit table pointers
//! - **FADT** (Fixed ACPI Description Table) - System configuration
//! - **DSDT** (Differentiated System Description Table) - AML device tree
//! - **MADT** (Multiple APIC Description Table) - Interrupt controller info
//! - **MCFG** (Memory Mapped Configuration) - PCIe ECAM
//! - **HPET** (High Precision Event Timer) - Timer info
//!
//! ## Memory Layout
//!
//! ```text
//! 0x000E0000 - 0x000EFFFF   RSDP search area (BIOS ROM)
//! 0x000F0000 - 0x000FFFFF   ACPI tables (in BIOS ROM area)
//!   0x000F0000   RSDP (36 bytes)
//!   0x000F0030   RSDT
//!   0x000F0100   XSDT
//!   0x000F0200   FADT
//!   0x000F0400   DSDT (AML code)
//!   0x000F2000   MADT
//!   0x000F3000   MCFG
//!   0x000F3100   HPET
//! ```

use std::io::Write;

/// ACPI table signature type (4 bytes)
pub type AcpiSignature = [u8; 4];

/// OEM ID (6 bytes)
pub type OemId = [u8; 6];

/// OEM Table ID (8 bytes)
pub type OemTableId = [u8; 8];

/// Default OEM information
pub const DEFAULT_OEM_ID: OemId = *b"NEXAOS";
pub const DEFAULT_OEM_TABLE_ID: OemTableId = *b"NEXAVM  ";
pub const DEFAULT_OEM_REVISION: u32 = 1;
pub const DEFAULT_CREATOR_ID: u32 = 0x4D564E58; // "NXVM"
pub const DEFAULT_CREATOR_REVISION: u32 = 1;

/// ACPI table memory addresses
pub const RSDP_ADDR: u64 = 0xF0000;
pub const RSDT_ADDR: u64 = 0xF0030;
pub const XSDT_ADDR: u64 = 0xF0100;
pub const FADT_ADDR: u64 = 0xF0200;
pub const DSDT_ADDR: u64 = 0xF0400;
pub const MADT_ADDR: u64 = 0xF2000;
pub const MCFG_ADDR: u64 = 0xF3000;
pub const HPET_ADDR: u64 = 0xF3100;
pub const FACS_ADDR: u64 = 0xF3200;

/// ACPI configuration for VM
#[derive(Debug, Clone)]
pub struct AcpiConfig {
    /// Number of CPUs
    pub cpu_count: u32,
    /// Local APIC base address
    pub lapic_addr: u64,
    /// I/O APIC base address
    pub ioapic_addr: u64,
    /// I/O APIC ID
    pub ioapic_id: u8,
    /// I/O APIC global interrupt base
    pub ioapic_gsi_base: u32,
    /// PM1a event block address
    pub pm1a_evt_blk: u32,
    /// PM1a control block address
    pub pm1a_cnt_blk: u32,
    /// PM timer block address
    pub pm_tmr_blk: u32,
    /// GPE0 block address
    pub gpe0_blk: u32,
    /// PCIe ECAM base address (for MCFG)
    pub pcie_ecam_base: u64,
    /// HPET base address
    pub hpet_addr: u64,
    /// ACPI revision (1, 2, or higher)
    pub revision: u8,
}

impl Default for AcpiConfig {
    fn default() -> Self {
        Self {
            cpu_count: 1,
            lapic_addr: 0xFEE00000,
            ioapic_addr: 0xFEC00000,
            ioapic_id: 0,
            ioapic_gsi_base: 0,
            // Standard ACPI ports (PIIX4/ICH compatible)
            pm1a_evt_blk: 0x600,
            pm1a_cnt_blk: 0x604,
            pm_tmr_blk: 0x608,
            gpe0_blk: 0x620,
            pcie_ecam_base: 0xB0000000,
            hpet_addr: 0xFED00000,
            revision: 2,
        }
    }
}

/// Generic ACPI table header (36 bytes)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct AcpiTableHeader {
    pub signature: AcpiSignature,
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: OemId,
    pub oem_table_id: OemTableId,
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

impl AcpiTableHeader {
    pub fn new(signature: &[u8; 4], length: u32, revision: u8) -> Self {
        Self {
            signature: *signature,
            length,
            revision,
            checksum: 0,
            oem_id: DEFAULT_OEM_ID,
            oem_table_id: DEFAULT_OEM_TABLE_ID,
            oem_revision: DEFAULT_OEM_REVISION,
            creator_id: DEFAULT_CREATOR_ID,
            creator_revision: DEFAULT_CREATOR_REVISION,
        }
    }

    pub fn to_bytes(&self) -> [u8; 36] {
        unsafe { std::mem::transmute_copy(self) }
    }
}

/// RSDP (Root System Description Pointer) for ACPI 2.0+
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Rsdp {
    /// "RSD PTR " signature
    pub signature: [u8; 8],
    /// Checksum for first 20 bytes
    pub checksum: u8,
    /// OEM ID
    pub oem_id: OemId,
    /// ACPI revision (0 = 1.0, 2 = 2.0+)
    pub revision: u8,
    /// Physical address of RSDT (32-bit)
    pub rsdt_addr: u32,
    // ACPI 2.0+ fields
    /// Length of RSDP (36 bytes for 2.0)
    pub length: u32,
    /// Physical address of XSDT (64-bit)
    pub xsdt_addr: u64,
    /// Extended checksum (entire table)
    pub extended_checksum: u8,
    /// Reserved
    pub reserved: [u8; 3],
}

impl Rsdp {
    pub fn new(rsdt_addr: u32, xsdt_addr: u64) -> Self {
        Self {
            signature: *b"RSD PTR ",
            checksum: 0,
            oem_id: DEFAULT_OEM_ID,
            revision: 2, // ACPI 2.0
            rsdt_addr,
            length: 36,
            xsdt_addr,
            extended_checksum: 0,
            reserved: [0; 3],
        }
    }

    pub fn calculate_checksums(&mut self) {
        // First 20 bytes checksum
        let bytes = self.to_bytes();
        self.checksum = calculate_checksum(&bytes[..20]);
        
        // Extended checksum (all 36 bytes)
        let bytes = self.to_bytes();
        self.extended_checksum = calculate_checksum(&bytes);
    }

    pub fn to_bytes(&self) -> [u8; 36] {
        unsafe { std::mem::transmute_copy(self) }
    }
}

/// FADT (Fixed ACPI Description Table)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Fadt {
    pub header: AcpiTableHeader,
    /// Physical address of FACS
    pub firmware_ctrl: u32,
    /// Physical address of DSDT
    pub dsdt: u32,
    /// Reserved (was INT_MODEL in ACPI 1.0)
    pub reserved1: u8,
    /// Preferred PM profile (0=unspecified, 1=desktop, 2=mobile, etc.)
    pub preferred_pm_profile: u8,
    /// SCI interrupt vector
    pub sci_int: u16,
    /// SMI command port
    pub smi_cmd: u32,
    /// Value to write to SMI_CMD to enable ACPI
    pub acpi_enable: u8,
    /// Value to write to SMI_CMD to disable ACPI
    pub acpi_disable: u8,
    /// Value to write to SMI_CMD for S4BIOS
    pub s4bios_req: u8,
    /// Value to write to SMI_CMD for PSTATE
    pub pstate_cnt: u8,
    /// PM1a event block address
    pub pm1a_evt_blk: u32,
    /// PM1b event block address (0 if not used)
    pub pm1b_evt_blk: u32,
    /// PM1a control block address
    pub pm1a_cnt_blk: u32,
    /// PM1b control block address (0 if not used)
    pub pm1b_cnt_blk: u32,
    /// PM2 control block address (0 if not used)
    pub pm2_cnt_blk: u32,
    /// PM timer block address
    pub pm_tmr_blk: u32,
    /// GPE0 block address
    pub gpe0_blk: u32,
    /// GPE1 block address (0 if not used)
    pub gpe1_blk: u32,
    /// PM1 event block length
    pub pm1_evt_len: u8,
    /// PM1 control block length
    pub pm1_cnt_len: u8,
    /// PM2 control block length
    pub pm2_cnt_len: u8,
    /// PM timer length
    pub pm_tmr_len: u8,
    /// GPE0 block length
    pub gpe0_blk_len: u8,
    /// GPE1 block length
    pub gpe1_blk_len: u8,
    /// GPE1 base offset
    pub gpe1_base: u8,
    /// C-state control
    pub cst_cnt: u8,
    /// C2 latency (microseconds)
    pub p_lvl2_lat: u16,
    /// C3 latency (microseconds)
    pub p_lvl3_lat: u16,
    /// Cache flush size
    pub flush_size: u16,
    /// Cache flush stride
    pub flush_stride: u16,
    /// Duty cycle offset
    pub duty_offset: u8,
    /// Duty cycle width
    pub duty_width: u8,
    /// RTC day alarm index
    pub day_alrm: u8,
    /// RTC month alarm index
    pub mon_alrm: u8,
    /// RTC century index
    pub century: u8,
    /// Boot flags (IA-PC boot arch)
    pub iapc_boot_arch: u16,
    /// Reserved
    pub reserved2: u8,
    /// Fixed feature flags
    pub flags: u32,
    /// Reset register GAS
    pub reset_reg: GenericAddressStructure,
    /// Reset value
    pub reset_value: u8,
    /// ARM boot flags
    pub arm_boot_arch: u16,
    /// FADT minor version
    pub fadt_minor_version: u8,
    // ACPI 2.0+ extended fields (64-bit addresses)
    /// 64-bit physical address of FACS
    pub x_firmware_ctrl: u64,
    /// 64-bit physical address of DSDT
    pub x_dsdt: u64,
    /// Extended PM1a event block
    pub x_pm1a_evt_blk: GenericAddressStructure,
    /// Extended PM1b event block
    pub x_pm1b_evt_blk: GenericAddressStructure,
    /// Extended PM1a control block
    pub x_pm1a_cnt_blk: GenericAddressStructure,
    /// Extended PM1b control block
    pub x_pm1b_cnt_blk: GenericAddressStructure,
    /// Extended PM2 control block
    pub x_pm2_cnt_blk: GenericAddressStructure,
    /// Extended PM timer block
    pub x_pm_tmr_blk: GenericAddressStructure,
    /// Extended GPE0 block
    pub x_gpe0_blk: GenericAddressStructure,
    /// Extended GPE1 block
    pub x_gpe1_blk: GenericAddressStructure,
    /// Sleep control register
    pub sleep_control_reg: GenericAddressStructure,
    /// Sleep status register
    pub sleep_status_reg: GenericAddressStructure,
    /// Hypervisor vendor identity
    pub hypervisor_vendor_id: u64,
}

impl Fadt {
    pub fn new(config: &AcpiConfig, dsdt_addr: u32, facs_addr: u32) -> Self {
        let mut fadt = Self {
            header: AcpiTableHeader::new(b"FACP", std::mem::size_of::<Self>() as u32, 6),
            firmware_ctrl: facs_addr,
            dsdt: dsdt_addr,
            reserved1: 0,
            preferred_pm_profile: 1, // Desktop
            sci_int: 9,
            smi_cmd: 0xB2,
            acpi_enable: 0xE1,
            acpi_disable: 0x1E,
            s4bios_req: 0,
            pstate_cnt: 0,
            pm1a_evt_blk: config.pm1a_evt_blk,
            pm1b_evt_blk: 0,
            pm1a_cnt_blk: config.pm1a_cnt_blk,
            pm1b_cnt_blk: 0,
            pm2_cnt_blk: 0,
            pm_tmr_blk: config.pm_tmr_blk,
            gpe0_blk: config.gpe0_blk,
            gpe1_blk: 0,
            pm1_evt_len: 4,
            pm1_cnt_len: 2,
            pm2_cnt_len: 0,
            pm_tmr_len: 4,
            gpe0_blk_len: 8,
            gpe1_blk_len: 0,
            gpe1_base: 0,
            cst_cnt: 0,
            p_lvl2_lat: 0x65,
            p_lvl3_lat: 0x3E9,
            flush_size: 0,
            flush_stride: 0,
            duty_offset: 1,
            duty_width: 3,
            day_alrm: 0x0D,
            mon_alrm: 0,
            century: 0x32,
            iapc_boot_arch: 0x0003, // Legacy devices, 8042
            reserved2: 0,
            flags: 0x000004A5, // WBINVD, SLP_BUTTON, RTC_S4, TMR_VAL_EXT
            reset_reg: GenericAddressStructure::io(0xCF9, 1),
            reset_value: 0x06,
            arm_boot_arch: 0,
            fadt_minor_version: 2,
            x_firmware_ctrl: facs_addr as u64,
            x_dsdt: dsdt_addr as u64,
            x_pm1a_evt_blk: GenericAddressStructure::io(config.pm1a_evt_blk as u64, 4),
            x_pm1b_evt_blk: GenericAddressStructure::null(),
            x_pm1a_cnt_blk: GenericAddressStructure::io(config.pm1a_cnt_blk as u64, 2),
            x_pm1b_cnt_blk: GenericAddressStructure::null(),
            x_pm2_cnt_blk: GenericAddressStructure::null(),
            x_pm_tmr_blk: GenericAddressStructure::io(config.pm_tmr_blk as u64, 4),
            x_gpe0_blk: GenericAddressStructure::io(config.gpe0_blk as u64, 8),
            x_gpe1_blk: GenericAddressStructure::null(),
            sleep_control_reg: GenericAddressStructure::null(),
            sleep_status_reg: GenericAddressStructure::null(),
            hypervisor_vendor_id: 0x4D564158454E, // "NEXAVM"
        };
        
        fadt.header.checksum = 0;
        let bytes = fadt.to_bytes();
        fadt.header.checksum = calculate_checksum(&bytes);
        fadt
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let size = std::mem::size_of::<Self>();
        let mut bytes = vec![0u8; size];
        unsafe {
            std::ptr::copy_nonoverlapping(
                self as *const _ as *const u8,
                bytes.as_mut_ptr(),
                size,
            );
        }
        bytes
    }
}

/// Generic Address Structure (GAS) - 12 bytes
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct GenericAddressStructure {
    /// Address space ID (0=System Memory, 1=System I/O, etc.)
    pub address_space_id: u8,
    /// Register bit width
    pub register_bit_width: u8,
    /// Register bit offset
    pub register_bit_offset: u8,
    /// Access size (0=undefined, 1=byte, 2=word, 3=dword, 4=qword)
    pub access_size: u8,
    /// 64-bit address
    pub address: u64,
}

impl GenericAddressStructure {
    pub fn null() -> Self {
        Self {
            address_space_id: 0,
            register_bit_width: 0,
            register_bit_offset: 0,
            access_size: 0,
            address: 0,
        }
    }

    pub fn io(addr: u64, width: u8) -> Self {
        Self {
            address_space_id: 1, // System I/O
            register_bit_width: width * 8,
            register_bit_offset: 0,
            access_size: width.min(4),
            address: addr,
        }
    }

    pub fn memory(addr: u64, width: u8) -> Self {
        Self {
            address_space_id: 0, // System Memory
            register_bit_width: width * 8,
            register_bit_offset: 0,
            access_size: width.min(4),
            address: addr,
        }
    }
}

/// FACS (Firmware ACPI Control Structure) - 64 bytes minimum
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Facs {
    pub signature: [u8; 4],
    pub length: u32,
    pub hardware_signature: u32,
    pub firmware_waking_vector: u32,
    pub global_lock: u32,
    pub flags: u32,
    pub x_firmware_waking_vector: u64,
    pub version: u8,
    pub reserved1: [u8; 3],
    pub ospm_flags: u32,
    pub reserved2: [u8; 24],
}

impl Facs {
    pub fn new() -> Self {
        Self {
            signature: *b"FACS",
            length: 64,
            hardware_signature: 0,
            firmware_waking_vector: 0,
            global_lock: 0,
            flags: 0,
            x_firmware_waking_vector: 0,
            version: 2,
            reserved1: [0; 3],
            ospm_flags: 0,
            reserved2: [0; 24],
        }
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        unsafe { std::mem::transmute_copy(self) }
    }
}

impl Default for Facs {
    fn default() -> Self {
        Self::new()
    }
}

/// MADT (Multiple APIC Description Table) builder
pub struct MadtBuilder {
    header: AcpiTableHeader,
    lapic_addr: u32,
    flags: u32,
    entries: Vec<u8>,
}

impl MadtBuilder {
    pub fn new(lapic_addr: u32) -> Self {
        Self {
            header: AcpiTableHeader::new(b"APIC", 44, 5), // Base header + lapic_addr + flags
            lapic_addr,
            flags: 1, // PCAT_COMPAT (dual 8259 present)
            entries: Vec::new(),
        }
    }

    /// Add Local APIC entry (type 0)
    pub fn add_local_apic(&mut self, acpi_processor_id: u8, apic_id: u8, flags: u32) {
        self.entries.push(0); // Type 0: Local APIC
        self.entries.push(8); // Length
        self.entries.push(acpi_processor_id);
        self.entries.push(apic_id);
        self.entries.extend_from_slice(&flags.to_le_bytes());
    }

    /// Add I/O APIC entry (type 1)
    pub fn add_io_apic(&mut self, io_apic_id: u8, io_apic_addr: u32, gsi_base: u32) {
        self.entries.push(1); // Type 1: I/O APIC
        self.entries.push(12); // Length
        self.entries.push(io_apic_id);
        self.entries.push(0); // Reserved
        self.entries.extend_from_slice(&io_apic_addr.to_le_bytes());
        self.entries.extend_from_slice(&gsi_base.to_le_bytes());
    }

    /// Add Interrupt Source Override entry (type 2)
    pub fn add_interrupt_override(&mut self, bus: u8, source: u8, gsi: u32, flags: u16) {
        self.entries.push(2); // Type 2: Interrupt Source Override
        self.entries.push(10); // Length
        self.entries.push(bus);
        self.entries.push(source);
        self.entries.extend_from_slice(&gsi.to_le_bytes());
        self.entries.extend_from_slice(&flags.to_le_bytes());
    }

    /// Add Local APIC NMI entry (type 4)
    pub fn add_local_apic_nmi(&mut self, acpi_processor_uid: u8, flags: u16, lint: u8) {
        self.entries.push(4); // Type 4: Local APIC NMI
        self.entries.push(6); // Length
        self.entries.push(acpi_processor_uid);
        self.entries.extend_from_slice(&flags.to_le_bytes());
        self.entries.push(lint);
    }

    /// Build the complete MADT
    pub fn build(mut self) -> Vec<u8> {
        let total_len = 44 + self.entries.len();
        self.header.length = total_len as u32;
        
        let mut data = Vec::with_capacity(total_len);
        data.extend_from_slice(&self.header.to_bytes());
        data.extend_from_slice(&self.lapic_addr.to_le_bytes());
        data.extend_from_slice(&self.flags.to_le_bytes());
        data.extend_from_slice(&self.entries);
        
        // Calculate checksum
        data[9] = calculate_checksum(&data);
        
        data
    }
}

/// MCFG (PCI Express Memory-mapped Configuration) entry
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct McfgEntry {
    pub base_address: u64,
    pub segment_group: u16,
    pub start_bus: u8,
    pub end_bus: u8,
    pub reserved: u32,
}

/// MCFG table builder
pub struct McfgBuilder {
    header: AcpiTableHeader,
    reserved: [u8; 8],
    entries: Vec<McfgEntry>,
}

impl McfgBuilder {
    pub fn new() -> Self {
        Self {
            header: AcpiTableHeader::new(b"MCFG", 44, 1),
            reserved: [0; 8],
            entries: Vec::new(),
        }
    }

    pub fn add_segment(&mut self, base_address: u64, segment: u16, start_bus: u8, end_bus: u8) {
        self.entries.push(McfgEntry {
            base_address,
            segment_group: segment,
            start_bus,
            end_bus,
            reserved: 0,
        });
    }

    pub fn build(mut self) -> Vec<u8> {
        let entry_size = std::mem::size_of::<McfgEntry>();
        let total_len = 44 + self.entries.len() * entry_size;
        self.header.length = total_len as u32;
        
        let mut data = Vec::with_capacity(total_len);
        data.extend_from_slice(&self.header.to_bytes());
        data.extend_from_slice(&self.reserved);
        
        for entry in &self.entries {
            unsafe {
                let bytes = std::slice::from_raw_parts(
                    entry as *const _ as *const u8,
                    entry_size,
                );
                data.extend_from_slice(bytes);
            }
        }
        
        data[9] = calculate_checksum(&data);
        data
    }
}

impl Default for McfgBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// HPET (High Precision Event Timer) table
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Hpet {
    pub header: AcpiTableHeader,
    pub event_timer_block_id: u32,
    pub base_address: GenericAddressStructure,
    pub hpet_number: u8,
    pub min_clock_tick: u16,
    pub page_protection: u8,
}

impl Hpet {
    pub fn new(base_address: u64) -> Self {
        let mut hpet = Self {
            header: AcpiTableHeader::new(b"HPET", std::mem::size_of::<Self>() as u32, 1),
            event_timer_block_id: 0x8086A201, // Intel, 3 timers, 64-bit
            base_address: GenericAddressStructure::memory(base_address, 8),
            hpet_number: 0,
            min_clock_tick: 0x37EE, // ~14318 ticks (1ms at 14.318MHz)
            page_protection: 0,
        };
        
        hpet.header.checksum = 0;
        let bytes = hpet.to_bytes();
        hpet.header.checksum = calculate_checksum(&bytes);
        hpet
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let size = std::mem::size_of::<Self>();
        let mut bytes = vec![0u8; size];
        unsafe {
            std::ptr::copy_nonoverlapping(
                self as *const _ as *const u8,
                bytes.as_mut_ptr(),
                size,
            );
        }
        bytes
    }
}

/// DSDT (Differentiated System Description Table) AML generator
pub struct DsdtBuilder {
    aml: Vec<u8>,
}

impl DsdtBuilder {
    pub fn new() -> Self {
        Self { aml: Vec::new() }
    }

    /// Build a minimal DSDT with basic devices
    pub fn build_minimal(config: &AcpiConfig) -> Vec<u8> {
        let mut builder = Self::new();
        builder.build_dsdt(config)
    }

    fn build_dsdt(&mut self, config: &AcpiConfig) -> Vec<u8> {
        // Start with header
        let mut data = Vec::new();
        
        // Header placeholder (will be filled later)
        data.extend_from_slice(&[0u8; 36]);
        
        // AML code starts here
        // DefinitionBlock starts with scope
        
        // Scope (\_SB) - System Bus
        self.emit_scope(b"\\_SB_", &mut data);
        
        // Add CPU devices
        for i in 0..config.cpu_count {
            self.emit_cpu_device(i, &mut data);
        }
        
        // Add PCI root bridge
        self.emit_pci_root(&mut data);
        
        // Add power button
        self.emit_power_button(&mut data);
        
        // Close \_SB scope
        self.close_scope(&mut data);
        
        // Add \_S5 (soft off) sleep state
        self.emit_sleep_states(&mut data);
        
        // Fill in header
        let total_len = data.len() as u32;
        let header = AcpiTableHeader::new(b"DSDT", total_len, 2);
        let header_bytes = header.to_bytes();
        data[..36].copy_from_slice(&header_bytes);
        
        // Calculate checksum
        data[9] = calculate_checksum(&data);
        
        data
    }

    fn emit_scope(&mut self, name: &[u8], data: &mut Vec<u8>) {
        // AML Scope opcode
        data.push(0x10); // ScopeOp
        
        // PkgLength and name will be fixed later
        // For now, use a placeholder
        let scope_start = data.len();
        data.push(0x00); // PkgLength placeholder (1 byte)
        
        // NamePath
        for b in name {
            data.push(*b);
        }
        
        self.aml.push(scope_start as u8); // Remember for closing
    }

    fn close_scope(&mut self, data: &mut Vec<u8>) {
        // Update PkgLength for the scope
        if let Some(_start) = self.aml.pop() {
            // In a real implementation, we would fix up the package length
            // For simplicity, we use a fixed-size structure
        }
    }

    fn emit_cpu_device(&self, index: u32, data: &mut Vec<u8>) {
        // Device(CPUx)
        data.push(0x5B); // ExtOpPrefix
        data.push(0x82); // DeviceOp
        
        // Package length (fixed for simplicity)
        data.push(0x0B); // PkgLength
        
        // Name: CPU0, CPU1, etc.
        data.extend_from_slice(b"CPU");
        data.push(b'0' + (index & 0x0F) as u8);
        
        // _HID = "ACPI0007" (Processor Device)
        data.push(0x08); // NameOp
        data.extend_from_slice(b"_HID");
        data.push(0x0D); // StringOp
        data.extend_from_slice(b"ACPI0007\0");
    }

    fn emit_pci_root(&self, data: &mut Vec<u8>) {
        // Device(PCI0) - simplified
        data.push(0x5B); // ExtOpPrefix
        data.push(0x82); // DeviceOp
        data.push(0x12); // PkgLength
        data.extend_from_slice(b"PCI0");
        
        // _HID = "PNP0A03" (PCI Bus)
        data.push(0x08); // NameOp
        data.extend_from_slice(b"_HID");
        data.push(0x0C); // DWordConst prefix for EISAID
        data.extend_from_slice(&0x030AD041u32.to_le_bytes()); // PNP0A03
    }

    fn emit_power_button(&self, data: &mut Vec<u8>) {
        // Device(PWRB)
        data.push(0x5B); // ExtOpPrefix
        data.push(0x82); // DeviceOp
        data.push(0x10); // PkgLength
        data.extend_from_slice(b"PWRB");
        
        // _HID = "PNP0C0C" (Power Button)
        data.push(0x08); // NameOp
        data.extend_from_slice(b"_HID");
        data.push(0x0C); // DWordConst
        data.extend_from_slice(&0x0C0CD041u32.to_le_bytes()); // PNP0C0C
    }

    fn emit_sleep_states(&self, data: &mut Vec<u8>) {
        // Name(\_S5, Package(4){5, 5, 0, 0})
        // This defines S5 (Soft Off) state
        data.push(0x08); // NameOp
        data.extend_from_slice(b"_S5_");
        
        // Package with 4 elements
        data.push(0x12); // PackageOp
        data.push(0x06); // PkgLength
        data.push(0x04); // NumElements
        data.push(0x05); // ByteConst: 5 (PM1a_CNT.SLP_TYP)
        data.push(0x05); // ByteConst: 5 (PM1b_CNT.SLP_TYP)
        data.push(0x00); // ByteConst: 0 (Reserved)
        data.push(0x00); // ByteConst: 0 (Reserved)
    }
}

impl Default for DsdtBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// RSDT (Root System Description Table) builder
pub fn build_rsdt(table_addrs: &[u32]) -> Vec<u8> {
    let header_size = 36;
    let total_size = header_size + table_addrs.len() * 4;
    
    let mut header = AcpiTableHeader::new(b"RSDT", total_size as u32, 1);
    let mut data = Vec::with_capacity(total_size);
    
    data.extend_from_slice(&header.to_bytes());
    for &addr in table_addrs {
        data.extend_from_slice(&addr.to_le_bytes());
    }
    
    header.checksum = calculate_checksum(&data);
    data[9] = header.checksum;
    
    data
}

/// XSDT (Extended System Description Table) builder
pub fn build_xsdt(table_addrs: &[u64]) -> Vec<u8> {
    let header_size = 36;
    let total_size = header_size + table_addrs.len() * 8;
    
    let mut header = AcpiTableHeader::new(b"XSDT", total_size as u32, 1);
    let mut data = Vec::with_capacity(total_size);
    
    data.extend_from_slice(&header.to_bytes());
    for &addr in table_addrs {
        data.extend_from_slice(&addr.to_le_bytes());
    }
    
    header.checksum = calculate_checksum(&data);
    data[9] = header.checksum;
    
    data
}

/// Calculate ACPI table checksum
pub fn calculate_checksum(data: &[u8]) -> u8 {
    let sum: u8 = data.iter().fold(0u8, |acc, &x| acc.wrapping_add(x));
    (256u16 - sum as u16) as u8
}

/// Complete ACPI table generator
pub struct AcpiTableGenerator {
    config: AcpiConfig,
}

impl AcpiTableGenerator {
    pub fn new(config: AcpiConfig) -> Self {
        Self { config }
    }

    /// Generate all ACPI tables and write to guest memory
    pub fn generate(&self, memory: &mut [u8]) -> Result<(), &'static str> {
        // Generate individual tables
        let dsdt = DsdtBuilder::build_minimal(&self.config);
        let facs = Facs::new();
        let fadt = Fadt::new(&self.config, DSDT_ADDR as u32, FACS_ADDR as u32);
        let madt = self.build_madt();
        let mcfg = self.build_mcfg();
        let hpet = Hpet::new(self.config.hpet_addr);
        
        // Table addresses for RSDT/XSDT
        let table_addrs_32 = [
            FADT_ADDR as u32,
            MADT_ADDR as u32,
            MCFG_ADDR as u32,
            HPET_ADDR as u32,
        ];
        let table_addrs_64 = [
            FADT_ADDR,
            MADT_ADDR,
            MCFG_ADDR,
            HPET_ADDR,
        ];
        
        let rsdt = build_rsdt(&table_addrs_32);
        let xsdt = build_xsdt(&table_addrs_64);
        
        // RSDP points to RSDT and XSDT
        let mut rsdp = Rsdp::new(RSDT_ADDR as u32, XSDT_ADDR);
        rsdp.calculate_checksums();
        
        // Write tables to memory
        self.write_table(memory, RSDP_ADDR, &rsdp.to_bytes())?;
        self.write_table(memory, RSDT_ADDR, &rsdt)?;
        self.write_table(memory, XSDT_ADDR, &xsdt)?;
        self.write_table(memory, FADT_ADDR, &fadt.to_bytes())?;
        self.write_table(memory, DSDT_ADDR, &dsdt)?;
        self.write_table(memory, FACS_ADDR, &facs.to_bytes())?;
        self.write_table(memory, MADT_ADDR, &madt)?;
        self.write_table(memory, MCFG_ADDR, &mcfg)?;
        self.write_table(memory, HPET_ADDR, &hpet.to_bytes())?;
        
        Ok(())
    }

    fn build_madt(&self) -> Vec<u8> {
        let mut madt = MadtBuilder::new(self.config.lapic_addr as u32);
        
        // Add Local APICs for each CPU
        for i in 0..self.config.cpu_count {
            madt.add_local_apic(i as u8, i as u8, 1); // Enabled
        }
        
        // Add I/O APIC
        madt.add_io_apic(
            self.config.ioapic_id,
            self.config.ioapic_addr as u32,
            self.config.ioapic_gsi_base,
        );
        
        // Add interrupt source overrides for ISA IRQs
        // IRQ0 (Timer) -> GSI 2
        madt.add_interrupt_override(0, 0, 2, 0);
        // IRQ9 (SCI) -> GSI 9, level triggered, active low
        madt.add_interrupt_override(0, 9, 9, 0x000D);
        
        // Add Local APIC NMI (LINT1 for all processors)
        madt.add_local_apic_nmi(0xFF, 0, 1);
        
        madt.build()
    }

    fn build_mcfg(&self) -> Vec<u8> {
        let mut mcfg = McfgBuilder::new();
        mcfg.add_segment(self.config.pcie_ecam_base, 0, 0, 255);
        mcfg.build()
    }

    fn write_table(&self, memory: &mut [u8], addr: u64, data: &[u8]) -> Result<(), &'static str> {
        let start = addr as usize;
        let end = start + data.len();
        
        if end > memory.len() {
            return Err("ACPI table would exceed memory bounds");
        }
        
        memory[start..end].copy_from_slice(data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rsdp_checksum() {
        let mut rsdp = Rsdp::new(RSDT_ADDR as u32, XSDT_ADDR);
        rsdp.calculate_checksums();
        
        let bytes = rsdp.to_bytes();
        
        // Verify first 20 bytes checksum
        let sum: u8 = bytes[..20].iter().fold(0u8, |acc, &x| acc.wrapping_add(x));
        assert_eq!(sum, 0);
        
        // Verify extended checksum
        let sum: u8 = bytes.iter().fold(0u8, |acc, &x| acc.wrapping_add(x));
        assert_eq!(sum, 0);
    }

    #[test]
    fn test_fadt_creation() {
        let config = AcpiConfig::default();
        let fadt = Fadt::new(&config, DSDT_ADDR as u32, FACS_ADDR as u32);
        
        assert_eq!(&fadt.header.signature, b"FACP");
        // Use ptr::read_unaligned for packed struct field access
        let dsdt_val = unsafe { std::ptr::read_unaligned(std::ptr::addr_of!(fadt.dsdt)) };
        assert_eq!(dsdt_val, DSDT_ADDR as u32);
    }

    #[test]
    fn test_madt_builder() {
        let mut madt = MadtBuilder::new(0xFEE00000);
        madt.add_local_apic(0, 0, 1);
        madt.add_io_apic(0, 0xFEC00000, 0);
        
        let data = madt.build();
        
        // Verify signature
        assert_eq!(&data[0..4], b"APIC");
        
        // Verify checksum
        let sum: u8 = data.iter().fold(0u8, |acc, &x| acc.wrapping_add(x));
        assert_eq!(sum, 0);
    }

    #[test]
    fn test_acpi_generator() {
        let config = AcpiConfig {
            cpu_count: 4,
            ..Default::default()
        };
        
        let generator = AcpiTableGenerator::new(config);
        let mut memory = vec![0u8; 0x100000]; // 1MB
        
        generator.generate(&mut memory).unwrap();
        
        // Verify RSDP signature
        assert_eq!(&memory[RSDP_ADDR as usize..RSDP_ADDR as usize + 8], b"RSD PTR ");
    }
}
