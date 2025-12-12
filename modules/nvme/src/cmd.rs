//! NVMe Command Definitions
//!
//! Defines Submission Queue Entry (SQE) and Completion Queue Entry (CQE) structures,
//! as well as Admin and NVM command opcodes.

// =============================================================================
// Admin Command Opcodes
// =============================================================================

/// Delete I/O Submission Queue
pub const ADMIN_DELETE_SQ: u8 = 0x00;
/// Create I/O Submission Queue
pub const ADMIN_CREATE_SQ: u8 = 0x01;
/// Get Log Page
pub const ADMIN_GET_LOG_PAGE: u8 = 0x02;
/// Delete I/O Completion Queue
pub const ADMIN_DELETE_CQ: u8 = 0x04;
/// Create I/O Completion Queue
pub const ADMIN_CREATE_CQ: u8 = 0x05;
/// Identify
pub const ADMIN_IDENTIFY: u8 = 0x06;
/// Abort
pub const ADMIN_ABORT: u8 = 0x08;
/// Set Features
pub const ADMIN_SET_FEATURES: u8 = 0x09;
/// Get Features
pub const ADMIN_GET_FEATURES: u8 = 0x0A;
/// Asynchronous Event Request
pub const ADMIN_ASYNC_EVENT: u8 = 0x0C;
/// Namespace Management
pub const ADMIN_NS_MGMT: u8 = 0x0D;
/// Firmware Commit
pub const ADMIN_FW_COMMIT: u8 = 0x10;
/// Firmware Image Download
pub const ADMIN_FW_DOWNLOAD: u8 = 0x11;
/// Device Self-test
pub const ADMIN_DEVICE_SELF_TEST: u8 = 0x14;
/// Namespace Attachment
pub const ADMIN_NS_ATTACH: u8 = 0x15;
/// Keep Alive
pub const ADMIN_KEEP_ALIVE: u8 = 0x18;
/// Directive Send
pub const ADMIN_DIRECTIVE_SEND: u8 = 0x19;
/// Directive Receive
pub const ADMIN_DIRECTIVE_RECV: u8 = 0x1A;
/// Virtualization Management
pub const ADMIN_VIRT_MGMT: u8 = 0x1C;
/// NVMe-MI Send
pub const ADMIN_MI_SEND: u8 = 0x1D;
/// NVMe-MI Receive
pub const ADMIN_MI_RECV: u8 = 0x1E;
/// Capacity Management
pub const ADMIN_CAP_MGMT: u8 = 0x20;
/// Lockdown
pub const ADMIN_LOCKDOWN: u8 = 0x24;
/// Doorbell Buffer Config
pub const ADMIN_DOORBELL_BUFFER_CONFIG: u8 = 0x7C;
/// Format NVM
pub const ADMIN_FORMAT_NVM: u8 = 0x80;
/// Security Send
pub const ADMIN_SECURITY_SEND: u8 = 0x81;
/// Security Receive
pub const ADMIN_SECURITY_RECV: u8 = 0x82;
/// Sanitize
pub const ADMIN_SANITIZE: u8 = 0x84;
/// Get LBA Status
pub const ADMIN_GET_LBA_STATUS: u8 = 0x86;

// =============================================================================
// NVM Command Opcodes
// =============================================================================

/// Flush
pub const NVM_FLUSH: u8 = 0x00;
/// Write
pub const NVM_WRITE: u8 = 0x01;
/// Read
pub const NVM_READ: u8 = 0x02;
/// Write Uncorrectable
pub const NVM_WRITE_UNCORRECTABLE: u8 = 0x04;
/// Compare
pub const NVM_COMPARE: u8 = 0x05;
/// Write Zeroes
pub const NVM_WRITE_ZEROES: u8 = 0x08;
/// Dataset Management
pub const NVM_DATASET_MGMT: u8 = 0x09;
/// Verify
pub const NVM_VERIFY: u8 = 0x0C;
/// Reservation Register
pub const NVM_RESV_REGISTER: u8 = 0x0D;
/// Reservation Report
pub const NVM_RESV_REPORT: u8 = 0x0E;
/// Reservation Acquire
pub const NVM_RESV_ACQUIRE: u8 = 0x11;
/// Reservation Release
pub const NVM_RESV_RELEASE: u8 = 0x15;
/// Copy
pub const NVM_COPY: u8 = 0x19;

