//! Native Code Generator
//!
//! Generates x86-64 machine code from IR.
//! Handles:
//! - Instruction encoding
//! - Register allocation finalization
//! - Stack frame management
//! - Relocation and patching

use super::{JitResult, JitError};
use super::ir::{IrBlock, IrInstr, IrOp, VReg, IrFlags, BlockId};
use super::cache::{CodeRegion, CompiledBlock, CompileTier, compute_checksum};
use super::block_manager::{IsaCodeGen, IsaCodegenConfig};
use super::nready::InstructionSets;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;

/// Native code generator
pub struct CodeGen {
    /// Register allocation
    reg_alloc: HashMap<VReg, HostReg>,
    /// Stack frame size
    frame_size: u32,
    /// Spill slots
    spill_slots: HashMap<VReg, i32>,
    /// Label offsets for patching
    labels: HashMap<u32, usize>,
    /// Pending relocations
    relocations: Vec<Relocation>,
    /// ISA-aware code generator for optimized instruction emission
    isa_codegen: IsaCodeGen,
    /// Target ISA configuration
    isa_config: IsaCodegenConfig,
}

/// Host register
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HostReg(pub u8);

impl HostReg {
    // General purpose registers
    pub const RAX: Self = Self(0);
    pub const RCX: Self = Self(1);
    pub const RDX: Self = Self(2);
    pub const RBX: Self = Self(3);
    pub const RSP: Self = Self(4);
    pub const RBP: Self = Self(5);
    pub const RSI: Self = Self(6);
    pub const RDI: Self = Self(7);
    pub const R8: Self = Self(8);
    pub const R9: Self = Self(9);
    pub const R10: Self = Self(10);
    pub const R11: Self = Self(11);
    pub const R12: Self = Self(12);
    pub const R13: Self = Self(13);
    pub const R14: Self = Self(14);
    pub const R15: Self = Self(15);
    
    pub fn is_extended(&self) -> bool {
        self.0 >= 8
    }
    
    pub fn low3(&self) -> u8 {
        self.0 & 7
    }
}

/// Relocation entry
#[derive(Clone, Debug)]
pub struct Relocation {
    /// Offset in code buffer
    pub offset: usize,
    /// Target label
    pub target: RelocationTarget,
    /// Relocation type
    pub kind: RelocKind,
}

/// Relocation target
#[derive(Clone, Debug)]
pub enum RelocationTarget {
    Label(u32),
    External(u64),
}

/// Relocation type
#[derive(Clone, Copy, Debug)]
pub enum RelocKind {
    /// 32-bit PC-relative
    Rel32,
    /// 64-bit absolute
    Abs64,
}

/// Code buffer with patching support
pub struct CodeBuffer {
    code: Vec<u8>,
    labels: HashMap<u32, usize>,
    relocations: Vec<Relocation>,
}

impl CodeBuffer {
    pub fn new() -> Self {
        Self {
            code: Vec::with_capacity(4096),
            labels: HashMap::new(),
            relocations: Vec::new(),
        }
    }
    
    pub fn len(&self) -> usize {
        self.code.len()
    }
    
    pub fn emit(&mut self, byte: u8) {
        self.code.push(byte);
    }
    
    pub fn emit_bytes(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }
    
    pub fn emit_u16(&mut self, val: u16) {
        self.code.extend_from_slice(&val.to_le_bytes());
    }
    
    pub fn emit_u32(&mut self, val: u32) {
        self.code.extend_from_slice(&val.to_le_bytes());
    }
    
    pub fn emit_u64(&mut self, val: u64) {
        self.code.extend_from_slice(&val.to_le_bytes());
    }
    
    pub fn emit_i32(&mut self, val: i32) {
        self.code.extend_from_slice(&val.to_le_bytes());
    }
    
    pub fn bind_label(&mut self, label: u32) {
        self.labels.insert(label, self.code.len());
    }
    
    pub fn emit_label_ref(&mut self, label: u32, kind: RelocKind) {
        self.relocations.push(Relocation {
            offset: self.code.len(),
            target: RelocationTarget::Label(label),
            kind,
        });
        
        // Placeholder
        match kind {
            RelocKind::Rel32 => self.emit_u32(0),
            RelocKind::Abs64 => self.emit_u64(0),
        }
    }
    
    pub fn patch_relocations(&mut self) -> JitResult<()> {
        for reloc in &self.relocations {
            match reloc.target {
                RelocationTarget::Label(label) => {
                    let target_offset = self.labels.get(&label)
                        .ok_or(JitError::UnresolvedLabel)?;
                    
                    match reloc.kind {
                        RelocKind::Rel32 => {
                            let rel = (*target_offset as i64 - (reloc.offset + 4) as i64) as i32;
                            self.code[reloc.offset..reloc.offset + 4]
                                .copy_from_slice(&rel.to_le_bytes());
                        }
                        RelocKind::Abs64 => {
                            self.code[reloc.offset..reloc.offset + 8]
                                .copy_from_slice(&(*target_offset as u64).to_le_bytes());
                        }
                    }
                }
                RelocationTarget::External(addr) => {
                    match reloc.kind {
                        RelocKind::Abs64 => {
                            self.code[reloc.offset..reloc.offset + 8]
                                .copy_from_slice(&addr.to_le_bytes());
                        }
                        _ => return Err(JitError::InvalidRelocation),
                    }
                }
            }
        }
        
        Ok(())
    }
    
    pub fn finish(mut self) -> JitResult<Vec<u8>> {
        self.patch_relocations()?;
        Ok(self.code)
    }
}

impl Default for CodeBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeGen {
    pub fn new() -> Self {
        let isa_config = IsaCodegenConfig::for_current_cpu();
        let isa_codegen = IsaCodeGen::new(isa_config.clone());
        Self {
            reg_alloc: HashMap::new(),
            frame_size: 0,
            spill_slots: HashMap::new(),
            labels: HashMap::new(),
            relocations: Vec::new(),
            isa_codegen,
            isa_config,
        }
    }
    
