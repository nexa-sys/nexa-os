//! NVMe Controller Implementation
//!
//! Handles controller initialization, identification, and namespace management.

use crate::cmd::{NvmeCmd, IDENTIFY_CNS_CONTROLLER, IDENTIFY_CNS_NAMESPACE};
use crate::queue::{NvmeQueuePair, create_admin_queue, setup_admin_queue_regs};
use crate::regs::*;
use crate::{kmod_mmio_read32, kmod_mmio_write32, kmod_zalloc, kmod_dealloc};
use crate::kmod_virt_to_phys;
use crate::{mod_info, mod_error};

// =============================================================================
// Identify Controller Data Structure (4KB)
// =============================================================================

/// Subset of Identify Controller data (first 512 bytes)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IdentifyController {
    /// PCI Vendor ID
    pub vid: u16,
    /// PCI Subsystem Vendor ID
    pub ssvid: u16,
    /// Serial Number (20 bytes)
    pub sn: [u8; 20],
    /// Model Number (40 bytes)
    pub mn: [u8; 40],
    /// Firmware Revision (8 bytes)
    pub fr: [u8; 8],
    /// Recommended Arbitration Burst
    pub rab: u8,
    /// IEEE OUI Identifier
    pub ieee: [u8; 3],
    /// Controller Multi-Path I/O and Namespace Sharing Capabilities
    pub cmic: u8,
    /// Maximum Data Transfer Size
    pub mdts: u8,
    /// Controller ID
    pub cntlid: u16,
    /// Version
    pub ver: u32,
    /// RTD3 Resume Latency
    pub rtd3r: u32,
    /// RTD3 Entry Latency
    pub rtd3e: u32,
    /// Optional Asynchronous Events Supported
    pub oaes: u32,
    /// Controller Attributes
    pub ctratt: u32,
    /// Read Recovery Levels Supported
    pub rrls: u16,
    /// Reserved
    pub _rsvd102: [u8; 9],
    /// Controller Type
    pub cntrltype: u8,
    /// FRU Globally Unique Identifier
    pub fguid: [u8; 16],
    /// Command Retry Delay Time 1-3
    pub crdt: [u16; 3],
    /// Reserved
    pub _rsvd134: [u8; 119],
    /// NVM Subsystem Report
    pub nvmsr: u8,
    /// VPD Write Cycle Information
    pub vwci: u8,
    /// Management Endpoint Capabilities
    pub mec: u8,
    /// Optional Admin Command Support
    pub oacs: u16,
    /// Abort Command Limit
    pub acl: u8,
    /// Asynchronous Event Request Limit
    pub aerl: u8,
    /// Firmware Updates
    pub frmw: u8,
    /// Log Page Attributes
    pub lpa: u8,
    /// Error Log Page Entries
    pub elpe: u8,
    /// Number of Power States Support
    pub npss: u8,
    /// Admin Vendor Specific Command Configuration
    pub avscc: u8,
    /// Autonomous Power State Transition Attributes
    pub apsta: u8,
    /// Warning Composite Temperature Threshold
    pub wctemp: u16,
    /// Critical Composite Temperature Threshold
    pub cctemp: u16,
    /// Maximum Time for Firmware Activation
    pub mtfa: u16,
    /// Host Memory Buffer Preferred Size
    pub hmpre: u32,
    /// Host Memory Buffer Minimum Size
    pub hmmin: u32,
    /// Total NVM Capacity (128-bit)
    pub tnvmcap: [u8; 16],
    /// Unallocated NVM Capacity (128-bit)
    pub unvmcap: [u8; 16],
    /// Replay Protected Memory Block Support
    pub rpmbs: u32,
    /// Extended Device Self-test Time
    pub edstt: u16,
    /// Device Self-test Options
    pub dsto: u8,
    /// Firmware Update Granularity
    pub fwug: u8,
    /// Keep Alive Support
    pub kas: u16,
    /// Host Controlled Thermal Management Attributes
    pub hctma: u16,
    /// Minimum Thermal Management Temperature
    pub mntmt: u16,
    /// Maximum Thermal Management Temperature
    pub mxtmt: u16,
    /// Sanitize Capabilities
    pub sanicap: u32,
    /// Host Memory Buffer Minimum Descriptor Entry Size
    pub hmminds: u32,
    /// Host Memory Maximum Descriptors Entries
    pub hmmaxd: u16,
    /// NVM Set Identifier Maximum
    pub nsetidmax: u16,
    /// Endurance Group Identifier Maximum
    pub endgidmax: u16,
    /// ANA Transition Time
    pub anatt: u8,
    /// Asymmetric Namespace Access Capabilities
    pub anacap: u8,
    /// ANA Group Identifier Maximum
    pub anagrpmax: u32,
    /// Number of ANA Group Identifiers
    pub nanagrpid: u32,
    /// Persistent Event Log Size
    pub pels: u32,
    /// Domain Identifier
    pub domainid: u16,
    /// Reserved
    pub _rsvd358: [u8; 10],
    /// Max Endurance Group Identifier
    pub megcap: [u8; 16],
    /// Reserved
    pub _rsvd384: [u8; 128],
    /// Submission Queue Entry Size
    pub sqes: u8,
    /// Completion Queue Entry Size
    pub cqes: u8,
    /// Maximum Outstanding Commands
    pub maxcmd: u16,
    /// Number of Namespaces
    pub nn: u32,
    /// Optional NVM Command Support
    pub oncs: u16,
    /// Fused Operation Support
    pub fuses: u16,
    /// Format NVM Attributes
    pub fna: u8,
    /// Volatile Write Cache
    pub vwc: u8,
    /// Atomic Write Unit Normal
    pub awun: u16,
    /// Atomic Write Unit Power Fail
    pub awupf: u16,
    /// NVM Vendor Specific Command Configuration
    pub nvscc: u8,
    /// Namespace Write Protection Capabilities
    pub nwpc: u8,
    /// Atomic Compare & Write Unit
    pub acwu: u16,
    /// Copy Descriptor Formats Supported
    pub cdfs: u16,
    /// SGL Support
    pub sgls: u32,
    /// Maximum Number of Allowed Namespaces
    pub mnan: u32,
    /// Max Domain Identifier
    pub maxdna: [u8; 16],
    /// Max I/O Commands Outstanding
    pub maxcna: u32,
}

