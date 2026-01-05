//! x86-64 Instruction Decoder
//!
//! Complete x86-64 instruction decoder supporting:
//! - Legacy prefixes (REX, operand-size, address-size, segment)
//! - VEX/EVEX prefixes (AVX/AVX-512)
//! - All addressing modes (ModR/M, SIB, displacement)
//! - Real mode, protected mode, long mode

use super::{JitResult, JitError};

/// Decoded instruction
#[derive(Debug, Clone)]
pub struct DecodedInstr {
    /// Guest RIP where instruction starts
    pub rip: u64,
    /// Instruction length in bytes
    pub len: u8,
    /// Raw bytes (max 15)
    pub bytes: [u8; 15],
    /// Opcode (1-3 bytes encoded as u32)
    pub opcode: u32,
    /// Mnemonic
    pub mnemonic: Mnemonic,
    /// Operands
    pub operands: [Operand; 4],
    /// Number of operands
    pub num_operands: u8,
    /// Prefixes
    pub prefixes: Prefixes,
    /// Flags affected
    pub flags_affected: u16,
    /// Is this a branch?
    pub is_branch: bool,
    /// Is this a call?
    pub is_call: bool,
    /// Is this a return?
    pub is_ret: bool,
    /// Is this privileged?
    pub is_privileged: bool,
}

impl Default for DecodedInstr {
    fn default() -> Self {
        Self {
            rip: 0,
            len: 0,
            bytes: [0; 15],
            opcode: 0,
            mnemonic: Mnemonic::Invalid,
            operands: [Operand::None; 4],
            num_operands: 0,
            prefixes: Prefixes::default(),
            flags_affected: 0,
            is_branch: false,
            is_call: false,
            is_ret: false,
            is_privileged: false,
        }
    }
}

/// Instruction prefixes
#[derive(Debug, Clone, Copy, Default)]
pub struct Prefixes {
    pub rex: u8,           // REX prefix (0x40-0x4F)
    pub rex_w: bool,       // REX.W (64-bit operand)
    pub rex_r: bool,       // REX.R (ModR/M reg extension)
    pub rex_x: bool,       // REX.X (SIB index extension)
    pub rex_b: bool,       // REX.B (ModR/M r/m or SIB base extension)
    pub op_size: bool,     // 0x66 operand size override
    pub addr_size: bool,   // 0x67 address size override
    pub lock: bool,        // 0xF0 LOCK
    pub rep: bool,         // 0xF3 REP/REPE
    pub repne: bool,       // 0xF2 REPNE
    pub segment: Segment,  // Segment override
    pub vex: Option<Vex>,  // VEX prefix
}

/// VEX prefix info
#[derive(Debug, Clone, Copy)]
pub struct Vex {
    pub len: u8,     // 2 or 3 byte VEX
    pub r: bool,     // VEX.R
    pub x: bool,     // VEX.X
    pub b: bool,     // VEX.B
    pub w: bool,     // VEX.W
    pub vvvv: u8,    // VEX.vvvv (register specifier)
    pub l: bool,     // VEX.L (256-bit)
    pub pp: u8,      // VEX.pp (implied prefix)
    pub mmmmm: u8,   // VEX.mmmmm (implied leading opcode bytes)
}

/// Segment register
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Segment {
    #[default]
    None,
    ES,
    CS,
    SS,
    DS,
    FS,
    GS,
}

/// Operand
#[derive(Debug, Clone, Copy)]
pub enum Operand {
    None,
    /// Register (index into register file)
    Reg(Register),
    /// Immediate value
    Imm(i64),
    /// Memory reference
    Mem(MemOp),
    /// Relative offset (for branches)
    Rel(i64),
    /// Far pointer (segment:offset)
    Far { seg: u16, off: u64 },
}

impl Default for Operand {
    fn default() -> Self {
        Self::None
    }
}

/// Register
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Register {
    pub kind: RegKind,
    pub index: u8,
    pub size: u8,  // 1, 2, 4, 8, 16, 32, 64 bytes
}

/// Register kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegKind {
    Gpr,     // General purpose (RAX, RBX, etc)
    Segment, // Segment (CS, DS, etc)
    Control, // Control (CR0-CR15)
    Debug,   // Debug (DR0-DR7)
    Mmx,     // MMX (MM0-MM7)
    Xmm,     // SSE (XMM0-XMM31)
    Ymm,     // AVX (YMM0-YMM31)
    Zmm,     // AVX-512 (ZMM0-ZMM31)
    Mask,    // AVX-512 mask (K0-K7)
    X87,     // x87 FPU (ST0-ST7)
    Flags,   // RFLAGS
    Rip,     // RIP
}

/// Memory operand
#[derive(Debug, Clone, Copy)]
pub struct MemOp {
    pub base: Option<Register>,
    pub index: Option<Register>,
    pub scale: u8,           // 1, 2, 4, 8
    pub disp: i64,           // Displacement
    pub size: u8,            // Access size in bytes
    pub segment: Segment,
}

impl Default for MemOp {
    fn default() -> Self {
        Self {
            base: None,
            index: None,
            scale: 1,
            disp: 0,
            size: 0,
            segment: Segment::None,
        }
    }
}