    /// Create with specific ISA configuration
    pub fn with_isa(isa_config: IsaCodegenConfig) -> Self {
        let isa_codegen = IsaCodeGen::new(isa_config.clone());
        Self {
            reg_alloc: HashMap::new(),
            frame_size: 0,
            spill_slots: HashMap::new(),
            labels: HashMap::new(),
            relocations: Vec::new(),
            isa_codegen,
            isa_config,
        }
    }
    
    /// Get the ISA configuration
    pub fn isa_config(&self) -> &IsaCodegenConfig {
        &self.isa_config
    }
    
    /// Check if a specific ISA feature is available
    pub fn has_feature(&self, feature: InstructionSets) -> bool {
        self.isa_config.has_feature(feature)
    }
    
    /// Generate native code from IR
    pub fn generate(
        &mut self,
        ir: &IrBlock,
        reg_alloc: HashMap<VReg, HostReg>,
        spills: &[VReg],
    ) -> JitResult<Vec<u8>> {
        self.reg_alloc = reg_alloc;
        self.frame_size = (spills.len() * 8) as u32;
        
        // Assign spill slots (negative offsets from rbp)
        for (i, &vreg) in spills.iter().enumerate() {
            self.spill_slots.insert(vreg, -(((i + 1) * 8) as i32));
        }
        
        let mut buf = CodeBuffer::new();
        
        // Prologue
        self.emit_prologue(&mut buf);
        
        // Body
        for (bb_idx, bb) in ir.blocks.iter().enumerate() {
            buf.bind_label(bb_idx as u32);
            
            for instr in &bb.instrs {
                self.emit_instr(&mut buf, instr)?;
            }
        }
        
        // Epilogue
        self.emit_epilogue(&mut buf);
        
        buf.finish()
    }
    
    /// Emit function prologue
    fn emit_prologue(&self, buf: &mut CodeBuffer) {
        // push rbp
        buf.emit(0x55);
        
        // mov rbp, rsp
        buf.emit_bytes(&[0x48, 0x89, 0xE5]);
        
        // Allocate stack frame
        if self.frame_size > 0 {
            // sub rsp, frame_size
            if self.frame_size <= 127 {
                buf.emit_bytes(&[0x48, 0x83, 0xEC, self.frame_size as u8]);
            } else {
                buf.emit_bytes(&[0x48, 0x81, 0xEC]);
                buf.emit_u32(self.frame_size);
            }
        }
        
        // Save callee-saved registers
        for &reg in &[HostReg::RBX, HostReg::R12, HostReg::R13, HostReg::R14, HostReg::R15] {
            self.emit_push(buf, reg);
        }
    }
    
    /// Emit function epilogue
    fn emit_epilogue(&self, buf: &mut CodeBuffer) {
        // Restore callee-saved registers
        for &reg in &[HostReg::R15, HostReg::R14, HostReg::R13, HostReg::R12, HostReg::RBX] {
            self.emit_pop(buf, reg);
        }
        
        // mov rsp, rbp
        buf.emit_bytes(&[0x48, 0x89, 0xEC]);
        
        // pop rbp
        buf.emit(0x5D);
        
        // ret
        buf.emit(0xC3);
    }
    
    fn emit_instr(&self, buf: &mut CodeBuffer, instr: &IrInstr) -> JitResult<()> {
        let dst = instr.dst;
        
        match &instr.op {
            IrOp::Const(val) => {
                let reg = self.get_reg(dst)?;
                self.emit_mov_imm64(buf, reg, *val as u64);
            }
            
            IrOp::ConstF64(_) => {
                // Floating point - would use XMM registers
                buf.emit(0x90); // NOP placeholder
            }
            
            IrOp::LoadGpr(idx) => {
                let dreg = self.get_reg(dst)?;
                // Would load from guest CPU state
                self.emit_mov_imm64(buf, dreg, *idx as u64);
            }
            
            IrOp::StoreGpr(idx, val) => {
                let vreg = self.get_reg(*val)?;
                let _ = (idx, vreg); // Would store to guest CPU state
            }
            
            IrOp::LoadFlags | IrOp::LoadRip => {
                let dreg = self.get_reg(dst)?;
                self.emit_mov_imm64(buf, dreg, 0); // Placeholder
            }
            
            IrOp::StoreFlags(_) | IrOp::StoreRip(_) => {
                // Would store to guest CPU state
            }
            
            IrOp::Add(a, b) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                
                if dreg != areg {
                    self.emit_mov_reg_reg(buf, dreg, areg);
                }
                self.emit_alu_reg_reg(buf, AluOp::Add, dreg, breg);
            }
            
            IrOp::Sub(a, b) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                
                if dreg != areg {
                    self.emit_mov_reg_reg(buf, dreg, areg);
                }
                self.emit_alu_reg_reg(buf, AluOp::Sub, dreg, breg);
            }
            
