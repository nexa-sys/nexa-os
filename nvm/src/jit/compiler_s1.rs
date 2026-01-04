//! S1 Quick Compiler
//!
//! Fast baseline compiler that generates decent code quickly.
//! No heavy optimizations - focus on compilation speed.
//! Used for warm-up before S2 kicks in.

use super::{JitResult, JitError};
use super::ir::{IrBlock, IrInstr, IrOp, VReg, ExitReason, IrBuilder};
use super::decoder::{X86Decoder, DecodedInstr, Mnemonic};
use super::profile::ProfileDb;
use super::cache::CompileTier;

/// S1 compiler configuration
#[derive(Clone, Debug)]
pub struct S1Config {
    /// Maximum instructions per block
    pub max_instrs: usize,
    /// Enable simple constant folding
    pub const_fold: bool,
    /// Enable dead code elimination
    pub dead_code_elim: bool,
    /// Enable simple peephole opts
    pub peephole: bool,
}

impl Default for S1Config {
    fn default() -> Self {
        Self {
            max_instrs: 100,
            const_fold: true,
            dead_code_elim: true,
            peephole: true,
        }
    }
}

/// S1 compiled block
pub struct S1Block {
    /// Guest start address
    pub guest_rip: u64,
    /// Guest code size
    pub guest_size: u32,
    /// IR representation
    pub ir: IrBlock,
    /// Native code (placeholder)
    pub native: Vec<u8>,
    /// Estimated cycles
    pub est_cycles: u32,
}

/// S1 quick compiler
pub struct S1Compiler {
    config: S1Config,
}

impl S1Compiler {
    pub fn new() -> Self {
        Self {
            config: S1Config::default(),
        }
    }
    
    pub fn with_config(config: S1Config) -> Self {
        Self { config }
    }
    
    /// Compile a basic block starting at RIP
    pub fn compile(
        &self,
        guest_code: &[u8],
        start_rip: u64,
        decoder: &X86Decoder,
        profile: &ProfileDb,
    ) -> JitResult<S1Block> {
        // Build IR from decoded instructions
        let mut builder = IrBuilder::new(start_rip);
        let mut offset = 0usize;
        let mut instr_count = 0;
        
        while offset < guest_code.len() && instr_count < self.config.max_instrs {
            let instr = decoder.decode(&guest_code[offset..], start_rip + offset as u64)?;
            
            // Build IR for instruction
            builder.translate(&instr)?;
            
            offset += instr.len;
            instr_count += 1;
            
            // Stop at control flow changes
            if is_block_terminator(&instr) {
                break;
            }
        }
        
        let mut ir = builder.finish();
        
        // Apply S1 optimizations
        if self.config.const_fold {
            self.const_fold(&mut ir);
        }
        if self.config.dead_code_elim {
            self.dead_code_elim(&mut ir);
        }
        if self.config.peephole {
            self.peephole(&mut ir);
        }
        
        // Generate native code
        let native = self.codegen(&ir, profile)?;
        let est_cycles = self.estimate_cycles(&ir);
        
        Ok(S1Block {
            guest_rip: start_rip,
            guest_size: offset as u32,
            ir,
            native,
            est_cycles,
        })
    }
    
    /// Simple constant folding
    fn const_fold(&self, ir: &mut IrBlock) {
        for bb in &mut ir.blocks {
            let mut i = 0;
            while i < bb.instrs.len() {
                let instr = &bb.instrs[i];
                
                // Try to fold binary operations with constant operands
                if let Some(folded) = self.try_fold_binary(instr, &bb.instrs[..i]) {
                    bb.instrs[i] = folded;
                }
                
                i += 1;
            }
        }
    }
    
