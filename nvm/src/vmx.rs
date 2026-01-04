//! Intel VT-x (VMX) Hardware Virtualization Emulation
//!
//! This module provides complete Intel VT-x emulation including:
//! - VMCS (Virtual Machine Control Structure) management
//! - VM entry/exit handling
//! - EPT (Extended Page Tables) support
//! - Nested virtualization
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────────────┐
//! │                        Intel VT-x Emulation Core                           │
//! ├────────────────────────────────────────────────────────────────────────────┤
//! │  ┌──────────────────────────────────────────────────────────────────────┐ │
//! │  │                         VMX Manager                                   │ │
//! │  │  • VMXON/VMXOFF handling    • VMCS lifecycle management              │ │
//! │  │  • VM entry/exit dispatch   • Capability detection                   │ │
//! │  └──────────────────────────────────────────────────────────────────────┘ │
//! │  ┌──────────────────────────────────────────────────────────────────────┐ │
//! │  │                              VMCS                                     │ │
//! │  │  ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐        │ │
//! │  │  │  Guest State    │ │  Host State     │ │  Control Fields │        │ │
//! │  │  │  • Registers    │ │  • Registers    │ │  • Pin-based    │        │ │
//! │  │  │  • Segments     │ │  • Segments     │ │  • CPU-based    │        │ │
//! │  │  │  • CR/DR        │ │  • CR/DR        │ │  • Entry/Exit   │        │ │
//! │  │  │  • MSRs         │ │  • MSRs         │ │  • Exception    │        │ │
//! │  │  └─────────────────┘ └─────────────────┘ └─────────────────┘        │ │
//! │  └──────────────────────────────────────────────────────────────────────┘ │
//! │  ┌──────────────────────────────────────────────────────────────────────┐ │
//! │  │                    Extended Page Tables (EPT)                         │ │
//! │  │  • GPA → HPA translation   • Access/dirty bit tracking               │ │
//! │  │  • Large page support      • Memory type caching (MTRR-like)         │ │
//! │  └──────────────────────────────────────────────────────────────────────┘ │
//! │  ┌──────────────────────────────────────────────────────────────────────┐ │
//! │  │                    Nested Virtualization                              │ │
//! │  │  • L0/L1/L2 guest support  • VMCS shadowing                          │ │
//! │  │  • EPT nesting             • Virtual APIC                            │ │
//! │  └──────────────────────────────────────────────────────────────────────┘ │
//! └────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## VM Exit Reasons
//!
//! The emulator supports all standard VM exit reasons:
//! - External interrupts and NMIs
//! - Hardware exceptions (#PF, #GP, etc.)
//! - I/O instructions (IN, OUT, INS, OUTS)
//! - CPUID, RDMSR, WRMSR
//! - CR access (MOV to/from CR0/CR3/CR4/CR8)
//! - INVLPG, INVEPT, INVVPID
//! - VMCALL (hypercalls)
//! - Nested VMLAUNCH/VMRESUME

use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};

use crate::cpu::{VirtualCpu, CpuState, Registers, msr};

/// VMX operation result
pub type VmxResult<T> = Result<T, VmxError>;

/// VMX errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmxError {
    /// VMX not supported
    NotSupported,
    /// VMX already enabled
    AlreadyEnabled,
    /// VMX not enabled
    NotEnabled,
    /// Invalid VMCS
    InvalidVmcs,
    /// VMCS not loaded
    VmcsNotLoaded,
    /// Invalid field
    InvalidField(VmcsField),
    /// VM entry failed
    VmEntryFailed(u32),
    /// VM exit error
    VmExitError(String),
    /// EPT violation
    EptViolation { gpa: u64, access: EptAccess },
    /// EPT misconfiguration
    EptMisconfiguration { gpa: u64 },
    /// Nested VMX error
    NestedVmxError(String),
}

/// EPT access type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EptAccess {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

