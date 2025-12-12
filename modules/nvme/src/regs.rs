//! NVMe Controller Register Definitions
//!
//! Based on NVM Express Base Specification 2.0

// =============================================================================
// PCI Configuration
// =============================================================================

/// NVMe PCI Class Code
pub const PCI_CLASS_NVME: u8 = 0x01;        // Mass Storage Controller
pub const PCI_SUBCLASS_NVME: u8 = 0x08;     // Non-Volatile Memory Controller  
pub const PCI_PROGIF_NVME: u8 = 0x02;       // NVM Express

// =============================================================================
// Controller Registers (offset from BAR0)
// =============================================================================

/// Controller Capabilities (64-bit)
pub const REG_CAP: u64 = 0x00;
/// Version
pub const REG_VS: u64 = 0x08;
/// Interrupt Mask Set
pub const REG_INTMS: u64 = 0x0C;
/// Interrupt Mask Clear
pub const REG_INTMC: u64 = 0x10;
/// Controller Configuration
pub const REG_CC: u64 = 0x14;
/// Controller Status
pub const REG_CSTS: u64 = 0x1C;
/// NVM Subsystem Reset
pub const REG_NSSR: u64 = 0x20;
/// Admin Queue Attributes
pub const REG_AQA: u64 = 0x24;
/// Admin Submission Queue Base Address (64-bit)
pub const REG_ASQ: u64 = 0x28;
/// Admin Completion Queue Base Address (64-bit)
pub const REG_ACQ: u64 = 0x30;
/// Controller Memory Buffer Location
pub const REG_CMBLOC: u64 = 0x38;
/// Controller Memory Buffer Size
pub const REG_CMBSZ: u64 = 0x3C;
/// Boot Partition Information
pub const REG_BPINFO: u64 = 0x40;
/// Boot Partition Read Select
pub const REG_BPRSEL: u64 = 0x44;
/// Boot Partition Memory Buffer Location
pub const REG_BPMBL: u64 = 0x48;

// =============================================================================
// Doorbell Registers
// =============================================================================

/// Submission Queue y Tail Doorbell (stride = 4 << CAP.DSTRD)
pub const REG_SQ0TDBL: u64 = 0x1000;
/// Completion Queue y Head Doorbell (stride = 4 << CAP.DSTRD)
pub fn sq_tail_doorbell(qid: u16, dstrd: u8) -> u64 {
    REG_SQ0TDBL + (2 * qid as u64) * (4 << dstrd as u64)
}
pub fn cq_head_doorbell(qid: u16, dstrd: u8) -> u64 {
    REG_SQ0TDBL + (2 * qid as u64 + 1) * (4 << dstrd as u64)
}

// =============================================================================
// CAP - Controller Capabilities Register (64-bit)
// =============================================================================

/// Maximum Queue Entries Supported (0-based, so actual max is MQES+1)
pub const CAP_MQES_MASK: u64 = 0xFFFF;
/// Contiguous Queues Required
pub const CAP_CQR: u64 = 1 << 16;
/// Arbitration Mechanism Supported
pub const CAP_AMS_SHIFT: u64 = 17;
pub const CAP_AMS_MASK: u64 = 0x3 << CAP_AMS_SHIFT;
/// Timeout (in 500ms units)
pub const CAP_TO_SHIFT: u64 = 24;
pub const CAP_TO_MASK: u64 = 0xFF << CAP_TO_SHIFT;
/// Doorbell Stride
pub const CAP_DSTRD_SHIFT: u64 = 32;
pub const CAP_DSTRD_MASK: u64 = 0xF << CAP_DSTRD_SHIFT;
/// NVM Subsystem Reset Supported
pub const CAP_NSSRS: u64 = 1 << 36;
/// Command Sets Supported
pub const CAP_CSS_SHIFT: u64 = 37;
pub const CAP_CSS_MASK: u64 = 0xFF << CAP_CSS_SHIFT;
/// Boot Partition Support
pub const CAP_BPS: u64 = 1 << 45;
/// Controller Power Scope
pub const CAP_CPS_SHIFT: u64 = 46;
/// Memory Page Size Minimum (2^(12+MPSMIN))
pub const CAP_MPSMIN_SHIFT: u64 = 48;
pub const CAP_MPSMIN_MASK: u64 = 0xF << CAP_MPSMIN_SHIFT;
/// Memory Page Size Maximum (2^(12+MPSMAX))
pub const CAP_MPSMAX_SHIFT: u64 = 52;
pub const CAP_MPSMAX_MASK: u64 = 0xF << CAP_MPSMAX_SHIFT;
/// Persistent Memory Region Supported
pub const CAP_PMRS: u64 = 1 << 56;
/// Controller Memory Buffer Supported
pub const CAP_CMBS: u64 = 1 << 57;
/// NVM Subsystem Shutdown Supported
pub const CAP_NSSS: u64 = 1 << 58;
/// Controller Ready Modes Supported
pub const CAP_CRMS_SHIFT: u64 = 59;

// =============================================================================
// CC - Controller Configuration Register (32-bit)
// =============================================================================