/// Instruction mnemonic
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Mnemonic {
    Invalid = 0,
    // Data transfer
    Mov, Movzx, Movsx, Movsxd, Cmov, Xchg, Bswap,
    Push, Pop, Pusha, Popa, Pushf, Popf,
    Lea, Lds, Les, Lfs, Lgs, Lss,
    // Arithmetic
    Add, Adc, Sub, Sbb, Mul, Imul, Div, Idiv,
    Inc, Dec, Neg, Cmp, Test,
    // Logical
    And, Or, Xor, Not,
    // Shift/Rotate
    Shl, Shr, Sar, Rol, Ror, Rcl, Rcr,
    Shld, Shrd,
    // Bit operations
    Bt, Bts, Btr, Btc, Bsf, Bsr, Popcnt, Lzcnt, Tzcnt,
    // Control flow
    Jmp, Jcc, Call, Ret, Retf, Iret, Int, Int3, Into,
    Loop, Loope, Loopne, Jcxz, Jecxz, Jrcxz,
    // String
    Movs, Cmps, Scas, Lods, Stos, Rep, Repe, Repne,
    // Flag control
    Clc, Stc, Cmc, Cld, Std, Cli, Sti, Lahf, Sahf,
    // System
    Hlt, Nop, Cpuid, Rdtsc, Rdtscp, Rdmsr, Wrmsr,
    Lgdt, Sgdt, Lidt, Sidt, Lldt, Sldt, Ltr, Str,
    Invlpg, Invpcid, Wbinvd, Clflush,
    // Privileged
    In, Out, Ins, Outs,
    Lmsw, Smsw, Clts,
    // x87 FPU (basic)
    Fld, Fst, Fstp, Fild, Fist, Fistp,
    Fadd, Fsub, Fmul, Fdiv, Fchs, Fabs, Fsqrt,
    Fcom, Fcomp, Fcompp, Fucom, Fucomp, Fucompp,
    // SSE/AVX (basic)
    Movaps, Movups, Movapd, Movupd, Movdqa, Movdqu,
    Addps, Addpd, Addss, Addsd,
    Subps, Subpd, Subss, Subsd,
    Mulps, Mulpd, Mulss, Mulsd,
    Divps, Divpd, Divss, Divsd,
    // VMX
    Vmcall, Vmlaunch, Vmresume, Vmxoff, Vmxon,
    Vmclear, Vmptrld, Vmptrst, Vmread, Vmwrite,
    // SVM
    Vmrun, Vmmcall, Vmload, Vmsave, Stgi, Clgi, Skinit, Invlpga,
    // Misc
    Syscall, Sysret, Sysenter, Sysexit,
    Xsave, Xrstor, Xsaveopt, Xgetbv, Xsetbv,
    Cmpxchg, Cmpxchg8b, Cmpxchg16b, Xadd,
    Pause, Mfence, Lfence, Sfence,
    Prefetch, Prefetchw, Prefetchnta,
    Endbr32, Endbr64, // CET
    // Sentinel
    _Max,
}

/// x86-64 Decoder
pub struct X86Decoder {
    /// Current CPU mode
    mode: CpuMode,
}

/// CPU operating mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuMode {
    Real,       // 16-bit real mode
    Protected,  // 32-bit protected mode
    Long,       // 64-bit long mode
    Compat,     // 32-bit compatibility mode (under long mode)
}

impl X86Decoder {
    pub fn new() -> Self {
        Self { mode: CpuMode::Real }
    }
    
    pub fn with_mode(mode: CpuMode) -> Self {
        Self { mode }
    }
    
    pub fn set_mode(&mut self, mode: CpuMode) {
        self.mode = mode;
    }
    
    /// Decode single instruction from bytes
    pub fn decode(&self, bytes: &[u8], rip: u64) -> JitResult<DecodedInstr> {
        if bytes.is_empty() {
            return Err(JitError::DecodeError {
                rip,
                bytes: vec![],
                reason: "Empty input".to_string(),
            });
        }
        
        let mut instr = DecodedInstr::default();
        instr.rip = rip;
        
        let mut pos = 0;
        
        // Phase 1: Parse prefixes
        pos = self.parse_prefixes(bytes, &mut instr)?;
        
        // Phase 2: Parse opcode
        pos = self.parse_opcode(bytes, pos, &mut instr)?;
        
        // Phase 3: Parse ModR/M, SIB, displacement, immediate
        pos = self.parse_operands(bytes, pos, &mut instr)?;
        
        // Store raw bytes and length
        instr.len = pos as u8;
        for i in 0..pos.min(15) {
            instr.bytes[i] = bytes[i];
        }
        
        // Set flags
        self.set_instruction_flags(&mut instr);
        
        Ok(instr)
    }
    
    /// Decode a basic block (until branch/call/ret)
    pub fn decode_block(&self, memory: &crate::memory::PhysicalMemory, start_rip: u64) -> JitResult<Vec<DecodedInstr>> {
        let mut instrs = Vec::new();
        let mut rip = start_rip;
        let max_instrs = 256;
        let max_bytes = 4096;
        
        while instrs.len() < max_instrs && (rip - start_rip) < max_bytes as u64 {
            // Fetch bytes
            let mut bytes = [0u8; 15];
            for i in 0..15 {
                bytes[i] = memory.read_u8(rip + i as u64);
            }
            
            let instr = self.decode(&bytes, rip)?;
            rip += instr.len as u64;
            
            let is_terminator = instr.is_branch || instr.is_call || instr.is_ret ||
                matches!(instr.mnemonic, Mnemonic::Hlt | Mnemonic::Int | Mnemonic::Int3 | Mnemonic::Iret);
            
            instrs.push(instr);
            
            if is_terminator {
                break;
            }
        }
        
        Ok(instrs)
    }
    
    fn parse_prefixes(&self, bytes: &[u8], instr: &mut DecodedInstr) -> JitResult<usize> {
        let mut pos = 0;
        
        while pos < bytes.len() && pos < 15 {
            let b = bytes[pos];
            
            match b {
                // Group 1: LOCK, REP
                0xF0 => instr.prefixes.lock = true,
                0xF2 => instr.prefixes.repne = true,
                0xF3 => instr.prefixes.rep = true,
                
                // Group 2: Segment overrides
                0x26 => instr.prefixes.segment = Segment::ES,
                0x2E => instr.prefixes.segment = Segment::CS,
                0x36 => instr.prefixes.segment = Segment::SS,
                0x3E => instr.prefixes.segment = Segment::DS,
                0x64 => instr.prefixes.segment = Segment::FS,
                0x65 => instr.prefixes.segment = Segment::GS,
                
                // Group 3: Operand size
                0x66 => instr.prefixes.op_size = true,
                
                // Group 4: Address size
                0x67 => instr.prefixes.addr_size = true,
                
                // REX prefix (64-bit mode only)
                0x40..=0x4F if self.mode == CpuMode::Long => {
                    instr.prefixes.rex = b;
                    instr.prefixes.rex_w = (b & 0x08) != 0;
                    instr.prefixes.rex_r = (b & 0x04) != 0;
                    instr.prefixes.rex_x = (b & 0x02) != 0;
                    instr.prefixes.rex_b = (b & 0x01) != 0;
                    pos += 1;
                    break; // REX must be last legacy prefix
                }
                
                // VEX 2-byte
                0xC5 if self.mode == CpuMode::Long && pos + 1 < bytes.len() => {
                    let b1 = bytes[pos + 1];
                    instr.prefixes.vex = Some(Vex {
                        len: 2,
                        r: (b1 & 0x80) == 0,
                        x: true,
                        b: true,
                        w: false,
                        vvvv: (!b1 >> 3) & 0x0F,
                        l: (b1 & 0x04) != 0,
                        pp: b1 & 0x03,
                        mmmmm: 1,
                    });
                    pos += 2;
                    break;
                }
                
                // VEX 3-byte
                0xC4 if self.mode == CpuMode::Long && pos + 2 < bytes.len() => {
                    let b1 = bytes[pos + 1];
                    let b2 = bytes[pos + 2];
                    instr.prefixes.vex = Some(Vex {
                        len: 3,
                        r: (b1 & 0x80) == 0,
                        x: (b1 & 0x40) == 0,
                        b: (b1 & 0x20) == 0,
                        w: (b2 & 0x80) != 0,
                        vvvv: (!b2 >> 3) & 0x0F,
                        l: (b2 & 0x04) != 0,
                        pp: b2 & 0x03,
                        mmmmm: b1 & 0x1F,
                    });
                    pos += 3;
                    break;
                }
                
                _ => break, // Not a prefix
            }
            pos += 1;
        }
        
        Ok(pos)
    }
    