            IrOp::And(a, b) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                
                if dreg != areg {
                    self.emit_mov_reg_reg(buf, dreg, areg);
                }
                self.emit_alu_reg_reg(buf, AluOp::And, dreg, breg);
            }
            
            IrOp::Or(a, b) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                
                if dreg != areg {
                    self.emit_mov_reg_reg(buf, dreg, areg);
                }
                self.emit_alu_reg_reg(buf, AluOp::Or, dreg, breg);
            }
            
            IrOp::Xor(a, b) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                
                if dreg != areg {
                    self.emit_mov_reg_reg(buf, dreg, areg);
                }
                self.emit_alu_reg_reg(buf, AluOp::Xor, dreg, breg);
            }
            
            IrOp::Mul(a, b) | IrOp::IMul(a, b) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                
                if dreg != areg {
                    self.emit_mov_reg_reg(buf, dreg, areg);
                }
                self.emit_imul_reg_reg(buf, dreg, breg);
            }
            
            IrOp::Div(_, _) | IrOp::IDiv(_, _) => {
                // Division requires special handling (RAX:RDX)
                buf.emit(0x90); // Placeholder
            }
            
            IrOp::Shl(a, b) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                
                if dreg != areg {
                    self.emit_mov_reg_reg(buf, dreg, areg);
                }
                if breg != HostReg::RCX {
                    self.emit_mov_reg_reg(buf, HostReg::RCX, breg);
                }
                self.emit_shift(buf, ShiftOp::Shl, dreg);
            }
            
            IrOp::Shr(a, b) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                
                if dreg != areg {
                    self.emit_mov_reg_reg(buf, dreg, areg);
                }
                if breg != HostReg::RCX {
                    self.emit_mov_reg_reg(buf, HostReg::RCX, breg);
                }
                self.emit_shift(buf, ShiftOp::Shr, dreg);
            }
            
            IrOp::Sar(a, b) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                
                if dreg != areg {
                    self.emit_mov_reg_reg(buf, dreg, areg);
                }
                if breg != HostReg::RCX {
                    self.emit_mov_reg_reg(buf, HostReg::RCX, breg);
                }
                self.emit_shift(buf, ShiftOp::Sar, dreg);
            }
            
            IrOp::Rol(_, _) | IrOp::Ror(_, _) => {
                // Rotate instructions
                buf.emit(0x90); // Placeholder
            }
            
            IrOp::Neg(a) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                
                if dreg != areg {
                    self.emit_mov_reg_reg(buf, dreg, areg);
                }
                self.emit_neg(buf, dreg);
            }
            
            IrOp::Not(a) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                
                if dreg != areg {
                    self.emit_mov_reg_reg(buf, dreg, areg);
                }
                self.emit_not(buf, dreg);
            }
            
            IrOp::Load8(addr) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*addr)?;
                self.emit_load(buf, dreg, areg, 1);
            }
            
            IrOp::Load16(addr) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*addr)?;
                self.emit_load(buf, dreg, areg, 2);
            }
            
            IrOp::Load32(addr) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*addr)?;
                self.emit_load(buf, dreg, areg, 4);
            }
            
            IrOp::Load64(addr) => {
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*addr)?;
                self.emit_load(buf, dreg, areg, 8);
            }
            
            IrOp::Store8(addr, val) => {
                let areg = self.get_reg(*addr)?;
                let vreg = self.get_reg(*val)?;
                self.emit_store(buf, areg, vreg, 1);
            }
            
            IrOp::Store16(addr, val) => {
                let areg = self.get_reg(*addr)?;
                let vreg = self.get_reg(*val)?;
                self.emit_store(buf, areg, vreg, 2);
            }
            
            IrOp::Store32(addr, val) => {
                let areg = self.get_reg(*addr)?;
                let vreg = self.get_reg(*val)?;
                self.emit_store(buf, areg, vreg, 4);
            }
            
            IrOp::Store64(addr, val) => {
                let areg = self.get_reg(*addr)?;
                let vreg = self.get_reg(*val)?;
                self.emit_store(buf, areg, vreg, 8);
            }
            
            IrOp::Cmp(a, b) | IrOp::Test(a, b) => {
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                
                if matches!(instr.op, IrOp::Cmp(_, _)) {
                    self.emit_cmp_reg_reg(buf, areg, breg);
                } else {
                    self.emit_test_reg_reg(buf, areg, breg);
                }
            }
            
            IrOp::GetCF(_) | IrOp::GetZF(_) | IrOp::GetSF(_) | 
            IrOp::GetOF(_) | IrOp::GetPF(_) => {
                let dreg = self.get_reg(dst)?;
                // Would extract flag from RFLAGS
                self.emit_xor_reg_reg(buf, dreg, dreg);
            }
            
            IrOp::Select(cond, t, f) => {
                let dreg = self.get_reg(dst)?;
                let creg = self.get_reg(*cond)?;
                let treg = self.get_reg(*t)?;
                let freg = self.get_reg(*f)?;
                
                // cmovnz dst, true_val; cmovz dst, false_val
                self.emit_test_reg_reg(buf, creg, creg);
                self.emit_mov_reg_reg(buf, dreg, freg);
                // Would add cmovnz here
                let _ = treg;
            }
            
            IrOp::Sext8(v) | IrOp::Sext16(v) | IrOp::Sext32(v) |
            IrOp::Zext8(v) | IrOp::Zext16(v) | IrOp::Zext32(v) |
            IrOp::Trunc8(v) | IrOp::Trunc16(v) | IrOp::Trunc32(v) => {
                let dreg = self.get_reg(dst)?;
                let vreg = self.get_reg(*v)?;
                if dreg != vreg {
                    self.emit_mov_reg_reg(buf, dreg, vreg);
                }
                // Would apply appropriate extension/truncation
            }
            
            // Control flow terminators
            IrOp::Jump(target) => {
                buf.emit(0xE9); // JMP rel32
                buf.emit_label_ref(target.0, RelocKind::Rel32);
            }
            
            IrOp::Branch(cond, true_block, false_block) => {
                let creg = self.get_reg(*cond)?;
                
                // test cond, cond
                self.emit_test_reg_reg(buf, creg, creg);
                
                // jnz true_block
                buf.emit_bytes(&[0x0F, 0x85]); // JNZ rel32
                buf.emit_label_ref(true_block.0, RelocKind::Rel32);
                
                // jmp false_block
                buf.emit(0xE9);
                buf.emit_label_ref(false_block.0, RelocKind::Rel32);
            }
            
            IrOp::Call(target) => {
                // Direct call
                buf.emit(0xE8); // CALL rel32
                buf.emit_i32((*target as i64 - (buf.len() as i64 + 4)) as i32);
            }
            
            IrOp::CallIndirect(target) => {
                let treg = self.get_reg(*target)?;
                self.emit_call_reg(buf, treg);
            }
            
            IrOp::Ret => {
                // Will fall through to epilogue
            }
            
            IrOp::Syscall => {
                buf.emit_bytes(&[0x0F, 0x05]); // SYSCALL
            }
            
            IrOp::Cpuid => {
                buf.emit_bytes(&[0x0F, 0xA2]); // CPUID
            }
            
            IrOp::Rdtsc => {
                buf.emit_bytes(&[0x0F, 0x31]); // RDTSC
            }
            
            IrOp::Hlt => {
                buf.emit(0xF4); // HLT
            }
            
            IrOp::Nop => {
                buf.emit(0x90);
            }
            
            IrOp::In8(port) | IrOp::In16(port) | IrOp::In32(port) => {
                let preg = self.get_reg(*port)?;
                // IN instruction requires DX for port
                if preg != HostReg::RDX {
                    self.emit_mov_reg_reg(buf, HostReg::RDX, preg);
                }
                match &instr.op {
                    IrOp::In8(_) => buf.emit_bytes(&[0xEC]), // IN AL, DX
                    IrOp::In16(_) => buf.emit_bytes(&[0x66, 0xED]), // IN AX, DX
                    _ => buf.emit(0xED), // IN EAX, DX
                }
            }
            
            IrOp::Out8(port, val) | IrOp::Out16(port, val) | IrOp::Out32(port, val) => {
                let preg = self.get_reg(*port)?;
                let vreg = self.get_reg(*val)?;
                
                if preg != HostReg::RDX {
                    self.emit_mov_reg_reg(buf, HostReg::RDX, preg);
                }
                if vreg != HostReg::RAX {
                    self.emit_mov_reg_reg(buf, HostReg::RAX, vreg);
                }
                match &instr.op {
                    IrOp::Out8(_, _) => buf.emit_bytes(&[0xEE]), // OUT DX, AL
                    IrOp::Out16(_, _) => buf.emit_bytes(&[0x66, 0xEF]), // OUT DX, AX
                    _ => buf.emit(0xEF), // OUT DX, EAX
                }
            }
            
            IrOp::Phi(_) => {
                // Phi nodes are resolved during register allocation
            }
            
            IrOp::Exit(reason) => {
                // Exit VM - store exit reason in RAX and return
                let code = match reason {
                    super::ir::ExitReason::Normal => 0,
                    super::ir::ExitReason::Halt => 1,
                    super::ir::ExitReason::Interrupt(v) => 0x100 | (*v as u64),
                    super::ir::ExitReason::Exception(v, _) => 0x200 | (*v as u64),
                    super::ir::ExitReason::IoRead(p, s) => 0x300 | ((*p as u64) << 8) | (*s as u64),
                    super::ir::ExitReason::IoWrite(p, s) => 0x400 | ((*p as u64) << 8) | (*s as u64),
                    super::ir::ExitReason::Mmio(_, _, _) => 0x500,
                    super::ir::ExitReason::Hypercall => 0x600,
                    super::ir::ExitReason::Reset => 0x700,
                };
                self.emit_mov_imm64(buf, HostReg::RAX, code);
            }
            
            // ================================================================
            // ISA-specific operations (BMI/POPCNT/etc)
            // Uses IsaCodeGen for ISA-aware optimized code emission
            // Automatically selects hardware instructions or software fallback
            // ================================================================
            
            IrOp::Popcnt(src) => {
                // ISA-aware POPCNT: uses hardware POPCNT if available, else software
                let dreg = self.get_reg(dst)?;
                let sreg = self.get_reg(*src)?;
                self.isa_codegen.emit_popcnt(&mut buf.code, dreg.0, sreg.0);
            }
            
            IrOp::Lzcnt(src) => {
                // ISA-aware LZCNT: uses hardware LZCNT if available, else BSR+adjust
                let dreg = self.get_reg(dst)?;
                let sreg = self.get_reg(*src)?;
                self.isa_codegen.emit_lzcnt(&mut buf.code, dreg.0, sreg.0);
            }
            
            IrOp::Tzcnt(src) => {
                // ISA-aware TZCNT: uses hardware TZCNT if available, else BSF+adjust
                let dreg = self.get_reg(dst)?;
                let sreg = self.get_reg(*src)?;
                self.isa_codegen.emit_tzcnt(&mut buf.code, dreg.0, sreg.0);
            }
            
            IrOp::Bsf(src) | IrOp::Bsr(src) => {
                let dreg = self.get_reg(dst)?;
                let sreg = self.get_reg(*src)?;
                // BSF/BSR are baseline instructions, always available
                self.emit_rex_w(buf, dreg, sreg);
                buf.emit_bytes(&[0x0F, if matches!(instr.op, IrOp::Bsf(_)) { 0xBC } else { 0xBD }]);
                self.emit_modrm(buf, 3, dreg, sreg);
            }
            
            IrOp::Bextr(src, start_reg, len_reg) => {
                // ISA-aware BEXTR: uses BMI1 if available, else shift+mask
                let dreg = self.get_reg(dst)?;
                let sreg = self.get_reg(*src)?;
                let start_r = self.get_reg(*start_reg)?;
                let len_r = self.get_reg(*len_reg)?;
                
                // For register-based start/len, we need to compute the control value
                // If they're constant (from prior Const ops), S2 should have folded them
                // Here we emit a dynamic version using shift+mask
                // Save start_r and len_r into temp registers if needed
                if self.isa_config.has_feature(InstructionSets::BMI1) {
                    // Construct control word: len << 8 | start in R11
                    // mov r11, len_r
                    self.emit_mov_reg_reg(buf, HostReg::R11, len_r);
                    // shl r11, 8
                    buf.emit_bytes(&[0x49, 0xC1, 0xE3, 0x08]);
                    // or r11, start_r
                    self.emit_rex_w(buf, HostReg::R11, start_r);
                    buf.emit_bytes(&[0x09]);
                    self.emit_modrm(buf, 3, start_r, HostReg::R11);
                    // BEXTR dst, src, r11 via VEX
                    self.emit_bextr_vex(buf, dreg, sreg, HostReg::R11);
                } else {
                    // Software fallback: dst = (src >> start) & ((1 << len) - 1)
                    // This requires runtime computation since start/len are in registers
                    self.emit_bextr_software(buf, dreg, sreg, start_r, len_r);
                }
            }
            
            IrOp::Pdep(src, mask) => {
                // ISA-aware PDEP: uses BMI2 if available, else software loop
                let dreg = self.get_reg(dst)?;
                let sreg = self.get_reg(*src)?;
                let mreg = self.get_reg(*mask)?;
                self.isa_codegen.emit_pdep(&mut buf.code, dreg.0, sreg.0, mreg.0);
            }
            
            IrOp::Pext(src, mask) => {
                // ISA-aware PEXT: uses BMI2 if available, else software loop
                let dreg = self.get_reg(dst)?;
                let sreg = self.get_reg(*src)?;
                let mreg = self.get_reg(*mask)?;
                self.isa_codegen.emit_pext(&mut buf.code, dreg.0, sreg.0, mreg.0);
            }
            
            IrOp::Fma(a, b, c) => {
                // ISA-aware FMA: uses VFMADD if available, else mul+add
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                let creg = self.get_reg(*c)?;
                self.emit_fma(buf, dreg, areg, breg, creg);
            }
            
            IrOp::Aesenc(state, key) => {
                // AES-NI AESENC instruction
                let dreg = self.get_reg(dst)?;
                let sreg = self.get_reg(*state)?;
                let kreg = self.get_reg(*key)?;
                self.emit_aesenc(buf, dreg, sreg, kreg);
            }
            
            IrOp::Aesdec(state, key) => {
                // AES-NI AESDEC instruction
                let dreg = self.get_reg(dst)?;
                let sreg = self.get_reg(*state)?;
                let kreg = self.get_reg(*key)?;
                self.emit_aesdec(buf, dreg, sreg, kreg);
            }
            
            IrOp::Pclmul(a, b, imm) => {
                // PCLMULQDQ instruction for carryless multiply
                let dreg = self.get_reg(dst)?;
                let areg = self.get_reg(*a)?;
                let breg = self.get_reg(*b)?;
                self.emit_pclmul(buf, dreg, areg, breg, *imm);
            }
            
            IrOp::VectorOp { kind, width, src1, src2 } => {
                // ISA-aware vector ops: uses best available SIMD (AVX-512/AVX/SSE)
                let dreg = self.get_reg(dst)?;
                let s1reg = self.get_reg(*src1)?;
                let s2reg = self.get_reg(*src2)?;
                self.emit_vector_op(buf, *kind, *width, dreg, s1reg, s2reg);
            }
        }
        
        Ok(())
    }
    
    fn get_reg(&self, vreg: VReg) -> JitResult<HostReg> {
        self.reg_alloc.get(&vreg)
            .copied()
            .ok_or(JitError::UnallocatedRegister)
    }
    
    // ========================================================================
    // Instruction emission
    // ========================================================================
    
    fn emit_rex(&self, buf: &mut CodeBuffer, w: bool, r: HostReg, b: HostReg) {
        let rex = 0x40 
            | ((w as u8) << 3)
            | ((r.is_extended() as u8) << 2)
            | (b.is_extended() as u8);
        if rex != 0x40 || w {
            buf.emit(rex);
        }
    }
    
    fn emit_rex_w(&self, buf: &mut CodeBuffer, r: HostReg, b: HostReg) {
        self.emit_rex(buf, true, r, b);
    }
    
    fn emit_modrm(&self, buf: &mut CodeBuffer, mode: u8, reg: HostReg, rm: HostReg) {
        buf.emit((mode << 6) | (reg.low3() << 3) | rm.low3());
    }
    
    fn emit_push(&self, buf: &mut CodeBuffer, reg: HostReg) {
        if reg.is_extended() {
            buf.emit(0x41);
        }
        buf.emit(0x50 + reg.low3());
    }
    
    fn emit_pop(&self, buf: &mut CodeBuffer, reg: HostReg) {
        if reg.is_extended() {
            buf.emit(0x41);
        }
        buf.emit(0x58 + reg.low3());
    }
    
    fn emit_mov_imm64(&self, buf: &mut CodeBuffer, dst: HostReg, val: u64) {
        self.emit_rex_w(buf, HostReg::RAX, dst);
        buf.emit(0xB8 + dst.low3());
        buf.emit_u64(val);
    }
    
    fn emit_mov_reg_reg(&self, buf: &mut CodeBuffer, dst: HostReg, src: HostReg) {
        self.emit_rex_w(buf, src, dst);
        buf.emit(0x89);
        self.emit_modrm(buf, 3, src, dst);
    }
    
    fn emit_alu_reg_reg(&self, buf: &mut CodeBuffer, op: AluOp, dst: HostReg, src: HostReg) {
        let opcode = match op {
            AluOp::Add => 0x01,
            AluOp::Sub => 0x29,
            AluOp::And => 0x21,
            AluOp::Or => 0x09,
            AluOp::Xor => 0x31,
        };
        
        self.emit_rex_w(buf, src, dst);
        buf.emit(opcode);
        self.emit_modrm(buf, 3, src, dst);
    }
    
    fn emit_imul_reg_reg(&self, buf: &mut CodeBuffer, dst: HostReg, src: HostReg) {
        self.emit_rex_w(buf, dst, src);
        buf.emit_bytes(&[0x0F, 0xAF]);
        self.emit_modrm(buf, 3, dst, src);
    }
    
    fn emit_shift(&self, buf: &mut CodeBuffer, op: ShiftOp, dst: HostReg) {
        let ext = match op {
            ShiftOp::Shl => 4,
            ShiftOp::Shr => 5,
            ShiftOp::Sar => 7,
        };
        
        self.emit_rex_w(buf, HostReg::RAX, dst);
        buf.emit(0xD3);
        self.emit_modrm(buf, 3, HostReg(ext), dst);
    }
    
    fn emit_neg(&self, buf: &mut CodeBuffer, dst: HostReg) {
        self.emit_rex_w(buf, HostReg::RAX, dst);
        buf.emit(0xF7);
        self.emit_modrm(buf, 3, HostReg(3), dst);
    }
    
    fn emit_not(&self, buf: &mut CodeBuffer, dst: HostReg) {
        self.emit_rex_w(buf, HostReg::RAX, dst);
        buf.emit(0xF7);
        self.emit_modrm(buf, 3, HostReg(2), dst);
    }
    
    fn emit_load(&self, buf: &mut CodeBuffer, dst: HostReg, addr: HostReg, size: u8) {
        match size {
            1 => {
                // movzx r64, byte [addr]
                self.emit_rex_w(buf, dst, addr);
                buf.emit_bytes(&[0x0F, 0xB6]);
                self.emit_modrm(buf, 0, dst, addr);
            }
            2 => {
                // movzx r64, word [addr]
                self.emit_rex_w(buf, dst, addr);
                buf.emit_bytes(&[0x0F, 0xB7]);
                self.emit_modrm(buf, 0, dst, addr);
            }
            4 => {
                // mov r32, [addr] (zero-extends to 64)
                self.emit_rex(buf, false, dst, addr);
                buf.emit(0x8B);
                self.emit_modrm(buf, 0, dst, addr);
            }
            8 => {
                // mov r64, [addr]
                self.emit_rex_w(buf, dst, addr);
                buf.emit(0x8B);
                self.emit_modrm(buf, 0, dst, addr);
            }
            _ => {}
        }
    }
    
    fn emit_store(&self, buf: &mut CodeBuffer, addr: HostReg, val: HostReg, size: u8) {
        match size {
            1 => {
                // mov byte [addr], r8
                if val.0 >= 4 {
                    self.emit_rex(buf, false, val, addr);
                }
                buf.emit(0x88);
                self.emit_modrm(buf, 0, val, addr);
            }
            2 => {
                // mov word [addr], r16
                buf.emit(0x66);
                self.emit_rex(buf, false, val, addr);
                buf.emit(0x89);
                self.emit_modrm(buf, 0, val, addr);
            }
            4 => {
                // mov dword [addr], r32
                self.emit_rex(buf, false, val, addr);
                buf.emit(0x89);
                self.emit_modrm(buf, 0, val, addr);
            }
            8 => {
                // mov qword [addr], r64
                self.emit_rex_w(buf, val, addr);
                buf.emit(0x89);
                self.emit_modrm(buf, 0, val, addr);
            }
            _ => {}
        }
    }
    
    fn emit_cmp_reg_reg(&self, buf: &mut CodeBuffer, a: HostReg, b: HostReg) {
        self.emit_rex_w(buf, b, a);
        buf.emit(0x39);
        self.emit_modrm(buf, 3, b, a);
    }
    
    fn emit_test_reg_reg(&self, buf: &mut CodeBuffer, a: HostReg, b: HostReg) {
        self.emit_rex_w(buf, b, a);
        buf.emit(0x85);
        self.emit_modrm(buf, 3, b, a);
    }
    
    fn emit_xor_reg_reg(&self, buf: &mut CodeBuffer, a: HostReg, b: HostReg) {
        self.emit_rex_w(buf, b, a);
        buf.emit(0x31);
        self.emit_modrm(buf, 3, b, a);
    }
    
    fn emit_call_reg(&self, buf: &mut CodeBuffer, target: HostReg) {
        if target.is_extended() {
            buf.emit(0x41);
        }
        buf.emit(0xFF);
        self.emit_modrm(buf, 3, HostReg(2), target);
    }
    
    fn emit_jmp_reg(&self, buf: &mut CodeBuffer, target: HostReg) {
        if target.is_extended() {
            buf.emit(0x41);
        }
        buf.emit(0xFF);
        self.emit_modrm(buf, 3, HostReg(4), target);
    }
    
    // ========================================================================
    // ISA-Specific Instruction Emission Helpers
    // ========================================================================
    
    /// Emit BEXTR instruction using VEX encoding (BMI1)
    fn emit_bextr_vex(&self, buf: &mut CodeBuffer, dst: HostReg, src: HostReg, ctrl: HostReg) {
        // VEX.NDS.LZ.0F38.W1 F7 /r
        let vex_r = if dst.0 < 8 { 0x80 } else { 0 };
        let vex_x = 0x40; // No index register
        let vex_b = if src.0 < 8 { 0x20 } else { 0 };
        let vex_w = 0x80; // 64-bit operand
        let vex_vvvv = (!(ctrl.0) & 0x0F) << 3;
        
        buf.emit(0xC4);
        buf.emit(vex_r | vex_x | vex_b | 0x02); // 0F38 map
        buf.emit(vex_w | vex_vvvv);
        buf.emit(0xF7);
        self.emit_modrm(buf, 3, dst, src);
    }
    
    /// Emit software BEXTR fallback (without BMI1)
    fn emit_bextr_software(&self, buf: &mut CodeBuffer, dst: HostReg, src: HostReg, start: HostReg, len: HostReg) {
        // dst = (src >> start) & ((1 << len) - 1)
        // Use R10/R11 as temps (caller-saved)
        
        // mov dst, src
        self.emit_mov_reg_reg(buf, dst, src);
        
        // mov cl, start_low8 (shift amount must be in CL)
        if start != HostReg::RCX {
            self.emit_mov_reg_reg(buf, HostReg::RCX, start);
        }
        
        // shr dst, cl
        self.emit_rex_w(buf, HostReg::RAX, dst);
        buf.emit(0xD3);
        self.emit_modrm(buf, 3, HostReg(5), dst); // /5 = SHR
        
        // Build mask: r10 = (1 << len) - 1
        // mov r10, 1
        buf.emit_bytes(&[0x49, 0xC7, 0xC2, 0x01, 0x00, 0x00, 0x00]);
        // mov cl, len_low8
        if len != HostReg::RCX {
            self.emit_mov_reg_reg(buf, HostReg::RCX, len);
        }
        // shl r10, cl
        buf.emit_bytes(&[0x49, 0xD3, 0xE2]);
        // dec r10 (r10 = (1 << len) - 1)
        buf.emit_bytes(&[0x49, 0xFF, 0xCA]);
        
        // and dst, r10
        self.emit_rex_w(buf, HostReg::R10, dst);
        buf.emit(0x21);
        self.emit_modrm(buf, 3, HostReg::R10, dst);
    }
    
    /// Emit FMA operation (fused multiply-add: dst = a * b + c)
    fn emit_fma(&self, buf: &mut CodeBuffer, dst: HostReg, a: HostReg, b: HostReg, c: HostReg) {
        if self.isa_config.has_feature(InstructionSets::FMA) {
            // VFMADD213SD xmm1, xmm2, xmm3 for scalar double
            // VEX.DDS.LIG.66.0F38.W1 A9 /r
            // For now, assume XMM registers map from GPR indices
            let vex_r = if dst.0 < 8 { 0x80 } else { 0 };
            let vex_x = 0x40;
            let vex_b = if c.0 < 8 { 0x20 } else { 0 };
            let vex_w = 0x80;
            let vex_vvvv = (!(a.0) & 0x0F) << 3;
            
            buf.emit(0xC4);
            buf.emit(vex_r | vex_x | vex_b | 0x02);
            buf.emit(vex_w | vex_vvvv | 0x01); // pp=01 (66)
            buf.emit(0xA9);
            self.emit_modrm(buf, 3, dst, c);
        } else {
            // Software fallback: mul then add
            // This is a placeholder - real FP ops need XMM handling
            // mulsd xmm_dst, xmm_b; addsd xmm_dst, xmm_c
            // For now, emit NOPs as this needs XMM register allocation
            buf.emit_bytes(&[0x90, 0x90, 0x90]);
            let _ = (dst, a, b, c);
        }
    }
    
    /// Emit AES encryption round (AESENC)
    fn emit_aesenc(&self, buf: &mut CodeBuffer, dst: HostReg, state: HostReg, key: HostReg) {
        if self.isa_config.has_feature(InstructionSets::AESNI) {
            // AESENC xmm1, xmm2: 66 0F 38 DC /r
            buf.emit(0x66);
            self.emit_rex(buf, false, dst, key);
            buf.emit_bytes(&[0x0F, 0x38, 0xDC]);
            self.emit_modrm(buf, 3, dst, key);
            let _ = state; // state should be in dst before this op
        } else {
            // Software AES - would be a function call to software implementation
            // This is extremely complex to inline, so emit a call stub
            buf.emit_bytes(&[0x90, 0x90, 0x90]);
            let _ = (dst, state, key);
        }
    }
    
    /// Emit AES decryption round (AESDEC)
    fn emit_aesdec(&self, buf: &mut CodeBuffer, dst: HostReg, state: HostReg, key: HostReg) {
        if self.isa_config.has_feature(InstructionSets::AESNI) {
            // AESDEC xmm1, xmm2: 66 0F 38 DE /r
            buf.emit(0x66);
            self.emit_rex(buf, false, dst, key);
            buf.emit_bytes(&[0x0F, 0x38, 0xDE]);
            self.emit_modrm(buf, 3, dst, key);
            let _ = state;
        } else {
            buf.emit_bytes(&[0x90, 0x90, 0x90]);
            let _ = (dst, state, key);
        }
    }
    
    /// Emit PCLMULQDQ (carryless multiply)
    fn emit_pclmul(&self, buf: &mut CodeBuffer, dst: HostReg, a: HostReg, b: HostReg, imm: u8) {
        if self.isa_config.has_feature(InstructionSets::PCLMUL) {
            // PCLMULQDQ xmm1, xmm2, imm8: 66 0F 3A 44 /r ib
            buf.emit(0x66);
            self.emit_rex(buf, false, dst, b);
            buf.emit_bytes(&[0x0F, 0x3A, 0x44]);
            self.emit_modrm(buf, 3, dst, b);
            buf.emit(imm);
            let _ = a; // a should be in dst
        } else {
            // Software carryless multiply - complex, emit stub
            buf.emit_bytes(&[0x90, 0x90, 0x90]);
            let _ = (dst, a, b, imm);
        }
    }
    
    /// Emit vector operation with appropriate SIMD width
    fn emit_vector_op(&self, buf: &mut CodeBuffer, kind: super::ir::VectorOpKind, width: u16, 
                      dst: HostReg, src1: HostReg, src2: HostReg) {
        use super::ir::VectorOpKind;
        
        // Select SIMD width based on requested width and available ISA
        let actual_width = if width >= 512 && self.isa_config.has_feature(InstructionSets::AVX512F) {
            512
        } else if width >= 256 && self.isa_config.has_feature(InstructionSets::AVX) {
            256
        } else {
            128
        };
        
        // For widths larger than available, we need to emit multiple ops
        let ops_needed = (width as usize + actual_width as usize - 1) / actual_width as usize;
        
        match kind {
            VectorOpKind::Add => {
                if actual_width == 256 && self.isa_config.has_feature(InstructionSets::AVX2) {
                    // VPADDQ ymm, ymm, ymm: VEX.256.66.0F.WIG D4 /r
                    self.emit_vex_256(buf, 0xD4, dst, src1, src2);
                } else {
                    // PADDQ xmm, xmm: 66 0F D4 /r
                    buf.emit(0x66);
                    self.emit_rex(buf, false, dst, src2);
                    buf.emit_bytes(&[0x0F, 0xD4]);
                    self.emit_modrm(buf, 3, dst, src2);
                }
            }
            VectorOpKind::Sub => {
                if actual_width == 256 && self.isa_config.has_feature(InstructionSets::AVX2) {
                    // VPSUBQ ymm: VEX.256.66.0F.WIG FB /r
                    self.emit_vex_256(buf, 0xFB, dst, src1, src2);
                } else {
                    // PSUBQ xmm: 66 0F FB /r
                    buf.emit(0x66);
                    self.emit_rex(buf, false, dst, src2);
                    buf.emit_bytes(&[0x0F, 0xFB]);
                    self.emit_modrm(buf, 3, dst, src2);
                }
            }
            VectorOpKind::Mul => {
                // PMULLQ only in AVX-512, use PMULUDQ for now
                buf.emit(0x66);
                self.emit_rex(buf, false, dst, src2);
                buf.emit_bytes(&[0x0F, 0xF4]); // PMULUDQ
                self.emit_modrm(buf, 3, dst, src2);
            }
            VectorOpKind::And => {
                if actual_width == 256 && self.isa_config.has_feature(InstructionSets::AVX) {
                    // VPAND ymm: VEX.256.66.0F.WIG DB /r
                    self.emit_vex_256(buf, 0xDB, dst, src1, src2);
                } else {
                    // PAND xmm: 66 0F DB /r
                    buf.emit(0x66);
                    self.emit_rex(buf, false, dst, src2);
                    buf.emit_bytes(&[0x0F, 0xDB]);
                    self.emit_modrm(buf, 3, dst, src2);
                }
            }
            VectorOpKind::Or => {
                if actual_width == 256 && self.isa_config.has_feature(InstructionSets::AVX) {
                    // VPOR ymm: VEX.256.66.0F.WIG EB /r
                    self.emit_vex_256(buf, 0xEB, dst, src1, src2);
                } else {
                    // POR xmm: 66 0F EB /r
                    buf.emit(0x66);
                    self.emit_rex(buf, false, dst, src2);
                    buf.emit_bytes(&[0x0F, 0xEB]);
                    self.emit_modrm(buf, 3, dst, src2);
                }
            }
            VectorOpKind::Xor => {
                if actual_width == 256 && self.isa_config.has_feature(InstructionSets::AVX) {
                    // VPXOR ymm: VEX.256.66.0F.WIG EF /r
                    self.emit_vex_256(buf, 0xEF, dst, src1, src2);
                } else {
                    // PXOR xmm: 66 0F EF /r
                    buf.emit(0x66);
                    self.emit_rex(buf, false, dst, src2);
                    buf.emit_bytes(&[0x0F, 0xEF]);
                    self.emit_modrm(buf, 3, dst, src2);
                }
            }
            VectorOpKind::Shuffle | VectorOpKind::Div | VectorOpKind::Min | VectorOpKind::Max => {
                // Complex vector ops - placeholder
                buf.emit(0x90);
            }
        }
        
        let _ = (ops_needed, src1); // For future multi-op emission
    }
    
    /// Emit VEX-encoded 256-bit instruction
    fn emit_vex_256(&self, buf: &mut CodeBuffer, opcode: u8, dst: HostReg, src1: HostReg, src2: HostReg) {
        // 3-byte VEX prefix for 256-bit operations
        let vex_r = if dst.0 < 8 { 0x80 } else { 0 };
        let vex_x = 0x40;
        let vex_b = if src2.0 < 8 { 0x20 } else { 0 };
        let vex_vvvv = (!(src1.0) & 0x0F) << 3;
        let vex_l = 0x04; // 256-bit
        let vex_pp = 0x01; // 66 prefix
        
        buf.emit(0xC4);
        buf.emit(vex_r | vex_x | vex_b | 0x01); // map = 0F
        buf.emit(vex_vvvv | vex_l | vex_pp);
        buf.emit(opcode);
        self.emit_modrm(buf, 3, dst, src2);
    }
}