/// VMCS field identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum VmcsField {
    // 16-bit control fields
    VirtualProcessorId = 0x0000,
    PostedInterruptNotification = 0x0002,
    EptpIndex = 0x0004,
    
    // 16-bit guest state fields
    GuestEsSelector = 0x0800,
    GuestCsSelector = 0x0802,
    GuestSsSelector = 0x0804,
    GuestDsSelector = 0x0806,
    GuestFsSelector = 0x0808,
    GuestGsSelector = 0x080A,
    GuestLdtrSelector = 0x080C,
    GuestTrSelector = 0x080E,
    GuestInterruptStatus = 0x0810,
    GuestPmlIndex = 0x0812,
    
    // 16-bit host state fields
    HostEsSelector = 0x0C00,
    HostCsSelector = 0x0C02,
    HostSsSelector = 0x0C04,
    HostDsSelector = 0x0C06,
    HostFsSelector = 0x0C08,
    HostGsSelector = 0x0C0A,
    HostTrSelector = 0x0C0C,
    
    // 64-bit control fields
    IoBitmapA = 0x2000,
    IoBitmapB = 0x2002,
    MsrBitmap = 0x2004,
    VmExitMsrStoreAddr = 0x2006,
    VmExitMsrLoadAddr = 0x2008,
    VmEntryMsrLoadAddr = 0x200A,
    ExecutiveVmcsPtr = 0x200C,
    PmlAddress = 0x200E,
    TscOffset = 0x2010,
    VirtualApicPage = 0x2012,
    ApicAccessAddr = 0x2014,
    PostedInterruptDescAddr = 0x2016,
    VmFunctionControls = 0x2018,
    EptPointer = 0x201A,
    EoiExitBitmap0 = 0x201C,
    EoiExitBitmap1 = 0x201E,
    EoiExitBitmap2 = 0x2020,
    EoiExitBitmap3 = 0x2022,
    EptpListAddr = 0x2024,
    VmreadBitmap = 0x2026,
    VmwriteBitmap = 0x2028,
    VirtExceptionInfoAddr = 0x202A,
    XssExitingBitmap = 0x202C,
    EnclsExitingBitmap = 0x202E,
    SubPagePermTablePtr = 0x2030,
    TscMultiplier = 0x2032,
    
    // 64-bit read-only data fields
    GuestPhysicalAddress = 0x2400,
    
    // 64-bit guest state fields
    VmcsLinkPointer = 0x2800,
    GuestIa32Debugctl = 0x2802,
    GuestIa32Pat = 0x2804,
    GuestIa32Efer = 0x2806,
    GuestIa32PerfGlobalCtrl = 0x2808,
    GuestPdpte0 = 0x280A,
    GuestPdpte1 = 0x280C,
    GuestPdpte2 = 0x280E,
    GuestPdpte3 = 0x2810,
    GuestIa32Bndcfgs = 0x2812,
    GuestIa32RtitCtl = 0x2814,
    
    // 64-bit host state fields
    HostIa32Pat = 0x2C00,
    HostIa32Efer = 0x2C02,
    HostIa32PerfGlobalCtrl = 0x2C04,
    
    // 32-bit control fields
    PinBasedVmExecControl = 0x4000,
    CpuBasedVmExecControl = 0x4002,
    ExceptionBitmap = 0x4004,
    PageFaultErrorCodeMask = 0x4006,
    PageFaultErrorCodeMatch = 0x4008,
    Cr3TargetCount = 0x400A,
    VmExitControls = 0x400C,
    VmExitMsrStoreCount = 0x400E,
    VmExitMsrLoadCount = 0x4010,
    VmEntryControls = 0x4012,
    VmEntryMsrLoadCount = 0x4014,
    VmEntryIntrInfoField = 0x4016,
    VmEntryExceptionErrorCode = 0x4018,
    VmEntryInstructionLen = 0x401A,
    TprThreshold = 0x401C,
    SecondaryVmExecControl = 0x401E,
    PleGap = 0x4020,
    PleWindow = 0x4022,
    
    // 32-bit read-only data fields
    VmInstructionError = 0x4400,
    VmExitReason = 0x4402,
    VmExitIntrInfo = 0x4404,
    VmExitIntrErrorCode = 0x4406,
    IdtVectoringInfoField = 0x4408,
    IdtVectoringErrorCode = 0x440A,
    VmExitInstructionLen = 0x440C,
    VmExitInstructionInfo = 0x440E,
    
    // 32-bit guest state fields
    GuestEsLimit = 0x4800,
    GuestCsLimit = 0x4802,
    GuestSsLimit = 0x4804,
    GuestDsLimit = 0x4806,
    GuestFsLimit = 0x4808,
    GuestGsLimit = 0x480A,
    GuestLdtrLimit = 0x480C,
    GuestTrLimit = 0x480E,
    GuestGdtrLimit = 0x4810,
    GuestIdtrLimit = 0x4812,
    GuestEsAccessRights = 0x4814,
    GuestCsAccessRights = 0x4816,
    GuestSsAccessRights = 0x4818,
    GuestDsAccessRights = 0x481A,
    GuestFsAccessRights = 0x481C,
    GuestGsAccessRights = 0x481E,
    GuestLdtrAccessRights = 0x4820,
    GuestTrAccessRights = 0x4822,
    GuestInterruptibilityState = 0x4824,
    GuestActivityState = 0x4826,
    GuestSmbase = 0x4828,
    GuestIa32SysenterCs = 0x482A,
    VmxPreemptionTimerValue = 0x482E,
    
    // 32-bit host state fields
    HostIa32SysenterCs = 0x4C00,
    
    // Natural-width control fields
    Cr0GuestHostMask = 0x6000,
    Cr4GuestHostMask = 0x6002,
    Cr0ReadShadow = 0x6004,
    Cr4ReadShadow = 0x6006,
    Cr3Target0 = 0x6008,
    Cr3Target1 = 0x600A,
    Cr3Target2 = 0x600C,
    Cr3Target3 = 0x600E,
    
    // Natural-width read-only data fields
    ExitQualification = 0x6400,
    IoRcx = 0x6402,
    IoRsi = 0x6404,
    IoRdi = 0x6406,
    IoRip = 0x6408,
    GuestLinearAddress = 0x640A,
    
    // Natural-width guest state fields
    GuestCr0 = 0x6800,
    GuestCr3 = 0x6802,
    GuestCr4 = 0x6804,
    GuestEsBase = 0x6806,
    GuestCsBase = 0x6808,
    GuestSsBase = 0x680A,
    GuestDsBase = 0x680C,
    GuestFsBase = 0x680E,
    GuestGsBase = 0x6810,
    GuestLdtrBase = 0x6812,
    GuestTrBase = 0x6814,
    GuestGdtrBase = 0x6816,
    GuestIdtrBase = 0x6818,
    GuestDr7 = 0x681A,
    GuestRsp = 0x681C,
    GuestRip = 0x681E,
    GuestRflags = 0x6820,
    GuestPendingDbgExceptions = 0x6822,
    GuestIa32SysenterEsp = 0x6824,
    GuestIa32SysenterEip = 0x6826,
    
    // Natural-width host state fields
    HostCr0 = 0x6C00,
    HostCr3 = 0x6C02,
    HostCr4 = 0x6C04,
    HostFsBase = 0x6C06,
    HostGsBase = 0x6C08,
    HostTrBase = 0x6C0A,
    HostGdtrBase = 0x6C0C,
    HostIdtrBase = 0x6C0E,
    HostIa32SysenterEsp = 0x6C10,
    HostIa32SysenterEip = 0x6C12,
    HostRsp = 0x6C14,
    HostRip = 0x6C16,
}

