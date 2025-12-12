//! AHCI Register Definitions

// PCI Class code for AHCI
pub const PCI_CLASS_AHCI: u8 = 0x06;
pub const PCI_SUBCLASS_AHCI: u8 = 0x01;

// Generic Host Control registers (offset from ABAR)
pub const HBA_CAP: u64 = 0x00;        // Host Capabilities
pub const HBA_GHC: u64 = 0x04;        // Global Host Control
pub const HBA_IS: u64 = 0x08;         // Interrupt Status
pub const HBA_PI: u64 = 0x0C;         // Ports Implemented
pub const HBA_VS: u64 = 0x10;         // Version
pub const HBA_CAP2: u64 = 0x24;       // Host Capabilities Extended

// GHC bits
pub const GHC_AE: u32 = 1 << 31;      // AHCI Enable
pub const GHC_IE: u32 = 1 << 1;       // Interrupt Enable
pub const GHC_HR: u32 = 1 << 0;       // HBA Reset

// Port register offsets (from port base)
pub const PORT_CLB: u64 = 0x00;       // Command List Base Address
pub const PORT_CLBU: u64 = 0x04;      // Command List Base Address Upper
pub const PORT_FB: u64 = 0x08;        // FIS Base Address
pub const PORT_FBU: u64 = 0x0C;       // FIS Base Address Upper
pub const PORT_IS: u64 = 0x10;        // Interrupt Status
pub const PORT_IE: u64 = 0x14;        // Interrupt Enable
pub const PORT_CMD: u64 = 0x18;       // Command and Status
pub const PORT_TFD: u64 = 0x20;       // Task File Data
pub const PORT_SIG: u64 = 0x24;       // Signature
pub const PORT_SSTS: u64 = 0x28;      // SATA Status
pub const PORT_SCTL: u64 = 0x2C;      // SATA Control
pub const PORT_SERR: u64 = 0x30;      // SATA Error
pub const PORT_SACT: u64 = 0x34;      // SATA Active
pub const PORT_CI: u64 = 0x38;        // Command Issue

// Port CMD bits
pub const PORT_CMD_ST: u32 = 1 << 0;  // Start
pub const PORT_CMD_FRE: u32 = 1 << 4; // FIS Receive Enable
pub const PORT_CMD_FR: u32 = 1 << 14; // FIS Receive Running
pub const PORT_CMD_CR: u32 = 1 << 15; // Command List Running

// Port TFD bits
pub const PORT_TFD_BSY: u32 = 1 << 7;
pub const PORT_TFD_DRQ: u32 = 1 << 3;
pub const PORT_TFD_ERR: u32 = 1 << 0;

// Device signatures
pub const SATA_SIG_ATA: u32 = 0x00000101;
pub const SATA_SIG_ATAPI: u32 = 0xEB140101;
pub const SATA_SIG_SEMB: u32 = 0xC33C0101;
pub const SATA_SIG_PM: u32 = 0x96690101;

// SSTS detection
pub const SSTS_DET_MASK: u32 = 0x0F;
pub const SSTS_DET_PRESENT: u32 = 0x03;

// ATA Commands
pub const ATA_CMD_IDENTIFY: u8 = 0xEC;
pub const ATA_CMD_READ_DMA_EX: u8 = 0x25;
pub const ATA_CMD_WRITE_DMA_EX: u8 = 0x35;
pub const ATA_CMD_FLUSH_EX: u8 = 0xEA;

pub const SECTOR_SIZE: u32 = 512;
pub const TIMEOUT: u32 = 1_000_000;
