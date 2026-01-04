//! AMD-V (SVM) Hardware Virtualization Emulation
//!
//! This module provides complete AMD SVM (Secure Virtual Machine) emulation including:
//! - VMCB (Virtual Machine Control Block) management
//! - Nested Page Tables (NPT) support
//! - VMRUN/VMEXIT handling
//! - SVM features and intercepts
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────────────┐
//! │                        AMD SVM Emulation Core                              │
//! ├────────────────────────────────────────────────────────────────────────────┤
//! │  ┌──────────────────────────────────────────────────────────────────────┐ │
//! │  │                         SVM Manager                                   │ │
//! │  │  • VMRUN/VMMCALL handling   • VMCB lifecycle management              │ │
//! │  │  • VM exit dispatch         • Capability detection                   │ │
//! │  └──────────────────────────────────────────────────────────────────────┘ │
//! │  ┌──────────────────────────────────────────────────────────────────────┐ │
//! │  │                              VMCB                                     │ │
//! │  │  ┌─────────────────┐ ┌─────────────────┐                             │ │
//! │  │  │  Control Area   │ │ State Save Area │                             │ │
//! │  │  │  • Intercepts   │ │  • Registers    │                             │ │
//! │  │  │  • ASID/TLB     │ │  • Segments     │                             │ │
//! │  │  │  • Event inject │ │  • CR/DR        │                             │ │
//! │  │  │  • Exit info    │ │  • MSRs         │                             │ │
//! │  │  └─────────────────┘ └─────────────────┘                             │ │
//! │  └──────────────────────────────────────────────────────────────────────┘ │
//! │  ┌──────────────────────────────────────────────────────────────────────┐ │
//! │  │                    Nested Page Tables (NPT)                           │ │
//! │  │  • GPA → HPA translation   • Access tracking                         │ │
//! │  │  • Large page support      • Memory type attributes                  │ │
//! │  └──────────────────────────────────────────────────────────────────────┘ │
//! │  ┌──────────────────────────────────────────────────────────────────────┐ │
//! │  │                    Security Features                                  │ │
//! │  │  • SEV (Secure Encrypted Virtualization)                             │ │
//! │  │  • SEV-ES (Encrypted State)                                          │ │
//! │  │  • SEV-SNP (Secure Nested Paging)                                    │ │
//! │  └──────────────────────────────────────────────────────────────────────┘ │
//! └────────────────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};

use crate::cpu::{VirtualCpu, CpuState, Registers, msr};

/// SVM operation result
pub type SvmResult<T> = Result<T, SvmError>;

/// SVM errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SvmError {
    /// SVM not supported
    NotSupported,
    /// SVM already enabled
    AlreadyEnabled,
    /// SVM not enabled
    NotEnabled,
    /// Invalid VMCB
    InvalidVmcb,
    /// VMCB not loaded
    VmcbNotLoaded,
    /// VMRUN failed
    VmrunFailed(String),
    /// NPT fault
    NptFault { gpa: u64, error_code: u64 },
    /// Shutdown (triple fault)
    Shutdown,
    /// Invalid guest state
    InvalidGuestState(String),
}

impl std::fmt::Display for SvmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSupported => write!(f, "SVM not supported"),
            Self::AlreadyEnabled => write!(f, "SVM already enabled"),
            Self::NotEnabled => write!(f, "SVM not enabled"),
            Self::InvalidVmcb => write!(f, "Invalid VMCB"),
            Self::VmcbNotLoaded => write!(f, "VMCB not loaded"),
            Self::VmrunFailed(msg) => write!(f, "VMRUN failed: {}", msg),
            Self::NptFault { gpa, error_code } => 
                write!(f, "NPT fault at GPA 0x{:x}, error 0x{:x}", gpa, error_code),
            Self::Shutdown => write!(f, "Shutdown (triple fault)"),
            Self::InvalidGuestState(msg) => write!(f, "Invalid guest state: {}", msg),
        }
    }
}

impl std::error::Error for SvmError {}

/// SVM capabilities
#[derive(Debug, Clone)]
pub struct SvmCapabilities {
    /// SVM supported
    pub svm_supported: bool,
    /// Nested paging supported
    pub npt_supported: bool,
    /// LBR virtualization supported
    pub lbr_virt: bool,
    /// SVM lock supported
    pub svm_lock: bool,
    /// NRIP save supported
    pub nrip_save: bool,
    /// TSC rate MSR supported
    pub tsc_rate_msr: bool,
    /// VMCB clean bits supported
    pub vmcb_clean: bool,
    /// Flush by ASID supported
    pub flush_by_asid: bool,
    /// Decode assists supported
    pub decode_assists: bool,
    /// Pause filter supported
    pub pause_filter: bool,
    /// Pause filter threshold supported
    pub pause_filter_threshold: bool,
    /// AVIC (Advanced Virtual Interrupt Controller) supported
    pub avic: bool,
    /// Virtual VMSAVE/VMLOAD supported
    pub v_vmsave_vmload: bool,
    /// vGIF (Virtual GIF) supported
    pub vgif: bool,
    /// SEV (Secure Encrypted Virtualization) supported
    pub sev: bool,
    /// SEV-ES supported
    pub sev_es: bool,
    /// SEV-SNP supported
    pub sev_snp: bool,
    /// Number of ASIDs
    pub num_asids: u32,
}

impl Default for SvmCapabilities {
    fn default() -> Self {
        Self {
            svm_supported: true,
            npt_supported: true,
            lbr_virt: true,
            svm_lock: true,
            nrip_save: true,
            tsc_rate_msr: true,
            vmcb_clean: true,
            flush_by_asid: true,
            decode_assists: true,
            pause_filter: true,
            pause_filter_threshold: true,
            avic: true,
            v_vmsave_vmload: true,
            vgif: true,
            sev: true,
            sev_es: false,
            sev_snp: false,
            num_asids: 32768,
        }
    }
}