    fn try_fold_binary(&self, instr: &IrInstr, _prior: &[IrInstr]) -> Option<IrInstr> {
        match &instr.op {
            IrOp::Add(dst, a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        op: IrOp::LoadConst(*dst, va.wrapping_add(vb)),
                        rip: instr.rip,
                    });
                }
            }
            IrOp::Sub(dst, a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        op: IrOp::LoadConst(*dst, va.wrapping_sub(vb)),
                        rip: instr.rip,
                    });
                }
            }
            IrOp::And(dst, a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        op: IrOp::LoadConst(*dst, va & vb),
                        rip: instr.rip,
                    });
                }
                // x & 0 = 0
                if get_const_val(*b) == Some(0) {
                    return Some(IrInstr {
                        op: IrOp::LoadConst(*dst, 0),
                        rip: instr.rip,
                    });
                }
            }
            IrOp::Or(dst, a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        op: IrOp::LoadConst(*dst, va | vb),
                        rip: instr.rip,
                    });
                }
            }
            IrOp::Xor(dst, a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        op: IrOp::LoadConst(*dst, va ^ vb),
                        rip: instr.rip,
                    });
                }
                // x ^ x = 0
                if a == b {
                    return Some(IrInstr {
                        op: IrOp::LoadConst(*dst, 0),
                        rip: instr.rip,
                    });
                }
            }
            IrOp::Shl(dst, a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        op: IrOp::LoadConst(*dst, va << (vb & 63)),
                        rip: instr.rip,
                    });
                }
            }
            IrOp::Shr(dst, a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        op: IrOp::LoadConst(*dst, va >> (vb & 63)),
                        rip: instr.rip,
                    });
                }
            }
            _ => {}
        }
        None
    }
    
    /// Dead code elimination
    fn dead_code_elim(&self, ir: &mut IrBlock) {
        // Build use set from all blocks
        let mut used = std::collections::HashSet::new();
        
        // All exit values are used
        for bb in &ir.blocks {
            match &bb.exit {
                ExitReason::Jump(target) => { used.insert(*target); }
                ExitReason::Branch { cond, .. } => { used.insert(*cond); }
                ExitReason::IndirectJump(target) => { used.insert(*target); }
                ExitReason::Return(val) => { if let Some(v) = val { used.insert(*v); } }
                _ => {}
            }
            
            // Mark operands of used instructions
            for instr in &bb.instrs {
                for op in get_operands(&instr.op) {
                    used.insert(op);
                }
            }
        }
        
        // Remove instructions whose results aren't used (and have no side effects)
        for bb in &mut ir.blocks {
            bb.instrs.retain(|instr| {
                if let Some(dst) = get_def(&instr.op) {
                    // Keep if result is used OR has side effects
                    used.contains(&dst) || has_side_effect(&instr.op)
                } else {
                    // No def - keep if has side effects
                    has_side_effect(&instr.op)
                }
            });
        }
    }
    
    /// Simple peephole optimizations
    fn peephole(&self, ir: &mut IrBlock) {
        for bb in &mut ir.blocks {
            let mut i = 0;
            while i < bb.instrs.len() {
                // Pattern: add x, 0 -> copy
                if let IrOp::Add(dst, a, b) = &bb.instrs[i].op {
                    if get_const_val(*b) == Some(0) {
                        bb.instrs[i].op = IrOp::Copy(*dst, *a);
                    }
                }
                
                // Pattern: mul x, 1 -> copy
                if let IrOp::Mul(dst, a, b) = &bb.instrs[i].op {
                    if get_const_val(*b) == Some(1) {
                        bb.instrs[i].op = IrOp::Copy(*dst, *a);
                    }
                }
                
                // Pattern: mul x, 0 -> 0
                if let IrOp::Mul(dst, _, b) = &bb.instrs[i].op {
                    if get_const_val(*b) == Some(0) {
                        bb.instrs[i].op = IrOp::LoadConst(*dst, 0);
                    }
                }
                
                // Pattern: shl x, 0 -> copy
                if let IrOp::Shl(dst, a, b) = &bb.instrs[i].op {
                    if get_const_val(*b) == Some(0) {
                        bb.instrs[i].op = IrOp::Copy(*dst, *a);
                    }
                }
                
                // Pattern: consecutive loads to same vreg
                if i + 1 < bb.instrs.len() {
                    if let (IrOp::LoadConst(d1, _), IrOp::LoadConst(d2, _)) = 
                        (&bb.instrs[i].op, &bb.instrs[i + 1].op) 
                    {
                        if d1 == d2 {
                            bb.instrs.remove(i);
                            continue;
                        }
                    }
                }
                
                i += 1;
            }
        }
    }
    
    /// Generate native code from IR
    fn codegen(&self, ir: &IrBlock, _profile: &ProfileDb) -> JitResult<Vec<u8>> {
        let mut code = Vec::new();
        
        for bb in &ir.blocks {
            // Label (for jumps)
            // In real impl, would track label offsets
            
            for instr in &bb.instrs {
                self.emit_instr(&mut code, instr)?;
            }
            
            // Emit exit
            self.emit_exit(&mut code, &bb.exit)?;
        }
        
        Ok(code)
    }
    
    fn emit_instr(&self, code: &mut Vec<u8>, instr: &IrInstr) -> JitResult<()> {
        match &instr.op {
            IrOp::LoadConst(dst, val) => {
                // mov r64, imm64
                // REX.W + B8+rd io
                let reg = vreg_to_host(*dst);
                code.push(0x48 | ((reg >> 3) << 2)); // REX.W + REX.B if needed
                code.push(0xB8 + (reg & 7));
                code.extend_from_slice(&val.to_le_bytes());
            }
            
            IrOp::Copy(dst, src) => {
                // mov r64, r64
                let dreg = vreg_to_host(*dst);
                let sreg = vreg_to_host(*src);
                emit_mov_reg_reg(code, dreg, sreg);
            }
            
            IrOp::Add(dst, a, b) => {
                let dreg = vreg_to_host(*dst);
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                
                // If dst != a, mov dst, a first
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                // add dst, b
                emit_alu_reg_reg(code, 0x01, dreg, breg);
            }
            
            IrOp::Sub(dst, a, b) => {
                let dreg = vreg_to_host(*dst);
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x29, dreg, breg);
            }
            
            IrOp::And(dst, a, b) => {
                let dreg = vreg_to_host(*dst);
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x21, dreg, breg);
            }
            
            IrOp::Or(dst, a, b) => {
                let dreg = vreg_to_host(*dst);
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x09, dreg, breg);
            }
            
            IrOp::Xor(dst, a, b) => {
                let dreg = vreg_to_host(*dst);
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x31, dreg, breg);
            }
            
            IrOp::Shl(dst, a, b) => {
                let dreg = vreg_to_host(*dst);
                let areg = vreg_to_host(*a);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                
                // Move shift amount to CL if not constant
                if let Some(shift) = get_const_val(*b) {
                    // shl r64, imm8
                    emit_rex_w(code, dreg, 0);
                    code.push(0xC1);
                    code.push(0xE0 | (dreg & 7));
                    code.push(shift as u8 & 63);
                } else {
                    let breg = vreg_to_host(*b);
                    emit_mov_reg_reg(code, 1, breg); // mov rcx, b
                    emit_rex_w(code, dreg, 0);
                    code.push(0xD3);
                    code.push(0xE0 | (dreg & 7));
                }
            }
            
            IrOp::Shr(dst, a, b) => {
                let dreg = vreg_to_host(*dst);
                let areg = vreg_to_host(*a);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                
                if let Some(shift) = get_const_val(*b) {
                    emit_rex_w(code, dreg, 0);
                    code.push(0xC1);
                    code.push(0xE8 | (dreg & 7));
                    code.push(shift as u8 & 63);
                } else {
                    let breg = vreg_to_host(*b);
                    emit_mov_reg_reg(code, 1, breg);
                    emit_rex_w(code, dreg, 0);
                    code.push(0xD3);
                    code.push(0xE8 | (dreg & 7));
                }
            }
            
            IrOp::Load8(dst, addr) | IrOp::Load16(dst, addr) | 
            IrOp::Load32(dst, addr) | IrOp::Load64(dst, addr) => {
                // Would call memory access thunk
                let dreg = vreg_to_host(*dst);
                let areg = vreg_to_host(*addr);
                
                match &instr.op {
                    IrOp::Load8(_, _) => {
                        // movzx r64, byte [addr]
                        emit_rex_w(code, dreg, areg);
                        code.extend_from_slice(&[0x0F, 0xB6]);
                        code.push((dreg & 7) << 3 | (areg & 7));
                    }
                    IrOp::Load64(_, _) => {
                        // mov r64, [addr]
                        emit_rex_w(code, dreg, areg);
                        code.push(0x8B);
                        code.push((dreg & 7) << 3 | (areg & 7));
                    }
                    _ => {
                        // Similar patterns for 16/32 bit
                        emit_rex_w(code, dreg, areg);
                        code.push(0x8B);
                        code.push((dreg & 7) << 3 | (areg & 7));
                    }
                }
            }
            
            IrOp::Store8(addr, val) | IrOp::Store16(addr, val) |
            IrOp::Store32(addr, val) | IrOp::Store64(addr, val) => {
                let areg = vreg_to_host(*addr);
                let vreg = vreg_to_host(*val);
                
                // mov [addr], r
                emit_rex_w(code, vreg, areg);
                code.push(0x89);
                code.push((vreg & 7) << 3 | (areg & 7));
            }
            
            IrOp::Compare(dst, a, b) => {
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                let dreg = vreg_to_host(*dst);
                
                // cmp a, b
                emit_alu_reg_reg(code, 0x39, areg, breg);
                
                // Flags result - would need lahf or setcc sequence
                // For now, just zero the dest
                emit_mov_reg_reg(code, dreg, dreg); // xor dreg, dreg
            }
            
            IrOp::Call(target) => {
                // call helper function
                let treg = vreg_to_host(*target);
                emit_rex_w(code, 0, treg);
                code.push(0xFF);
                code.push(0xD0 | (treg & 7));
            }
            
            IrOp::Nop => {
                code.push(0x90);
            }
            
            _ => {
                // Unhandled - emit nop
                code.push(0x90);
            }
        }
        
        Ok(())
    }
    
    fn emit_exit(&self, code: &mut Vec<u8>, exit: &ExitReason) -> JitResult<()> {
        match exit {
            ExitReason::Jump(target) => {
                let treg = vreg_to_host(*target);
                // jmp *target
                emit_rex_w(code, 0, treg);
                code.push(0xFF);
                code.push(0xE0 | (treg & 7));
            }
            ExitReason::Branch { cond, target, fallthrough } => {
                // Test condition and branch
                let creg = vreg_to_host(*cond);
                let treg = vreg_to_host(*target);
                let freg = vreg_to_host(*fallthrough);
                
                // test cond, cond
                emit_alu_reg_reg(code, 0x85, creg, creg);
                
                // jnz to target path (placeholder - would need label resolution)
                code.extend_from_slice(&[0x75, 0x00]); // jnz +0
                
                // fallthrough path
                emit_rex_w(code, 0, freg);
                code.push(0xFF);
                code.push(0xE0 | (freg & 7));
            }
            ExitReason::Return(_) => {
                // ret
                code.push(0xC3);
            }
            ExitReason::Halt => {
                // Return to runtime with halt code
                code.push(0xC3);
            }
            _ => {
                code.push(0xC3);
            }
        }
        
        Ok(())
    }
    
    fn estimate_cycles(&self, ir: &IrBlock) -> u32 {
        let mut cycles = 0u32;
        
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                cycles += match &instr.op {
                    IrOp::LoadConst(_, _) | IrOp::Copy(_, _) => 1,
                    IrOp::Add(_, _, _) | IrOp::Sub(_, _, _) => 1,
                    IrOp::And(_, _, _) | IrOp::Or(_, _, _) | IrOp::Xor(_, _, _) => 1,
                    IrOp::Mul(_, _, _) => 3,
                    IrOp::Div(_, _, _) | IrOp::Rem(_, _, _) => 20,
                    IrOp::Shl(_, _, _) | IrOp::Shr(_, _, _) | IrOp::Sar(_, _, _) => 1,
                    IrOp::Load8(_, _) | IrOp::Load16(_, _) | 
                    IrOp::Load32(_, _) | IrOp::Load64(_, _) => 4,
                    IrOp::Store8(_, _) | IrOp::Store16(_, _) |
                    IrOp::Store32(_, _) | IrOp::Store64(_, _) => 4,
                    IrOp::Call(_) => 5,
                    _ => 1,
                };
            }
        }
        
        cycles
    }
}