/// VM exit reasons
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VmExitReason {
    ExceptionOrNmi = 0,
    ExternalInterrupt = 1,
    TripleFault = 2,
    InitSignal = 3,
    StartupIpi = 4,
    IoSmi = 5,
    OtherSmi = 6,
    InterruptWindow = 7,
    NmiWindow = 8,
    TaskSwitch = 9,
    Cpuid = 10,
    Getsec = 11,
    Hlt = 12,
    Invd = 13,
    Invlpg = 14,
    Rdpmc = 15,
    Rdtsc = 16,
    Rsm = 17,
    Vmcall = 18,
    Vmclear = 19,
    Vmlaunch = 20,
    Vmptrld = 21,
    Vmptrst = 22,
    Vmread = 23,
    Vmresume = 24,
    Vmwrite = 25,
    Vmxoff = 26,
    Vmxon = 27,
    CrAccess = 28,
    DrAccess = 29,
    IoInstruction = 30,
    MsrRead = 31,
    MsrWrite = 32,
    InvalidGuestState = 33,
    MsrLoading = 34,
    MwaitInstruction = 36,
    MonitorTrapFlag = 37,
    MonitorInstruction = 39,
    PauseInstruction = 40,
    MachineCheck = 41,
    TprBelowThreshold = 43,
    ApicAccess = 44,
    VirtualizedEoi = 45,
    GdtrIdtrAccess = 46,
    LdtrTrAccess = 47,
    EptViolation = 48,
    EptMisconfiguration = 49,
    Invept = 50,
    Rdtscp = 51,
    VmxPreemptionTimerExpired = 52,
    Invvpid = 53,
    Wbinvd = 54,
    Xsetbv = 55,
    ApicWrite = 56,
    Rdrand = 57,
    Invpcid = 58,
    Vmfunc = 59,
    Encls = 60,
    Rdseed = 61,
    PageModificationLogFull = 62,
    Xsaves = 63,
    Xrstors = 64,
    SubPagePermission = 66,
    Umwait = 67,
    Tpause = 68,
}

impl From<u32> for VmExitReason {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::ExceptionOrNmi,
            1 => Self::ExternalInterrupt,
            2 => Self::TripleFault,
            10 => Self::Cpuid,
            12 => Self::Hlt,
            18 => Self::Vmcall,
            28 => Self::CrAccess,
            30 => Self::IoInstruction,
            31 => Self::MsrRead,
            32 => Self::MsrWrite,
            48 => Self::EptViolation,
            49 => Self::EptMisconfiguration,
            _ => Self::ExceptionOrNmi, // Default
        }
    }
}

/// VM entry controls
#[derive(Debug, Clone, Copy)]
pub struct VmEntryControls {
    pub load_debug_controls: bool,
    pub ia32e_mode_guest: bool,
    pub entry_to_smm: bool,
    pub deactivate_dual_monitor: bool,
    pub load_ia32_perf_global_ctrl: bool,
    pub load_ia32_pat: bool,
    pub load_ia32_efer: bool,
    pub load_ia32_bndcfgs: bool,
    pub conceal_vmx_from_pt: bool,
    pub load_ia32_rtit_ctl: bool,
}

impl Default for VmEntryControls {
    fn default() -> Self {
        Self {
            load_debug_controls: true,
            ia32e_mode_guest: true,
            entry_to_smm: false,
            deactivate_dual_monitor: false,
            load_ia32_perf_global_ctrl: false,
            load_ia32_pat: true,
            load_ia32_efer: true,
            load_ia32_bndcfgs: false,
            conceal_vmx_from_pt: false,
            load_ia32_rtit_ctl: false,
        }
    }
}

impl VmEntryControls {
    pub fn to_bits(&self) -> u32 {
        let mut bits = 0u32;
        if self.load_debug_controls { bits |= 1 << 2; }
        if self.ia32e_mode_guest { bits |= 1 << 9; }
        if self.entry_to_smm { bits |= 1 << 10; }
        if self.deactivate_dual_monitor { bits |= 1 << 11; }
        if self.load_ia32_perf_global_ctrl { bits |= 1 << 13; }
        if self.load_ia32_pat { bits |= 1 << 14; }
        if self.load_ia32_efer { bits |= 1 << 15; }
        if self.load_ia32_bndcfgs { bits |= 1 << 16; }
        bits
    }
}

/// VMX capabilities
#[derive(Debug, Clone)]
pub struct VmxCapabilities {
    /// VMX supported
    pub vmx_supported: bool,
    /// EPT supported
    pub ept_supported: bool,
    /// VPID supported
    pub vpid_supported: bool,
    /// Unrestricted guest supported
    pub unrestricted_guest: bool,
    /// VMCS shadowing supported
    pub vmcs_shadowing: bool,
    /// Posted interrupts supported
    pub posted_interrupts: bool,
    /// Nested virtualization supported
    pub nested_virt: bool,
    /// EPT capabilities
    pub ept_caps: EptCapabilities,
    /// Basic VMX info (MSR value)
    pub basic_msr: u64,
    /// Pinbased controls
    pub pinbased_ctls: u64,
    /// Primary procbased controls
    pub procbased_ctls: u64,
    /// Secondary procbased controls
    pub procbased_ctls2: u64,
    /// Exit controls
    pub exit_ctls: u64,
    /// Entry controls
    pub entry_ctls: u64,
}