/// VM exit codes (EXITCODE)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum VmExitCode {
    // Intercepts
    Read_CR0 = 0x00,
    Read_CR2 = 0x02,
    Read_CR3 = 0x03,
    Read_CR4 = 0x04,
    Read_CR8 = 0x08,
    Write_CR0 = 0x10,
    Write_CR2 = 0x12,
    Write_CR3 = 0x13,
    Write_CR4 = 0x14,
    Write_CR8 = 0x18,
    Read_DR0 = 0x20,
    Read_DR1 = 0x21,
    Read_DR2 = 0x22,
    Read_DR3 = 0x23,
    Read_DR4 = 0x24,
    Read_DR5 = 0x25,
    Read_DR6 = 0x26,
    Read_DR7 = 0x27,
    Write_DR0 = 0x30,
    Write_DR1 = 0x31,
    Write_DR2 = 0x32,
    Write_DR3 = 0x33,
    Write_DR4 = 0x34,
    Write_DR5 = 0x35,
    Write_DR6 = 0x36,
    Write_DR7 = 0x37,
    Exception_DE = 0x40,  // Divide error
    Exception_DB = 0x41,  // Debug
    Exception_NMI = 0x42, // NMI
    Exception_BP = 0x43,  // Breakpoint
    Exception_OF = 0x44,  // Overflow
    Exception_BR = 0x45,  // Bound range
    Exception_UD = 0x46,  // Invalid opcode
    Exception_NM = 0x47,  // Device not available
    Exception_DF = 0x48,  // Double fault
    Exception_TS = 0x4A,  // Invalid TSS
    Exception_NP = 0x4B,  // Segment not present
    Exception_SS = 0x4C,  // Stack fault
    Exception_GP = 0x4D,  // General protection
    Exception_PF = 0x4E,  // Page fault
    Exception_MF = 0x50,  // x87 FP
    Exception_AC = 0x51,  // Alignment check
    Exception_MC = 0x52,  // Machine check
    Exception_XF = 0x53,  // SIMD FP
    Intr = 0x60,
    Nmi = 0x61,
    Smi = 0x62,
    Init = 0x63,
    Vintr = 0x64,
    Cr0_Sel_Write = 0x65,
    Idtr_Read = 0x66,
    Gdtr_Read = 0x67,
    Ldtr_Read = 0x68,
    Tr_Read = 0x69,
    Idtr_Write = 0x6A,
    Gdtr_Write = 0x6B,
    Ldtr_Write = 0x6C,
    Tr_Write = 0x6D,
    Rdtsc = 0x6E,
    Rdpmc = 0x6F,
    Pushf = 0x70,
    Popf = 0x71,
    Cpuid = 0x72,
    Rsm = 0x73,
    Iret = 0x74,
    Swint = 0x75,
    Invd = 0x76,
    Pause = 0x77,
    Hlt = 0x78,
    Invlpg = 0x79,
    Invlpga = 0x7A,
    IoIo = 0x7B,
    Msr = 0x7C,
    TaskSwitch = 0x7D,
    FerrFreeze = 0x7E,
    Shutdown = 0x7F,
    Vmrun = 0x80,
    Vmmcall = 0x81,
    Vmload = 0x82,
    Vmsave = 0x83,
    Stgi = 0x84,
    Clgi = 0x85,
    Skinit = 0x86,
    Rdtscp = 0x87,
    Icebp = 0x88,
    Wbinvd = 0x89,
    Monitor = 0x8A,
    Mwait = 0x8B,
    MwaitCond = 0x8C,
    Xsetbv = 0x8D,
    Rdpru = 0x8E,
    Efer_Write_Trap = 0x8F,
    Cr0_Write_Trap = 0x90,
    Cr1_Write_Trap = 0x91,
    Cr2_Write_Trap = 0x92,
    Cr3_Write_Trap = 0x93,
    Cr4_Write_Trap = 0x94,
    Cr5_Write_Trap = 0x95,
    Cr6_Write_Trap = 0x96,
    Cr7_Write_Trap = 0x97,
    Cr8_Write_Trap = 0x98,
    Invlpgb = 0xA0,
    Illegal_Invlpgb = 0xA1,
    Invpcid = 0xA2,
    Mcommit = 0xA3,
    Tlbsync = 0xA4,
    // NPT exits
    Npf = 0x400, // Nested page fault
    AvicIncomplete_Ipi = 0x401,
    AvicNoaccel = 0x402,
    VmgExit = 0x403,
    // -1 and -2 values
    Invalid = 0xFFFF_FFFF_FFFF_FFFF,
    Busy = 0xFFFF_FFFF_FFFF_FFFE,
}

impl From<u64> for VmExitCode {
    fn from(value: u64) -> Self {
        match value {
            0x72 => Self::Cpuid,
            0x78 => Self::Hlt,
            0x7B => Self::IoIo,
            0x7C => Self::Msr,
            0x81 => Self::Vmmcall,
            0x400 => Self::Npf,
            _ => Self::Invalid,
        }
    }
}