// =============================================================================
// Identify Namespace Data Structure (4KB)
// =============================================================================

/// Subset of Identify Namespace data
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IdentifyNamespace {
    /// Namespace Size (total blocks)
    pub nsze: u64,
    /// Namespace Capacity
    pub ncap: u64,
    /// Namespace Utilization
    pub nuse: u64,
    /// Namespace Features
    pub nsfeat: u8,
    /// Number of LBA Formats
    pub nlbaf: u8,
    /// Formatted LBA Size
    pub flbas: u8,
    /// Metadata Capabilities
    pub mc: u8,
    /// End-to-end Data Protection Capabilities
    pub dpc: u8,
    /// End-to-end Data Protection Type Settings
    pub dps: u8,
    /// Namespace Multi-path I/O and Namespace Sharing Capabilities
    pub nmic: u8,
    /// Reservation Capabilities
    pub rescap: u8,
    /// Format Progress Indicator
    pub fpi: u8,
    /// Deallocate Logical Block Features
    pub dlfeat: u8,
    /// Namespace Atomic Write Unit Normal
    pub nawun: u16,
    /// Namespace Atomic Write Unit Power Fail
    pub nawupf: u16,
    /// Namespace Atomic Compare & Write Unit
    pub nacwu: u16,
    /// Namespace Atomic Boundary Size Normal
    pub nabsn: u16,
    /// Namespace Atomic Boundary Offset
    pub nabo: u16,
    /// Namespace Atomic Boundary Size Power Fail
    pub nabspf: u16,
    /// Namespace Optimal I/O Boundary
    pub noiob: u16,
    /// NVM Capacity (128-bit)
    pub nvmcap: [u8; 16],
    /// Namespace Preferred Write Granularity
    pub npwg: u16,
    /// Namespace Preferred Write Alignment
    pub npwa: u16,
    /// Namespace Preferred Deallocate Granularity
    pub npdg: u16,
    /// Namespace Preferred Deallocate Alignment
    pub npda: u16,
    /// Namespace Optimal Write Size
    pub nows: u16,
    /// Maximum Single Source Range Length
    pub mssrl: u16,
    /// Maximum Copy Length
    pub mcl: u32,
    /// Maximum Source Range Count
    pub msrc: u8,
    /// Reserved
    pub _rsvd81: [u8; 11],
    /// ANA Group Identifier
    pub anagrpid: u32,
    /// Reserved
    pub _rsvd96: [u8; 3],
    /// Namespace Attributes
    pub nsattr: u8,
    /// NVM Set Identifier
    pub nvmsetid: u16,
    /// Endurance Group Identifier
    pub endgid: u16,
    /// Namespace Globally Unique Identifier
    pub nguid: [u8; 16],
    /// IEEE Extended Unique Identifier
    pub eui64: [u8; 8],
    /// LBA Format Support (up to 64 formats, we only care about first 16)
    pub lbaf: [LbaFormat; 16],
}