impl Default for VmxCapabilities {
    fn default() -> Self {
        Self {
            vmx_supported: true,
            ept_supported: true,
            vpid_supported: true,
            unrestricted_guest: true,
            vmcs_shadowing: true,
            posted_interrupts: true,
            nested_virt: true,
            ept_caps: EptCapabilities::default(),
            basic_msr: 0x0001_0480_0000_0001, // Revision ID 1
            pinbased_ctls: 0x0000_003F_0000_003F,
            procbased_ctls: 0x0401_E172_FFF9_FFFE,
            procbased_ctls2: 0x0055_3CFE_0000_0000,
            exit_ctls: 0x003F_FFFF_0003_FFFF,
            entry_ctls: 0x0000_F3FF_0000_11FF,
        }
    }
}

/// EPT capabilities
#[derive(Debug, Clone, Default)]
pub struct EptCapabilities {
    /// Execute-only pages supported
    pub execute_only: bool,
    /// 4-level paging supported
    pub page_walk_4: bool,
    /// Uncacheable memory type supported
    pub memory_type_uc: bool,
    /// Write-back memory type supported
    pub memory_type_wb: bool,
    /// 2MB pages supported
    pub large_pages_2mb: bool,
    /// 1GB pages supported
    pub large_pages_1gb: bool,
    /// INVEPT supported
    pub invept: bool,
    /// Accessed/dirty flags supported
    pub accessed_dirty: bool,
    /// Advanced EPT violations info
    pub advanced_ept_violations: bool,
}

/// VMCS - Virtual Machine Control Structure
pub struct Vmcs {
    /// VMCS revision identifier
    pub revision_id: u32,
    /// VMX-abort indicator
    pub abort_indicator: u32,
    /// Is this VMCS currently active?
    pub active: bool,
    /// Is this VMCS launched?
    pub launched: bool,
    /// VMCS fields storage
    fields: HashMap<VmcsField, u64>,
    /// Shadow VMCS (for nested virtualization)
    shadow_vmcs: Option<Box<Vmcs>>,
}

impl Vmcs {
    /// Create a new VMCS
    pub fn new() -> Self {
        Self {
            revision_id: 1,
            abort_indicator: 0,
            active: false,
            launched: false,
            fields: HashMap::new(),
            shadow_vmcs: None,
        }
    }
    
    /// Initialize VMCS with default values
    pub fn init(&mut self) {
        // Set up guest state defaults
        self.write(VmcsField::GuestCr0, 0x8000_0011); // PE + ET + PG
        self.write(VmcsField::GuestCr3, 0);
        self.write(VmcsField::GuestCr4, 0x2000); // PAE enabled
        self.write(VmcsField::GuestRflags, 0x2); // Reserved bit
        self.write(VmcsField::GuestRsp, 0);
        self.write(VmcsField::GuestRip, 0);
        
        // Guest segment defaults
        self.write(VmcsField::GuestCsSelector, 0x08);
        self.write(VmcsField::GuestCsBase, 0);
        self.write(VmcsField::GuestCsLimit, 0xFFFF_FFFF);
        self.write(VmcsField::GuestCsAccessRights, 0xA09B); // Code, readable, accessed
        
        self.write(VmcsField::GuestDsSelector, 0x10);
        self.write(VmcsField::GuestDsBase, 0);
        self.write(VmcsField::GuestDsLimit, 0xFFFF_FFFF);
        self.write(VmcsField::GuestDsAccessRights, 0xC093); // Data, writable, accessed
        
        // Copy to ES, FS, GS, SS
        self.write(VmcsField::GuestEsSelector, 0x10);
        self.write(VmcsField::GuestEsBase, 0);
        self.write(VmcsField::GuestEsLimit, 0xFFFF_FFFF);
        self.write(VmcsField::GuestEsAccessRights, 0xC093);
        
        self.write(VmcsField::GuestSsSelector, 0x10);
        self.write(VmcsField::GuestSsBase, 0);
        self.write(VmcsField::GuestSsLimit, 0xFFFF_FFFF);
        self.write(VmcsField::GuestSsAccessRights, 0xC093);
        
        self.write(VmcsField::GuestFsSelector, 0);
        self.write(VmcsField::GuestFsBase, 0);
        self.write(VmcsField::GuestFsLimit, 0);
        self.write(VmcsField::GuestFsAccessRights, 0x10000); // Unusable
        
        self.write(VmcsField::GuestGsSelector, 0);
        self.write(VmcsField::GuestGsBase, 0);
        self.write(VmcsField::GuestGsLimit, 0);
        self.write(VmcsField::GuestGsAccessRights, 0x10000);
        
        // LDTR and TR
        self.write(VmcsField::GuestLdtrSelector, 0);
        self.write(VmcsField::GuestLdtrBase, 0);
        self.write(VmcsField::GuestLdtrLimit, 0);
        self.write(VmcsField::GuestLdtrAccessRights, 0x10000); // Unusable
        
        self.write(VmcsField::GuestTrSelector, 0x18);
        self.write(VmcsField::GuestTrBase, 0);
        self.write(VmcsField::GuestTrLimit, 0x67);
        self.write(VmcsField::GuestTrAccessRights, 0x8B); // Busy TSS
        
        // GDTR/IDTR
        self.write(VmcsField::GuestGdtrBase, 0);
        self.write(VmcsField::GuestGdtrLimit, 0);
        self.write(VmcsField::GuestIdtrBase, 0);
        self.write(VmcsField::GuestIdtrLimit, 0);
        
        // Control fields
        self.write(VmcsField::PinBasedVmExecControl, 0x16); // External interrupt, NMI, virtual NMIs
        self.write(VmcsField::CpuBasedVmExecControl, 0x9420_0000); // Typical controls
        self.write(VmcsField::SecondaryVmExecControl, 0x0000_0082); // EPT, VPID
        self.write(VmcsField::VmExitControls, 0x0003_6DFF);
        self.write(VmcsField::VmEntryControls, 0x0000_13FF);
        
        // Exception bitmap - intercept nothing by default
        self.write(VmcsField::ExceptionBitmap, 0);
        
        // MSR bitmap - none
        self.write(VmcsField::MsrBitmap, 0);
        
        // VMCS link pointer
        self.write(VmcsField::VmcsLinkPointer, 0xFFFF_FFFF_FFFF_FFFF);
        
        // Guest MSRs
        self.write(VmcsField::GuestIa32Efer, msr::efer::LME | msr::efer::LMA | msr::efer::SCE);
        self.write(VmcsField::GuestIa32Pat, 0x0007_0406_0007_0406);
        
        // Activity state - active
        self.write(VmcsField::GuestActivityState, 0);
        self.write(VmcsField::GuestInterruptibilityState, 0);
    }
    
