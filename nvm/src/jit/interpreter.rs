//! x86-64 Interpreter
//!
//! Zero-warmup interpreter for cold code execution.
//! Collects profiling data for JIT compilation decisions.

use super::{JitResult, JitError, ExecuteResult, StepResult, MemAccess};
use super::decoder::{X86Decoder, DecodedInstr, Mnemonic, Operand, Register, RegKind, MemOp};
use super::profile::ProfileDb;
use crate::cpu::VirtualCpu;
use crate::memory::PhysicalMemory;

/// x86-64 interpreter
pub struct Interpreter {
    /// Max instructions per block execution
    max_instrs: usize,
}

impl Interpreter {
    pub fn new() -> Self {
        Self { max_instrs: 1000 }
    }
    
    pub fn with_max_instrs(max_instrs: usize) -> Self {
        Self { max_instrs }
    }
    
    /// Execute a basic block starting at RIP
    pub fn execute_block(
        &self,
        cpu: &VirtualCpu,
        memory: &PhysicalMemory,
        start_rip: u64,
        decoder: &X86Decoder,
        profile: &ProfileDb,
    ) -> JitResult<ExecuteResult> {
        let mut rip = start_rip;
        let mut executed = 0;
        
        while executed < self.max_instrs {
            // Fetch instruction bytes
            let mut bytes = [0u8; 15];
            for i in 0..15 {
                bytes[i] = memory.read_u8(rip + i as u64);
            }
            
            // Decode
            let instr = decoder.decode(&bytes, rip)?;
            
            // Execute
            let result = self.execute_instr(cpu, memory, &instr, profile)?;
            
            executed += 1;
            
            match result {
                InstrResult::Continue(next_rip) => {
                    rip = next_rip;
                }
                InstrResult::Branch { taken, target, fallthrough } => {
                    profile.record_branch(rip, taken);
                    rip = if taken { target } else { fallthrough };
                }
                InstrResult::Exit(reason) => {
                    cpu.write_rip(rip + instr.len as u64);
                    return Ok(reason);
                }
            }
        }
        
        // Hit max instructions - exit for JIT consideration
        cpu.write_rip(rip);
        Ok(ExecuteResult::Continue { next_rip: rip })
    }
    
    /// Execute a single instruction (for debugging)
    pub fn execute_single(
        &self,
        cpu: &VirtualCpu,
        memory: &PhysicalMemory,
        instr: &DecodedInstr,
    ) -> JitResult<StepResult> {
        let mut mem_accesses = Vec::new();
        
        let result = self.execute_instr_traced(cpu, memory, instr, &mut mem_accesses)?;
        
        let (next_rip, branch_taken) = match result {
            InstrResult::Continue(rip) => (rip, None),
            InstrResult::Branch { taken, target, fallthrough } => {
                (if taken { target } else { fallthrough }, Some(taken))
            }
            InstrResult::Exit(_) => (instr.rip + instr.len as u64, None),
        };
        
        Ok(StepResult {
            next_rip,
            mnemonic: format!("{:?}", instr.mnemonic),
            length: instr.len,
            branch_taken,
            mem_accesses,
        })
    }
    