// =============================================================================
// Identify CNS Values
// =============================================================================

/// Identify Namespace data structure
pub const IDENTIFY_CNS_NAMESPACE: u8 = 0x00;
/// Identify Controller data structure
pub const IDENTIFY_CNS_CONTROLLER: u8 = 0x01;
/// Active Namespace ID list
pub const IDENTIFY_CNS_NS_LIST: u8 = 0x02;
/// Namespace Identification Descriptor list
pub const IDENTIFY_CNS_NS_DESC_LIST: u8 = 0x03;
/// NVM Set List
pub const IDENTIFY_CNS_NVMSET_LIST: u8 = 0x04;

// =============================================================================
// Feature Identifiers
// =============================================================================

/// Arbitration
pub const FEATURE_ARBITRATION: u8 = 0x01;
/// Power Management
pub const FEATURE_POWER_MGMT: u8 = 0x02;
/// LBA Range Type
pub const FEATURE_LBA_RANGE: u8 = 0x03;
/// Temperature Threshold
pub const FEATURE_TEMP_THRESH: u8 = 0x04;
/// Error Recovery
pub const FEATURE_ERROR_RECOVERY: u8 = 0x05;
/// Volatile Write Cache
pub const FEATURE_VOLATILE_WC: u8 = 0x06;
/// Number of Queues
pub const FEATURE_NUM_QUEUES: u8 = 0x07;
/// Interrupt Coalescing
pub const FEATURE_INT_COALESCING: u8 = 0x08;
/// Interrupt Vector Configuration
pub const FEATURE_INT_VECTOR_CFG: u8 = 0x09;
/// Write Atomicity Normal
pub const FEATURE_WRITE_ATOMICITY: u8 = 0x0A;
/// Asynchronous Event Configuration
pub const FEATURE_ASYNC_EVENT_CFG: u8 = 0x0B;
/// Autonomous Power State Transition
pub const FEATURE_AUTO_PST: u8 = 0x0C;
/// Host Memory Buffer
pub const FEATURE_HOST_MEM_BUF: u8 = 0x0D;
/// Timestamp
pub const FEATURE_TIMESTAMP: u8 = 0x0E;
/// Keep Alive Timer
pub const FEATURE_KEEP_ALIVE: u8 = 0x0F;

// =============================================================================
// Submission Queue Entry (SQE) - 64 bytes
// =============================================================================

/// Common fields for all NVMe commands
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct NvmeCmd {
    /// Command Dword 0: Opcode, FUSE, Reserved, PSDT, CID
    pub cdw0: u32,
    /// Namespace Identifier
    pub nsid: u32,
    /// Command Dword 2 (reserved in most commands)
    pub cdw2: u32,
    /// Command Dword 3 (reserved in most commands)
    pub cdw3: u32,
    /// Metadata Pointer
    pub mptr: u64,
    /// Data Pointer: PRP Entry 1
    pub dptr_prp1: u64,
    /// Data Pointer: PRP Entry 2 (or PRP List pointer)
    pub dptr_prp2: u64,
    /// Command Dword 10
    pub cdw10: u32,
    /// Command Dword 11
    pub cdw11: u32,
    /// Command Dword 12
    pub cdw12: u32,
    /// Command Dword 13
    pub cdw13: u32,
    /// Command Dword 14
    pub cdw14: u32,
    /// Command Dword 15
    pub cdw15: u32,
}

impl NvmeCmd {
    /// Create a new command with the given opcode and command ID
    #[inline]
    pub fn new(opcode: u8, cid: u16) -> Self {
        Self {
            cdw0: (opcode as u32) | ((cid as u32) << 16),
            ..Default::default()
        }
    }