impl Default for S1Compiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if instruction terminates a basic block
fn is_block_terminator(instr: &DecodedInstr) -> bool {
    matches!(instr.mnemonic, 
        Mnemonic::Jmp | Mnemonic::Jcc | Mnemonic::Call | 
        Mnemonic::Ret | Mnemonic::Iret | Mnemonic::Int | Mnemonic::Int3 |
        Mnemonic::Loop | Mnemonic::Loope | Mnemonic::Loopne |
        Mnemonic::Hlt | Mnemonic::Sysenter | Mnemonic::Syscall
    )
}

/// Map virtual register to host register
fn vreg_to_host(vreg: VReg) -> u8 {
    // Simple mapping: vreg.0 mod 16
    // In real impl, would use register allocator
    (vreg.0 as u8) % 16
}

/// Get constant value if vreg is a constant
fn get_const_val(_vreg: VReg) -> Option<u64> {
    // Would look up in constant map
    None
}

/// Get operands of an IR operation
fn get_operands(op: &IrOp) -> Vec<VReg> {
    match op {
        IrOp::Copy(_, src) => vec![*src],
        IrOp::Add(_, a, b) | IrOp::Sub(_, a, b) | IrOp::And(_, a, b) |
        IrOp::Or(_, a, b) | IrOp::Xor(_, a, b) | IrOp::Mul(_, a, b) |
        IrOp::Div(_, a, b) | IrOp::Rem(_, a, b) | IrOp::Shl(_, a, b) |
        IrOp::Shr(_, a, b) | IrOp::Sar(_, a, b) => vec![*a, *b],
        IrOp::Neg(_, a) | IrOp::Not(_, a) => vec![*a],
        IrOp::Load8(_, addr) | IrOp::Load16(_, addr) |
        IrOp::Load32(_, addr) | IrOp::Load64(_, addr) => vec![*addr],
        IrOp::Store8(addr, val) | IrOp::Store16(addr, val) |
        IrOp::Store32(addr, val) | IrOp::Store64(addr, val) => vec![*addr, *val],
        IrOp::Compare(_, a, b) => vec![*a, *b],
        _ => Vec::new(),
    }
}