/// LBA Format descriptor
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LbaFormat {
    /// Metadata Size
    pub ms: u16,
    /// LBA Data Size (as power of 2)
    pub lbads: u8,
    /// Relative Performance
    pub rp: u8,
}

impl IdentifyNamespace {
    /// Get the block size for this namespace
    pub fn block_size(&self) -> u32 {
        let format_idx = (self.flbas & 0x0F) as usize;
        if format_idx < self.lbaf.len() {
            1u32 << self.lbaf[format_idx].lbads
        } else {
            512 // Default to 512 bytes
        }
    }
}

// =============================================================================
// NVMe Controller
// =============================================================================

/// Maximum number of I/O queues
pub const MAX_IO_QUEUES: usize = 16;
/// Maximum number of namespaces to track
pub const MAX_NAMESPACES: usize = 16;

/// NVMe Controller State
#[repr(C)]
pub struct NvmeController {
    /// BAR0 base address (virtual)
    pub bar0: u64,
    /// BAR0 physical address
    pub bar0_phys: u64,
    /// BAR0 size
    pub bar0_size: u64,

    /// PCI location
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,

    /// Controller capabilities
    pub cap: u64,
    /// Doorbell stride
    pub dstrd: u8,
    /// Maximum queue entries
    pub mqes: u16,
    /// Timeout (in 500ms units)
    pub timeout: u8,
    /// Minimum page size (4KB << mpsmin)
    pub mpsmin: u8,
    /// Maximum page size (4KB << mpsmax)
    pub mpsmax: u8,

    /// Admin queue pair
    pub admin_queue: NvmeQueuePair,

    /// Number of I/O queue pairs
    pub num_io_queues: u16,
    /// I/O queue pairs (static array)
    pub io_queues: [Option<NvmeQueuePair>; MAX_IO_QUEUES],

    /// Namespace info (NSID -> block size, total blocks)
    pub namespaces: [(u32, u64, u32); MAX_NAMESPACES], // (nsid, blocks, block_size)
    pub num_namespaces: u8,

    /// Model string (trimmed)
    pub model: [u8; 41],
    /// Serial number (trimmed)
    pub serial: [u8; 21],
    /// Firmware revision
    pub firmware: [u8; 9],

    /// Maximum transfer size in bytes
    pub max_transfer_size: u32,

    /// Lock
    pub lock: u64,

    /// Controller initialized
    pub initialized: bool,
}

impl NvmeController {
    /// Read controller register (32-bit)
    #[inline]
    pub fn read32(&self, offset: u64) -> u32 {
        unsafe { kmod_mmio_read32(self.bar0 + offset) }
    }

    /// Write controller register (32-bit)
    #[inline]
    pub fn write32(&self, offset: u64, value: u32) {
        unsafe { kmod_mmio_write32(self.bar0 + offset, value) }
    }

    /// Read controller register (64-bit)
    #[inline]
    pub fn read64(&self, offset: u64) -> u64 {
        let lo = self.read32(offset) as u64;
        let hi = self.read32(offset + 4) as u64;
        lo | (hi << 32)
    }