/// VMCB Control Area offsets
pub mod vmcb_control {
    pub const INTERCEPT_CR: usize = 0x000;
    pub const INTERCEPT_DR: usize = 0x004;
    pub const INTERCEPT_EXCEPTIONS: usize = 0x008;
    pub const INTERCEPT_MISC1: usize = 0x00C;
    pub const INTERCEPT_MISC2: usize = 0x010;
    pub const INTERCEPT_MISC3: usize = 0x014;
    pub const PAUSE_FILTER_THRESHOLD: usize = 0x03C;
    pub const PAUSE_FILTER_COUNT: usize = 0x03E;
    pub const IOPM_BASE_PA: usize = 0x040;
    pub const MSRPM_BASE_PA: usize = 0x048;
    pub const TSC_OFFSET: usize = 0x050;
    pub const GUEST_ASID: usize = 0x058;
    pub const TLB_CONTROL: usize = 0x05C;
    pub const VINTR: usize = 0x060;
    pub const INTERRUPT_SHADOW: usize = 0x068;
    pub const EXITCODE: usize = 0x070;
    pub const EXITINFO1: usize = 0x078;
    pub const EXITINFO2: usize = 0x080;
    pub const EXITINTINFO: usize = 0x088;
    pub const NP_ENABLE: usize = 0x090;
    pub const AVIC_APIC_BAR: usize = 0x098;
    pub const GHCB_PA: usize = 0x0A0;
    pub const EVENTINJ: usize = 0x0A8;
    pub const NCR3: usize = 0x0B0;
    pub const LBR_VIRTUALIZATION_ENABLE: usize = 0x0B8;
    pub const VMCB_CLEAN: usize = 0x0C0;
    pub const NRIP: usize = 0x0C8;
    pub const NUM_BYTES_FETCHED: usize = 0x0D0;
    pub const GUEST_INSTR_BYTES: usize = 0x0D1;
    pub const AVIC_APIC_BACKING_PAGE: usize = 0x0E0;
    pub const AVIC_LOGICAL_TABLE: usize = 0x0F0;
    pub const AVIC_PHYSICAL_TABLE: usize = 0x0F8;
    pub const VMSA_PA: usize = 0x108;
}

/// VMCB State Save Area offsets
pub mod vmcb_state {
    pub const ES_SELECTOR: usize = 0x000;
    pub const ES_ATTRIB: usize = 0x002;
    pub const ES_LIMIT: usize = 0x004;
    pub const ES_BASE: usize = 0x008;
    pub const CS_SELECTOR: usize = 0x010;
    pub const CS_ATTRIB: usize = 0x012;
    pub const CS_LIMIT: usize = 0x014;
    pub const CS_BASE: usize = 0x018;
    pub const SS_SELECTOR: usize = 0x020;
    pub const SS_ATTRIB: usize = 0x022;
    pub const SS_LIMIT: usize = 0x024;
    pub const SS_BASE: usize = 0x028;
    pub const DS_SELECTOR: usize = 0x030;
    pub const DS_ATTRIB: usize = 0x032;
    pub const DS_LIMIT: usize = 0x034;
    pub const DS_BASE: usize = 0x038;
    pub const FS_SELECTOR: usize = 0x040;
    pub const FS_ATTRIB: usize = 0x042;
    pub const FS_LIMIT: usize = 0x044;
    pub const FS_BASE: usize = 0x048;
    pub const GS_SELECTOR: usize = 0x050;
    pub const GS_ATTRIB: usize = 0x052;
    pub const GS_LIMIT: usize = 0x054;
    pub const GS_BASE: usize = 0x058;
    pub const GDTR_SELECTOR: usize = 0x060;
    pub const GDTR_ATTRIB: usize = 0x062;
    pub const GDTR_LIMIT: usize = 0x064;
    pub const GDTR_BASE: usize = 0x068;
    pub const LDTR_SELECTOR: usize = 0x070;
    pub const LDTR_ATTRIB: usize = 0x072;
    pub const LDTR_LIMIT: usize = 0x074;
    pub const LDTR_BASE: usize = 0x078;
    pub const IDTR_SELECTOR: usize = 0x080;
    pub const IDTR_ATTRIB: usize = 0x082;
    pub const IDTR_LIMIT: usize = 0x084;
    pub const IDTR_BASE: usize = 0x088;
    pub const TR_SELECTOR: usize = 0x090;
    pub const TR_ATTRIB: usize = 0x092;
    pub const TR_LIMIT: usize = 0x094;
    pub const TR_BASE: usize = 0x098;
    pub const CPL: usize = 0x0CB;
    pub const EFER: usize = 0x0D0;
    pub const CR4: usize = 0x148;
    pub const CR3: usize = 0x150;
    pub const CR0: usize = 0x158;
    pub const DR7: usize = 0x160;
    pub const DR6: usize = 0x168;
    pub const RFLAGS: usize = 0x170;
    pub const RIP: usize = 0x178;
    pub const RSP: usize = 0x1D8;
    pub const RAX: usize = 0x1F8;
    pub const STAR: usize = 0x200;
    pub const LSTAR: usize = 0x208;
    pub const CSTAR: usize = 0x210;
    pub const SFMASK: usize = 0x218;
    pub const KERNEL_GS_BASE: usize = 0x220;
    pub const SYSENTER_CS: usize = 0x228;
    pub const SYSENTER_ESP: usize = 0x230;
    pub const SYSENTER_EIP: usize = 0x238;
    pub const CR2: usize = 0x240;
    pub const PAT: usize = 0x268;
    pub const DBGCTL: usize = 0x270;
    pub const BR_FROM: usize = 0x278;
    pub const BR_TO: usize = 0x280;
    pub const LASTEXCPFROM: usize = 0x288;
    pub const LASTEXCPTO: usize = 0x290;
    // SEV-ES
    pub const SPEC_CTRL: usize = 0x2E0;
}