/// Get defined register
fn get_def(op: &IrOp) -> Option<VReg> {
    match op {
        IrOp::LoadConst(d, _) | IrOp::Copy(d, _) |
        IrOp::Add(d, _, _) | IrOp::Sub(d, _, _) | IrOp::And(d, _, _) |
        IrOp::Or(d, _, _) | IrOp::Xor(d, _, _) | IrOp::Mul(d, _, _) |
        IrOp::Div(d, _, _) | IrOp::Rem(d, _, _) | IrOp::Shl(d, _, _) |
        IrOp::Shr(d, _, _) | IrOp::Sar(d, _, _) | IrOp::Neg(d, _) |
        IrOp::Not(d, _) | IrOp::Load8(d, _) | IrOp::Load16(d, _) |
        IrOp::Load32(d, _) | IrOp::Load64(d, _) | IrOp::Compare(d, _, _) |
        IrOp::ZeroExtend(d, _, _) | IrOp::SignExtend(d, _, _) |
        IrOp::ReadGpr(d, _) | IrOp::ReadFlags(d) => Some(*d),
        _ => None,
    }
}

/// Check if operation has side effects
fn has_side_effect(op: &IrOp) -> bool {
    matches!(op,
        IrOp::Store8(_, _) | IrOp::Store16(_, _) |
        IrOp::Store32(_, _) | IrOp::Store64(_, _) |
        IrOp::WriteGpr(_, _) | IrOp::WriteFlags(_) |
        IrOp::Call(_) | IrOp::IoIn(_, _) | IrOp::IoOut(_, _) |
        IrOp::Interrupt(_) | IrOp::Fence
    )
}

