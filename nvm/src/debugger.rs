//! VM Debugger Interface
//!
//! Provides GDB-style debugging capabilities for the virtual machine.
//! This module enables:
//!
//! - Breakpoint management (software & hardware)
//! - Single-step execution
//! - Register/memory inspection
//! - Call stack unwinding
//! - Watchpoints (data breakpoints)
//! - Symbol resolution (if debug info available)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use tests::mock::debugger::{VmDebugger, DebugCommand};
//!
//! let vm = VirtualMachine::new();
//! let debugger = VmDebugger::attach(&vm);
//!
//! // Set breakpoint
//! debugger.add_breakpoint(0x1000);
//!
//! // Run until breakpoint
//! debugger.continue_execution();
//!
//! // Inspect state
//! let regs = debugger.read_registers();
//! let mem = debugger.read_memory(0x2000, 64);
//!
//! // Single step
//! debugger.step();
//! ```

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};

use super::cpu::{VirtualCpu, CpuState, Registers, BreakpointType, CpuEvent};
use super::vm::{VirtualMachine, VmState};

/// Debug event types
#[derive(Debug, Clone)]
pub enum DebugEvent {
    /// Breakpoint hit
    BreakpointHit { address: u64, cpu_id: u32 },
    /// Watchpoint triggered
    WatchpointHit { address: u64, is_write: bool, value: u64 },
    /// Single-step completed
    StepCompleted { rip: u64 },
    /// Exception occurred
    Exception { vector: u8, error_code: Option<u32> },
    /// Process/thread created
    ThreadCreated { tid: u64 },
    /// Process/thread exited
    ThreadExited { tid: u64, exit_code: i32 },
    /// Syscall entry
    SyscallEntry { number: u64, args: [u64; 6] },
    /// Syscall exit
    SyscallExit { number: u64, result: i64 },
}

/// Debug command for debugger interface
#[derive(Debug, Clone)]
pub enum DebugCommand {
    /// Continue execution
    Continue,
    /// Single step
    Step,
    /// Step over (skip function calls)
    StepOver,
    /// Step out (run until return)
    StepOut,
    /// Run to address
    RunTo(u64),
    /// Break execution
    Break,
    /// Detach debugger
    Detach,
}

/// Breakpoint state
#[derive(Debug, Clone)]
pub struct BreakpointInfo {
    pub id: u32,
    pub address: u64,
    pub bp_type: BreakpointType,
    pub enabled: bool,
    pub hit_count: u64,
    pub condition: Option<String>,
    pub commands: Vec<String>,
    /// Temporary breakpoint (delete after hit)
    pub temporary: bool,
}

/// Memory watchpoint
#[derive(Debug, Clone)]
pub struct Watchpoint {
    pub id: u32,
    pub address: u64,
    pub size: usize,
    pub watch_read: bool,
    pub watch_write: bool,
    pub enabled: bool,
    pub hit_count: u64,
}

/// Stack frame information
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub frame_number: u32,
    pub rip: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub function_name: Option<String>,
    pub source_file: Option<String>,
    pub line_number: Option<u32>,
}

/// Debugger state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebuggerState {
    /// Debugger not attached
    Detached,
    /// Target is running
    Running,
    /// Target is stopped (at breakpoint, etc.)
    Stopped,
    /// Single-stepping
    Stepping,
}

/// VM Debugger
pub struct VmDebugger {
    /// Reference to the VM being debugged
    vm: Arc<VirtualMachine>,
    /// Current debugger state
    state: RwLock<DebuggerState>,
    /// Breakpoints by ID
    breakpoints: RwLock<HashMap<u32, BreakpointInfo>>,
    /// Address to breakpoint ID mapping
    bp_addresses: RwLock<HashMap<u64, u32>>,
    /// Watchpoints
    watchpoints: RwLock<HashMap<u32, Watchpoint>>,
    /// Next breakpoint ID
    next_bp_id: Mutex<u32>,
    /// Event queue
    events: Mutex<VecDeque<DebugEvent>>,
    /// Currently selected CPU
    selected_cpu: Mutex<u32>,
    /// Symbol table (address -> name)
    symbols: RwLock<HashMap<u64, String>>,
    /// Reverse symbol table (name -> address)
    symbols_by_name: RwLock<HashMap<String, u64>>,
}