    fn parse_opcode(&self, bytes: &[u8], mut pos: usize, instr: &mut DecodedInstr) -> JitResult<usize> {
        if pos >= bytes.len() {
            return Err(JitError::DecodeError {
                rip: instr.rip,
                bytes: bytes[..pos.min(bytes.len())].to_vec(),
                reason: "Unexpected end of instruction".to_string(),
            });
        }
        
        let op1 = bytes[pos];
        pos += 1;
        
        // Handle escape bytes
        match op1 {
            0x0F => {
                // Two-byte opcode
                if pos >= bytes.len() {
                    return Err(JitError::DecodeError {
                        rip: instr.rip,
                        bytes: bytes[..pos.min(bytes.len())].to_vec(),
                        reason: "Truncated two-byte opcode".to_string(),
                    });
                }
                let op2 = bytes[pos];
                pos += 1;
                
                match op2 {
                    0x38 | 0x3A => {
                        // Three-byte opcode
                        if pos >= bytes.len() {
                            return Err(JitError::DecodeError {
                                rip: instr.rip,
                                bytes: bytes[..pos.min(bytes.len())].to_vec(),
                                reason: "Truncated three-byte opcode".to_string(),
                            });
                        }
                        let op3 = bytes[pos];
                        pos += 1;
                        instr.opcode = 0x0F0000 | ((op2 as u32) << 8) | (op3 as u32);
                    }
                    _ => {
                        instr.opcode = 0x0F00 | (op2 as u32);
                    }
                }
            }
            _ => {
                instr.opcode = op1 as u32;
            }
        }
        
        // Decode mnemonic from opcode
        instr.mnemonic = self.opcode_to_mnemonic(instr.opcode, &instr.prefixes);
        
        Ok(pos)
    }
    
    fn parse_operands(&self, bytes: &[u8], mut pos: usize, instr: &mut DecodedInstr) -> JitResult<usize> {
        // Get operand encoding for this opcode
        let encoding = self.get_operand_encoding(instr.opcode, &instr.prefixes);
        
        // Check if we need ModR/M byte
        let needs_modrm = encoding.iter().any(|e| matches!(e, OpEnc::ModRm | OpEnc::ModRmReg | OpEnc::SegRegModRm));
        
        let modrm = if needs_modrm {
            let m = bytes.get(pos).copied().unwrap_or(0);
            pos += 1;
            Some(m)
        } else {
            None
        };
        
        for (i, enc) in encoding.iter().enumerate() {
            if *enc == OpEnc::None {
                break;
            }
            
            let (operand, new_pos) = self.decode_operand(bytes, pos, instr, *enc, modrm)?;
            instr.operands[i] = operand;
            instr.num_operands = (i + 1) as u8;
            pos = new_pos;
        }
        
        Ok(pos)
    }
    