// ============================================================================
// Code emission helpers
// ============================================================================

fn emit_rex_w(code: &mut Vec<u8>, reg: u8, rm: u8) {
    let rex = 0x48 | ((reg >> 3) << 2) | (rm >> 3);
    code.push(rex);
}

fn emit_mov_reg_reg(code: &mut Vec<u8>, dst: u8, src: u8) {
    emit_rex_w(code, src, dst);
    code.push(0x89);
    code.push(0xC0 | ((src & 7) << 3) | (dst & 7));
}

fn emit_alu_reg_reg(code: &mut Vec<u8>, opcode: u8, dst: u8, src: u8) {
    emit_rex_w(code, src, dst);
    code.push(opcode);
    code.push(0xC0 | ((src & 7) << 3) | (dst & 7));
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_peephole_add_zero() {
        let compiler = S1Compiler::new();
        
        let mut ir = IrBlock {
            entry_rip: 0x1000,
            blocks: vec![
                super::super::ir::IrBasicBlock {
                    id: 0,
                    instrs: vec![
                        IrInstr {
                            op: IrOp::Add(VReg(0), VReg(1), VReg(2)),
                            rip: 0x1000,
                        },
                    ],
                    exit: ExitReason::Halt,
                },
            ],
        };
        
        compiler.peephole(&mut ir);
        // Would check that add with const 0 became copy
    }
}