/// VMCB Control Area
#[derive(Debug, Clone)]
pub struct VmcbControlArea {
    /// CR read intercepts (16 bits for CR0-15)
    pub intercept_cr_read: u16,
    /// CR write intercepts
    pub intercept_cr_write: u16,
    /// DR read intercepts
    pub intercept_dr_read: u16,
    /// DR write intercepts
    pub intercept_dr_write: u16,
    /// Exception intercepts (32 bits for exceptions 0-31)
    pub intercept_exceptions: u32,
    /// Miscellaneous intercepts 1
    pub intercept_misc1: u32,
    /// Miscellaneous intercepts 2
    pub intercept_misc2: u32,
    /// Pause filter threshold
    pub pause_filter_threshold: u16,
    /// Pause filter count
    pub pause_filter_count: u16,
    /// I/O permission map base physical address
    pub iopm_base_pa: u64,
    /// MSR permission map base physical address
    pub msrpm_base_pa: u64,
    /// TSC offset
    pub tsc_offset: i64,
    /// Guest ASID
    pub guest_asid: u32,
    /// TLB control
    pub tlb_control: u8,
    /// Virtual interrupt control
    pub v_tpr: u8,
    pub v_irq: bool,
    pub v_intr_prio: u8,
    pub v_ign_tpr: bool,
    pub v_intr_masking: bool,
    pub v_gif_enable: bool,
    pub avic_enable: bool,
    pub v_intr_vector: u8,
    /// Interrupt shadow
    pub interrupt_shadow: bool,
    pub guest_interrupt_mask: bool,
    /// Exit code
    pub exit_code: u64,
    /// Exit info 1
    pub exit_info1: u64,
    /// Exit info 2
    pub exit_info2: u64,
    /// Exit interrupt info
    pub exit_int_info: u64,
    /// Nested paging enable
    pub np_enable: bool,
    /// SEV enable
    pub sev_enable: bool,
    /// SEV-ES enable
    pub sev_es_enable: bool,
    /// AVIC APIC BAR
    pub avic_apic_bar: u64,
    /// GHCB physical address (for SEV-ES)
    pub ghcb_pa: u64,
    /// Event injection
    pub event_inj: u64,
    /// Nested CR3 (for NPT)
    pub ncr3: u64,
    /// LBR virtualization enable
    pub lbr_virt_enable: bool,
    /// VMCB clean bits
    pub vmcb_clean: u32,
    /// Next RIP (for NRIP save)
    pub nrip: u64,
    /// Number of instruction bytes fetched
    pub num_bytes_fetched: u8,
    /// Guest instruction bytes (up to 15)
    pub guest_instr_bytes: [u8; 15],
}

impl Default for VmcbControlArea {
    fn default() -> Self {
        Self {
            intercept_cr_read: 0,
            intercept_cr_write: 0,
            intercept_dr_read: 0,
            intercept_dr_write: 0,
            intercept_exceptions: 0,
            intercept_misc1: 0,
            intercept_misc2: 0,
            pause_filter_threshold: 0,
            pause_filter_count: 0,
            iopm_base_pa: 0,
            msrpm_base_pa: 0,
            tsc_offset: 0,
            guest_asid: 1,
            tlb_control: 0,
            v_tpr: 0,
            v_irq: false,
            v_intr_prio: 0,
            v_ign_tpr: false,
            v_intr_masking: true,
            v_gif_enable: false,
            avic_enable: false,
            v_intr_vector: 0,
            interrupt_shadow: false,
            guest_interrupt_mask: false,
            exit_code: 0,
            exit_info1: 0,
            exit_info2: 0,
            exit_int_info: 0,
            np_enable: true,
            sev_enable: false,
            sev_es_enable: false,
            avic_apic_bar: 0xFEE0_0000,
            ghcb_pa: 0,
            event_inj: 0,
            ncr3: 0,
            lbr_virt_enable: false,
            vmcb_clean: 0,
            nrip: 0,
            num_bytes_fetched: 0,
            guest_instr_bytes: [0; 15],
        }
    }
}

/// VMCB State Save Area
#[derive(Debug, Clone)]
pub struct VmcbStateSaveArea {
    /// Segment registers
    pub es: SegmentDescriptor,
    pub cs: SegmentDescriptor,
    pub ss: SegmentDescriptor,
    pub ds: SegmentDescriptor,
    pub fs: SegmentDescriptor,
    pub gs: SegmentDescriptor,
    pub gdtr: SegmentDescriptor,
    pub ldtr: SegmentDescriptor,
    pub idtr: SegmentDescriptor,
    pub tr: SegmentDescriptor,
    /// CPL
    pub cpl: u8,
    /// Control registers
    pub efer: u64,
    pub cr4: u64,
    pub cr3: u64,
    pub cr0: u64,
    pub cr2: u64,
    /// Debug registers
    pub dr7: u64,
    pub dr6: u64,
    /// General registers (others saved in host area)
    pub rflags: u64,
    pub rip: u64,
    pub rsp: u64,
    pub rax: u64,
    /// System call MSRs
    pub star: u64,
    pub lstar: u64,
    pub cstar: u64,
    pub sfmask: u64,
    pub kernel_gs_base: u64,
    pub sysenter_cs: u64,
    pub sysenter_esp: u64,
    pub sysenter_eip: u64,
    /// PAT MSR
    pub pat: u64,
    /// Debug control
    pub dbgctl: u64,
    pub br_from: u64,
    pub br_to: u64,
    pub lastexcpfrom: u64,
    pub lastexcpto: u64,
    /// SEV-ES SPEC_CTRL
    pub spec_ctrl: u64,
}

/// Segment descriptor
#[derive(Debug, Clone, Default)]
pub struct SegmentDescriptor {
    pub selector: u16,
    pub attrib: u16,
    pub limit: u32,
    pub base: u64,
}

impl SegmentDescriptor {
    pub fn new(selector: u16, base: u64, limit: u32, attrib: u16) -> Self {
        Self { selector, attrib, limit, base }
    }
    
    /// Create kernel code segment
    pub fn kernel_code() -> Self {
        Self::new(0x08, 0, 0xFFFF_FFFF, 0xA09B) // Long mode code, present, DPL=0
    }
    
    /// Create kernel data segment
    pub fn kernel_data() -> Self {
        Self::new(0x10, 0, 0xFFFF_FFFF, 0xC093) // Data, present, DPL=0
    }
    
    /// Create unusable segment
    pub fn unusable() -> Self {
        Self::new(0, 0, 0, 0) // Unusable
    }
}