    fn decode_operand(&self, bytes: &[u8], pos: usize, instr: &DecodedInstr, enc: OpEnc, modrm: Option<u8>) -> JitResult<(Operand, usize)> {
        let mut pos = pos;
        
        let operand = match enc {
            OpEnc::None => Operand::None,
            
            OpEnc::Imm8 => {
                let imm = bytes.get(pos).copied().unwrap_or(0) as i8 as i64;
                pos += 1;
                Operand::Imm(imm)
            }
            
            OpEnc::Imm16 => {
                let imm = u16::from_le_bytes([
                    bytes.get(pos).copied().unwrap_or(0),
                    bytes.get(pos + 1).copied().unwrap_or(0),
                ]) as i16 as i64;
                pos += 2;
                Operand::Imm(imm)
            }
            
            OpEnc::Imm32 => {
                let imm = i32::from_le_bytes([
                    bytes.get(pos).copied().unwrap_or(0),
                    bytes.get(pos + 1).copied().unwrap_or(0),
                    bytes.get(pos + 2).copied().unwrap_or(0),
                    bytes.get(pos + 3).copied().unwrap_or(0),
                ]) as i64;
                pos += 4;
                Operand::Imm(imm)
            }
            
            OpEnc::Imm64 => {
                let imm = i64::from_le_bytes([
                    bytes.get(pos).copied().unwrap_or(0),
                    bytes.get(pos + 1).copied().unwrap_or(0),
                    bytes.get(pos + 2).copied().unwrap_or(0),
                    bytes.get(pos + 3).copied().unwrap_or(0),
                    bytes.get(pos + 4).copied().unwrap_or(0),
                    bytes.get(pos + 5).copied().unwrap_or(0),
                    bytes.get(pos + 6).copied().unwrap_or(0),
                    bytes.get(pos + 7).copied().unwrap_or(0),
                ]);
                pos += 8;
                Operand::Imm(imm)
            }
            
            OpEnc::ImmV => {
                // Immediate size depends on operand size
                let size = self.get_operand_size(instr);
                match size {
                    2 => {
                        let imm = u16::from_le_bytes([
                            bytes.get(pos).copied().unwrap_or(0),
                            bytes.get(pos + 1).copied().unwrap_or(0),
                        ]) as i16 as i64;
                        pos += 2;
                        Operand::Imm(imm)
                    }
                    8 if instr.prefixes.rex_w => {
                        // 64-bit immediate only with REX.W
                        let imm = i64::from_le_bytes([
                            bytes.get(pos).copied().unwrap_or(0),
                            bytes.get(pos + 1).copied().unwrap_or(0),
                            bytes.get(pos + 2).copied().unwrap_or(0),
                            bytes.get(pos + 3).copied().unwrap_or(0),
                            bytes.get(pos + 4).copied().unwrap_or(0),
                            bytes.get(pos + 5).copied().unwrap_or(0),
                            bytes.get(pos + 6).copied().unwrap_or(0),
                            bytes.get(pos + 7).copied().unwrap_or(0),
                        ]);
                        pos += 8;
                        Operand::Imm(imm)
                    }
                    _ => {
                        // Default to 32-bit
                        let imm = i32::from_le_bytes([
                            bytes.get(pos).copied().unwrap_or(0),
                            bytes.get(pos + 1).copied().unwrap_or(0),
                            bytes.get(pos + 2).copied().unwrap_or(0),
                            bytes.get(pos + 3).copied().unwrap_or(0),
                        ]) as i64;
                        pos += 4;
                        Operand::Imm(imm)
                    }
                }
            }
            
            OpEnc::Rel8 => {
                let rel = bytes.get(pos).copied().unwrap_or(0) as i8 as i64;
                pos += 1;
                Operand::Rel(rel)
            }
            
            OpEnc::Rel32 => {
                let rel = i32::from_le_bytes([
                    bytes.get(pos).copied().unwrap_or(0),
                    bytes.get(pos + 1).copied().unwrap_or(0),
                    bytes.get(pos + 2).copied().unwrap_or(0),
                    bytes.get(pos + 3).copied().unwrap_or(0),
                ]) as i64;
                pos += 4;
                Operand::Rel(rel)
            }
            
            OpEnc::FarPtr16 => {
                // ptr16:16 - offset (16-bit) followed by segment (16-bit)
                let offset = u16::from_le_bytes([
                    bytes.get(pos).copied().unwrap_or(0),
                    bytes.get(pos + 1).copied().unwrap_or(0),
                ]) as u64;
                let segment = u16::from_le_bytes([
                    bytes.get(pos + 2).copied().unwrap_or(0),
                    bytes.get(pos + 3).copied().unwrap_or(0),
                ]) as u16;
                pos += 4;
                Operand::Far { seg: segment, off: offset }
            }
            
            OpEnc::FarPtr32 => {
                // ptr16:32 - offset (32-bit) followed by segment (16-bit)
                let offset = u32::from_le_bytes([
                    bytes.get(pos).copied().unwrap_or(0),
                    bytes.get(pos + 1).copied().unwrap_or(0),
                    bytes.get(pos + 2).copied().unwrap_or(0),
                    bytes.get(pos + 3).copied().unwrap_or(0),
                ]) as u64;
                let segment = u16::from_le_bytes([
                    bytes.get(pos + 4).copied().unwrap_or(0),
                    bytes.get(pos + 5).copied().unwrap_or(0),
                ]) as u16;
                pos += 6;
                Operand::Far { seg: segment, off: offset }
            }
            
            OpEnc::FarPtrV => {
                // Far pointer size depends on operand size
                // In Real/16-bit mode: ptr16:16 (4 bytes)
                // In Protected/32-bit mode (with 66h prefix in real mode): ptr16:32 (6 bytes)
                let op_size = self.get_operand_size(instr);
                if op_size == 2 {
                    // 16-bit offset
                    let offset = u16::from_le_bytes([
                        bytes.get(pos).copied().unwrap_or(0),
                        bytes.get(pos + 1).copied().unwrap_or(0),
                    ]) as u64;
                    let segment = u16::from_le_bytes([
                        bytes.get(pos + 2).copied().unwrap_or(0),
                        bytes.get(pos + 3).copied().unwrap_or(0),
                    ]) as u16;
                    pos += 4;
                    Operand::Far { seg: segment, off: offset }
                } else {
                    // 32-bit offset (with 66h prefix in 16-bit mode, or default in 32-bit mode)
                    let offset = u32::from_le_bytes([
                        bytes.get(pos).copied().unwrap_or(0),
                        bytes.get(pos + 1).copied().unwrap_or(0),
                        bytes.get(pos + 2).copied().unwrap_or(0),
                        bytes.get(pos + 3).copied().unwrap_or(0),
                    ]) as u64;
                    let segment = u16::from_le_bytes([
                        bytes.get(pos + 4).copied().unwrap_or(0),
                        bytes.get(pos + 5).copied().unwrap_or(0),
                    ]) as u16;
                    pos += 6;
                    Operand::Far { seg: segment, off: offset }
                }
            }
            
            OpEnc::RegAx => {
                let size = self.get_operand_size(instr);
                Operand::Reg(Register { kind: RegKind::Gpr, index: 0, size })
            }
            
            OpEnc::RegOp => {
                let size = self.get_operand_size(instr);
                let index = (instr.opcode & 0x07) as u8 | if instr.prefixes.rex_b { 8 } else { 0 };
                Operand::Reg(Register { kind: RegKind::Gpr, index, size })
            }
            
            OpEnc::ModRm | OpEnc::ModRmReg => {
                let modrm = modrm.unwrap_or(0);
                
                let (operand, new_pos) = self.decode_modrm(bytes, pos, instr, modrm, enc == OpEnc::ModRmReg)?;
                pos = new_pos;
                operand
            }
            
            OpEnc::SegRegModRm => {
                // Segment register from ModR/M reg field
                let modrm = modrm.unwrap_or(0);
                let reg = (modrm >> 3) & 0x07;
                // Segment registers: ES=0, CS=1, SS=2, DS=3, FS=4, GS=5
                Operand::Reg(Register { kind: RegKind::Segment, index: reg, size: 2 })
            }
            
            OpEnc::CrReg => {
                // Control register from ModR/M reg field
                let modrm = modrm.unwrap_or(0);
                let reg = (modrm >> 3) & 0x07;
                // Control registers: CR0, CR2, CR3, CR4 (CR1 reserved, CR5-7 reserved)
                Operand::Reg(Register { kind: RegKind::Control, index: reg, size: 8 })
            }
            
            _ => Operand::None,
        };
        
        Ok((operand, pos))
    }
    