/// Enable
pub const CC_EN: u32 = 1 << 0;
/// I/O Command Set Selected
pub const CC_CSS_SHIFT: u32 = 4;
pub const CC_CSS_MASK: u32 = 0x7 << CC_CSS_SHIFT;
/// Memory Page Size (2^(12+MPS))
pub const CC_MPS_SHIFT: u32 = 7;
pub const CC_MPS_MASK: u32 = 0xF << CC_MPS_SHIFT;
/// Arbitration Mechanism Selected
pub const CC_AMS_SHIFT: u32 = 11;
pub const CC_AMS_MASK: u32 = 0x7 << CC_AMS_SHIFT;
/// Shutdown Notification
pub const CC_SHN_SHIFT: u32 = 14;
pub const CC_SHN_MASK: u32 = 0x3 << CC_SHN_SHIFT;
pub const CC_SHN_NONE: u32 = 0 << CC_SHN_SHIFT;
pub const CC_SHN_NORMAL: u32 = 1 << CC_SHN_SHIFT;
pub const CC_SHN_ABRUPT: u32 = 2 << CC_SHN_SHIFT;
/// I/O Submission Queue Entry Size (2^IOSQES bytes)
pub const CC_IOSQES_SHIFT: u32 = 16;
pub const CC_IOSQES_MASK: u32 = 0xF << CC_IOSQES_SHIFT;
/// I/O Completion Queue Entry Size (2^IOCQES bytes)
pub const CC_IOCQES_SHIFT: u32 = 20;
pub const CC_IOCQES_MASK: u32 = 0xF << CC_IOCQES_SHIFT;
/// Controller Ready Independent of Media Enable
pub const CC_CRIME: u32 = 1 << 24;

/// Default CC value for NVMe initialization
/// - IOSQES = 6 (64 bytes per SQ entry)
/// - IOCQES = 4 (16 bytes per CQ entry)
/// - MPS = 0 (4KB pages)
/// - CSS = 0 (NVM command set)
pub const CC_DEFAULT: u32 = (6 << CC_IOSQES_SHIFT) | (4 << CC_IOCQES_SHIFT);

// =============================================================================
// CSTS - Controller Status Register (32-bit)
// =============================================================================

/// Ready
pub const CSTS_RDY: u32 = 1 << 0;
/// Controller Fatal Status
pub const CSTS_CFS: u32 = 1 << 1;
/// Shutdown Status
pub const CSTS_SHST_SHIFT: u32 = 2;
pub const CSTS_SHST_MASK: u32 = 0x3 << CSTS_SHST_SHIFT;
pub const CSTS_SHST_NORMAL: u32 = 0 << CSTS_SHST_SHIFT;
pub const CSTS_SHST_OCCURRING: u32 = 1 << CSTS_SHST_SHIFT;
pub const CSTS_SHST_COMPLETE: u32 = 2 << CSTS_SHST_SHIFT;
/// NVM Subsystem Reset Occurred
pub const CSTS_NSSRO: u32 = 1 << 4;
/// Processing Paused
pub const CSTS_PP: u32 = 1 << 5;
/// Shutdown Type
pub const CSTS_ST: u32 = 1 << 6;

// =============================================================================
// AQA - Admin Queue Attributes (32-bit)
// =============================================================================

/// Admin Submission Queue Size (0-based)
pub const AQA_ASQS_SHIFT: u32 = 0;
pub const AQA_ASQS_MASK: u32 = 0xFFF;
/// Admin Completion Queue Size (0-based)
pub const AQA_ACQS_SHIFT: u32 = 16;
pub const AQA_ACQS_MASK: u32 = 0xFFF << AQA_ACQS_SHIFT;

/// Build AQA register value
#[inline]
pub fn aqa_value(asqs: u16, acqs: u16) -> u32 {
    ((asqs as u32 - 1) & AQA_ASQS_MASK) | (((acqs as u32 - 1) << AQA_ACQS_SHIFT) & AQA_ACQS_MASK)
}

// =============================================================================
// Queue Entry Sizes
// =============================================================================

/// Submission Queue Entry size in bytes
pub const SQE_SIZE: usize = 64;
/// Completion Queue Entry size in bytes
pub const CQE_SIZE: usize = 16;

// =============================================================================
// Constants
// =============================================================================

/// Default sector size (512 bytes, may vary per namespace)
pub const SECTOR_SIZE: u32 = 512;
/// Timeout in microseconds for controller ready
pub const TIMEOUT_US: u64 = 5_000_000; // 5 seconds
/// Timeout in loop iterations
pub const TIMEOUT_LOOPS: u32 = 10_000_000;
/// Maximum PRPs per command (for 4KB pages with 512B sectors, 256 PRPs for 128KB)
pub const MAX_PRPS: usize = 256;
/// Admin queue depth
pub const ADMIN_QUEUE_DEPTH: u16 = 64;
/// Default I/O queue depth
pub const IO_QUEUE_DEPTH: u16 = 256;
/// Maximum transfer size (128KB)
pub const MAX_TRANSFER_SIZE: usize = 128 * 1024;
/// Maximum sectors per transfer
pub const MAX_SECTORS_PER_TRANSFER: u32 = MAX_TRANSFER_SIZE as u32 / SECTOR_SIZE;