impl Default for CodeGen {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum AluOp { Add, Sub, And, Or, Xor }

#[derive(Clone, Copy)]
enum ShiftOp { Shl, Shr, Sar }

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_code_buffer() {
        let mut buf = CodeBuffer::new();
        buf.emit(0x90);
        buf.emit_u32(0x12345678);
        assert_eq!(buf.len(), 5);
    }
    
    #[test]
    fn test_rex_encoding() {
        let gen = CodeGen::new();
        let mut buf = CodeBuffer::new();
        
        // mov rax, rbx (no extension needed)
        gen.emit_mov_reg_reg(&mut buf, HostReg::RAX, HostReg::RBX);
        assert_eq!(buf.code[0], 0x48); // REX.W
        
        // mov r8, r9 (both extended)
        let mut buf2 = CodeBuffer::new();
        gen.emit_mov_reg_reg(&mut buf2, HostReg::R8, HostReg::R9);
        assert_eq!(buf2.code[0] & 0x45, 0x45); // REX.W + REX.R + REX.B
    }
    
    #[test]
    fn test_isa_config_initialization() {
        let gen = CodeGen::new();
        // Should detect current CPU's ISA features
        let config = gen.isa_config();
        // At minimum, SSE2 should be available on any x86-64 CPU
        assert!(config.target_isa.contains(InstructionSets::SSE2));
    }
    