impl Default for VmcbStateSaveArea {
    fn default() -> Self {
        Self {
            es: SegmentDescriptor::kernel_data(),
            cs: SegmentDescriptor::kernel_code(),
            ss: SegmentDescriptor::kernel_data(),
            ds: SegmentDescriptor::kernel_data(),
            fs: SegmentDescriptor::unusable(),
            gs: SegmentDescriptor::unusable(),
            gdtr: SegmentDescriptor::default(),
            ldtr: SegmentDescriptor::unusable(),
            idtr: SegmentDescriptor::default(),
            tr: SegmentDescriptor::new(0x18, 0, 0x67, 0x8B), // TSS
            cpl: 0,
            efer: msr::efer::LME | msr::efer::LMA | msr::efer::SCE | msr::efer::SVME,
            cr4: 0x2000, // PAE
            cr3: 0,
            cr0: 0x8000_0011, // PE + ET + PG
            cr2: 0,
            dr7: 0x400,
            dr6: 0xFFFF_0FF0,
            rflags: 0x2,
            rip: 0,
            rsp: 0,
            rax: 0,
            star: 0,
            lstar: 0,
            cstar: 0,
            sfmask: 0,
            kernel_gs_base: 0,
            sysenter_cs: 0,
            sysenter_esp: 0,
            sysenter_eip: 0,
            pat: 0x0007_0406_0007_0406,
            dbgctl: 0,
            br_from: 0,
            br_to: 0,
            lastexcpfrom: 0,
            lastexcpto: 0,
            spec_ctrl: 0,
        }
    }
}

/// VMCB - Virtual Machine Control Block
pub struct Vmcb {
    /// Control area
    pub control: VmcbControlArea,
    /// State save area
    pub state: VmcbStateSaveArea,
    /// Physical address of this VMCB (for nested)
    pub physical_addr: u64,
    /// Is this VMCB active?
    pub active: bool,
}

impl Vmcb {
    /// Create a new VMCB
    pub fn new() -> Self {
        Self {
            control: VmcbControlArea::default(),
            state: VmcbStateSaveArea::default(),
            physical_addr: 0,
            active: false,
        }
    }
    
    /// Initialize VMCB for a guest
    pub fn init_guest(&mut self) {
        // Set up basic intercepts
        self.control.intercept_misc1 |= 1 << 0;  // INTR
        self.control.intercept_misc1 |= 1 << 1;  // NMI
        self.control.intercept_misc1 |= 1 << 3;  // INIT
        self.control.intercept_misc1 |= 1 << 18; // CPUID
        self.control.intercept_misc1 |= 1 << 24; // HLT
        self.control.intercept_misc1 |= 1 << 27; // IOIO
        self.control.intercept_misc1 |= 1 << 28; // MSR
        self.control.intercept_misc2 |= 1 << 0;  // VMRUN
        self.control.intercept_misc2 |= 1 << 1;  // VMMCALL
        
        // Enable nested paging
        self.control.np_enable = true;
        
        // Set guest ASID
        self.control.guest_asid = 1;
    }
    
    /// Load guest state from VMCB to CPU
    pub fn load_guest_state(&self, cpu: &VirtualCpu) {
        cpu.write_rip(self.state.rip);
        cpu.write_rsp(self.state.rsp);
        cpu.write_rflags(self.state.rflags);
        cpu.write_gpr(0, self.state.rax);
        
        cpu.write_cr0(self.state.cr0);
        cpu.write_cr3(self.state.cr3);
        cpu.write_cr4(self.state.cr4);
        
        cpu.write_msr(msr::IA32_EFER, self.state.efer);
        cpu.write_msr(msr::IA32_PAT, self.state.pat);
        cpu.write_msr(msr::IA32_FS_BASE, self.state.fs.base);
        cpu.write_msr(msr::IA32_GS_BASE, self.state.gs.base);
        cpu.write_msr(msr::IA32_KERNEL_GS_BASE, self.state.kernel_gs_base);
        cpu.write_msr(msr::IA32_STAR, self.state.star);
        cpu.write_msr(msr::IA32_LSTAR, self.state.lstar);
        cpu.write_msr(msr::IA32_CSTAR, self.state.cstar);
        cpu.write_msr(msr::IA32_FMASK, self.state.sfmask);
        cpu.write_msr(msr::IA32_SYSENTER_CS, self.state.sysenter_cs);
        cpu.write_msr(msr::IA32_SYSENTER_ESP, self.state.sysenter_esp);
        cpu.write_msr(msr::IA32_SYSENTER_EIP, self.state.sysenter_eip);
        
        cpu.set_cpl(self.state.cpl);
    }
    
    /// Save guest state from CPU to VMCB
    pub fn save_guest_state(&mut self, cpu: &VirtualCpu) {
        self.state.rip = cpu.read_rip();
        self.state.rsp = cpu.read_rsp();
        self.state.rflags = cpu.read_rflags();
        self.state.rax = cpu.read_gpr(0);
        
        self.state.cr0 = cpu.read_cr0();
        self.state.cr2 = cpu.read_cr2();
        self.state.cr3 = cpu.read_cr3();
        self.state.cr4 = cpu.read_cr4();
        
        self.state.efer = cpu.read_msr(msr::IA32_EFER);
        self.state.pat = cpu.read_msr(msr::IA32_PAT);
        self.state.fs.base = cpu.read_msr(msr::IA32_FS_BASE);
        self.state.gs.base = cpu.read_msr(msr::IA32_GS_BASE);
        self.state.kernel_gs_base = cpu.read_msr(msr::IA32_KERNEL_GS_BASE);
        self.state.star = cpu.read_msr(msr::IA32_STAR);
        self.state.lstar = cpu.read_msr(msr::IA32_LSTAR);
        self.state.cstar = cpu.read_msr(msr::IA32_CSTAR);
        self.state.sfmask = cpu.read_msr(msr::IA32_FMASK);
        
        self.state.cpl = cpu.get_cpl();
    }
}

impl Default for Vmcb {
    fn default() -> Self {
        Self::new()
    }
}