impl VmDebugger {
    /// Attach debugger to a VM
    pub fn attach(vm: Arc<VirtualMachine>) -> Self {
        Self {
            vm,
            state: RwLock::new(DebuggerState::Stopped),
            breakpoints: RwLock::new(HashMap::new()),
            bp_addresses: RwLock::new(HashMap::new()),
            watchpoints: RwLock::new(HashMap::new()),
            next_bp_id: Mutex::new(1),
            events: Mutex::new(VecDeque::new()),
            selected_cpu: Mutex::new(0),
            symbols: RwLock::new(HashMap::new()),
            symbols_by_name: RwLock::new(HashMap::new()),
        }
    }
    
    /// Get debugger state
    pub fn get_state(&self) -> DebuggerState {
        *self.state.read().unwrap()
    }
    
    // ========================================================================
    // Breakpoint Management
    // ========================================================================
    
    /// Add a breakpoint at address
    pub fn add_breakpoint(&self, address: u64) -> u32 {
        self.add_breakpoint_ext(address, false, None)
    }
    
    /// Add breakpoint with options
    pub fn add_breakpoint_ext(&self, address: u64, temporary: bool, condition: Option<String>) -> u32 {
        let mut next_id = self.next_bp_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;
        
        let bp = BreakpointInfo {
            id,
            address,
            bp_type: BreakpointType::Execution,
            enabled: true,
            hit_count: 0,
            condition,
            commands: Vec::new(),
            temporary,
        };
        
        // Add to vCPU
        if let Some(cpu) = self.vm.get_cpu(*self.selected_cpu.lock().unwrap()) {
            cpu.add_breakpoint(address);
        }
        
        self.breakpoints.write().unwrap().insert(id, bp);
        self.bp_addresses.write().unwrap().insert(address, id);
        
        id
    }
    
    /// Remove a breakpoint
    pub fn remove_breakpoint(&self, id: u32) -> bool {
        let mut bps = self.breakpoints.write().unwrap();
        if let Some(bp) = bps.remove(&id) {
            self.bp_addresses.write().unwrap().remove(&bp.address);
            
            // Remove from vCPU
            if let Some(cpu) = self.vm.get_cpu(*self.selected_cpu.lock().unwrap()) {
                cpu.remove_breakpoint(bp.address);
            }
            
            true
        } else {
            false
        }
    }
    
    /// Enable/disable breakpoint
    pub fn set_breakpoint_enabled(&self, id: u32, enabled: bool) -> bool {
        if let Some(bp) = self.breakpoints.write().unwrap().get_mut(&id) {
            bp.enabled = enabled;
            
            if let Some(cpu) = self.vm.get_cpu(*self.selected_cpu.lock().unwrap()) {
                cpu.set_breakpoint_enabled(bp.address, enabled);
            }
            
            true
        } else {
            false
        }
    }
    
    /// List all breakpoints
    pub fn list_breakpoints(&self) -> Vec<BreakpointInfo> {
        self.breakpoints.read().unwrap().values().cloned().collect()
    }
    
    /// Clear all breakpoints
    pub fn clear_breakpoints(&self) {
        let bps: Vec<_> = self.breakpoints.read().unwrap().keys().cloned().collect();
        for id in bps {
            self.remove_breakpoint(id);
        }
    }
    
    // ========================================================================
    // Watchpoint Management
    // ========================================================================
    
    /// Add a memory watchpoint
    pub fn add_watchpoint(&self, address: u64, size: usize, watch_read: bool, watch_write: bool) -> u32 {
        let mut next_id = self.next_bp_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;
        
        let wp = Watchpoint {
            id,
            address,
            size,
            watch_read,
            watch_write,
            enabled: true,
            hit_count: 0,
        };
        
        // Add to vCPU as data breakpoint
        if let Some(cpu) = self.vm.get_cpu(*self.selected_cpu.lock().unwrap()) {
            cpu.add_watchpoint(address, !watch_read); // write_only if not watching reads
        }
        
        self.watchpoints.write().unwrap().insert(id, wp);
        
        id
    }
    
    /// Remove watchpoint
    pub fn remove_watchpoint(&self, id: u32) -> bool {
        let mut wps = self.watchpoints.write().unwrap();
        if let Some(wp) = wps.remove(&id) {
            if let Some(cpu) = self.vm.get_cpu(*self.selected_cpu.lock().unwrap()) {
                cpu.remove_breakpoint(wp.address);
            }
            true
        } else {
            false
        }
    }
    