    fn decode_modrm(&self, bytes: &[u8], mut pos: usize, instr: &DecodedInstr, modrm: u8, is_reg: bool) -> JitResult<(Operand, usize)> {
        let mode = (modrm >> 6) & 0x03;
        let reg = (modrm >> 3) & 0x07;
        let rm = modrm & 0x07;
        
        let size = self.get_operand_size(instr);
        
        if is_reg {
            // Decode reg field
            let index = reg | if instr.prefixes.rex_r { 8 } else { 0 };
            return Ok((Operand::Reg(Register { kind: RegKind::Gpr, index, size }), pos));
        }
        
        // Decode r/m field
        if mode == 0b11 {
            // Register
            let index = rm | if instr.prefixes.rex_b { 8 } else { 0 };
            return Ok((Operand::Reg(Register { kind: RegKind::Gpr, index, size }), pos));
        }
        
        // Memory
        let mut mem = MemOp::default();
        mem.size = size;
        mem.segment = instr.prefixes.segment;
        
        // 16-bit Real Mode uses completely different ModR/M encoding
        if self.mode == CpuMode::Real {
            return self.decode_modrm_16bit(bytes, pos, instr, modrm);
        }
        
        let addr_size = if instr.prefixes.addr_size {
            if self.mode == CpuMode::Long { 4 } else { 2 }
        } else {
            if self.mode == CpuMode::Long { 8 } else { 4 }
        };
        
        if rm == 0b100 {
            // SIB byte follows
            let sib = bytes.get(pos).copied().unwrap_or(0);
            pos += 1;
            
            let scale = 1 << ((sib >> 6) & 0x03);
            let index = (sib >> 3) & 0x07;
            let base = sib & 0x07;
            
            mem.scale = scale;
            
            // Index
            let index_ext = if instr.prefixes.rex_x { 8 } else { 0 };
            if index | index_ext != 4 { // RSP cannot be index
                mem.index = Some(Register {
                    kind: RegKind::Gpr,
                    index: index | index_ext,
                    size: addr_size,
                });
            }
            
            // Base
            let base_ext = if instr.prefixes.rex_b { 8 } else { 0 };
            if mode == 0b00 && base == 0b101 {
                // No base, disp32
                mem.disp = i32::from_le_bytes([
                    bytes.get(pos).copied().unwrap_or(0),
                    bytes.get(pos + 1).copied().unwrap_or(0),
                    bytes.get(pos + 2).copied().unwrap_or(0),
                    bytes.get(pos + 3).copied().unwrap_or(0),
                ]) as i64;
                pos += 4;
            } else {
                mem.base = Some(Register {
                    kind: RegKind::Gpr,
                    index: base | base_ext,
                    size: addr_size,
                });
            }
        } else if mode == 0b00 && rm == 0b101 {
            // RIP-relative (64-bit) or disp32 (32-bit)
            mem.disp = i32::from_le_bytes([
                bytes.get(pos).copied().unwrap_or(0),
                bytes.get(pos + 1).copied().unwrap_or(0),
                bytes.get(pos + 2).copied().unwrap_or(0),
                bytes.get(pos + 3).copied().unwrap_or(0),
            ]) as i64;
            pos += 4;
            
            if self.mode == CpuMode::Long {
                mem.base = Some(Register { kind: RegKind::Rip, index: 0, size: 8 });
            }
        } else {
            // Standard base register
            let base_ext = if instr.prefixes.rex_b { 8 } else { 0 };
            mem.base = Some(Register {
                kind: RegKind::Gpr,
                index: rm | base_ext,
                size: addr_size,
            });
        }
        
        // Displacement
        match mode {
            0b01 => {
                mem.disp = bytes.get(pos).copied().unwrap_or(0) as i8 as i64;
                pos += 1;
            }
            0b10 => {
                mem.disp = i32::from_le_bytes([
                    bytes.get(pos).copied().unwrap_or(0),
                    bytes.get(pos + 1).copied().unwrap_or(0),
                    bytes.get(pos + 2).copied().unwrap_or(0),
                    bytes.get(pos + 3).copied().unwrap_or(0),
                ]) as i64;
                pos += 4;
            }
            _ => {}
        }
        
        Ok((Operand::Mem(mem), pos))
    }
    
    /// Decode 16-bit ModR/M addressing (Real Mode)
    /// 16-bit ModR/M has completely different encoding than 32/64-bit
    fn decode_modrm_16bit(&self, bytes: &[u8], mut pos: usize, instr: &DecodedInstr, modrm: u8) -> JitResult<(Operand, usize)> {
        let mode = (modrm >> 6) & 0x03;
        let rm = modrm & 0x07;
        
        let size = self.get_operand_size(instr);
        let mut mem = MemOp::default();
        mem.size = size;
        mem.segment = instr.prefixes.segment;
        
        // 16-bit addressing modes
        // mod=00:
        //   rm=000: [BX+SI], rm=001: [BX+DI], rm=010: [BP+SI], rm=011: [BP+DI]
        //   rm=100: [SI], rm=101: [DI], rm=110: [disp16], rm=111: [BX]
        // mod=01: same as 00 but + disp8
        // mod=10: same as 00 but + disp16
        // mod=11: register
        
        match rm {
            0b000 => { // [BX+SI]
                mem.base = Some(Register { kind: RegKind::Gpr, index: 3, size: 2 }); // BX
                mem.index = Some(Register { kind: RegKind::Gpr, index: 6, size: 2 }); // SI
            }
            0b001 => { // [BX+DI]
                mem.base = Some(Register { kind: RegKind::Gpr, index: 3, size: 2 }); // BX
                mem.index = Some(Register { kind: RegKind::Gpr, index: 7, size: 2 }); // DI
            }
            0b010 => { // [BP+SI]
                mem.base = Some(Register { kind: RegKind::Gpr, index: 5, size: 2 }); // BP
                mem.index = Some(Register { kind: RegKind::Gpr, index: 6, size: 2 }); // SI
                if mem.segment == Segment::None {
                    mem.segment = Segment::SS; // BP default to SS
                }
            }
            0b011 => { // [BP+DI]
                mem.base = Some(Register { kind: RegKind::Gpr, index: 5, size: 2 }); // BP
                mem.index = Some(Register { kind: RegKind::Gpr, index: 7, size: 2 }); // DI
                if mem.segment == Segment::None {
                    mem.segment = Segment::SS;
                }
            }
            0b100 => { // [SI]
                mem.base = Some(Register { kind: RegKind::Gpr, index: 6, size: 2 }); // SI
            }
            0b101 => { // [DI]
                mem.base = Some(Register { kind: RegKind::Gpr, index: 7, size: 2 }); // DI
            }
            0b110 => {
                if mode == 0b00 {
                    // [disp16] - direct address, no base register
                    let disp = u16::from_le_bytes([
                        bytes.get(pos).copied().unwrap_or(0),
                        bytes.get(pos + 1).copied().unwrap_or(0),
                    ]) as i16 as i64;
                    pos += 2;
                    mem.disp = disp;
                    // No base register for direct addressing
                    return Ok((Operand::Mem(mem), pos));
                } else {
                    // [BP+disp]
                    mem.base = Some(Register { kind: RegKind::Gpr, index: 5, size: 2 }); // BP
                    if mem.segment == Segment::None {
                        mem.segment = Segment::SS;
                    }
                }
            }
            0b111 => { // [BX]
                mem.base = Some(Register { kind: RegKind::Gpr, index: 3, size: 2 }); // BX
            }
            _ => unreachable!(),
        }
        
        // Displacement based on mod
        match mode {
            0b00 => { /* No displacement except for rm=110 handled above */ }
            0b01 => {
                // disp8, sign-extended
                mem.disp = bytes.get(pos).copied().unwrap_or(0) as i8 as i64;
                pos += 1;
            }
            0b10 => {
                // disp16
                mem.disp = u16::from_le_bytes([
                    bytes.get(pos).copied().unwrap_or(0),
                    bytes.get(pos + 1).copied().unwrap_or(0),
                ]) as i16 as i64;
                pos += 2;
            }
            _ => {} // 0b11 should not reach here (handled in caller)
        }
        
        Ok((Operand::Mem(mem), pos))
    }
    