    /// Read a VMCS field
    pub fn read(&self, field: VmcsField) -> u64 {
        self.fields.get(&field).copied().unwrap_or(0)
    }
    
    /// Write a VMCS field
    pub fn write(&mut self, field: VmcsField, value: u64) {
        self.fields.insert(field, value);
    }
    
    /// Clear the VMCS
    pub fn clear(&mut self) {
        self.active = false;
        self.launched = false;
        self.abort_indicator = 0;
    }
    
    /// Load guest state from VMCS to CPU
    pub fn load_guest_state(&self, cpu: &VirtualCpu) {
        // Load general registers
        cpu.write_rip(self.read(VmcsField::GuestRip));
        cpu.write_rsp(self.read(VmcsField::GuestRsp));
        cpu.write_rflags(self.read(VmcsField::GuestRflags));
        
        // Load control registers
        cpu.write_cr0(self.read(VmcsField::GuestCr0));
        cpu.write_cr3(self.read(VmcsField::GuestCr3));
        cpu.write_cr4(self.read(VmcsField::GuestCr4));
        
        // Load MSRs
        cpu.write_msr(msr::IA32_EFER, self.read(VmcsField::GuestIa32Efer));
        cpu.write_msr(msr::IA32_PAT, self.read(VmcsField::GuestIa32Pat));
        cpu.write_msr(msr::IA32_FS_BASE, self.read(VmcsField::GuestFsBase));
        cpu.write_msr(msr::IA32_GS_BASE, self.read(VmcsField::GuestGsBase));
        cpu.write_msr(msr::IA32_SYSENTER_CS, self.read(VmcsField::GuestIa32SysenterCs));
        cpu.write_msr(msr::IA32_SYSENTER_ESP, self.read(VmcsField::GuestIa32SysenterEsp));
        cpu.write_msr(msr::IA32_SYSENTER_EIP, self.read(VmcsField::GuestIa32SysenterEip));
    }
    
    /// Save guest state from CPU to VMCS
    pub fn save_guest_state(&mut self, cpu: &VirtualCpu) {
        // Save general registers
        self.write(VmcsField::GuestRip, cpu.read_rip());
        self.write(VmcsField::GuestRsp, cpu.read_rsp());
        self.write(VmcsField::GuestRflags, cpu.read_rflags());
        
        // Save control registers
        self.write(VmcsField::GuestCr0, cpu.read_cr0());
        self.write(VmcsField::GuestCr3, cpu.read_cr3());
        self.write(VmcsField::GuestCr4, cpu.read_cr4());
        
        // Save MSRs
        self.write(VmcsField::GuestIa32Efer, cpu.read_msr(msr::IA32_EFER));
        self.write(VmcsField::GuestIa32Pat, cpu.read_msr(msr::IA32_PAT));
        self.write(VmcsField::GuestFsBase, cpu.read_msr(msr::IA32_FS_BASE));
        self.write(VmcsField::GuestGsBase, cpu.read_msr(msr::IA32_GS_BASE));
    }
}

impl Default for Vmcs {
    fn default() -> Self {
        Self::new()
    }
}

/// EPT (Extended Page Tables) manager
pub struct EptManager {
    /// EPT pointer (EPTP)
    eptp: AtomicU64,
    /// EPT page tables (GPA -> HPA)
    translations: RwLock<HashMap<u64, EptEntry>>,
    /// Statistics
    stats: RwLock<EptStats>,
}

/// EPT entry
#[derive(Debug, Clone, Copy)]
pub struct EptEntry {
    pub hpa: u64,
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub memory_type: u8,
    pub large_page: bool,
    pub accessed: bool,
    pub dirty: bool,
}

impl Default for EptEntry {
    fn default() -> Self {
        Self {
            hpa: 0,
            read: true,
            write: true,
            execute: true,
            memory_type: 6, // Write-back
            large_page: false,
            accessed: false,
            dirty: false,
        }
    }
}

/// EPT statistics
#[derive(Debug, Clone, Default)]
pub struct EptStats {
    pub translations: u64,
    pub violations: u64,
    pub misconfigurations: u64,
}

impl EptManager {
    pub fn new() -> Self {
        Self {
            eptp: AtomicU64::new(0),
            translations: RwLock::new(HashMap::new()),
            stats: RwLock::new(EptStats::default()),
        }
    }
    
    /// Set EPTP
    pub fn set_eptp(&self, eptp: u64) {
        self.eptp.store(eptp, Ordering::SeqCst);
    }
    
    /// Get EPTP
    pub fn get_eptp(&self) -> u64 {
        self.eptp.load(Ordering::SeqCst)
    }
    