    /// Wait for controller ready
    fn wait_ready(&self, expected: bool) -> Result<(), i32> {
        for _ in 0..TIMEOUT_LOOPS {
            let csts = self.read32(REG_CSTS);
            if (csts & CSTS_CFS) != 0 {
                mod_error!(b"nvme: Controller fatal status\n");
                return Err(-1);
            }
            let ready = (csts & CSTS_RDY) != 0;
            if ready == expected {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        mod_error!(b"nvme: Timeout waiting for ready\n");
        Err(-2)
    }

    /// Disable controller
    fn disable(&mut self) -> Result<(), i32> {
        let cc = self.read32(REG_CC);
        if (cc & CC_EN) != 0 {
            self.write32(REG_CC, cc & !CC_EN);
            self.wait_ready(false)?;
        }
        Ok(())
    }

    /// Enable controller
    fn enable(&mut self) -> Result<(), i32> {
        // Configure CC: NVM command set, 4KB pages, default entry sizes
        let cc = CC_DEFAULT | CC_EN;
        self.write32(REG_CC, cc);
        self.wait_ready(true)
    }

    /// Initialize the controller
    pub fn init(&mut self) -> Result<(), i32> {
        mod_info!(b"nvme: Reading capabilities\n");

        // Read capabilities
        self.cap = self.read64(REG_CAP);
        self.mqes = ((self.cap & CAP_MQES_MASK) + 1) as u16;
        self.timeout = ((self.cap & CAP_TO_MASK) >> CAP_TO_SHIFT) as u8;
        self.dstrd = ((self.cap & CAP_DSTRD_MASK) >> CAP_DSTRD_SHIFT) as u8;
        self.mpsmin = ((self.cap & CAP_MPSMIN_MASK) >> CAP_MPSMIN_SHIFT) as u8;
        self.mpsmax = ((self.cap & CAP_MPSMAX_MASK) >> CAP_MPSMAX_SHIFT) as u8;

        // Disable controller first
        self.disable()?;

        // Create admin queue
        let admin_depth = core::cmp::min(ADMIN_QUEUE_DEPTH, self.mqes);
        let admin_q = create_admin_queue(admin_depth, self.bar0, self.dstrd);
        if admin_q.is_none() {
            mod_error!(b"nvme: Failed to create admin queue\n");
            return Err(-3);
        }
        self.admin_queue = admin_q.unwrap();

        // Setup admin queue registers
        setup_admin_queue_regs(&self.admin_queue, self.bar0);

        // Enable controller
        mod_info!(b"nvme: Enabling controller\n");
        self.enable()?;

        // Identify controller
        mod_info!(b"nvme: Identifying controller\n");
        self.identify_controller()?;

        // Request I/O queues
        self.configure_io_queues()?;

        // Identify namespaces
        self.identify_namespaces()?;

        self.initialized = true;
        mod_info!(b"nvme: Controller initialized\n");
        Ok(())
    }

    /// Identify controller
    fn identify_controller(&mut self) -> Result<(), i32> {
        // Allocate 4KB buffer for identify data
        let buf = unsafe { kmod_zalloc(4096, 4096) };
        if buf.is_null() {
            return Err(-4);
        }
        let buf_phys = unsafe { kmod_virt_to_phys(buf as u64) };

        // Build identify command
        let cid = self.admin_queue.alloc_cid();
        let cmd = NvmeCmd::identify(cid, 0, IDENTIFY_CNS_CONTROLLER, buf_phys);

        // Submit and wait
        let result = self.admin_queue.submit_and_wait(&cmd);

        if result.is_ok() {
            // Parse identify data
            let id = unsafe { &*(buf as *const IdentifyController) };
            
            // Copy model name (swap byte pairs and trim)
            for i in 0..40 {
                self.model[i] = id.mn[i];
            }
            self.model[40] = 0;
            
            // Copy serial
            for i in 0..20 {
                self.serial[i] = id.sn[i];
            }
            self.serial[20] = 0;

            // Copy firmware
            for i in 0..8 {
                self.firmware[i] = id.fr[i];
            }
            self.firmware[8] = 0;

            // Calculate max transfer size
            if id.mdts > 0 {
                let page_size: u32 = 4096 << self.mpsmin;
                self.max_transfer_size = page_size << id.mdts;
            } else {
                self.max_transfer_size = MAX_TRANSFER_SIZE as u32;
            }
        }

        unsafe { kmod_dealloc(buf, 4096, 4096); }
        result.map(|_| ())
    }

    /// Configure I/O queues
    fn configure_io_queues(&mut self) -> Result<(), i32> {
        // Request number of queues
        let desired_queues = MAX_IO_QUEUES as u16;
        let cid = self.admin_queue.alloc_cid();
        let cmd = NvmeCmd::set_num_queues(cid, desired_queues, desired_queues);
        
        let result = self.admin_queue.submit_and_wait(&cmd)?;
        
        // Parse result - CDW0 contains allocated queues
        let ncqr = ((result.result_low() >> 16) & 0xFFFF) as u16 + 1;
        let nsqr = (result.result_low() & 0xFFFF) as u16 + 1;
        self.num_io_queues = core::cmp::min(ncqr, nsqr);
        self.num_io_queues = core::cmp::min(self.num_io_queues, MAX_IO_QUEUES as u16);

        // Create I/O queues
        let io_depth = core::cmp::min(IO_QUEUE_DEPTH, self.mqes);
        
        for i in 0..self.num_io_queues as usize {
            let qid = (i + 1) as u16;
            
            // Create queue pair
            let qp = NvmeQueuePair::new(qid, io_depth, self.bar0, self.dstrd);
            if qp.is_none() {
                mod_error!(b"nvme: Failed to create I/O queue\n");
                continue;
            }
            let qp = qp.unwrap();

            // Create completion queue first
            let cid = self.admin_queue.alloc_cid();
            let cmd = NvmeCmd::create_io_cq(cid, qid, io_depth, qp.cq_phys, qid - 1, false);
            if self.admin_queue.submit_and_wait(&cmd).is_err() {
                mod_error!(b"nvme: Failed to create I/O CQ\n");
                continue;
            }

            // Create submission queue
            let cid = self.admin_queue.alloc_cid();
            let cmd = NvmeCmd::create_io_sq(cid, qid, io_depth, qp.sq_phys, qid);
            if self.admin_queue.submit_and_wait(&cmd).is_err() {
                mod_error!(b"nvme: Failed to create I/O SQ\n");
                // Delete CQ we just created
                let cid = self.admin_queue.alloc_cid();
                let _ = self.admin_queue.submit_and_wait(&NvmeCmd::delete_io_cq(cid, qid));
                continue;
            }

            self.io_queues[i] = Some(qp);
        }

        Ok(())
    }

    /// Identify namespaces
    fn identify_namespaces(&mut self) -> Result<(), i32> {
        // Allocate buffer
        let buf = unsafe { kmod_zalloc(4096, 4096) };
        if buf.is_null() {
            return Err(-5);
        }
        let buf_phys = unsafe { kmod_virt_to_phys(buf as u64) };

        // Try to identify namespace 1 (most common case)
        for nsid in 1..=MAX_NAMESPACES as u32 {
            let cid = self.admin_queue.alloc_cid();
            let cmd = NvmeCmd::identify(cid, nsid, IDENTIFY_CNS_NAMESPACE, buf_phys);
            
            if self.admin_queue.submit_and_wait(&cmd).is_ok() {
                let ns = unsafe { &*(buf as *const IdentifyNamespace) };
                if ns.nsze > 0 {
                    let block_size = ns.block_size();
                    let idx = self.num_namespaces as usize;
                    if idx < MAX_NAMESPACES {
                        self.namespaces[idx] = (nsid, ns.nsze, block_size);
                        self.num_namespaces += 1;
                    }
                }
            } else {
                // No more namespaces
                break;
            }
        }

        unsafe { kmod_dealloc(buf, 4096, 4096); }
        Ok(())
    }

    /// Get the first I/O queue (for single-threaded I/O)
    pub fn get_io_queue(&mut self) -> Option<&mut NvmeQueuePair> {
        for q in self.io_queues.iter_mut() {
            if let Some(ref mut qp) = q {
                return Some(qp);
            }
        }
        None
    }

    /// Get namespace info by NSID
    pub fn get_namespace(&self, nsid: u32) -> Option<(u64, u32)> {
        for i in 0..self.num_namespaces as usize {
            if self.namespaces[i].0 == nsid {
                return Some((self.namespaces[i].1, self.namespaces[i].2));
            }
        }
        None
    }

    /// Shutdown controller
    pub fn shutdown(&mut self) {
        if !self.initialized {
            return;
        }

        // Set shutdown notification
        let cc = self.read32(REG_CC);
        self.write32(REG_CC, (cc & !CC_SHN_MASK) | CC_SHN_NORMAL);

        // Wait for shutdown complete
        for _ in 0..TIMEOUT_LOOPS {
            let csts = self.read32(REG_CSTS);
            if (csts & CSTS_SHST_MASK) == CSTS_SHST_COMPLETE {
                break;
            }
            core::hint::spin_loop();
        }

        // Cleanup I/O queues
        for q in self.io_queues.iter_mut() {
            if let Some(ref mut qp) = q {
                qp.cleanup();
            }
            *q = None;
        }

        // Cleanup admin queue
        self.admin_queue.cleanup();
        self.initialized = false;
    }
}

impl Drop for NvmeController {
    fn drop(&mut self) {
        self.shutdown();
    }
}