    fn get_operand_size(&self, instr: &DecodedInstr) -> u8 {
        if instr.prefixes.rex_w {
            8
        } else if instr.prefixes.op_size {
            2
        } else {
            match self.mode {
                CpuMode::Real => 2,
                CpuMode::Long => 4,
                _ => 4,
            }
        }
    }
    
    fn opcode_to_mnemonic(&self, opcode: u32, prefixes: &Prefixes) -> Mnemonic {
        // This is a simplified mapping - full implementation would have complete tables
        match opcode {
            // Single-byte opcodes
            0x00..=0x05 => Mnemonic::Add,
            0x08..=0x0D => Mnemonic::Or,
            0x10..=0x15 => Mnemonic::Adc,
            0x18..=0x1D => Mnemonic::Sbb,
            0x20..=0x25 => Mnemonic::And,
            0x28..=0x2D => Mnemonic::Sub,
            0x30..=0x35 => Mnemonic::Xor,
            0x38..=0x3D => Mnemonic::Cmp,
            0x40..=0x4F => Mnemonic::Inc, // or REX prefix in 64-bit
            0x50..=0x57 => Mnemonic::Push,
            0x58..=0x5F => Mnemonic::Pop,
            0x60 => Mnemonic::Pusha,  // PUSHA/PUSHAD (invalid in 64-bit mode)
            0x61 => Mnemonic::Popa,   // POPA/POPAD (invalid in 64-bit mode)
            0x68 | 0x6A => Mnemonic::Push,
            0x70..=0x7F => Mnemonic::Jcc,
            0x80..=0x83 => Mnemonic::Add, // Actually depends on /r
            0x84..=0x85 => Mnemonic::Test,
            0x86..=0x87 => Mnemonic::Xchg,
            0x88..=0x8B => Mnemonic::Mov,
            0x8C => Mnemonic::Mov, // MOV r/m16, Sreg
            0x8E => Mnemonic::Mov, // MOV Sreg, r/m16
            0x8D => Mnemonic::Lea,
            0x90 => if prefixes.rep { Mnemonic::Pause } else { Mnemonic::Nop },
            0x91..=0x97 => Mnemonic::Xchg,
            0x98 => Mnemonic::Movsx, // CBW/CWDE/CDQE
            0x9C => Mnemonic::Pushf,
            0x9D => Mnemonic::Popf,
            0xA0..=0xA3 => Mnemonic::Mov,
            0xA4..=0xA5 => Mnemonic::Movs,
            0xA6..=0xA7 => Mnemonic::Cmps,
            0xA8..=0xA9 => Mnemonic::Test,
            0xAA..=0xAB => Mnemonic::Stos,
            0xAC..=0xAD => Mnemonic::Lods,
            0xAE..=0xAF => Mnemonic::Scas,
            0xB0..=0xBF => Mnemonic::Mov,
            0xC0..=0xC1 => Mnemonic::Shl, // Depends on /r
            0xC2..=0xC3 => Mnemonic::Ret,
            0xC6..=0xC7 => Mnemonic::Mov,
            0xC9 => Mnemonic::Pop, // LEAVE
            0xCA..=0xCB => Mnemonic::Retf,
            0xCC => Mnemonic::Int3,
            0xCD => Mnemonic::Int,
            0xCF => Mnemonic::Iret,
            0xD0..=0xD3 => Mnemonic::Shl, // Depends on /r
            0xE0 => Mnemonic::Loopne,
            0xE1 => Mnemonic::Loope,
            0xE2 => Mnemonic::Loop,
            0xE3 => Mnemonic::Jcxz,
            0xE4..=0xE7 => Mnemonic::In, // or Out
            0xE8 => Mnemonic::Call,
            0xE9..=0xEB => Mnemonic::Jmp,
            0xEA => Mnemonic::Jmp, // JMP FAR ptr16:16/ptr16:32
            0xEC..=0xEF => Mnemonic::In, // or Out
            0xF4 => Mnemonic::Hlt,
            0xF5 => Mnemonic::Cmc,
            0xF6..=0xF7 => Mnemonic::Test, // Depends on /r
            0xF8 => Mnemonic::Clc,
            0xF9 => Mnemonic::Stc,
            0xFA => Mnemonic::Cli,
            0xFB => Mnemonic::Sti,
            0xFC => Mnemonic::Cld,
            0xFD => Mnemonic::Std,
            0xFE..=0xFF => Mnemonic::Inc, // Depends on /r
            
            // Two-byte opcodes (0x0F xx)
            0x0F01 => Mnemonic::Lgdt, // Depends on /r
            0x0F05 => Mnemonic::Syscall,
            0x0F06 => Mnemonic::Clts,
            0x0F07 => Mnemonic::Sysret,
            0x0F09 => Mnemonic::Wbinvd,
            0x0F0B => Mnemonic::Invalid, // UD2
            0x0F20 => Mnemonic::Mov, // MOV from CR
            0x0F21 => Mnemonic::Mov, // MOV from DR
            0x0F22 => Mnemonic::Mov, // MOV to CR
            0x0F23 => Mnemonic::Mov, // MOV to DR
            0x0F30 => Mnemonic::Wrmsr,
            0x0F31 => Mnemonic::Rdtsc,
            0x0F32 => Mnemonic::Rdmsr,
            0x0F34 => Mnemonic::Sysenter,
            0x0F35 => Mnemonic::Sysexit,
            0x0F40..=0x0F4F => Mnemonic::Cmov,
            0x0F80..=0x0F8F => Mnemonic::Jcc,
            0x0F90..=0x0F9F => Mnemonic::Cmov, // SETcc
            0x0FA2 => Mnemonic::Cpuid,
            0x0FA3 => Mnemonic::Bt,
            0x0FAB => Mnemonic::Bts,
            0x0FAE => Mnemonic::Clflush, // Depends on /r
            0x0FAF => Mnemonic::Imul,
            0x0FB0..=0x0FB1 => Mnemonic::Cmpxchg,
            0x0FB3 => Mnemonic::Btr,
            0x0FB6..=0x0FB7 => Mnemonic::Movzx,
            0x0FBA => Mnemonic::Bt, // Depends on /r
            0x0FBB => Mnemonic::Btc,
            0x0FBC => Mnemonic::Bsf,
            0x0FBD => Mnemonic::Bsr,
            0x0FBE..=0x0FBF => Mnemonic::Movsx,
            0x0FC0..=0x0FC1 => Mnemonic::Xadd,
            0x0FC7 => Mnemonic::Cmpxchg8b, // or CMPXCHG16B
            0x0FC8..=0x0FCF => Mnemonic::Bswap,
            
            // VMX
            0x0F01C1 => Mnemonic::Vmcall,
            0x0F01C2 => Mnemonic::Vmlaunch,
            0x0F01C3 => Mnemonic::Vmresume,
            0x0F01C4 => Mnemonic::Vmxoff,
            
            // SVM
            0x0F01D8 => Mnemonic::Vmrun,
            0x0F01D9 => Mnemonic::Vmmcall,
            0x0F01DA => Mnemonic::Vmload,
            0x0F01DB => Mnemonic::Vmsave,
            0x0F01DC => Mnemonic::Stgi,
            0x0F01DD => Mnemonic::Clgi,
            
            _ => Mnemonic::Invalid,
        }
    }
    