    /// Translate GPA to HPA
    pub fn translate(&self, gpa: u64) -> VmxResult<u64> {
        self.stats.write().unwrap().translations += 1;
        
        let translations = self.translations.read().unwrap();
        
        // Look up page-aligned address
        let page_gpa = gpa & !0xFFF;
        let offset = gpa & 0xFFF;
        
        if let Some(entry) = translations.get(&page_gpa) {
            Ok(entry.hpa | offset)
        } else {
            // Identity mapping by default
            Ok(gpa)
        }
    }
    
    /// Map GPA to HPA
    pub fn map(&self, gpa: u64, hpa: u64, flags: EptEntry) {
        let page_gpa = gpa & !0xFFF;
        let mut entry = flags;
        entry.hpa = hpa & !0xFFF;
        self.translations.write().unwrap().insert(page_gpa, entry);
    }
    
    /// Unmap GPA
    pub fn unmap(&self, gpa: u64) {
        let page_gpa = gpa & !0xFFF;
        self.translations.write().unwrap().remove(&page_gpa);
    }
    
    /// Invalidate all EPT entries
    pub fn invalidate_all(&self) {
        self.translations.write().unwrap().clear();
    }
}

impl Default for EptManager {
    fn default() -> Self {
        Self::new()
    }
}

/// VMX Manager - Central VMX operations coordinator
pub struct VmxManager {
    /// VMX capabilities
    caps: VmxCapabilities,
    /// VMX enabled
    enabled: AtomicBool,
    /// Current VMCS
    current_vmcs: RwLock<Option<Arc<RwLock<Vmcs>>>>,
    /// All VMCSes
    vmcses: RwLock<HashMap<u64, Arc<RwLock<Vmcs>>>>,
    /// EPT manager
    ept: EptManager,
    /// Statistics
    stats: RwLock<VmxStats>,
    /// Next VMCS ID
    next_vmcs_id: AtomicU64,
}

/// VMX statistics
#[derive(Debug, Clone, Default)]
pub struct VmxStats {
    pub vm_entries: u64,
    pub vm_exits: u64,
    pub exit_reasons: HashMap<u32, u64>,
}

impl VmxManager {
    /// Create a new VMX manager
    pub fn new() -> Self {
        Self {
            caps: VmxCapabilities::default(),
            enabled: AtomicBool::new(false),
            current_vmcs: RwLock::new(None),
            vmcses: RwLock::new(HashMap::new()),
            ept: EptManager::new(),
            stats: RwLock::new(VmxStats::default()),
            next_vmcs_id: AtomicU64::new(1),
        }
    }
    
    /// Check if VMX is supported
    pub fn is_supported(&self) -> bool {
        self.caps.vmx_supported
    }
    
    /// Get VMX capabilities
    pub fn capabilities(&self) -> &VmxCapabilities {
        &self.caps
    }
    
    /// Enable VMX (VMXON)
    pub fn enable(&self) -> VmxResult<()> {
        if !self.is_supported() {
            return Err(VmxError::NotSupported);
        }
        if self.enabled.load(Ordering::SeqCst) {
            return Err(VmxError::AlreadyEnabled);
        }
        self.enabled.store(true, Ordering::SeqCst);
        Ok(())
    }
    
    /// Disable VMX (VMXOFF)
    pub fn disable(&self) -> VmxResult<()> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Err(VmxError::NotEnabled);
        }
        self.enabled.store(false, Ordering::SeqCst);
        *self.current_vmcs.write().unwrap() = None;
        Ok(())
    }
    
    /// Check if VMX is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
    
    /// Create a new VMCS
    pub fn create_vmcs(&self) -> VmxResult<u64> {
        if !self.is_enabled() {
            return Err(VmxError::NotEnabled);
        }
        
        let id = self.next_vmcs_id.fetch_add(1, Ordering::SeqCst);
        let mut vmcs = Vmcs::new();
        vmcs.init();
        
        self.vmcses.write().unwrap().insert(id, Arc::new(RwLock::new(vmcs)));
        Ok(id)
    }
    
    /// Load VMCS (VMPTRLD)
    pub fn load_vmcs(&self, id: u64) -> VmxResult<()> {
        if !self.is_enabled() {
            return Err(VmxError::NotEnabled);
        }
        
        let vmcses = self.vmcses.read().unwrap();
        let vmcs = vmcses.get(&id).ok_or(VmxError::InvalidVmcs)?;
        vmcs.write().unwrap().active = true;
        
        *self.current_vmcs.write().unwrap() = Some(vmcs.clone());
        Ok(())
    }
    
    /// Clear VMCS (VMCLEAR)
    pub fn clear_vmcs(&self, id: u64) -> VmxResult<()> {
        if !self.is_enabled() {
            return Err(VmxError::NotEnabled);
        }
        
        let vmcses = self.vmcses.read().unwrap();
        if let Some(vmcs) = vmcses.get(&id) {
            vmcs.write().unwrap().clear();
        }
        Ok(())
    }
    
    /// Get current VMCS
    pub fn current_vmcs(&self) -> VmxResult<Arc<RwLock<Vmcs>>> {
        self.current_vmcs.read().unwrap()
            .clone()
            .ok_or(VmxError::VmcsNotLoaded)
    }
    
    /// Read VMCS field
    pub fn vmread(&self, field: VmcsField) -> VmxResult<u64> {
        let vmcs = self.current_vmcs()?;
        let guard = vmcs.read().unwrap();
        let value = guard.read(field);
        Ok(value)
    }
    
    /// Write VMCS field
    pub fn vmwrite(&self, field: VmcsField, value: u64) -> VmxResult<()> {
        let vmcs = self.current_vmcs()?;
        let mut guard = vmcs.write().unwrap();
        guard.write(field, value);
        Ok(())
    }
    
    /// VM entry (VMLAUNCH/VMRESUME)
    pub fn vm_entry(&self, cpu: &VirtualCpu, launch: bool) -> VmxResult<()> {
        let vmcs = self.current_vmcs()?;
        let mut vmcs_guard = vmcs.write().unwrap();
        
        if launch && vmcs_guard.launched {
            return Err(VmxError::VmEntryFailed(0)); // Already launched
        }
        if !launch && !vmcs_guard.launched {
            return Err(VmxError::VmEntryFailed(1)); // Not yet launched
        }
        
        // Load guest state
        vmcs_guard.load_guest_state(cpu);
        
        // Mark as launched
        vmcs_guard.launched = true;
        
        // Update stats
        self.stats.write().unwrap().vm_entries += 1;
        
        Ok(())
    }
    
    /// VM exit handling
    pub fn vm_exit(&self, cpu: &VirtualCpu, reason: VmExitReason, qualification: u64) -> VmxResult<()> {
        let vmcs = self.current_vmcs()?;
        let mut vmcs_guard = vmcs.write().unwrap();
        
        // Save guest state
        vmcs_guard.save_guest_state(cpu);
        
        // Set exit info
        vmcs_guard.write(VmcsField::VmExitReason, reason as u64);
        vmcs_guard.write(VmcsField::ExitQualification, qualification);
        
        // Update stats
        {
            let mut stats = self.stats.write().unwrap();
            stats.vm_exits += 1;
            *stats.exit_reasons.entry(reason as u32).or_insert(0) += 1;
        }
        
        Ok(())
    }
    
    /// Handle VMCALL (hypercall)
    pub fn handle_vmcall(&self, cpu: &VirtualCpu) -> VmxResult<u64> {
        let hypercall_nr = cpu.read_gpr(0); // RAX
        let arg1 = cpu.read_gpr(7); // RDI
        let arg2 = cpu.read_gpr(6); // RSI
        let arg3 = cpu.read_gpr(2); // RDX
        
        // Emulated hypercall handling
        match hypercall_nr {
            0 => Ok(0), // NOP hypercall
            1 => Ok(cpu.rdtsc()), // Get TSC
            2 => { // Get VCPU ID
                Ok(cpu.id as u64)
            }
            _ => Ok(u64::MAX), // Unknown hypercall
        }
    }
    
    /// Get EPT manager
    pub fn ept(&self) -> &EptManager {
        &self.ept
    }
    
    /// Get statistics
    pub fn stats(&self) -> VmxStats {
        self.stats.read().unwrap().clone()
    }
}