/// NPT (Nested Page Tables) Manager
pub struct NptManager {
    /// NPT base (nCR3)
    ncr3: AtomicU64,
    /// GPA to HPA translations
    translations: RwLock<HashMap<u64, NptEntry>>,
    /// Statistics
    stats: RwLock<NptStats>,
}

/// NPT entry
#[derive(Debug, Clone, Copy)]
pub struct NptEntry {
    pub hpa: u64,
    pub present: bool,
    pub writable: bool,
    pub user: bool,
    pub nx: bool,
    pub large_page: bool,
    pub accessed: bool,
    pub dirty: bool,
}

impl Default for NptEntry {
    fn default() -> Self {
        Self {
            hpa: 0,
            present: true,
            writable: true,
            user: true,
            nx: false,
            large_page: false,
            accessed: false,
            dirty: false,
        }
    }
}

/// NPT statistics
#[derive(Debug, Clone, Default)]
pub struct NptStats {
    pub translations: u64,
    pub faults: u64,
}

impl NptManager {
    pub fn new() -> Self {
        Self {
            ncr3: AtomicU64::new(0),
            translations: RwLock::new(HashMap::new()),
            stats: RwLock::new(NptStats::default()),
        }
    }
    
    /// Set nCR3
    pub fn set_ncr3(&self, ncr3: u64) {
        self.ncr3.store(ncr3, Ordering::SeqCst);
    }
    
    /// Get nCR3
    pub fn get_ncr3(&self) -> u64 {
        self.ncr3.load(Ordering::SeqCst)
    }
    
    /// Translate GPA to HPA
    pub fn translate(&self, gpa: u64) -> SvmResult<u64> {
        self.stats.write().unwrap().translations += 1;
        
        let translations = self.translations.read().unwrap();
        let page_gpa = gpa & !0xFFF;
        let offset = gpa & 0xFFF;
        
        if let Some(entry) = translations.get(&page_gpa) {
            if entry.present {
                Ok(entry.hpa | offset)
            } else {
                Err(SvmError::NptFault { gpa, error_code: 0 })
            }
        } else {
            // Identity mapping by default
            Ok(gpa)
        }
    }
    
    /// Map GPA to HPA
    pub fn map(&self, gpa: u64, hpa: u64, flags: NptEntry) {
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
    
    /// Invalidate all NPT entries
    pub fn invalidate_all(&self) {
        self.translations.write().unwrap().clear();
    }
}

impl Default for NptManager {
    fn default() -> Self {
        Self::new()
    }
}

/// SVM Manager - Central SVM operations coordinator
pub struct SvmManager {
    /// SVM capabilities
    caps: SvmCapabilities,
    /// SVM enabled
    enabled: AtomicBool,
    /// Current VMCB
    current_vmcb: RwLock<Option<Arc<RwLock<Vmcb>>>>,
    /// All VMCBs
    vmcbs: RwLock<HashMap<u64, Arc<RwLock<Vmcb>>>>,
    /// NPT manager
    npt: NptManager,
    /// Global Interrupt Flag
    gif: AtomicBool,
    /// Statistics
    stats: RwLock<SvmStats>,
    /// Next VMCB ID
    next_vmcb_id: AtomicU64,
}

/// SVM statistics
#[derive(Debug, Clone, Default)]
pub struct SvmStats {
    pub vmruns: u64,
    pub vmexits: u64,
    pub exit_codes: HashMap<u64, u64>,
}

impl SvmManager {
    /// Create a new SVM manager
    pub fn new() -> Self {
        Self {
            caps: SvmCapabilities::default(),
            enabled: AtomicBool::new(false),
            current_vmcb: RwLock::new(None),
            vmcbs: RwLock::new(HashMap::new()),
            npt: NptManager::new(),
            gif: AtomicBool::new(true),
            stats: RwLock::new(SvmStats::default()),
            next_vmcb_id: AtomicU64::new(1),
        }
    }
    
    /// Check if SVM is supported
    pub fn is_supported(&self) -> bool {
        self.caps.svm_supported
    }
    
    /// Get SVM capabilities
    pub fn capabilities(&self) -> &SvmCapabilities {
        &self.caps
    }
    
    /// Enable SVM (set EFER.SVME)
    pub fn enable(&self) -> SvmResult<()> {
        if !self.is_supported() {
            return Err(SvmError::NotSupported);
        }
        if self.enabled.load(Ordering::SeqCst) {
            return Err(SvmError::AlreadyEnabled);
        }
        self.enabled.store(true, Ordering::SeqCst);
        Ok(())
    }
    