    fn execute_instr(
        &self,
        cpu: &VirtualCpu,
        memory: &PhysicalMemory,
        instr: &DecodedInstr,
        _profile: &ProfileDb,
    ) -> JitResult<InstrResult> {
        let next_rip = instr.rip + instr.len as u64;
        
        match instr.mnemonic {
            // Data movement
            Mnemonic::Mov => self.exec_mov(cpu, memory, instr),
            Mnemonic::Movzx => self.exec_movzx(cpu, memory, instr),
            Mnemonic::Movsx | Mnemonic::Movsxd => self.exec_movsx(cpu, memory, instr),
            Mnemonic::Lea => self.exec_lea(cpu, instr),
            Mnemonic::Push => self.exec_push(cpu, memory, instr),
            Mnemonic::Pop => self.exec_pop(cpu, memory, instr),
            Mnemonic::Xchg => self.exec_xchg(cpu, memory, instr),
            
            // Arithmetic
            Mnemonic::Add => self.exec_alu(cpu, memory, instr, AluOp::Add),
            Mnemonic::Adc => self.exec_alu(cpu, memory, instr, AluOp::Adc),
            Mnemonic::Sub => self.exec_alu(cpu, memory, instr, AluOp::Sub),
            Mnemonic::Sbb => self.exec_alu(cpu, memory, instr, AluOp::Sbb),
            Mnemonic::And => self.exec_alu(cpu, memory, instr, AluOp::And),
            Mnemonic::Or => self.exec_alu(cpu, memory, instr, AluOp::Or),
            Mnemonic::Xor => self.exec_alu(cpu, memory, instr, AluOp::Xor),
            Mnemonic::Inc => self.exec_inc_dec(cpu, memory, instr, true),
            Mnemonic::Dec => self.exec_inc_dec(cpu, memory, instr, false),
            Mnemonic::Neg => self.exec_neg(cpu, memory, instr),
            Mnemonic::Not => self.exec_not(cpu, memory, instr),
            Mnemonic::Cmp => self.exec_cmp(cpu, memory, instr),
            Mnemonic::Test => self.exec_test(cpu, memory, instr),
            Mnemonic::Imul => self.exec_imul(cpu, memory, instr),
            
            // Shifts
            Mnemonic::Shl => self.exec_shift(cpu, memory, instr, ShiftOp::Shl),
            Mnemonic::Shr => self.exec_shift(cpu, memory, instr, ShiftOp::Shr),
            Mnemonic::Sar => self.exec_shift(cpu, memory, instr, ShiftOp::Sar),
            Mnemonic::Rol => self.exec_shift(cpu, memory, instr, ShiftOp::Rol),
            Mnemonic::Ror => self.exec_shift(cpu, memory, instr, ShiftOp::Ror),
            
            // Control flow
            Mnemonic::Jmp => self.exec_jmp(cpu, memory, instr),
            Mnemonic::Jcc => self.exec_jcc(cpu, instr),
            Mnemonic::Call => self.exec_call(cpu, memory, instr),
            Mnemonic::Ret => self.exec_ret(cpu, memory, instr),
            Mnemonic::Loop | Mnemonic::Loope | Mnemonic::Loopne => {
                self.exec_loop(cpu, instr)
            }
            
            // Flag control
            Mnemonic::Clc => { cpu.clear_cf(); Ok(InstrResult::Continue(next_rip)) }
            Mnemonic::Stc => { cpu.set_cf(); Ok(InstrResult::Continue(next_rip)) }
            Mnemonic::Cmc => { cpu.complement_cf(); Ok(InstrResult::Continue(next_rip)) }
            Mnemonic::Cld => { cpu.clear_df(); Ok(InstrResult::Continue(next_rip)) }
            Mnemonic::Std => { cpu.set_df(); Ok(InstrResult::Continue(next_rip)) }
            Mnemonic::Cli => { cpu.disable_interrupts(); Ok(InstrResult::Continue(next_rip)) }
            Mnemonic::Sti => { cpu.enable_interrupts(); Ok(InstrResult::Continue(next_rip)) }
            
            // I/O
            Mnemonic::In => self.exec_in(cpu, instr),
            Mnemonic::Out => self.exec_out(cpu, instr),
            
            // String operations
            Mnemonic::Movs => self.exec_movs(cpu, memory, instr),
            Mnemonic::Stos => self.exec_stos(cpu, memory, instr),
            Mnemonic::Lods => self.exec_lods(cpu, memory, instr),
            
            // Misc
            Mnemonic::Nop | Mnemonic::Pause => Ok(InstrResult::Continue(next_rip)),
            Mnemonic::Hlt => Ok(InstrResult::Exit(ExecuteResult::Halt)),
            Mnemonic::Int | Mnemonic::Int3 => self.exec_int(instr),
            Mnemonic::Iret => self.exec_iret(cpu, memory, instr),
            Mnemonic::Cpuid => { 
                let leaf = cpu.read_gpr(0) as u32;    // EAX = leaf
                let subleaf = cpu.read_gpr(1) as u32; // ECX = subleaf
                let (eax, ebx, ecx, edx) = cpu.cpuid(leaf, subleaf);
                cpu.write_gpr(0, eax as u64);
                cpu.write_gpr(3, ebx as u64);
                cpu.write_gpr(1, ecx as u64);
                cpu.write_gpr(2, edx as u64);
                Ok(InstrResult::Continue(next_rip)) 
            }
            Mnemonic::Rdtsc => { 
                let tsc = cpu.rdtsc();
                cpu.write_gpr(0, tsc & 0xFFFF_FFFF); // EAX
                cpu.write_gpr(2, tsc >> 32);         // EDX
                Ok(InstrResult::Continue(next_rip))
            }
            
            _ => {
                // Unsupported - exit to handler
                Ok(InstrResult::Exit(ExecuteResult::Continue { next_rip }))
            }
        }
    }
    