    /// List all watchpoints
    pub fn list_watchpoints(&self) -> Vec<Watchpoint> {
        self.watchpoints.read().unwrap().values().cloned().collect()
    }
    
    // ========================================================================
    // Execution Control
    // ========================================================================
    
    /// Continue execution
    pub fn continue_execution(&self) {
        *self.state.write().unwrap() = DebuggerState::Running;
        self.vm.resume_vm();
    }
    
    /// Single step one instruction
    pub fn step(&self) {
        *self.state.write().unwrap() = DebuggerState::Stepping;
        
        if let Some(cpu) = self.vm.get_cpu(*self.selected_cpu.lock().unwrap()) {
            cpu.enable_single_step();
            // In real impl, would run one instruction
            cpu.advance_cycles(1);
            cpu.disable_single_step();
            
            let rip = cpu.read_rip();
            self.push_event(DebugEvent::StepCompleted { rip });
        }
        
        *self.state.write().unwrap() = DebuggerState::Stopped;
    }
    
    /// Step over (skip function calls)
    pub fn step_over(&self) {
        // Simplified: just step for now
        // Real impl would analyze instruction and set temporary breakpoint after call
        self.step();
    }
    
    /// Step out (run until return)
    pub fn step_out(&self) {
        // Simplified: set breakpoint at return address and continue
        // Real impl would analyze stack frame
        if let Some(cpu) = self.vm.get_cpu(*self.selected_cpu.lock().unwrap()) {
            let rbp = cpu.get_state().regs.rbp;
            // Return address is typically at rbp+8 in x86_64
            let return_addr = self.vm.memory().read_u64(rbp + 8);
            self.add_breakpoint_ext(return_addr, true, None);
            self.continue_execution();
        }
    }
    
    /// Run to specific address
    pub fn run_to(&self, address: u64) {
        self.add_breakpoint_ext(address, true, None);
        self.continue_execution();
    }
    
    /// Break execution
    pub fn break_execution(&self) {
        self.vm.pause_vm();
        *self.state.write().unwrap() = DebuggerState::Stopped;
    }
    
    /// Detach debugger
    pub fn detach(&self) {
        self.clear_breakpoints();
        self.vm.resume_vm();
        *self.state.write().unwrap() = DebuggerState::Detached;
    }
    
    // ========================================================================
    // State Inspection
    // ========================================================================
    
    /// Read all registers
    pub fn read_registers(&self) -> Option<Registers> {
        self.vm.get_cpu(*self.selected_cpu.lock().unwrap())
            .map(|cpu| cpu.get_registers())
    }
    
    /// Read specific register
    pub fn read_register(&self, name: &str) -> Option<u64> {
        let cpu = self.vm.get_cpu(*self.selected_cpu.lock().unwrap())?;
        let regs = cpu.get_registers();
        
        match name.to_lowercase().as_str() {
            "rax" => Some(regs.rax),
            "rbx" => Some(regs.rbx),
            "rcx" => Some(regs.rcx),
            "rdx" => Some(regs.rdx),
            "rsi" => Some(regs.rsi),
            "rdi" => Some(regs.rdi),
            "rbp" => Some(regs.rbp),
            "rsp" => Some(regs.rsp),
            "r8" => Some(regs.r8),
            "r9" => Some(regs.r9),
            "r10" => Some(regs.r10),
            "r11" => Some(regs.r11),
            "r12" => Some(regs.r12),
            "r13" => Some(regs.r13),
            "r14" => Some(regs.r14),
            "r15" => Some(regs.r15),
            "rip" | "pc" => Some(regs.rip),
            "rflags" | "eflags" => Some(regs.rflags),
            _ => None,
        }
    }
    
    /// Write register
    pub fn write_register(&self, name: &str, value: u64) -> bool {
        let cpu = match self.vm.get_cpu(*self.selected_cpu.lock().unwrap()) {
            Some(c) => c,
            None => return false,
        };
        
        let mut regs = cpu.get_registers();
        match name.to_lowercase().as_str() {
            "rax" => regs.rax = value,
            "rbx" => regs.rbx = value,
            "rcx" => regs.rcx = value,
            "rdx" => regs.rdx = value,
            "rsi" => regs.rsi = value,
            "rdi" => regs.rdi = value,
            "rbp" => regs.rbp = value,
            "rsp" => regs.rsp = value,
            "r8" => regs.r8 = value,
            "r9" => regs.r9 = value,
            "r10" => regs.r10 = value,
            "r11" => regs.r11 = value,
            "r12" => regs.r12 = value,
            "r13" => regs.r13 = value,
            "r14" => regs.r14 = value,
            "r15" => regs.r15 = value,
            "rip" | "pc" => regs.rip = value,
            "rflags" | "eflags" => regs.rflags = value,
            _ => return false,
        }
        cpu.set_registers(regs);
        true
    }
    