    /// Create an Identify command
    pub fn identify(cid: u16, nsid: u32, cns: u8, prp1: u64) -> Self {
        let mut cmd = Self::new(ADMIN_IDENTIFY, cid);
        cmd.nsid = nsid;
        cmd.dptr_prp1 = prp1;
        cmd.cdw10 = cns as u32;
        cmd
    }

    /// Create a Create I/O Completion Queue command
    pub fn create_io_cq(cid: u16, qid: u16, qsize: u16, prp1: u64, iv: u16, ien: bool) -> Self {
        let mut cmd = Self::new(ADMIN_CREATE_CQ, cid);
        cmd.dptr_prp1 = prp1;
        // CDW10: QSIZE[31:16] | QID[15:0]
        cmd.cdw10 = ((qsize as u32 - 1) << 16) | (qid as u32);
        // CDW11: IV[31:16] | IEN[1] | PC[0]
        cmd.cdw11 = ((iv as u32) << 16) | (if ien { 2 } else { 0 }) | 1; // PC=1 (physically contiguous)
        cmd
    }

    /// Create a Create I/O Submission Queue command
    pub fn create_io_sq(cid: u16, qid: u16, qsize: u16, prp1: u64, cqid: u16) -> Self {
        let mut cmd = Self::new(ADMIN_CREATE_SQ, cid);
        cmd.dptr_prp1 = prp1;
        // CDW10: QSIZE[31:16] | QID[15:0]
        cmd.cdw10 = ((qsize as u32 - 1) << 16) | (qid as u32);
        // CDW11: CQID[31:16] | QPRIO[2:1] | PC[0]
        cmd.cdw11 = ((cqid as u32) << 16) | 1; // PC=1
        cmd
    }

    /// Create a Delete I/O Submission Queue command
    pub fn delete_io_sq(cid: u16, qid: u16) -> Self {
        let mut cmd = Self::new(ADMIN_DELETE_SQ, cid);
        cmd.cdw10 = qid as u32;
        cmd
    }

    /// Create a Delete I/O Completion Queue command
    pub fn delete_io_cq(cid: u16, qid: u16) -> Self {
        let mut cmd = Self::new(ADMIN_DELETE_CQ, cid);
        cmd.cdw10 = qid as u32;
        cmd
    }

    /// Create a Set Features command for Number of Queues
    pub fn set_num_queues(cid: u16, nsq: u16, ncq: u16) -> Self {
        let mut cmd = Self::new(ADMIN_SET_FEATURES, cid);
        cmd.cdw10 = FEATURE_NUM_QUEUES as u32;
        cmd.cdw11 = ((ncq as u32 - 1) << 16) | (nsq as u32 - 1);
        cmd
    }

    /// Create a Read command
    pub fn read(cid: u16, nsid: u32, slba: u64, nlb: u16, prp1: u64, prp2: u64) -> Self {
        let mut cmd = Self::new(NVM_READ, cid);
        cmd.nsid = nsid;
        cmd.dptr_prp1 = prp1;
        cmd.dptr_prp2 = prp2;
        // CDW10: Starting LBA [31:0]
        cmd.cdw10 = slba as u32;
        // CDW11: Starting LBA [63:32]
        cmd.cdw11 = (slba >> 32) as u32;
        // CDW12: NLB[15:0] (0-based)
        cmd.cdw12 = (nlb - 1) as u32;
        cmd
    }

    /// Create a Write command
    pub fn write(cid: u16, nsid: u32, slba: u64, nlb: u16, prp1: u64, prp2: u64) -> Self {
        let mut cmd = Self::new(NVM_WRITE, cid);
        cmd.nsid = nsid;
        cmd.dptr_prp1 = prp1;
        cmd.dptr_prp2 = prp2;
        cmd.cdw10 = slba as u32;
        cmd.cdw11 = (slba >> 32) as u32;
        cmd.cdw12 = (nlb - 1) as u32;
        cmd
    }