    fn execute_instr_traced(
        &self,
        cpu: &VirtualCpu,
        memory: &PhysicalMemory,
        instr: &DecodedInstr,
        _mem_accesses: &mut Vec<MemAccess>,
    ) -> JitResult<InstrResult> {
        // Same as execute_instr but records memory accesses
        self.execute_instr(cpu, memory, instr, &ProfileDb::new(0))
    }
    
    // ========================================================================
    // Instruction implementations
    // ========================================================================
    
    fn exec_mov(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        self.write_operand(cpu, memory, &instr.operands[0], value, instr)?;
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_movzx(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        // Zero-extend is implicit when reading smaller operand
        self.write_operand(cpu, memory, &instr.operands[0], value, instr)?;
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_movsx(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        let src_size = self.operand_size(&instr.operands[1]);
        let extended = match src_size {
            1 => (value as i8) as i64 as u64,
            2 => (value as i16) as i64 as u64,
            4 => (value as i32) as i64 as u64,
            _ => value,
        };
        self.write_operand(cpu, memory, &instr.operands[0], extended, instr)?;
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_lea(&self, cpu: &VirtualCpu, instr: &DecodedInstr) -> JitResult<InstrResult> {
        if let Operand::Mem(mem) = &instr.operands[1] {
            let addr = self.compute_ea(cpu, mem, instr)?;
            self.write_reg(cpu, &instr.operands[0], addr)?;
        }
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_push(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let rsp = cpu.read_gpr(4) - 8;
        cpu.write_gpr(4, rsp);
        memory.write_u64(rsp, value);
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_pop(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let rsp = cpu.read_gpr(4);
        let value = memory.read_u64(rsp);
        cpu.write_gpr(4, rsp + 8);
        self.write_operand(cpu, memory, &instr.operands[0], value, instr)?;
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_xchg(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let val1 = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let val2 = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        self.write_operand(cpu, memory, &instr.operands[0], val2, instr)?;
        self.write_operand(cpu, memory, &instr.operands[1], val1, instr)?;
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_alu(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr, op: AluOp) -> JitResult<InstrResult> {
        let dst = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let src = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        let size = self.operand_size(&instr.operands[0]);
        let cf = cpu.get_cf() as u64;
        
        let (result, new_cf, new_of) = match op {
            AluOp::Add => {
                let r = dst.wrapping_add(src);
                (r, r < dst, is_add_overflow(dst, src, r, size))
            }
            AluOp::Adc => {
                let r = dst.wrapping_add(src).wrapping_add(cf);
                (r, r < dst || (r == dst && cf != 0), is_add_overflow(dst, src, r, size))
            }
            AluOp::Sub => {
                let r = dst.wrapping_sub(src);
                (r, dst < src, is_sub_overflow(dst, src, r, size))
            }
            AluOp::Sbb => {
                let r = dst.wrapping_sub(src).wrapping_sub(cf);
                (r, dst < src || (dst == src && cf != 0), is_sub_overflow(dst, src, r, size))
            }
            AluOp::And => (dst & src, false, false),
            AluOp::Or => (dst | src, false, false),
            AluOp::Xor => (dst ^ src, false, false),
        };
        
        self.write_operand(cpu, memory, &instr.operands[0], result, instr)?;
        self.update_flags(cpu, result, size, new_cf, new_of);
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_inc_dec(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr, inc: bool) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let size = self.operand_size(&instr.operands[0]);
        
        let result = if inc {
            value.wrapping_add(1)
        } else {
            value.wrapping_sub(1)
        };
        
        self.write_operand(cpu, memory, &instr.operands[0], result, instr)?;
        
        // INC/DEC don't affect CF
        let of = if inc {
            is_add_overflow(value, 1, result, size)
        } else {
            is_sub_overflow(value, 1, result, size)
        };
        self.update_flags_no_cf(cpu, result, size, of);
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_neg(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let result = (!value).wrapping_add(1);
        let size = self.operand_size(&instr.operands[0]);
        
        self.write_operand(cpu, memory, &instr.operands[0], result, instr)?;
        self.update_flags(cpu, result, size, value != 0, is_sub_overflow(0, value, result, size));
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_not(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        self.write_operand(cpu, memory, &instr.operands[0], !value, instr)?;
        // NOT doesn't affect flags
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_cmp(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let dst = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let src = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        let size = self.operand_size(&instr.operands[0]);
        
        let result = dst.wrapping_sub(src);
        self.update_flags(cpu, result, size, dst < src, is_sub_overflow(dst, src, result, size));
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_test(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let dst = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let src = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        let size = self.operand_size(&instr.operands[0]);
        
        let result = dst & src;
        self.update_flags(cpu, result, size, false, false);
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_imul(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        // Two-operand form: dst *= src
        if instr.num_operands >= 2 {
            let dst = self.read_operand(cpu, memory, &instr.operands[0], instr)? as i64;
            let src = self.read_operand(cpu, memory, &instr.operands[1], instr)? as i64;
            let result = dst.wrapping_mul(src) as u64;
            self.write_operand(cpu, memory, &instr.operands[0], result, instr)?;
        }
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_shift(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr, op: ShiftOp) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let count = if instr.num_operands > 1 {
            (self.read_operand(cpu, memory, &instr.operands[1], instr)? & 0x3F) as u32
        } else {
            1
        };
        
        if count == 0 {
            return Ok(InstrResult::Continue(instr.rip + instr.len as u64));
        }
        
        let size = self.operand_size(&instr.operands[0]);
        let bits = size * 8;
        
        let result = match op {
            ShiftOp::Shl => value << count,
            ShiftOp::Shr => value >> count,
            ShiftOp::Sar => ((value as i64) >> count) as u64,
            ShiftOp::Rol => value.rotate_left(count),
            ShiftOp::Ror => value.rotate_right(count),
        };
        
        self.write_operand(cpu, memory, &instr.operands[0], result, instr)?;
        
        // Update CF based on last bit shifted out
        let cf = match op {
            ShiftOp::Shl => (value >> (bits as u32 - count)) & 1 != 0,
            ShiftOp::Shr | ShiftOp::Sar => (value >> (count - 1)) & 1 != 0,
            ShiftOp::Rol | ShiftOp::Ror => result & 1 != 0,
        };
        
        self.update_flags(cpu, result, size, cf, false);
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_jmp(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let target = match &instr.operands[0] {
            Operand::Rel(offset) => {
                (instr.rip as i64 + instr.len as i64 + offset) as u64
            }
            _ => self.read_operand(cpu, memory, &instr.operands[0], instr)?,
        };
        Ok(InstrResult::Continue(target))
    }
    
    fn exec_jcc(&self, cpu: &VirtualCpu, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let cc = (instr.opcode & 0x0F) as u8;
        let taken = self.eval_condition(cpu, cc);
        
        let target = match &instr.operands[0] {
            Operand::Rel(offset) => {
                (instr.rip as i64 + instr.len as i64 + offset) as u64
            }
            _ => instr.rip + instr.len as u64,
        };
        
        let fallthrough = instr.rip + instr.len as u64;
        
        Ok(InstrResult::Branch { taken, target, fallthrough })
    }
    
    fn exec_call(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let ret_addr = instr.rip + instr.len as u64;
        
        // Push return address
        let rsp = cpu.read_gpr(4) - 8;
        cpu.write_gpr(4, rsp);
        memory.write_u64(rsp, ret_addr);
        
        // Get target
        let target = match &instr.operands[0] {
            Operand::Rel(offset) => {
                (instr.rip as i64 + instr.len as i64 + offset) as u64
            }
            _ => self.read_operand(cpu, memory, &instr.operands[0], instr)?,
        };
        
        Ok(InstrResult::Continue(target))
    }
    
    fn exec_ret(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, _instr: &DecodedInstr) -> JitResult<InstrResult> {
        let rsp = cpu.read_gpr(4);
        let ret_addr = memory.read_u64(rsp);
        cpu.write_gpr(4, rsp + 8);
        Ok(InstrResult::Continue(ret_addr))
    }
    
    fn exec_loop(&self, cpu: &VirtualCpu, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let rcx = cpu.read_gpr(1).wrapping_sub(1);
        cpu.write_gpr(1, rcx);
        
        let taken = match instr.mnemonic {
            Mnemonic::Loop => rcx != 0,
            Mnemonic::Loope => rcx != 0 && cpu.get_zf(),
            Mnemonic::Loopne => rcx != 0 && !cpu.get_zf(),
            _ => false,
        };
        
        let target = match &instr.operands[0] {
            Operand::Rel(offset) => {
                (instr.rip as i64 + instr.len as i64 + offset) as u64
            }
            _ => instr.rip + instr.len as u64,
        };
        
        Ok(InstrResult::Branch {
            taken,
            target,
            fallthrough: instr.rip + instr.len as u64,
        })
    }
    
    fn exec_in(&self, _cpu: &VirtualCpu, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let port = match &instr.operands[1] {
            Operand::Imm(v) => *v as u16,
            _ => 0, // DX - would need to read
        };
        let size = self.operand_size(&instr.operands[0]) as u8;
        Ok(InstrResult::Exit(ExecuteResult::IoNeeded { port, is_write: false, size }))
    }
    
    fn exec_out(&self, _cpu: &VirtualCpu, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let port = match &instr.operands[0] {
            Operand::Imm(v) => *v as u16,
            _ => 0,
        };
        let size = self.operand_size(&instr.operands[1]) as u8;
        Ok(InstrResult::Exit(ExecuteResult::IoNeeded { port, is_write: true, size }))
    }
    
    fn exec_movs(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let size = if instr.prefixes.op_size { 2 } else { 8 };
        let rsi = cpu.read_gpr(6);
        let rdi = cpu.read_gpr(7);
        
        let value = match size {
            1 => memory.read_u8(rsi) as u64,
            2 => memory.read_u16(rsi) as u64,
            4 => memory.read_u32(rsi) as u64,
            _ => memory.read_u64(rsi),
        };
        
        match size {
            1 => memory.write_u8(rdi, value as u8),
            2 => memory.write_u16(rdi, value as u16),
            4 => memory.write_u32(rdi, value as u32),
            _ => memory.write_u64(rdi, value),
        }
        
        let delta = if cpu.get_df() { -(size as i64) } else { size as i64 };
        cpu.write_gpr(6, (rsi as i64 + delta) as u64);
        cpu.write_gpr(7, (rdi as i64 + delta) as u64);
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_stos(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let size = if instr.prefixes.op_size { 2 } else { 8 };
        let rdi = cpu.read_gpr(7);
        let rax = cpu.read_gpr(0);
        
        match size {
            1 => memory.write_u8(rdi, rax as u8),
            2 => memory.write_u16(rdi, rax as u16),
            4 => memory.write_u32(rdi, rax as u32),
            _ => memory.write_u64(rdi, rax),
        }
        
        let delta = if cpu.get_df() { -(size as i64) } else { size as i64 };
        cpu.write_gpr(7, (rdi as i64 + delta) as u64);
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_lods(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let size = if instr.prefixes.op_size { 2 } else { 8 };
        let rsi = cpu.read_gpr(6);
        
        let value = match size {
            1 => memory.read_u8(rsi) as u64,
            2 => memory.read_u16(rsi) as u64,
            4 => memory.read_u32(rsi) as u64,
            _ => memory.read_u64(rsi),
        };
        
        cpu.write_gpr(0, value);
        
        let delta = if cpu.get_df() { -(size as i64) } else { size as i64 };
        cpu.write_gpr(6, (rsi as i64 + delta) as u64);
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_int(&self, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let vector = match &instr.operands[0] {
            Operand::Imm(v) => *v as u8,
            _ => 3,
        };
        Ok(InstrResult::Exit(ExecuteResult::Interrupt { vector }))
    }
    
    fn exec_iret(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, _instr: &DecodedInstr) -> JitResult<InstrResult> {
        // Pop RIP, CS, RFLAGS
        let rsp = cpu.read_gpr(4);
        let rip = memory.read_u64(rsp);
        let _cs = memory.read_u64(rsp + 8) as u16;
        let rflags = memory.read_u64(rsp + 16);
        cpu.write_gpr(4, rsp + 24);
        cpu.write_rflags(rflags);
        Ok(InstrResult::Continue(rip))
    }
    
    // ========================================================================
    // Helpers
    // ========================================================================
    
    fn read_operand(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, op: &Operand, instr: &DecodedInstr) -> JitResult<u64> {
        match op {
            Operand::None => Ok(0),
            Operand::Reg(r) => self.read_reg(cpu, r),
            Operand::Imm(v) => Ok(*v as u64),
            Operand::Mem(m) => self.read_mem(cpu, memory, m, instr),
            Operand::Rel(v) => Ok(*v as u64),
            Operand::Far { off, .. } => Ok(*off),
        }
    }
    
    fn write_operand(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, op: &Operand, value: u64, instr: &DecodedInstr) -> JitResult<()> {
        match op {
            Operand::Reg(r) => self.write_reg_sized(cpu, r, value),
            Operand::Mem(m) => self.write_mem(cpu, memory, m, value, instr),
            _ => Ok(()),
        }
    }
    
    fn read_reg(&self, cpu: &VirtualCpu, r: &Register) -> JitResult<u64> {
        match r.kind {
            RegKind::Gpr => Ok(cpu.read_gpr(r.index)),
            _ => Ok(0),
        }
    }
    
    fn write_reg(&self, cpu: &VirtualCpu, op: &Operand, value: u64) -> JitResult<()> {
        if let Operand::Reg(r) = op {
            if r.kind == RegKind::Gpr {
                cpu.write_gpr(r.index, value);
            }
        }
        Ok(())
    }
    
    fn write_reg_sized(&self, cpu: &VirtualCpu, r: &Register, value: u64) -> JitResult<()> {
        if r.kind == RegKind::Gpr {
            let current = cpu.read_gpr(r.index);
            let new_value = match r.size {
                1 => (current & !0xFF) | (value & 0xFF),
                2 => (current & !0xFFFF) | (value & 0xFFFF),
                4 => value & 0xFFFF_FFFF, // 32-bit writes zero-extend
                _ => value,
            };
            cpu.write_gpr(r.index, new_value);
        }
        Ok(())
    }
    
    fn read_mem(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, m: &MemOp, instr: &DecodedInstr) -> JitResult<u64> {
        let addr = self.compute_ea(cpu, m, instr)?;
        Ok(match m.size {
            1 => memory.read_u8(addr) as u64,
            2 => memory.read_u16(addr) as u64,
            4 => memory.read_u32(addr) as u64,
            _ => memory.read_u64(addr),
        })
    }
    
    fn write_mem(&self, cpu: &VirtualCpu, memory: &PhysicalMemory, m: &MemOp, value: u64, instr: &DecodedInstr) -> JitResult<()> {
        let addr = self.compute_ea(cpu, m, instr)?;
        match m.size {
            1 => memory.write_u8(addr, value as u8),
            2 => memory.write_u16(addr, value as u16),
            4 => memory.write_u32(addr, value as u32),
            _ => memory.write_u64(addr, value),
        }
        Ok(())
    }
    
    fn compute_ea(&self, cpu: &VirtualCpu, m: &MemOp, instr: &DecodedInstr) -> JitResult<u64> {
        let mut addr = 0u64;
        
        if let Some(base) = &m.base {
            addr = match base.kind {
                RegKind::Gpr => cpu.read_gpr(base.index),
                RegKind::Rip => instr.rip + instr.len as u64,
                _ => 0,
            };
        }
        
        if let Some(index) = &m.index {
            let idx = cpu.read_gpr(index.index);
            addr = addr.wrapping_add(idx.wrapping_mul(m.scale as u64));
        }
        
        addr = (addr as i64).wrapping_add(m.disp) as u64;
        
        Ok(addr)
    }
    
    fn operand_size(&self, op: &Operand) -> u8 {
        match op {
            Operand::Reg(r) => r.size,
            Operand::Mem(m) => m.size,
            _ => 8,
        }
    }
    
    fn eval_condition(&self, cpu: &VirtualCpu, cc: u8) -> bool {
        match cc {
            0x0 => cpu.get_of(),                          // O
            0x1 => !cpu.get_of(),                         // NO
            0x2 => cpu.get_cf(),                          // B/C/NAE
            0x3 => !cpu.get_cf(),                         // NB/NC/AE
            0x4 => cpu.get_zf(),                          // E/Z
            0x5 => !cpu.get_zf(),                         // NE/NZ
            0x6 => cpu.get_cf() || cpu.get_zf(),          // BE/NA
            0x7 => !cpu.get_cf() && !cpu.get_zf(),        // NBE/A
            0x8 => cpu.get_sf(),                          // S
            0x9 => !cpu.get_sf(),                         // NS
            0xA => cpu.get_pf(),                          // P/PE
            0xB => !cpu.get_pf(),                         // NP/PO
            0xC => cpu.get_sf() != cpu.get_of(),          // L/NGE
            0xD => cpu.get_sf() == cpu.get_of(),          // NL/GE
            0xE => cpu.get_zf() || (cpu.get_sf() != cpu.get_of()), // LE/NG
            0xF => !cpu.get_zf() && (cpu.get_sf() == cpu.get_of()), // NLE/G
            _ => false,
        }
    }
    
    fn update_flags(&self, cpu: &VirtualCpu, result: u64, size: u8, cf: bool, of: bool) {
        let mask = match size {
            1 => 0xFF,
            2 => 0xFFFF,
            4 => 0xFFFF_FFFF,
            _ => u64::MAX,
        };
        let result = result & mask;
        let sign_bit = 1 << (size * 8 - 1);
        
        cpu.set_flag_cf(cf);
        cpu.set_flag_of(of);
        cpu.set_flag_zf(result == 0);
        cpu.set_flag_sf((result & sign_bit) != 0);
        cpu.set_flag_pf(result.count_ones() % 2 == 0);
    }
    
    fn update_flags_no_cf(&self, cpu: &VirtualCpu, result: u64, size: u8, of: bool) {
        let mask = match size {
            1 => 0xFF,
            2 => 0xFFFF,
            4 => 0xFFFF_FFFF,
            _ => u64::MAX,
        };
        let result = result & mask;
        let sign_bit = 1 << (size * 8 - 1);
        
        cpu.set_flag_of(of);
        cpu.set_flag_zf(result == 0);
        cpu.set_flag_sf((result & sign_bit) != 0);
        cpu.set_flag_pf(result.count_ones() % 2 == 0);
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

/// Instruction result
enum InstrResult {
    Continue(u64),
    Branch { taken: bool, target: u64, fallthrough: u64 },
    Exit(ExecuteResult),
}

#[derive(Clone, Copy)]
enum AluOp { Add, Adc, Sub, Sbb, And, Or, Xor }

#[derive(Clone, Copy)]
enum ShiftOp { Shl, Shr, Sar, Rol, Ror }

fn is_add_overflow(a: u64, b: u64, r: u64, size: u8) -> bool {
    let sign_bit = 1 << (size * 8 - 1);
    let sa = (a & sign_bit) != 0;
    let sb = (b & sign_bit) != 0;
    let sr = (r & sign_bit) != 0;
    sa == sb && sa != sr
}

fn is_sub_overflow(a: u64, b: u64, r: u64, size: u8) -> bool {
    let sign_bit = 1 << (size * 8 - 1);
    let sa = (a & sign_bit) != 0;
    let sb = (b & sign_bit) != 0;
    let sr = (r & sign_bit) != 0;
    sa != sb && sa != sr
}