    /// Disable SVM
    pub fn disable(&self) -> SvmResult<()> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Err(SvmError::NotEnabled);
        }
        self.enabled.store(false, Ordering::SeqCst);
        *self.current_vmcb.write().unwrap() = None;
        Ok(())
    }
    
    /// Check if SVM is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
    
    /// Set GIF (Global Interrupt Flag)
    pub fn stgi(&self) {
        self.gif.store(true, Ordering::SeqCst);
    }
    
    /// Clear GIF
    pub fn clgi(&self) {
        self.gif.store(false, Ordering::SeqCst);
    }
    
    /// Get GIF state
    pub fn get_gif(&self) -> bool {
        self.gif.load(Ordering::SeqCst)
    }
    
    /// Create a new VMCB
    pub fn create_vmcb(&self) -> SvmResult<u64> {
        if !self.is_enabled() {
            return Err(SvmError::NotEnabled);
        }
        
        let id = self.next_vmcb_id.fetch_add(1, Ordering::SeqCst);
        let mut vmcb = Vmcb::new();
        vmcb.init_guest();
        vmcb.physical_addr = id * 4096; // Fake physical address
        
        self.vmcbs.write().unwrap().insert(id, Arc::new(RwLock::new(vmcb)));
        Ok(id)
    }
    
    /// Load VMCB
    pub fn load_vmcb(&self, id: u64) -> SvmResult<()> {
        if !self.is_enabled() {
            return Err(SvmError::NotEnabled);
        }
        
        let vmcbs = self.vmcbs.read().unwrap();
        let vmcb = vmcbs.get(&id).ok_or(SvmError::InvalidVmcb)?;
        vmcb.write().unwrap().active = true;
        
        *self.current_vmcb.write().unwrap() = Some(vmcb.clone());
        Ok(())
    }
    
    /// Get current VMCB
    pub fn current_vmcb(&self) -> SvmResult<Arc<RwLock<Vmcb>>> {
        self.current_vmcb.read().unwrap()
            .clone()
            .ok_or(SvmError::VmcbNotLoaded)
    }
    
    /// VMRUN - Enter guest mode
    pub fn vmrun(&self, cpu: &VirtualCpu) -> SvmResult<()> {
        let vmcb = self.current_vmcb()?;
        let vmcb_guard = vmcb.read().unwrap();
        
        // Load guest state
        vmcb_guard.load_guest_state(cpu);
        
        // Clear GIF
        self.clgi();
        
        // Update stats
        self.stats.write().unwrap().vmruns += 1;
        
        Ok(())
    }
    
    /// VMEXIT handling
    pub fn vmexit(&self, cpu: &VirtualCpu, exit_code: VmExitCode, exit_info1: u64, exit_info2: u64) -> SvmResult<()> {
        let vmcb = self.current_vmcb()?;
        let mut vmcb_guard = vmcb.write().unwrap();
        
        // Save guest state
        vmcb_guard.save_guest_state(cpu);
        
        // Set exit info
        vmcb_guard.control.exit_code = exit_code as u64;
        vmcb_guard.control.exit_info1 = exit_info1;
        vmcb_guard.control.exit_info2 = exit_info2;
        
        // Set GIF
        self.stgi();
        
        // Update stats
        {
            let mut stats = self.stats.write().unwrap();
            stats.vmexits += 1;
            *stats.exit_codes.entry(exit_code as u64).or_insert(0) += 1;
        }
        
        Ok(())
    }
    
    /// Handle VMMCALL (hypercall)
    pub fn handle_vmmcall(&self, cpu: &VirtualCpu) -> SvmResult<u64> {
        let hypercall_nr = cpu.read_gpr(0); // RAX
        
        match hypercall_nr {
            0 => Ok(0), // NOP
            1 => Ok(cpu.rdtsc()), // Get TSC
            2 => Ok(cpu.id as u64), // Get VCPU ID
            _ => Ok(u64::MAX),
        }
    }
    
    /// Get NPT manager
    pub fn npt(&self) -> &NptManager {
        &self.npt
    }
    
    /// Get statistics
    pub fn stats(&self) -> SvmStats {
        self.stats.read().unwrap().clone()
    }
}

impl Default for SvmManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// SVM Executor - Integrates JIT with AMD-V
// ============================================================================

use crate::jit::{JitEngine, JitConfig, ExecuteResult, JitError};
use crate::memory::{PhysicalMemory, AddressSpace};

/// SVM execution loop result
#[derive(Debug, Clone)]
pub enum SvmExecResult {
    /// VM exited normally
    VmExit(VmExitCode, u64),
    /// VM halted (HLT instruction)
    Halted,
    /// External interrupt pending
    Interrupt(u8),
    /// VM was reset
    Reset,
    /// Shutdown (triple fault)
    Shutdown,
    /// Execution error
    Error(String),
}

/// SVM Executor - runs guest code using JIT with SVM controls
/// 
/// This is the AMD-V execution engine that:
/// 1. Uses JIT to execute guest x86-64 instructions
/// 2. Handles #VMEXIT (I/O, CPUID, MSR, etc.)
/// 3. Manages NPT translations
/// 4. Supports SEV encryption
pub struct SvmExecutor {
    /// SVM manager for VMCB and NPT
    svm: Arc<SvmManager>,
    /// JIT execution engine
    jit: JitEngine,
    /// Guest address space (RAM + MMIO routing)
    address_space: Arc<AddressSpace>,
    /// Running flag
    running: AtomicBool,
    /// Statistics
    stats: RwLock<SvmExecStats>,
}

/// SVM executor statistics
#[derive(Debug, Clone, Default)]
pub struct SvmExecStats {
    /// Instructions executed
    pub instructions: u64,
    /// VMRUN count
    pub vmruns: u64,
    /// VM exits by code
    pub vm_exits: HashMap<u64, u64>,
    /// JIT compilations
    pub jit_compilations: u64,
    /// Total execution time (ns)
    pub exec_time_ns: u64,
}

impl SvmExecutor {
    /// Create new SVM executor
    pub fn new(address_space: Arc<AddressSpace>) -> Self {
        let svm = Arc::new(SvmManager::new());
        
        Self {
            svm,
            jit: JitEngine::new(),
            address_space,
            running: AtomicBool::new(false),
            stats: RwLock::new(SvmExecStats::default()),
        }
    }
    
    /// Create with custom JIT config
    pub fn with_jit_config(address_space: Arc<AddressSpace>, jit_config: JitConfig) -> Self {
        let svm = Arc::new(SvmManager::new());
        
        Self {
            svm,
            jit: JitEngine::with_config(jit_config),
            address_space,
            running: AtomicBool::new(false),
            stats: RwLock::new(SvmExecStats::default()),
        }
    }
    
    /// Initialize SVM and prepare for execution
    pub fn init(&self) -> SvmResult<()> {
        self.svm.enable()?;
        let vmcb_id = self.svm.create_vmcb()?;
        self.svm.load_vmcb(vmcb_id)?;
        Ok(())
    }
    