    /// Read memory
    pub fn read_memory(&self, address: u64, size: usize) -> Vec<u8> {
        self.vm.read_memory(address, size)
    }
    
    /// Write memory
    pub fn write_memory(&self, address: u64, data: &[u8]) {
        self.vm.write_memory(address, data);
    }
    
    /// Get current instruction pointer
    pub fn get_pc(&self) -> Option<u64> {
        self.vm.get_cpu(*self.selected_cpu.lock().unwrap())
            .map(|cpu| cpu.read_rip())
    }
    
    /// Get stack pointer
    pub fn get_sp(&self) -> Option<u64> {
        self.vm.get_cpu(*self.selected_cpu.lock().unwrap())
            .map(|cpu| cpu.read_rsp())
    }
    
    // ========================================================================
    // Stack Unwinding
    // ========================================================================
    
    /// Get call stack (backtrace)
    pub fn backtrace(&self, max_frames: usize) -> Vec<StackFrame> {
        let cpu = match self.vm.get_cpu(*self.selected_cpu.lock().unwrap()) {
            Some(c) => c,
            None => return Vec::new(),
        };
        
        let regs = cpu.get_registers();
        let mut frames = Vec::new();
        let mut rbp = regs.rbp;
        let mut rip = regs.rip;
        
        // Add current frame
        frames.push(StackFrame {
            frame_number: 0,
            rip,
            rsp: regs.rsp,
            rbp,
            function_name: self.resolve_symbol(rip),
            source_file: None,
            line_number: None,
        });
        
        // Unwind stack
        for i in 1..max_frames {
            if rbp == 0 || rbp % 8 != 0 {
                break;
            }
            
            // Return address is at rbp+8
            let return_addr = self.vm.memory().read_u64(rbp + 8);
            if return_addr == 0 {
                break;
            }
            
            // Previous rbp is at rbp
            let prev_rbp = self.vm.memory().read_u64(rbp);
            
            frames.push(StackFrame {
                frame_number: i as u32,
                rip: return_addr,
                rsp: rbp + 16,
                rbp: prev_rbp,
                function_name: self.resolve_symbol(return_addr),
                source_file: None,
                line_number: None,
            });
            
            if prev_rbp <= rbp {
                break; // Stack growing wrong direction
            }
            rbp = prev_rbp;
        }
        
        frames
    }
    
    // ========================================================================
    // Symbol Management
    // ========================================================================
    
    /// Add a symbol
    pub fn add_symbol(&self, address: u64, name: &str) {
        self.symbols.write().unwrap().insert(address, name.to_string());
        self.symbols_by_name.write().unwrap().insert(name.to_string(), address);
    }
    
    /// Load symbols from a simple symbol file (address name format)
    pub fn load_symbols(&self, symbol_data: &str) {
        for line in symbol_data.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(addr) = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16) {
                    self.add_symbol(addr, parts[1]);
                }
            }
        }
    }
    
    /// Resolve address to symbol name
    pub fn resolve_symbol(&self, address: u64) -> Option<String> {
        let symbols = self.symbols.read().unwrap();
        
        // Exact match
        if let Some(name) = symbols.get(&address) {
            return Some(name.clone());
        }
        
        // Find closest symbol before address
        let mut closest: Option<(u64, &String)> = None;
        for (addr, name) in symbols.iter() {
            if *addr <= address {
                match closest {
                    Some((prev_addr, _)) if *addr > prev_addr => {
                        closest = Some((*addr, name));
                    }
                    None => {
                        closest = Some((*addr, name));
                    }
                    _ => {}
                }
            }
        }
        
        closest.map(|(addr, name)| {
            let offset = address - addr;
            if offset == 0 {
                name.clone()
            } else {
                format!("{}+0x{:x}", name, offset)
            }
        })
    }
    
    /// Look up symbol by name
    pub fn lookup_symbol(&self, name: &str) -> Option<u64> {
        self.symbols_by_name.read().unwrap().get(name).copied()
    }
    
    // ========================================================================
    // CPU Selection
    // ========================================================================
    
    /// Select CPU for debugging
    pub fn select_cpu(&self, id: u32) -> bool {
        if self.vm.get_cpu(id).is_some() {
            *self.selected_cpu.lock().unwrap() = id;
            true
        } else {
            false
        }
    }
    
    /// Get selected CPU ID
    pub fn selected_cpu(&self) -> u32 {
        *self.selected_cpu.lock().unwrap()
    }
    
    /// List all CPU IDs
    pub fn list_cpus(&self) -> Vec<u32> {
        (0..self.vm.cpu_count() as u32).collect()
    }
    
    // ========================================================================
    // Event Queue
    // ========================================================================
    
    fn push_event(&self, event: DebugEvent) {
        self.events.lock().unwrap().push_back(event);
    }
    
    /// Get pending events
    pub fn get_events(&self) -> Vec<DebugEvent> {
        self.events.lock().unwrap().drain(..).collect()
    }
    
    /// Wait for event (blocking)
    pub fn wait_event(&self) -> Option<DebugEvent> {
        // Simplified: just pop from queue
        self.events.lock().unwrap().pop_front()
    }
}