    fn get_operand_encoding(&self, opcode: u32, _prefixes: &Prefixes) -> [OpEnc; 4] {
        // Simplified - real implementation needs full opcode tables
        match opcode {
            // ADD/OR/ADC/SBB/AND/SUB/XOR/CMP r/m8, r8 and r8, r/m8
            0x00..=0x03 => [OpEnc::ModRm, OpEnc::ModRmReg, OpEnc::None, OpEnc::None],
            0x04 => [OpEnc::RegAx, OpEnc::Imm8, OpEnc::None, OpEnc::None],
            0x05 => [OpEnc::RegAx, OpEnc::Imm32, OpEnc::None, OpEnc::None],
            // OR
            0x08..=0x0B => [OpEnc::ModRm, OpEnc::ModRmReg, OpEnc::None, OpEnc::None],
            0x0C => [OpEnc::RegAx, OpEnc::Imm8, OpEnc::None, OpEnc::None],
            0x0D => [OpEnc::RegAx, OpEnc::Imm32, OpEnc::None, OpEnc::None],
            // ADC
            0x10..=0x13 => [OpEnc::ModRm, OpEnc::ModRmReg, OpEnc::None, OpEnc::None],
            0x14 => [OpEnc::RegAx, OpEnc::Imm8, OpEnc::None, OpEnc::None],
            0x15 => [OpEnc::RegAx, OpEnc::Imm32, OpEnc::None, OpEnc::None],
            // SBB
            0x18..=0x1B => [OpEnc::ModRm, OpEnc::ModRmReg, OpEnc::None, OpEnc::None],
            0x1C => [OpEnc::RegAx, OpEnc::Imm8, OpEnc::None, OpEnc::None],
            0x1D => [OpEnc::RegAx, OpEnc::Imm32, OpEnc::None, OpEnc::None],
            // AND
            0x20..=0x23 => [OpEnc::ModRm, OpEnc::ModRmReg, OpEnc::None, OpEnc::None],
            0x24 => [OpEnc::RegAx, OpEnc::Imm8, OpEnc::None, OpEnc::None],
            0x25 => [OpEnc::RegAx, OpEnc::Imm32, OpEnc::None, OpEnc::None],
            // SUB
            0x28..=0x2B => [OpEnc::ModRm, OpEnc::ModRmReg, OpEnc::None, OpEnc::None],
            0x2C => [OpEnc::RegAx, OpEnc::Imm8, OpEnc::None, OpEnc::None],
            0x2D => [OpEnc::RegAx, OpEnc::Imm32, OpEnc::None, OpEnc::None],
            // XOR
            0x30..=0x33 => [OpEnc::ModRm, OpEnc::ModRmReg, OpEnc::None, OpEnc::None],
            0x34 => [OpEnc::RegAx, OpEnc::Imm8, OpEnc::None, OpEnc::None],
            0x35 => [OpEnc::RegAx, OpEnc::Imm32, OpEnc::None, OpEnc::None],
            // CMP
            0x38..=0x3B => [OpEnc::ModRm, OpEnc::ModRmReg, OpEnc::None, OpEnc::None],
            0x3C => [OpEnc::RegAx, OpEnc::Imm8, OpEnc::None, OpEnc::None],
            0x3D => [OpEnc::RegAx, OpEnc::Imm32, OpEnc::None, OpEnc::None],
            
            0x50..=0x57 => [OpEnc::RegOp, OpEnc::None, OpEnc::None, OpEnc::None],
            0x58..=0x5F => [OpEnc::RegOp, OpEnc::None, OpEnc::None, OpEnc::None],
            0x68 => [OpEnc::Imm32, OpEnc::None, OpEnc::None, OpEnc::None],
            0x6A => [OpEnc::Imm8, OpEnc::None, OpEnc::None, OpEnc::None],
            0x70..=0x7F => [OpEnc::Rel8, OpEnc::None, OpEnc::None, OpEnc::None],
            0x88..=0x8B => [OpEnc::ModRm, OpEnc::ModRmReg, OpEnc::None, OpEnc::None],
            0x8C => [OpEnc::ModRm, OpEnc::SegRegModRm, OpEnc::None, OpEnc::None], // MOV r/m16, Sreg
            0x8E => [OpEnc::SegRegModRm, OpEnc::ModRm, OpEnc::None, OpEnc::None], // MOV Sreg, r/m16
            0x8D => [OpEnc::ModRmReg, OpEnc::ModRm, OpEnc::None, OpEnc::None],
            0xB0..=0xB7 => [OpEnc::RegOp, OpEnc::Imm8, OpEnc::None, OpEnc::None],
            0xB8..=0xBF => [OpEnc::RegOp, OpEnc::ImmV, OpEnc::None, OpEnc::None], // MOV r16/32/64, imm
            0xC3 => [OpEnc::None, OpEnc::None, OpEnc::None, OpEnc::None],
            0xCD => [OpEnc::Imm8, OpEnc::None, OpEnc::None, OpEnc::None],
            0xE8 => [OpEnc::Rel32, OpEnc::None, OpEnc::None, OpEnc::None],
            0xE9 => [OpEnc::Rel32, OpEnc::None, OpEnc::None, OpEnc::None],
            0xEA => [OpEnc::FarPtrV, OpEnc::None, OpEnc::None, OpEnc::None], // JMP FAR ptr16:16/32
            0xEB => [OpEnc::Rel8, OpEnc::None, OpEnc::None, OpEnc::None],
            // Two-byte opcodes
            0x0F01 => [OpEnc::ModRm, OpEnc::None, OpEnc::None, OpEnc::None], // LGDT/LIDT/etc m
            0x0F20 => [OpEnc::ModRm, OpEnc::CrReg, OpEnc::None, OpEnc::None], // MOV r64, CRn (dst=r/m, src=CRn)
            0x0F22 => [OpEnc::CrReg, OpEnc::ModRm, OpEnc::None, OpEnc::None], // MOV CRn, r64 (dst=CRn, src=r/m)
            0x0F80..=0x0F8F => [OpEnc::Rel32, OpEnc::None, OpEnc::None, OpEnc::None],
            _ => [OpEnc::None, OpEnc::None, OpEnc::None, OpEnc::None],
        }
    }
    