    #[test]
    fn test_codegen_with_custom_isa() {
        // Test creating CodeGen with baseline-only ISA
        let config = IsaCodegenConfig::with_target(InstructionSets::SSE2);
        let gen = CodeGen::with_isa(config);
        
        assert!(!gen.has_feature(InstructionSets::POPCNT));
        assert!(!gen.has_feature(InstructionSets::BMI1));
        assert!(!gen.has_feature(InstructionSets::AVX));
    }
    
    #[test]
    fn test_codegen_with_full_isa() {
        // Test creating CodeGen with all ISA features
        let config = IsaCodegenConfig::with_target(
            InstructionSets::SSE2 | InstructionSets::POPCNT | InstructionSets::BMI1 | 
            InstructionSets::BMI2 | InstructionSets::AVX | InstructionSets::AVX2
        );
        let gen = CodeGen::with_isa(config);
        
        assert!(gen.has_feature(InstructionSets::POPCNT));
        assert!(gen.has_feature(InstructionSets::BMI1));
        assert!(gen.has_feature(InstructionSets::AVX));
    }
    
    #[test]
    fn test_vex_256_encoding() {
        let config = IsaCodegenConfig::with_target(
            InstructionSets::SSE2 | InstructionSets::AVX | InstructionSets::AVX2
        );
        let gen = CodeGen::with_isa(config);
        let mut buf = CodeBuffer::new();
        
        // Test VEX 256-bit instruction encoding
        gen.emit_vex_256(&mut buf, 0xD4, HostReg::RAX, HostReg::RCX, HostReg::RDX);
        
        // Should start with VEX prefix
        assert_eq!(buf.code[0], 0xC4);
        // Length should be VEX 3-byte + opcode + modrm
        assert_eq!(buf.len(), 5);
    }
    
    #[test]
    fn test_bextr_software_fallback() {
        let config = IsaCodegenConfig::with_target(InstructionSets::SSE2);
        let gen = CodeGen::with_isa(config);
        let mut buf = CodeBuffer::new();
        
        // Emit BEXTR software fallback
        gen.emit_bextr_software(&mut buf, HostReg::RAX, HostReg::RBX, HostReg::RCX, HostReg::RDX);
        
        // Should emit multiple instructions for shift + mask
        assert!(buf.len() > 10);
    }
}