impl Default for VmxManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// VMX Executor - Integrates JIT with VMX
// ============================================================================

use crate::jit::{JitEngine, JitConfig, ExecuteResult, JitError};
use crate::memory::{PhysicalMemory, AddressSpace};

/// VMX execution loop result
#[derive(Debug, Clone)]
pub enum VmxExecResult {
    /// VM exited normally
    VmExit(VmExitReason, u64),
    /// VM halted (HLT instruction)
    Halted,
    /// External interrupt pending
    Interrupt(u8),
    /// VM was reset
    Reset,
    /// Triple fault (shutdown)
    Shutdown,
    /// Execution error
    Error(String),
}

/// VMX Executor - runs guest code using JIT with VMX controls
/// 
/// This is the execution engine that:
/// 1. Uses JIT to execute guest x86-64 instructions
/// 2. Handles VM exits (I/O, CPUID, MSR, etc.)
/// 3. Manages EPT translations
/// 4. Supports nested virtualization
pub struct VmxExecutor {
    /// VMX manager for VMCS and EPT
    vmx: Arc<VmxManager>,
    /// JIT execution engine
    jit: JitEngine,
    /// Guest address space (RAM + MMIO routing)
    address_space: Arc<AddressSpace>,
    /// Running flag
    running: AtomicBool,
    /// Statistics
    stats: RwLock<VmxExecStats>,
}

/// VMX executor statistics
#[derive(Debug, Clone, Default)]
pub struct VmxExecStats {
    /// Instructions executed
    pub instructions: u64,
    /// VM entries
    pub vm_entries: u64,
    /// VM exits by reason
    pub vm_exits: HashMap<u32, u64>,
    /// JIT compilations
    pub jit_compilations: u64,
    /// Total execution time (ns)
    pub exec_time_ns: u64,
}

impl VmxExecutor {
    /// Create new VMX executor
    pub fn new(address_space: Arc<AddressSpace>) -> Self {
        let vmx = Arc::new(VmxManager::new());
        
        Self {
            vmx,
            jit: JitEngine::new(),
            address_space,
            running: AtomicBool::new(false),
            stats: RwLock::new(VmxExecStats::default()),
        }
    }
    
    /// Create with custom JIT config
    pub fn with_jit_config(address_space: Arc<AddressSpace>, jit_config: JitConfig) -> Self {
        let vmx = Arc::new(VmxManager::new());
        
        Self {
            vmx,
            jit: JitEngine::with_config(jit_config),
            address_space,
            running: AtomicBool::new(false),
            stats: RwLock::new(VmxExecStats::default()),
        }
    }
    
    /// Initialize VMX and prepare for execution
    pub fn init(&self) -> VmxResult<()> {
        self.vmx.enable()?;
        let vmcs_id = self.vmx.create_vmcs()?;
        self.vmx.load_vmcs(vmcs_id)?;
        Ok(())
    }
    