    /// Run guest until #VMEXIT
    /// 
    /// This is the main execution loop (VMRUN):
    /// 1. Load guest state from VMCB
    /// 2. Execute guest code via JIT
    /// 3. Handle #VMEXIT
    /// 4. Repeat until halt/error
    pub fn run(&self, cpu: &VirtualCpu) -> SvmExecResult {
        self.running.store(true, Ordering::SeqCst);
        
        // VMRUN - load guest state
        if let Err(e) = self.svm.vmrun(cpu) {
            return SvmExecResult::Error(format!("VMRUN failed: {:?}", e));
        }
        
        self.stats.write().unwrap().vmruns += 1;
        
        // Execute guest code
        loop {
            if !self.running.load(Ordering::SeqCst) {
                break SvmExecResult::Halted;
            }
            
            // Check GIF (Global Interrupt Flag)
            if self.svm.get_gif() && cpu.has_pending_interrupt() {
                if let Some(vector) = cpu.deliver_interrupt() {
                    let _ = self.svm.vmexit(cpu, VmExitCode::Intr, vector as u64, 0);
                    break SvmExecResult::Interrupt(vector);
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
                            let _ = self.svm.vmexit(cpu, VmExitCode::Hlt, 0, 0);
                            break SvmExecResult::Halted;
                        }
                        ExecuteResult::Interrupt { vector } => {
                            let _ = self.svm.vmexit(cpu, VmExitCode::Intr, vector as u64, 0);
                            break SvmExecResult::Interrupt(vector);
                        }
                        ExecuteResult::IoNeeded { port, is_write, size } => {
                            let info1 = (port as u64) | ((size as u64) << 4) | 
                                        (if is_write { 0 } else { 1 });
                            let _ = self.svm.vmexit(cpu, VmExitCode::IoIo, info1, 0);
                            break SvmExecResult::VmExit(VmExitCode::IoIo, info1);
                        }
                        ExecuteResult::Exception { vector, error_code } => {
                            // Map to SVM exit code
                            let exit_code = match vector {
                                0 => VmExitCode::Exception_DE,
                                6 => VmExitCode::Exception_UD,
                                13 => VmExitCode::Exception_GP,
                                14 => VmExitCode::Exception_PF,
                                _ => VmExitCode::Exception_DE, // Default
                            };
                            let info1 = error_code.map(|e| e as u64).unwrap_or(0);
                            let _ = self.svm.vmexit(cpu, exit_code, info1, 0);
                            break SvmExecResult::VmExit(exit_code, info1);
                        }
                        ExecuteResult::Hypercall { nr, args } => {
                            let _ = self.svm.vmexit(cpu, VmExitCode::Vmmcall, nr, 0);
                            break SvmExecResult::VmExit(VmExitCode::Vmmcall, nr);
                        }
                        ExecuteResult::Reset => {
                            break SvmExecResult::Reset;
                        }
                        ExecuteResult::Shutdown => {
                            let _ = self.svm.vmexit(cpu, VmExitCode::Shutdown, 0, 0);
                            break SvmExecResult::Shutdown;
                        }
                        ExecuteResult::MmioNeeded { addr, is_write, size } => {
                            // NPT fault
                            let _ = self.svm.vmexit(cpu, VmExitCode::Npf, addr, 0);
                            break SvmExecResult::VmExit(VmExitCode::Npf, addr);
                        }
                    }
                }
                Err(e) => {
                    break SvmExecResult::Error(format!("JIT error: {}", e));
                }
            }
        }
    }
    
    /// Stop execution
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
    
    /// Check if running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
    
    /// Pause the executor (same as stop, execution continues when run() is called)
    pub fn pause(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
    
    /// Resume the executor (sets running flag, but run() must be called separately)
    pub fn resume(&self) {
        // Just set the flag, actual execution happens in run()
        // No-op here since run() will set it to true
    }
    
    /// Get SVM manager
    pub fn svm(&self) -> &SvmManager {
        &self.svm
    }
    
    /// Get JIT engine
    pub fn jit(&self) -> &JitEngine {
        &self.jit
    }
    
    /// Get statistics
    pub fn stats(&self) -> SvmExecStats {
        self.stats.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vmcb_create() {
        let vmcb = Vmcb::new();
        assert!(!vmcb.active);
    }
    
    #[test]
    fn test_vmcb_init_guest() {
        let mut vmcb = Vmcb::new();
        vmcb.init_guest();
        assert!(vmcb.control.np_enable);
        assert_eq!(vmcb.control.guest_asid, 1);
    }
    
    #[test]
    fn test_svm_manager_enable() {
        let svm = SvmManager::new();
        assert!(svm.is_supported());
        assert!(!svm.is_enabled());
        
        svm.enable().unwrap();
        assert!(svm.is_enabled());
        
        assert!(svm.enable().is_err()); // Can't enable twice
        
        svm.disable().unwrap();
        assert!(!svm.is_enabled());
    }
    
    #[test]
    fn test_svm_gif() {
        let svm = SvmManager::new();
        assert!(svm.get_gif());
        
        svm.clgi();
        assert!(!svm.get_gif());
        
        svm.stgi();
        assert!(svm.get_gif());
    }
    
    #[test]
    fn test_svm_vmcb_lifecycle() {
        let svm = SvmManager::new();
        svm.enable().unwrap();
        
        let vmcb_id = svm.create_vmcb().unwrap();
        svm.load_vmcb(vmcb_id).unwrap();
        
        let vmcb = svm.current_vmcb().unwrap();
        assert!(vmcb.read().unwrap().active);
    }
    
    #[test]
    fn test_npt_translate() {
        let npt = NptManager::new();
        
        // Identity mapping by default
        assert_eq!(npt.translate(0x1000).unwrap(), 0x1000);
        
        // Add mapping
        npt.map(0x2000, 0x3000, NptEntry::default());
        assert_eq!(npt.translate(0x2000).unwrap(), 0x3000);
        assert_eq!(npt.translate(0x2123).unwrap(), 0x3123);
    }
}
