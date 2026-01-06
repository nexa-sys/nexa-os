//! x86-64 Interpreter
//!
//! Zero-warmup interpreter for cold code execution.
//! Collects profiling data for JIT compilation decisions.
//!
//! Stateful architecture: syncs CPU mode at block entry,
//! exits block on mode-changing instructions (MOV CR0, WRMSR to EFER).

use super::{JitResult, JitError, ExecuteResult, StepResult, MemAccess};
use super::decoder::{X86Decoder, DecodedInstr, Mnemonic, Operand, Register, RegKind, MemOp, CpuMode};
use super::profile::ProfileDb;
use crate::cpu::VirtualCpu;
use crate::memory::AddressSpace;

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
    
    /// Get CPU mode from CR0, EFER, and CS.L bit
    fn get_cpu_mode(cpu: &VirtualCpu) -> CpuMode {
        use crate::cpu::SegmentRegister;
        
        let cr0 = cpu.read_cr0();
        let efer = cpu.read_msr(0xC000_0080); // IA32_EFER
        
        let pe = (cr0 & 1) != 0;          // Protected mode enabled
        let pg = (cr0 & 0x8000_0000) != 0; // Paging enabled
        let lma = (efer & 0x400) != 0;    // Long mode active
        
        if lma && pg {
            // In Long Mode, check CS.L bit to distinguish 64-bit vs compatibility mode
            // CS.attrib bit 9 (0x200) is the L (Long) bit in segment descriptor
            let cs_attrib = cpu.read_segment_attrib(SegmentRegister::Cs);
            let cs_l = (cs_attrib & 0x200) != 0;  // L bit = bit 9 of attrib
            
            if cs_l {
                CpuMode::Long  // 64-bit mode
            } else {
                CpuMode::Protected  // Compatibility mode (32-bit under long mode)
            }
        } else if pe {
            CpuMode::Protected
        } else {
            CpuMode::Real
        }
    }
    
    /// Execute a basic block starting at RIP
    pub fn execute_block(
        &self,
        cpu: &VirtualCpu,
        memory: &AddressSpace,
        start_rip: u64,
        _decoder: &X86Decoder,  // Ignored - we determine mode from CPU state
        profile: &ProfileDb,
    ) -> JitResult<ExecuteResult> {
        use crate::cpu::SegmentRegister;
        
        // Record block execution for hot spot detection
        profile.record_block(start_rip);
        
        let mut rip = start_rip;
        let mut executed = 0;
        
        // Determine mode ONCE at block entry (exits on mode change)
        let mode = Self::get_cpu_mode(cpu);
        let cr0 = cpu.read_cr0();
        let efer = cpu.read_msr(0xC000_0080); // EFER
        let cs_attrib = cpu.read_segment_attrib(SegmentRegister::Cs);
        log::trace!("[Interp] Block start at {:#x}, mode={:?}, CR0={:#x}, EFER={:#x}, CS.attrib={:#x}", 
                   start_rip, mode, cr0, efer, cs_attrib);
        let decoder = X86Decoder::with_mode(mode);

        while executed < self.max_instrs {
            // Fetch instruction bytes
            let mut bytes = [0u8; 15];
            for i in 0..15 {
                bytes[i] = memory.read_u8(rip + i as u64);
            }
            
            // Decode with current mode
            let instr = decoder.decode(&bytes, rip)?;
            
            // Debug first few instructions
            if executed < 30 {
                log::trace!("[Interp] RIP={:#x} mnemonic={:?} len={} bytes={:02x?}", 
                          rip, instr.mnemonic, instr.len, &bytes[..instr.len as usize]);
            }
            
            // Execute
            let result = self.execute_instr(cpu, memory, &instr, profile)?;
            
            executed += 1;
            
            match result {
                InstrResult::Continue(next_rip) => {
                    if executed <= 30 {
                        log::trace!("[Interp] Continue to {:#x}", next_rip);
                    }
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
                InstrResult::ModeChanged(next_rip) => {
                    // CPU mode changed - exit block so next iteration re-syncs mode
                    cpu.write_rip(next_rip);
                    log::debug!("[Interp] Mode changed at {:#x}, exiting block", instr.rip);
                    return Ok(ExecuteResult::Continue { next_rip });
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
        memory: &AddressSpace,
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
            InstrResult::ModeChanged(rip) => (rip, None), // Mode change in single-step is just continue
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
        memory: &AddressSpace,
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
            
            // Real mode specific: PUSHA/POPA/PUSHF/POPF
            Mnemonic::Pusha => self.exec_pusha(cpu, memory, instr),
            Mnemonic::Popa => self.exec_popa(cpu, memory, instr),
            Mnemonic::Pushf => self.exec_pushf(cpu, memory, instr),
            Mnemonic::Popf => self.exec_popf(cpu, memory, instr),
            
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
            Mnemonic::Retf => self.exec_retf(cpu, memory, instr),  // FAR RET for real mode
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
            
            // RDMSR: Read MSR[ECX] into EDX:EAX
            Mnemonic::Rdmsr => {
                let msr_addr = cpu.read_gpr(1) as u32; // ECX
                let value = cpu.read_msr(msr_addr);
                cpu.write_gpr(0, value & 0xFFFF_FFFF);  // EAX = low 32 bits
                cpu.write_gpr(2, value >> 32);          // EDX = high 32 bits
                log::trace!("[JIT] RDMSR: MSR[{:#x}] = {:#x}", msr_addr, value);
                Ok(InstrResult::Continue(next_rip))
            }
            
            // WRMSR: Write EDX:EAX to MSR[ECX]
            Mnemonic::Wrmsr => {
                let msr_addr = cpu.read_gpr(1) as u32; // ECX
                let eax = cpu.read_gpr(0) as u32;
                let edx = cpu.read_gpr(2) as u32;
                let value = ((edx as u64) << 32) | (eax as u64);
                
                // Check if writing to EFER (0xC0000080) and LME/LMA bits change
                if msr_addr == 0xC000_0080 {
                    let old_efer = cpu.read_msr(msr_addr);
                    cpu.write_msr(msr_addr, value);
                    let lme_changed = (old_efer & 0x100) != (value & 0x100);  // LME bit
                    let lma_changed = (old_efer & 0x400) != (value & 0x400);  // LMA bit
                    if lme_changed || lma_changed {
                        log::debug!("[JIT] WRMSR EFER: {:#x} -> {:#x}, mode change!", old_efer, value);
                        return Ok(InstrResult::ModeChanged(next_rip));
                    }
                } else {
                    cpu.write_msr(msr_addr, value);
                }
                
                log::trace!("[JIT] WRMSR: MSR[{:#x}] = {:#x}", msr_addr, value);
                Ok(InstrResult::Continue(next_rip))
            }
            
            // System instructions - LGDT/LIDT load descriptor table registers
            Mnemonic::Lgdt | Mnemonic::Lidt => {
                self.exec_lgdt_lidt(cpu, memory, instr)
            }
            
            _ => {
                // Unsupported instruction - log and return error instead of skipping
                log::warn!("[JIT] Unsupported instruction {:?} at RIP={:#x}, len={}", 
                          instr.mnemonic, instr.rip, instr.len);
                Err(JitError::UnsupportedInstruction { 
                    rip: instr.rip, 
                    mnemonic: format!("{:?}", instr.mnemonic) 
                })
            }
        }
    }
    
    fn execute_instr_traced(
        &self,
        cpu: &VirtualCpu,
        memory: &AddressSpace,
        instr: &DecodedInstr,
        _mem_accesses: &mut Vec<MemAccess>,
    ) -> JitResult<InstrResult> {
        // Same as execute_instr but records memory accesses
        self.execute_instr(cpu, memory, instr, &ProfileDb::new(0))
    }
    
    // ========================================================================
    // Instruction implementations
    // ========================================================================
    
    fn exec_mov(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let next_rip = instr.rip + instr.len as u64;
        let value = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        
        // Check if writing to control register that affects CPU mode
        if let Operand::Reg(r) = &instr.operands[0] {
            if r.kind == RegKind::Control {
                match r.index {
                    0 => {
                        // MOV CR0 - check if PE or PG bits change
                        let old_cr0 = cpu.read_cr0();
                        cpu.write_cr0(value);
                        let pe_changed = (old_cr0 & 1) != (value & 1);
                        let pg_changed = (old_cr0 & 0x8000_0000) != (value & 0x8000_0000);
                        if pe_changed || pg_changed {
                            log::debug!("[Interp] MOV CR0: {:#x} -> {:#x}, mode change!", old_cr0, value);
                            return Ok(InstrResult::ModeChanged(next_rip));
                        }
                        return Ok(InstrResult::Continue(next_rip));
                    }
                    3 => {
                        cpu.write_cr3(value);
                        return Ok(InstrResult::Continue(next_rip));
                    }
                    4 => {
                        cpu.write_cr4(value);
                        return Ok(InstrResult::Continue(next_rip));
                    }
                    _ => {}
                }
            }
        }
        
        self.write_operand(cpu, memory, &instr.operands[0], value, instr)?;
        Ok(InstrResult::Continue(next_rip))
    }
    
    fn exec_movzx(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        // Zero-extend is implicit when reading smaller operand
        self.write_operand(cpu, memory, &instr.operands[0], value, instr)?;
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_movsx(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
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
    
    fn exec_push(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let mode = Self::get_cpu_mode(cpu);
        
        match mode {
            CpuMode::Real => {
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = (cpu.read_gpr(4) as u16).wrapping_sub(2);
                memory.write_u16(ss_base + sp as u64, value as u16);
                cpu.write_gpr(4, sp as u64);
            }
            CpuMode::Protected | CpuMode::Compat => {
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = (cpu.read_gpr(4) as u32).wrapping_sub(4);
                memory.write_u32(ss_base + sp as u64, value as u32);
                cpu.write_gpr(4, sp as u64);
            }
            CpuMode::Long => {
                let rsp = cpu.read_gpr(4) - 8;
                cpu.write_gpr(4, rsp);
                memory.write_u64(rsp, value);
            }
        }
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_pop(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let mode = Self::get_cpu_mode(cpu);
        
        let value = match mode {
            CpuMode::Real => {
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = cpu.read_gpr(4) as u16;
                let val = memory.read_u16(ss_base + sp as u64) as u64;
                cpu.write_gpr(4, sp.wrapping_add(2) as u64);
                val
            }
            CpuMode::Protected | CpuMode::Compat => {
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = cpu.read_gpr(4) as u32;
                let val = memory.read_u32(ss_base + sp as u64) as u64;
                cpu.write_gpr(4, sp.wrapping_add(4) as u64);
                val
            }
            CpuMode::Long => {
                let rsp = cpu.read_gpr(4);
                let val = memory.read_u64(rsp);
                cpu.write_gpr(4, rsp + 8);
                val
            }
        };
        
        self.write_operand(cpu, memory, &instr.operands[0], value, instr)?;
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_xchg(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let val1 = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let val2 = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        self.write_operand(cpu, memory, &instr.operands[0], val2, instr)?;
        self.write_operand(cpu, memory, &instr.operands[1], val1, instr)?;
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_alu(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr, op: AluOp) -> JitResult<InstrResult> {
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
    
    fn exec_inc_dec(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr, inc: bool) -> JitResult<InstrResult> {
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
    
    fn exec_neg(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let result = (!value).wrapping_add(1);
        let size = self.operand_size(&instr.operands[0]);
        
        self.write_operand(cpu, memory, &instr.operands[0], result, instr)?;
        self.update_flags(cpu, result, size, value != 0, is_sub_overflow(0, value, result, size));
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_not(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let value = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        self.write_operand(cpu, memory, &instr.operands[0], !value, instr)?;
        // NOT doesn't affect flags
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_cmp(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let dst = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let src = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        let size = self.operand_size(&instr.operands[0]);
        
        let result = dst.wrapping_sub(src);
        self.update_flags(cpu, result, size, dst < src, is_sub_overflow(dst, src, result, size));
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_test(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let dst = self.read_operand(cpu, memory, &instr.operands[0], instr)?;
        let src = self.read_operand(cpu, memory, &instr.operands[1], instr)?;
        let size = self.operand_size(&instr.operands[0]);
        
        let result = dst & src;
        self.update_flags(cpu, result, size, false, false);
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_imul(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        // Two-operand form: dst *= src
        if instr.num_operands >= 2 {
            let dst = self.read_operand(cpu, memory, &instr.operands[0], instr)? as i64;
            let src = self.read_operand(cpu, memory, &instr.operands[1], instr)? as i64;
            let result = dst.wrapping_mul(src) as u64;
            self.write_operand(cpu, memory, &instr.operands[0], result, instr)?;
        }
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_shift(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr, op: ShiftOp) -> JitResult<InstrResult> {
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
    
    fn exec_jmp(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let target = match &instr.operands[0] {
            Operand::Rel(offset) => {
                return Ok(InstrResult::Continue(
                    (instr.rip as i64 + instr.len as i64 + offset) as u64
                ));
            }
            Operand::Far { seg, off } => {
                // FAR JMP: segment:offset
                let old_mode = Self::get_cpu_mode(cpu);
                let linear = match old_mode {
                    CpuMode::Real => {
                        // Real mode: linear = segment * 16 + offset
                        cpu.write_segment_base(crate::cpu::SegmentRegister::Cs, (*seg as u64) * 16);
                        (*seg as u64) * 16 + *off
                    }
                    CpuMode::Protected | CpuMode::Long | CpuMode::Compat => {
                        // Protected/Compatibility mode: segment is a selector
                        // Load the segment descriptor from GDT and compute linear address
                        let selector = *seg;
                        let (gdt_limit, gdt_base) = cpu.get_gdtr();
                        let index = (selector >> 3) as u64;
                        let desc_addr = gdt_base + index * 8;
                        
                        // Check bounds
                        if desc_addr + 8 > gdt_base + gdt_limit as u64 + 1 {
                            log::error!("JMP FAR: GDT index {} out of bounds", index);
                            return Err(JitError::DecodeError { 
                                rip: instr.rip, 
                                bytes: vec![], 
                                reason: format!("GDT index {} out of bounds", index)
                            });
                        }
                        
                        // Read 8-byte descriptor
                        let desc_lo = memory.read_u32(desc_addr) as u64;
                        let desc_hi = memory.read_u32(desc_addr + 4) as u64;
                        
                        // Extract base address from descriptor
                        // base[15:0] = desc[31:16], base[23:16] = desc[39:32], base[31:24] = desc[63:56]
                        let base = ((desc_lo >> 16) & 0xFFFF) | 
                                   ((desc_hi & 0xFF) << 16) |
                                   ((desc_hi >> 24) << 24);
                        
                        // Extract attributes (including L bit)
                        // attrib = desc[55:40] (Type, S, DPL, P, AVL, L, D/B, G)
                        let attrib = ((desc_hi >> 8) & 0xFFFF) as u16;
                        
                        // Update CS with new selector and attributes
                        cpu.write_segment_selector(crate::cpu::SegmentRegister::Cs, selector);
                        cpu.write_segment_base(crate::cpu::SegmentRegister::Cs, base);
                        cpu.write_segment_attrib(crate::cpu::SegmentRegister::Cs, attrib);
                        
                        log::debug!("JMP FAR: sel={:#x}, base={:#x}, attrib={:#x}, off={:#x}, target={:#x}",
                                   selector, base, attrib, *off, base + *off);
                        
                        base + *off
                    }
                };
                
                // Check if mode changed (CS.L bit affects decoder mode)
                let new_mode = Self::get_cpu_mode(cpu);
                if new_mode != old_mode {
                    log::debug!("[Interp] JMP FAR changed mode: {:?} -> {:?}", old_mode, new_mode);
                    return Ok(InstrResult::ModeChanged(linear));
                }
                
                linear
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
    
    fn exec_call(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let ret_addr = instr.rip + instr.len as u64;
        let mode = Self::get_cpu_mode(cpu);
        
        // Push return address based on CPU mode
        match mode {
            CpuMode::Real => {
                // 16-bit: push 16-bit return address
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = (cpu.read_gpr(4) as u16).wrapping_sub(2);
                memory.write_u16(ss_base + sp as u64, ret_addr as u16);
                cpu.write_gpr(4, sp as u64);
            }
            CpuMode::Protected | CpuMode::Compat => {
                // 32-bit: push 32-bit return address
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = (cpu.read_gpr(4) as u32).wrapping_sub(4);
                memory.write_u32(ss_base + sp as u64, ret_addr as u32);
                cpu.write_gpr(4, sp as u64);
            }
            CpuMode::Long => {
                // 64-bit: push 64-bit return address
                let rsp = cpu.read_gpr(4) - 8;
                cpu.write_gpr(4, rsp);
                memory.write_u64(rsp, ret_addr);
            }
        }
        
        // Get target
        let target = match &instr.operands[0] {
            Operand::Rel(offset) => {
                (instr.rip as i64 + instr.len as i64 + offset) as u64
            }
            Operand::Far { seg, off } => {
                // FAR CALL: push CS first, then IP
                match mode {
                    CpuMode::Real => {
                        // Push current CS
                        let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                        let sp = cpu.read_gpr(4) as u16;
                        let cs = (cpu.read_segment_base(crate::cpu::SegmentRegister::Cs) / 16) as u16;
                        let sp = sp.wrapping_sub(2);
                        memory.write_u16(ss_base + sp as u64, cs);
                        cpu.write_gpr(4, sp as u64);
                        
                        // Update CS to new segment
                        cpu.write_segment_base(crate::cpu::SegmentRegister::Cs, (*seg as u64) * 16);
                        
                        // Return linear address
                        (*seg as u64) * 16 + *off
                    }
                    _ => *off,  // Protected/Long mode would need GDT lookup
                }
            }
            _ => self.read_operand(cpu, memory, &instr.operands[0], instr)?,
        };
        
        Ok(InstrResult::Continue(target))
    }
    
    fn exec_ret(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let mode = Self::get_cpu_mode(cpu);
        
        // Pop return address based on CPU mode
        let ret_addr = match mode {
            CpuMode::Real => {
                // 16-bit: pop 16-bit return address
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = cpu.read_gpr(4) as u16;
                let ret = memory.read_u16(ss_base + sp as u64) as u64;
                
                // Handle RET imm16 - pop extra bytes
                let pop_bytes = if let Some(Operand::Imm(imm)) = instr.operands.get(0) {
                    *imm as u16
                } else {
                    0
                };
                
                cpu.write_gpr(4, sp.wrapping_add(2).wrapping_add(pop_bytes) as u64);
                
                // In real mode, return address is relative to CS base
                let cs_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Cs);
                cs_base + ret
            }
            CpuMode::Protected | CpuMode::Compat => {
                // 32-bit: pop 32-bit return address
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = cpu.read_gpr(4) as u32;
                let ret = memory.read_u32(ss_base + sp as u64) as u64;
                
                let pop_bytes = if let Some(Operand::Imm(imm)) = instr.operands.get(0) {
                    *imm as u32
                } else {
                    0
                };
                
                cpu.write_gpr(4, sp.wrapping_add(4).wrapping_add(pop_bytes) as u64);
                ret
            }
            CpuMode::Long => {
                // 64-bit: pop 64-bit return address
                let rsp = cpu.read_gpr(4);
                let ret = memory.read_u64(rsp);
                
                let pop_bytes = if let Some(Operand::Imm(imm)) = instr.operands.get(0) {
                    *imm as u64
                } else {
                    0
                };
                
                cpu.write_gpr(4, rsp + 8 + pop_bytes);
                ret
            }
        };
        
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
    
    fn exec_movs(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        // Determine operand size based on opcode and prefix
        // 0xA4 = MOVSB (byte)
        // 0xA5 = MOVSW/MOVSD/MOVSQ (depends on operand-size prefix and mode)
        let size: i64 = if (instr.opcode & 0xFF) == 0xA4 {
            1  // MOVSB always 1 byte
        } else {
            // 0xA5: MOVSW (with 0x66 prefix) or MOVSD (default in 64-bit mode)
            if instr.prefixes.op_size { 2 } else { 4 }
        };
        
        let delta = if cpu.get_df() { -size } else { size };
        
        // Handle REP prefix
        let count = if instr.prefixes.rep {
            cpu.read_gpr(1) as usize  // RCX
        } else {
            1
        };
        
        let mut rsi = cpu.read_gpr(6);
        let mut rdi = cpu.read_gpr(7);
        
        for _ in 0..count {
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
            
            rsi = (rsi as i64 + delta) as u64;
            rdi = (rdi as i64 + delta) as u64;
        }
        
        cpu.write_gpr(6, rsi);
        cpu.write_gpr(7, rdi);
        if instr.prefixes.rep {
            cpu.write_gpr(1, 0);  // RCX = 0 after REP
        }
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_stos(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        // Determine operand size based on opcode and prefix
        // 0xAA = STOSB (byte)
        // 0xAB = STOSW/STOSD/STOSQ (depends on operand-size prefix and REX.W)
        let size: u8 = if (instr.opcode & 0xFF) == 0xAA {
            1  // STOSB always 1 byte
        } else {
            // 0xAB: STOSQ (REX.W), STOSW (0x66 prefix), or STOSD (default)
            if instr.prefixes.rex_w {
                8  // STOSQ: REX.W prefix means 64-bit
            } else if instr.prefixes.op_size {
                2  // STOSW: 0x66 prefix means 16-bit
            } else {
                4  // STOSD: default in 64-bit mode
            }
        };
        
        let df = cpu.get_df();
        
        // Handle REP prefix
        let count = if instr.prefixes.rep {
            cpu.read_gpr(1) as usize  // RCX
        } else {
            1
        };
        
        if count == 0 {
            return Ok(InstrResult::Continue(instr.rip + instr.len as u64));
        }
        
        let rdi = cpu.read_gpr(7);
        let rax = cpu.read_gpr(0);
        
        // For forward direction (DF=0), use optimized bulk fill
        // For backward direction (DF=1), must iterate individually
        if !df && instr.prefixes.rep && count > 1 {
            // Use AddressSpace::fill() for bulk operation
            let written = memory.fill(rdi, rax, count, size);
            
            // Update RDI: advance by (written * size) bytes
            let new_rdi = rdi + (written * size as usize) as u64;
            cpu.write_gpr(7, new_rdi);
            cpu.write_gpr(1, 0);  // RCX = 0 after REP
        } else {
            // Individual writes (backward direction or single iteration)
            let delta: i64 = if df { -(size as i64) } else { size as i64 };
            let mut addr = rdi;
            
            for _ in 0..count {
                match size {
                    1 => memory.write_u8(addr, rax as u8),
                    2 => memory.write_u16(addr, rax as u16),
                    4 => memory.write_u32(addr, rax as u32),
                    8 => memory.write_u64(addr, rax),
                    _ => memory.write_u32(addr, rax as u32),
                }
                addr = (addr as i64 + delta) as u64;
            }
            
            cpu.write_gpr(7, addr);
            if instr.prefixes.rep {
                cpu.write_gpr(1, 0);  // RCX = 0 after REP
            }
        }
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_lods(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        // Determine operand size based on opcode and prefix
        // 0xAC = LODSB (byte)
        // 0xAD = LODSW/LODSD/LODSQ (depends on operand-size prefix and mode)
        let size: i64 = if (instr.opcode & 0xFF) == 0xAC {
            1  // LODSB always 1 byte
        } else {
            // 0xAD: LODSW (with 0x66 prefix) or LODSD (default in 64-bit mode)
            if instr.prefixes.op_size { 2 } else { 4 }
        };
        
        let delta = if cpu.get_df() { -size } else { size };
        
        // Handle REP prefix (rarely used with LODS, but should work)
        let count = if instr.prefixes.rep {
            cpu.read_gpr(1) as usize  // RCX
        } else {
            1
        };
        
        let mut rsi = cpu.read_gpr(6);
        let mut value = 0u64;
        
        for _ in 0..count {
            value = match size {
                1 => memory.read_u8(rsi) as u64,
                2 => memory.read_u16(rsi) as u64,
                4 => memory.read_u32(rsi) as u64,
                _ => memory.read_u64(rsi),
            };
            rsi = (rsi as i64 + delta) as u64;
        }
        
        cpu.write_gpr(0, value);
        cpu.write_gpr(6, rsi);
        if instr.prefixes.rep {
            cpu.write_gpr(1, 0);  // RCX = 0 after REP
        }
        
        Ok(InstrResult::Continue(instr.rip + instr.len as u64))
    }
    
    fn exec_int(&self, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let vector = match &instr.operands[0] {
            Operand::Imm(v) => *v as u8,
            _ => 3,
        };
        Ok(InstrResult::Exit(ExecuteResult::Interrupt { vector }))
    }
    
    fn exec_iret(&self, cpu: &VirtualCpu, memory: &AddressSpace, _instr: &DecodedInstr) -> JitResult<InstrResult> {
        let mode = Self::get_cpu_mode(cpu);
        
        match mode {
            CpuMode::Real => {
                // 16-bit IRET: pop IP, CS, FLAGS (2+2+2 = 6 bytes)
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = cpu.read_gpr(4) as u16;
                
                let ip = memory.read_u16(ss_base + sp as u64);
                let cs = memory.read_u16(ss_base + (sp.wrapping_add(2)) as u64);
                let flags = memory.read_u16(ss_base + (sp.wrapping_add(4)) as u64);
                
                cpu.write_gpr(4, sp.wrapping_add(6) as u64);
                
                // Update FLAGS (preserve reserved bits)
                let old_flags = cpu.read_rflags();
                cpu.write_rflags((old_flags & !0xFFFF) | flags as u64);
                
                // Update CS segment base
                cpu.write_segment_base(crate::cpu::SegmentRegister::Cs, (cs as u64) * 16);
                
                // Return to CS:IP (linear address)
                let rip = (cs as u64) * 16 + ip as u64;
                Ok(InstrResult::Continue(rip))
            }
            CpuMode::Protected | CpuMode::Compat => {
                // 32-bit IRET: pop EIP, CS, EFLAGS (4+4+4 = 12 bytes)
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = cpu.read_gpr(4) as u32;
                
                let eip = memory.read_u32(ss_base + sp as u64);
                let _cs = memory.read_u32(ss_base + (sp.wrapping_add(4)) as u64) as u16;
                let eflags = memory.read_u32(ss_base + (sp.wrapping_add(8)) as u64);
                
                cpu.write_gpr(4, sp.wrapping_add(12) as u64);
                
                // Update EFLAGS (preserve some protected bits)
                let mask = 0x00257FD5u32;
                let old_flags = cpu.read_rflags() as u32;
                cpu.write_rflags(((old_flags & !mask) | (eflags & mask)) as u64);
                
                Ok(InstrResult::Continue(eip as u64))
            }
            CpuMode::Long => {
                // 64-bit IRET: pop RIP, CS, RFLAGS, RSP, SS (8+8+8+8+8 = 40 bytes)
                let rsp = cpu.read_gpr(4);
                let rip = memory.read_u64(rsp);
                let _cs = memory.read_u64(rsp + 8) as u16;
                let rflags = memory.read_u64(rsp + 16);
                let new_rsp = memory.read_u64(rsp + 24);
                let _ss = memory.read_u64(rsp + 32) as u16;
                
                cpu.write_gpr(4, new_rsp);
                cpu.write_rflags(rflags);
                
                Ok(InstrResult::Continue(rip))
            }
        }
    }
    
    /// Execute LGDT or LIDT instruction
    /// 
    /// LGDT/LIDT load the Global/Interrupt Descriptor Table Register from memory.
    /// The memory operand contains:
    /// - 16-bit mode: 2-byte limit + 3-byte base (5 bytes, but base is 24-bit, zero-extended)
    /// - 32-bit mode: 2-byte limit + 4-byte base (6 bytes)
    /// - 64-bit mode: 2-byte limit + 8-byte base (10 bytes)
    fn exec_lgdt_lidt(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let next_rip = instr.rip + instr.len as u64;
        
        // Get memory address from operand
        let addr = match &instr.operands[0] {
            Operand::Mem(m) => {
                let ea = self.compute_ea(cpu, m, instr)?;
                log::debug!("[JIT] LGDT/LIDT: mem operand {:?}, computed EA={:#x}", m, ea);
                ea
            }
            _ => {
                log::error!("[JIT] LGDT/LIDT requires memory operand, got {:?}", instr.operands[0]);
                return Err(JitError::UnsupportedInstruction {
                    rip: instr.rip,
                    mnemonic: format!("{:?}", instr.mnemonic),
                });
            }
        };
        
        // Read limit (2 bytes, always)
        let limit = memory.read_u16(addr);
        
        // Debug: dump the GDT descriptor region
        let b0 = memory.read_u8(addr);
        let b1 = memory.read_u8(addr + 1);
        let b2 = memory.read_u8(addr + 2);
        let b3 = memory.read_u8(addr + 3);
        let b4 = memory.read_u8(addr + 4);
        let b5 = memory.read_u8(addr + 5);
        log::debug!("[JIT] LGDT: mem at {:#x} = [{:02x} {:02x} {:02x} {:02x} {:02x} {:02x}]", 
                   addr, b0, b1, b2, b3, b4, b5);
        
        // Read base address - size depends on operand size (CPU mode)
        // In 16-bit real mode: 24-bit (3 bytes), but we read 4 bytes and mask
        // In 32-bit mode: 32-bit (4 bytes)
        // In 64-bit mode: 64-bit (8 bytes)
        let cr0 = cpu.read_cr0();
        let is_protected = (cr0 & 1) != 0;
        let efer = cpu.read_msr(0xC000_0080); // IA32_EFER
        let is_long_mode = is_protected && (efer & 0x400) != 0; // LMA bit
        
        let base = if is_long_mode {
            // 64-bit mode: 8-byte base
            memory.read_u64(addr + 2)
        } else if is_protected {
            // 32-bit protected mode: 4-byte base
            memory.read_u32(addr + 2) as u64
        } else {
            // 16-bit real mode: 24-bit base (stored as 3 bytes, we read 4 and mask)
            // Note: In real mode, we read 4 bytes but only use 24 bits
            let raw = memory.read_u32(addr + 2);
            log::debug!("[JIT] LGDT real mode: raw 4 bytes at addr+2 = {:#x}", raw);
            raw as u64 & 0x00FF_FFFF
        };
        
        // Update the appropriate register
        match instr.mnemonic {
            Mnemonic::Lgdt => {
                log::debug!("[JIT] LGDT: limit={:#x}, base={:#x} at RIP={:#x}", limit, base, instr.rip);
                cpu.set_gdtr(limit, base);
            }
            Mnemonic::Lidt => {
                log::debug!("[JIT] LIDT: limit={:#x}, base={:#x} at RIP={:#x}", limit, base, instr.rip);
                cpu.set_idtr(limit, base);
            }
            _ => unreachable!(),
        }
        
        Ok(InstrResult::Continue(next_rip))
    }

    // ========================================================================
    // Helpers
    // ========================================================================
    
    fn read_operand(&self, cpu: &VirtualCpu, memory: &AddressSpace, op: &Operand, instr: &DecodedInstr) -> JitResult<u64> {
        match op {
            Operand::None => Ok(0),
            Operand::Reg(r) => {
                if r.kind == RegKind::Segment {
                    // Map x86 segment encoding to SegmentRegister enum
                    let seg = match r.index {
                        0 => crate::cpu::SegmentRegister::Es,
                        1 => crate::cpu::SegmentRegister::Cs,
                        2 => crate::cpu::SegmentRegister::Ss,
                        3 => crate::cpu::SegmentRegister::Ds,
                        4 => crate::cpu::SegmentRegister::Fs,
                        5 => crate::cpu::SegmentRegister::Gs,
                        _ => return Ok(0),
                    };
                    Ok(cpu.read_segment_selector(seg) as u64)
                } else if r.kind == RegKind::Control {
                    // Control registers CR0, CR2, CR3, CR4
                    match r.index {
                        0 => Ok(cpu.read_cr0()),
                        2 => Ok(cpu.read_cr2()),
                        3 => Ok(cpu.read_cr3()),
                        4 => Ok(cpu.read_cr4()),
                        _ => Ok(0),
                    }
                } else {
                    self.read_reg(cpu, r)
                }
            }
            Operand::Imm(v) => Ok(*v as u64),
            Operand::Mem(m) => self.read_mem(cpu, memory, m, instr),
            Operand::Rel(v) => Ok(*v as u64),
            Operand::Far { off, .. } => Ok(*off),
        }
    }
    
    fn write_operand(&self, cpu: &VirtualCpu, memory: &AddressSpace, op: &Operand, value: u64, instr: &DecodedInstr) -> JitResult<()> {
        match op {
            Operand::Reg(r) => {
                if r.kind == RegKind::Segment {
                    // Map x86 segment encoding to SegmentRegister enum
                    // x86: ES=0, CS=1, SS=2, DS=3, FS=4, GS=5
                    // enum: CS=0, DS=1, ES=2, FS=3, GS=4, SS=5
                    let seg = match r.index {
                        0 => crate::cpu::SegmentRegister::Es,
                        1 => crate::cpu::SegmentRegister::Cs,
                        2 => crate::cpu::SegmentRegister::Ss,
                        3 => crate::cpu::SegmentRegister::Ds,
                        4 => crate::cpu::SegmentRegister::Fs,
                        5 => crate::cpu::SegmentRegister::Gs,
                        _ => return Ok(()), // Invalid segment
                    };
                    cpu.write_segment_selector(seg, value as u16);
                    // In real mode, base = selector * 16
                    let cr0 = cpu.read_cr0();
                    if (cr0 & 1) == 0 {
                        // Real mode: base = selector * 16
                        cpu.write_segment_base(seg, (value as u64) * 16);
                    }
                    Ok(())
                } else {
                    self.write_reg_sized(cpu, r, value)
                }
            }
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
    
    fn read_mem(&self, cpu: &VirtualCpu, memory: &AddressSpace, m: &MemOp, instr: &DecodedInstr) -> JitResult<u64> {
        let addr = self.compute_ea(cpu, m, instr)?;
        Ok(match m.size {
            1 => memory.read_u8(addr) as u64,
            2 => memory.read_u16(addr) as u64,
            4 => memory.read_u32(addr) as u64,
            _ => memory.read_u64(addr),
        })
    }
    
    fn write_mem(&self, cpu: &VirtualCpu, memory: &AddressSpace, m: &MemOp, value: u64, instr: &DecodedInstr) -> JitResult<()> {
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
    
    // ========================================================================
    // Real Mode Instructions (16-bit)
    // ========================================================================
    
    /// PUSHA - Push All General Registers (16-bit mode: AX, CX, DX, BX, SP, BP, SI, DI)
    fn exec_pusha(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let mode = Self::get_cpu_mode(cpu);
        let next_rip = instr.rip + instr.len as u64;
        
        match mode {
            CpuMode::Real | CpuMode::Protected | CpuMode::Compat => {
                let temp_sp = cpu.read_gpr(4) as u16;  // Original SP
                
                // Push order: AX, CX, DX, BX, SP (original), BP, SI, DI
                let regs_to_push = [
                    cpu.read_gpr(0) as u16,  // AX
                    cpu.read_gpr(1) as u16,  // CX
                    cpu.read_gpr(2) as u16,  // DX
                    cpu.read_gpr(3) as u16,  // BX
                    temp_sp,                  // Original SP
                    cpu.read_gpr(5) as u16,  // BP
                    cpu.read_gpr(6) as u16,  // SI
                    cpu.read_gpr(7) as u16,  // DI
                ];
                
                let mut sp = temp_sp;
                for &val in &regs_to_push {
                    sp = sp.wrapping_sub(2);
                    // In real mode, address is SS:SP
                    let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                    let addr = ss_base + sp as u64;
                    memory.write_u16(addr, val);
                }
                cpu.write_gpr(4, sp as u64);
            }
            CpuMode::Long => {
                // PUSHA is invalid in 64-bit mode
                return Err(JitError::DecodeError {
                    rip: instr.rip,
                    bytes: vec![],
                    reason: "PUSHA invalid in 64-bit mode".to_string(),
                });
            }
        }
        
        Ok(InstrResult::Continue(next_rip))
    }
    
    /// POPA - Pop All General Registers (16-bit mode)
    fn exec_popa(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let mode = Self::get_cpu_mode(cpu);
        let next_rip = instr.rip + instr.len as u64;
        
        match mode {
            CpuMode::Real | CpuMode::Protected | CpuMode::Compat => {
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let mut sp = cpu.read_gpr(4) as u16;
                
                // Pop order: DI, SI, BP, (skip SP), BX, DX, CX, AX
                let di = memory.read_u16(ss_base + sp as u64); sp = sp.wrapping_add(2);
                let si = memory.read_u16(ss_base + sp as u64); sp = sp.wrapping_add(2);
                let bp = memory.read_u16(ss_base + sp as u64); sp = sp.wrapping_add(2);
                let _skip_sp = memory.read_u16(ss_base + sp as u64); sp = sp.wrapping_add(2);  // SP is discarded
                let bx = memory.read_u16(ss_base + sp as u64); sp = sp.wrapping_add(2);
                let dx = memory.read_u16(ss_base + sp as u64); sp = sp.wrapping_add(2);
                let cx = memory.read_u16(ss_base + sp as u64); sp = sp.wrapping_add(2);
                let ax = memory.read_u16(ss_base + sp as u64); sp = sp.wrapping_add(2);
                
                // Write to low 16 bits of registers
                cpu.write_gpr(0, (cpu.read_gpr(0) & !0xFFFF) | ax as u64);  // AX
                cpu.write_gpr(1, (cpu.read_gpr(1) & !0xFFFF) | cx as u64);  // CX
                cpu.write_gpr(2, (cpu.read_gpr(2) & !0xFFFF) | dx as u64);  // DX
                cpu.write_gpr(3, (cpu.read_gpr(3) & !0xFFFF) | bx as u64);  // BX
                cpu.write_gpr(4, sp as u64);  // SP updated
                cpu.write_gpr(5, (cpu.read_gpr(5) & !0xFFFF) | bp as u64);  // BP
                cpu.write_gpr(6, (cpu.read_gpr(6) & !0xFFFF) | si as u64);  // SI
                cpu.write_gpr(7, (cpu.read_gpr(7) & !0xFFFF) | di as u64);  // DI
            }
            CpuMode::Long => {
                return Err(JitError::DecodeError {
                    rip: instr.rip,
                    bytes: vec![],
                    reason: "POPA invalid in 64-bit mode".to_string(),
                });
            }
        }
        
        Ok(InstrResult::Continue(next_rip))
    }
    
    /// PUSHF - Push FLAGS register
    fn exec_pushf(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let mode = Self::get_cpu_mode(cpu);
        let next_rip = instr.rip + instr.len as u64;
        let flags = cpu.read_rflags();
        
        match mode {
            CpuMode::Real => {
                // Push 16-bit FLAGS
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = (cpu.read_gpr(4) as u16).wrapping_sub(2);
                memory.write_u16(ss_base + sp as u64, flags as u16);
                cpu.write_gpr(4, sp as u64);
            }
            CpuMode::Protected | CpuMode::Compat => {
                // Push 32-bit EFLAGS
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = (cpu.read_gpr(4) as u32).wrapping_sub(4);
                memory.write_u32(ss_base + sp as u64, flags as u32);
                cpu.write_gpr(4, sp as u64);
            }
            CpuMode::Long => {
                // Push 64-bit RFLAGS
                let sp = cpu.read_gpr(4).wrapping_sub(8);
                memory.write_u64(sp, flags);
                cpu.write_gpr(4, sp);
            }
        }
        
        Ok(InstrResult::Continue(next_rip))
    }
    
    /// POPF - Pop FLAGS register  
    fn exec_popf(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let mode = Self::get_cpu_mode(cpu);
        let next_rip = instr.rip + instr.len as u64;
        
        match mode {
            CpuMode::Real => {
                // Pop 16-bit FLAGS
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = cpu.read_gpr(4) as u16;
                let flags = memory.read_u16(ss_base + sp as u64);
                cpu.write_rflags((cpu.read_rflags() & !0xFFFF) | flags as u64);
                cpu.write_gpr(4, sp.wrapping_add(2) as u64);
            }
            CpuMode::Protected | CpuMode::Compat => {
                // Pop 32-bit EFLAGS (some bits are protected)
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = cpu.read_gpr(4) as u32;
                let flags = memory.read_u32(ss_base + sp as u64);
                // Preserve some protected flags
                let mask = 0x00257FD5u32;  // Modifiable flags
                let new_flags = (cpu.read_rflags() as u32 & !mask) | (flags & mask);
                cpu.write_rflags(new_flags as u64);
                cpu.write_gpr(4, sp.wrapping_add(4) as u64);
            }
            CpuMode::Long => {
                // Pop 64-bit RFLAGS
                let sp = cpu.read_gpr(4);
                let flags = memory.read_u64(sp);
                let mask = 0x0000000000257FD5u64;
                let new_flags = (cpu.read_rflags() & !mask) | (flags & mask);
                cpu.write_rflags(new_flags);
                cpu.write_gpr(4, sp.wrapping_add(8));
            }
        }
        
        Ok(InstrResult::Continue(next_rip))
    }
    
    /// RETF - Far Return (pop IP and CS from stack)
    fn exec_retf(&self, cpu: &VirtualCpu, memory: &AddressSpace, instr: &DecodedInstr) -> JitResult<InstrResult> {
        let mode = Self::get_cpu_mode(cpu);
        
        match mode {
            CpuMode::Real => {
                // Pop 16-bit IP and CS
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = cpu.read_gpr(4) as u16;
                let ip = memory.read_u16(ss_base + sp as u64);
                let cs = memory.read_u16(ss_base + (sp.wrapping_add(2)) as u64);
                cpu.write_gpr(4, sp.wrapping_add(4) as u64);
                
                // Update CS segment base for real mode
                cpu.write_segment_base(crate::cpu::SegmentRegister::Cs, (cs as u64) * 16);
                
                // Return address is CS:IP (linear = CS*16 + IP)
                let linear = (cs as u64) * 16 + ip as u64;
                Ok(InstrResult::Continue(linear))
            }
            CpuMode::Protected | CpuMode::Compat => {
                // Pop 32-bit EIP and CS (selector)
                let ss_base = cpu.read_segment_base(crate::cpu::SegmentRegister::Ss);
                let sp = cpu.read_gpr(4) as u32;
                let eip = memory.read_u32(ss_base + sp as u64);
                let cs_selector = memory.read_u16(ss_base + (sp.wrapping_add(4)) as u64);
                cpu.write_gpr(4, sp.wrapping_add(8) as u64);
                
                // Load CS from selector (simplified - just update segment base from GDT)
                let (gdt_limit, gdt_base) = cpu.get_gdtr();
                let index = (cs_selector >> 3) as u64;
                let desc_addr = gdt_base + index * 8;
                
                if desc_addr + 8 <= gdt_base + gdt_limit as u64 + 1 {
                    let desc_lo = memory.read_u32(desc_addr) as u64;
                    let desc_hi = memory.read_u32(desc_addr + 4) as u64;
                    let base = ((desc_lo >> 16) & 0xFFFF) | 
                               ((desc_hi & 0xFF) << 16) |
                               ((desc_hi >> 24) << 24);
                    cpu.write_segment_base(crate::cpu::SegmentRegister::Cs, base);
                }
                
                Ok(InstrResult::Continue(eip as u64))
            }
            CpuMode::Long => {
                // Pop 64-bit RIP and CS
                let sp = cpu.read_gpr(4);
                let rip = memory.read_u64(sp);
                let cs_selector = memory.read_u16(sp + 8);
                cpu.write_gpr(4, sp.wrapping_add(16));
                
                // CS selector in long mode
                let (gdt_limit, gdt_base) = cpu.get_gdtr();
                let index = (cs_selector >> 3) as u64;
                let desc_addr = gdt_base + index * 8;
                
                if desc_addr + 8 <= gdt_base + gdt_limit as u64 + 1 {
                    let desc_lo = memory.read_u32(desc_addr) as u64;
                    let desc_hi = memory.read_u32(desc_addr + 4) as u64;
                    // In 64-bit mode, base is mostly ignored for code segments
                    let attrib = ((desc_hi >> 8) & 0xFF) | ((desc_hi >> 12) & 0xF00);
                    cpu.write_segment_attrib(crate::cpu::SegmentRegister::Cs, attrib as u16);
                }
                
                Ok(InstrResult::Continue(rip))
            }
        }
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
    /// CPU mode changed (MOV CR0 or WRMSR to EFER) - must exit block and re-sync
    ModeChanged(u64),
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