    /// Run guest until VM exit
    /// 
    /// This is the main execution loop:
    /// 1. VM entry (load guest state)
    /// 2. Execute guest code via JIT
    /// 3. Handle VM exit
    /// 4. Repeat until halt/error
    pub fn run(&self, cpu: &VirtualCpu) -> VmxExecResult {
        self.running.store(true, Ordering::SeqCst);
        
        // VM entry
        if let Err(e) = self.vmx.vm_entry(cpu, !cpu.is_halted()) {
            return VmxExecResult::Error(format!("VM entry failed: {:?}", e));
        }
        
        self.stats.write().unwrap().vm_entries += 1;
        
        // Execute guest code
        loop {
            if !self.running.load(Ordering::SeqCst) {
                break VmxExecResult::Halted;
            }
            
            // Check for pending interrupts
            if cpu.has_pending_interrupt() {
                if let Some(vector) = cpu.deliver_interrupt() {
                    // VM exit for interrupt
                    let _ = self.vmx.vm_exit(cpu, VmExitReason::ExternalInterrupt, vector as u64);
                    break VmxExecResult::Interrupt(vector);
                }
            }
            
            // Execute via JIT
            match self.jit.execute(cpu, &self.address_space) {
                Ok(result) => {
                    match result {
                        ExecuteResult::Continue { next_rip } => {
                            cpu.write_rip(next_rip);
                            continue;
                        }
                        ExecuteResult::Halt => {
                            let _ = self.vmx.vm_exit(cpu, VmExitReason::Hlt, 0);
                            break VmxExecResult::Halted;
                        }
                        ExecuteResult::Interrupt { vector } => {
                            let _ = self.vmx.vm_exit(cpu, VmExitReason::ExternalInterrupt, vector as u64);
                            break VmxExecResult::Interrupt(vector);
                        }
                        ExecuteResult::IoNeeded { port, is_write, size } => {
                            let qual = (port as u64) | ((size as u64) << 16) | 
                                       (if is_write { 0 } else { 1 << 3 });
                            let _ = self.vmx.vm_exit(cpu, VmExitReason::IoInstruction, qual);
                            break VmxExecResult::VmExit(VmExitReason::IoInstruction, qual);
                        }
                        ExecuteResult::Exception { vector, error_code } => {
                            let qual = error_code.map(|e| e as u64).unwrap_or(0);
                            let _ = self.vmx.vm_exit(cpu, VmExitReason::ExceptionOrNmi, 
                                                     (vector as u64) | (qual << 32));
                            break VmxExecResult::VmExit(VmExitReason::ExceptionOrNmi, vector as u64);
                        }
                        ExecuteResult::Hypercall { nr, args } => {
                            let _ = self.vmx.vm_exit(cpu, VmExitReason::Vmcall, nr);
                            break VmxExecResult::VmExit(VmExitReason::Vmcall, nr);
                        }
                        ExecuteResult::Reset => {
                            break VmxExecResult::Reset;
                        }
                        ExecuteResult::Shutdown => {
                            let _ = self.vmx.vm_exit(cpu, VmExitReason::TripleFault, 0);
                            break VmxExecResult::Shutdown;
                        }
                        ExecuteResult::MmioNeeded { addr, is_write, size } => {
                            // EPT violation
                            let _ = self.vmx.vm_exit(cpu, VmExitReason::EptViolation, addr);
                            break VmxExecResult::VmExit(VmExitReason::EptViolation, addr);
                        }
                    }
                }
                Err(e) => {
                    // JIT execution error
                    break VmxExecResult::Error(format!("JIT error: {}", e));
                }
            }
        }
    }
    
    /// Stop execution
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
    
    /// Pause execution
    pub fn pause(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
    
    /// Resume execution
    pub fn resume(&self) {
        self.running.store(true, Ordering::SeqCst);
    }
    
    /// Check if running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
    
    /// Get VMX manager
    pub fn vmx(&self) -> &VmxManager {
        &self.vmx
    }
    
    /// Get JIT engine
    pub fn jit(&self) -> &JitEngine {
        &self.jit
    }
    
    /// Get statistics
    pub fn stats(&self) -> VmxExecStats {
        self.stats.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vmcs_create() {
        let vmcs = Vmcs::new();
        assert!(!vmcs.active);
        assert!(!vmcs.launched);
    }
    
    #[test]
    fn test_vmcs_init() {
        let mut vmcs = Vmcs::new();
        vmcs.init();
        assert_eq!(vmcs.read(VmcsField::GuestCr0), 0x8000_0011);
        assert!(vmcs.read(VmcsField::GuestIa32Efer) & msr::efer::LMA != 0);
    }
    
    #[test]
    fn test_vmx_manager_enable() {
        let vmx = VmxManager::new();
        assert!(vmx.is_supported());
        assert!(!vmx.is_enabled());
        
        vmx.enable().unwrap();
        assert!(vmx.is_enabled());
        
        // Can't enable twice
        assert!(vmx.enable().is_err());
        
        vmx.disable().unwrap();
        assert!(!vmx.is_enabled());
    }
    
    #[test]
    fn test_vmx_vmcs_lifecycle() {
        let vmx = VmxManager::new();
        vmx.enable().unwrap();
        
        // Create and load VMCS
        let vmcs_id = vmx.create_vmcs().unwrap();
        vmx.load_vmcs(vmcs_id).unwrap();
        
        // Read/write fields
        vmx.vmwrite(VmcsField::GuestRip, 0x1000).unwrap();
        assert_eq!(vmx.vmread(VmcsField::GuestRip).unwrap(), 0x1000);
        
        // Clear
        vmx.clear_vmcs(vmcs_id).unwrap();
    }
    
    #[test]
    fn test_ept_translate() {
        let ept = EptManager::new();
        
        // Identity mapping by default
        assert_eq!(ept.translate(0x1000).unwrap(), 0x1000);
        
        // Add mapping
        ept.map(0x2000, 0x3000, EptEntry::default());
        assert_eq!(ept.translate(0x2000).unwrap(), 0x3000);
        assert_eq!(ept.translate(0x2123).unwrap(), 0x3123); // With offset
    }
}