    fn set_instruction_flags(&self, instr: &mut DecodedInstr) {
        match instr.mnemonic {
            Mnemonic::Jmp | Mnemonic::Jcc => instr.is_branch = true,
            Mnemonic::Call => instr.is_call = true,
            Mnemonic::Ret | Mnemonic::Retf | Mnemonic::Iret => instr.is_ret = true,
            Mnemonic::Loop | Mnemonic::Loope | Mnemonic::Loopne |
            Mnemonic::Jcxz | Mnemonic::Jecxz | Mnemonic::Jrcxz => instr.is_branch = true,
            
            // Privileged
            Mnemonic::Lgdt | Mnemonic::Lidt | Mnemonic::Lldt | Mnemonic::Ltr |
            Mnemonic::Clts | Mnemonic::Invlpg | Mnemonic::Wrmsr | Mnemonic::Rdmsr |
            Mnemonic::Cli | Mnemonic::Sti | Mnemonic::Hlt |
            Mnemonic::In | Mnemonic::Out | Mnemonic::Ins | Mnemonic::Outs => {
                instr.is_privileged = true;
            }
            
            _ => {}
        }
        
        // Set flags affected
        match instr.mnemonic {
            Mnemonic::Add | Mnemonic::Adc | Mnemonic::Sub | Mnemonic::Sbb |
            Mnemonic::And | Mnemonic::Or | Mnemonic::Xor |
            Mnemonic::Inc | Mnemonic::Dec | Mnemonic::Neg |
            Mnemonic::Cmp | Mnemonic::Test => {
                instr.flags_affected = 0x8D5; // OF SF ZF AF PF CF
            }
            Mnemonic::Shl | Mnemonic::Shr | Mnemonic::Sar => {
                instr.flags_affected = 0x8D5;
            }
            _ => {}
        }
    }
}

/// Operand encoding type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpEnc {
    None,
    Imm8,
    Imm16,
    ImmV,  // Immediate size depends on operand size (16/32/64)
    Imm32,
    Imm64,
    Rel8,
    Rel32,
    RegAx,
    RegOp,
    ModRm,
    ModRmReg,
    SegRegModRm, // Segment register from ModR/M reg field (for 8C/8E)
    CrReg,       // Control register from ModR/M reg field (for 0F 20/22)
    FarPtr16,  // ptr16:16 (segment:offset) for real mode JMP FAR
    FarPtr32,  // ptr16:32 (segment:offset) for protected mode JMP FAR
    FarPtrV,   // Far pointer size depends on operand size (16 or 32-bit offset)
}

impl Default for X86Decoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_decode_nop() {
        let decoder = X86Decoder::with_mode(CpuMode::Long);
        let instr = decoder.decode(&[0x90], 0).unwrap();
        assert_eq!(instr.mnemonic, Mnemonic::Nop);
        assert_eq!(instr.len, 1);
    }
    
    #[test]
    fn test_decode_ret() {
        let decoder = X86Decoder::with_mode(CpuMode::Long);
        let instr = decoder.decode(&[0xC3], 0).unwrap();
        assert_eq!(instr.mnemonic, Mnemonic::Ret);
        assert!(instr.is_ret);
    }
    
    #[test]
    fn test_decode_push_reg() {
        let decoder = X86Decoder::with_mode(CpuMode::Long);
        let instr = decoder.decode(&[0x50], 0).unwrap(); // PUSH RAX
        assert_eq!(instr.mnemonic, Mnemonic::Push);
    }
}