// ============================================================================
// Disassembler (Basic)
// ============================================================================

/// Simple instruction representation
#[derive(Debug, Clone)]
pub struct Instruction {
    pub address: u64,
    pub bytes: Vec<u8>,
    pub mnemonic: String,
    pub operands: String,
}

/// Basic disassembler (placeholder - real impl would use capstone or similar)
pub fn disassemble(_address: u64, _bytes: &[u8], _count: usize) -> Vec<Instruction> {
    // Placeholder - would integrate with a real disassembler
    Vec::new()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_debugger_basic() {
        let vm = Arc::new(VirtualMachine::new());
        let debugger = VmDebugger::attach(vm);
        
        assert_eq!(debugger.get_state(), DebuggerState::Stopped);
    }
    
    #[test]
    fn test_debugger_breakpoints() {
        let vm = Arc::new(VirtualMachine::new());
        
        let debugger = VmDebugger::attach(vm.clone());
        
        // Add breakpoint
        let bp_id = debugger.add_breakpoint(0x1000);
        assert_eq!(debugger.list_breakpoints().len(), 1);
        
        // Disable
        debugger.set_breakpoint_enabled(bp_id, false);
        assert!(!debugger.list_breakpoints()[0].enabled);
        
        // Remove
        debugger.remove_breakpoint(bp_id);
        assert!(debugger.list_breakpoints().is_empty());
    }
    
    #[test]
    fn test_debugger_registers() {
        let vm = Arc::new(VirtualMachine::new());
        
        let debugger = VmDebugger::attach(vm.clone());
        
        // Write and read register
        debugger.write_register("rax", 0x12345);
        assert_eq!(debugger.read_register("rax"), Some(0x12345));
    }
    
    #[test]
    fn test_debugger_memory() {
        let vm = Arc::new(VirtualMachine::new());
        
        let debugger = VmDebugger::attach(vm.clone());
        
        // Write and read memory
        debugger.write_memory(0x10000, b"Test");
        assert_eq!(&debugger.read_memory(0x10000, 4)[..], b"Test");
    }
    
    #[test]
    fn test_debugger_symbols() {
        let vm = Arc::new(VirtualMachine::new());
        let debugger = VmDebugger::attach(vm);
        
        // Add symbols
        debugger.add_symbol(0x1000, "kernel_main");
        debugger.add_symbol(0x2000, "do_syscall");
        
        // Resolve
        assert_eq!(debugger.resolve_symbol(0x1000), Some("kernel_main".to_string()));
        assert_eq!(debugger.resolve_symbol(0x1010), Some("kernel_main+0x10".to_string()));
        
        // Lookup
        assert_eq!(debugger.lookup_symbol("do_syscall"), Some(0x2000));
    }
    
    #[test]
    fn test_debugger_cpu_selection() {
        let config = super::super::vm::VmConfig::smp(4);
        let vm = Arc::new(VirtualMachine::with_config(config));
        let debugger = VmDebugger::attach(vm);
        
        assert_eq!(debugger.selected_cpu(), 0);
        assert!(debugger.select_cpu(2));
        assert_eq!(debugger.selected_cpu(), 2);
        assert!(!debugger.select_cpu(10)); // Invalid CPU
    }
}