    /// Create a Flush command
    pub fn flush(cid: u16, nsid: u32) -> Self {
        let mut cmd = Self::new(NVM_FLUSH, cid);
        cmd.nsid = nsid;
        cmd
    }
}

// =============================================================================
// Completion Queue Entry (CQE) - 16 bytes
// =============================================================================

/// Completion Queue Entry
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct NvmeCqe {
    /// Command-specific result
    pub result: u64,
    /// Submission Queue Head Pointer
    pub sq_head: u16,
    /// Submission Queue Identifier
    pub sq_id: u16,
    /// Command Identifier
    pub cid: u16,
    /// Status Field: Phase Tag[0], Status Code[15:1]
    pub status: u16,
}

impl NvmeCqe {
    /// Get the phase tag bit
    #[inline]
    pub fn phase(&self) -> bool {
        (self.status & 1) != 0
    }

    /// Get the status code type (SCT)
    #[inline]
    pub fn sct(&self) -> u8 {
        ((self.status >> 9) & 0x7) as u8
    }

    /// Get the status code (SC)
    #[inline]
    pub fn sc(&self) -> u8 {
        ((self.status >> 1) & 0xFF) as u8
    }

    /// Check if the command completed successfully
    #[inline]
    pub fn is_success(&self) -> bool {
        // SCT = 0 and SC = 0 means success
        (self.status & 0xFFFE) == 0
    }

    /// Get command-specific 32-bit result (lower)
    #[inline]
    pub fn result_low(&self) -> u32 {
        self.result as u32
    }

    /// Get command-specific 32-bit result (upper)
    #[inline]
    pub fn result_high(&self) -> u32 {
        (self.result >> 32) as u32
    }
}

// =============================================================================
// Status Code Types
// =============================================================================

/// Generic Command Status
pub const SCT_GENERIC: u8 = 0;
/// Command Specific Status
pub const SCT_COMMAND_SPECIFIC: u8 = 1;
/// Media and Data Integrity Errors
pub const SCT_MEDIA_ERROR: u8 = 2;
/// Path Related Status
pub const SCT_PATH: u8 = 3;
/// Vendor Specific
pub const SCT_VENDOR: u8 = 7;

// =============================================================================
// Generic Status Codes (SCT = 0)
// =============================================================================

/// Successful Completion
pub const SC_SUCCESS: u8 = 0x00;
/// Invalid Command Opcode
pub const SC_INVALID_OPCODE: u8 = 0x01;
/// Invalid Field in Command
pub const SC_INVALID_FIELD: u8 = 0x02;
/// Command ID Conflict
pub const SC_CID_CONFLICT: u8 = 0x03;
/// Data Transfer Error
pub const SC_DATA_XFER_ERROR: u8 = 0x04;
/// Commands Aborted due to Power Loss Notification
pub const SC_POWER_LOSS: u8 = 0x05;
/// Internal Error
pub const SC_INTERNAL: u8 = 0x06;
/// Command Abort Requested
pub const SC_ABORT_REQUESTED: u8 = 0x07;
/// Command Aborted due to SQ Deletion
pub const SC_ABORT_SQ_DELETED: u8 = 0x08;
/// Command Aborted due to Failed Fused Command
pub const SC_ABORT_FUSED_FAIL: u8 = 0x09;
/// Command Aborted due to Missing Fused Command
pub const SC_ABORT_FUSED_MISSING: u8 = 0x0A;
/// Invalid Namespace or Format
pub const SC_INVALID_NS: u8 = 0x0B;
/// Command Sequence Error
pub const SC_CMD_SEQ_ERROR: u8 = 0x0C;
/// LBA Out of Range
pub const SC_LBA_RANGE: u8 = 0x80;
/// Capacity Exceeded
pub const SC_CAP_EXCEEDED: u8 = 0x81;
/// Namespace Not Ready
pub const SC_NS_NOT_READY: u8 = 0x82;
