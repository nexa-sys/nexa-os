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
        Self {
            reg_alloc: HashMap::new(),
            frame_size: 0,
            spill_slots: HashMap::new(),
            labels: HashMap::new(),
            relocations: Vec::new(),
        }
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
}
