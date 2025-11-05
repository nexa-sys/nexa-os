/// Init process management and system initialization
/// Follows POSIX and Unix-like conventions for hybrid kernel architecture
///
/// This module implements a multi-stage init system:
/// 1. Early kernel initialization (already done in lib.rs)
/// 2. Init process spawning (PID 1)
/// 3. Service/daemon management
/// 4. Runlevel/target management
/// 5. System reboot/shutdown handling
use crate::process::{Pid, Process};
use crate::scheduler;
use spin::Mutex;

/// Init process PID (always 1 in Unix-like systems)
pub const INIT_PID: Pid = 1;

/// System runlevels (traditional Unix System V style)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RunLevel {
    /// Halt - System is shut down
    Halt = 0,
    /// Single-user mode (maintenance)
    SingleUser = 1,
    /// Multi-user mode without networking
    MultiUser = 2,
    /// Multi-user mode with networking
    MultiUserNetwork = 3,
    /// Not used (reserved)
    Unused = 4,
    /// Multi-user mode with networking and GUI (X11)
    MultiUserGUI = 5,
    /// Reboot
    Reboot = 6,
}

/// Init system state
pub struct InitState {
    current_runlevel: RunLevel,
    init_process_pid: Option<Pid>,
    services: [Option<ServiceEntry>; MAX_SERVICES],
    respawn_counts: [u32; MAX_SERVICES],
}

/// Service entry for init system
#[derive(Clone, Copy)]
pub struct ServiceEntry {
    pub name: [u8; 32],
    pub name_len: usize,
    pub path: [u8; 64],
    pub path_len: usize,
    pub pid: Option<Pid>,
    pub respawn: bool, // Should we restart if it dies?
    pub runlevels: u8, // Bitmask of runlevels where this should run
    pub priority: u8,  // Start priority (lower = earlier)
}

const MAX_SERVICES: usize = 16;

static INIT_STATE: Mutex<InitState> = Mutex::new(InitState {
    current_runlevel: RunLevel::MultiUser,
    init_process_pid: None,
    services: [None; MAX_SERVICES],
    respawn_counts: [0; MAX_SERVICES],
});

/// Respawn limit per service (prevent fork bombs)
const MAX_RESPAWN_COUNT: u32 = 5;

fn load_system_file(path: &str) -> Option<&'static [u8]> {
    crate::fs::read_file_bytes(path).or_else(|| crate::initramfs::find_file(path))
}

impl InitState {
    #[allow(dead_code)]
    const fn new() -> Self {
        Self {
            current_runlevel: RunLevel::SingleUser,
            init_process_pid: None,
            services: [None; MAX_SERVICES],
            respawn_counts: [0; MAX_SERVICES],
        }
    }
}

/// Initialize the init system
pub fn init() {
    crate::kinfo!("Initializing init system (PID 1 management)");

    let mut state = INIT_STATE.lock();
    state.current_runlevel = RunLevel::MultiUser;

    // Register default services
    // These would typically be read from /etc/inittab or systemd unit files
    register_service_internal(
        &mut state,
        "getty",
        "/sbin/getty",
        true,
        0b00111110, // All runlevels except 0 and 6
        50,
    );

    crate::kinfo!(
        "Init system initialized, runlevel: {:?}",
        state.current_runlevel
    );
}

/// Register a service with the init system
fn register_service_internal(
    state: &mut InitState,
    name: &str,
    path: &str,
    respawn: bool,
    runlevels: u8,
    priority: u8,
) {
    for slot in state.services.iter_mut() {
        if slot.is_none() {
            let mut service = ServiceEntry {
                name: [0; 32],
                name_len: name.len().min(32),
                path: [0; 64],
                path_len: path.len().min(64),
                pid: None,
                respawn,
                runlevels,
                priority,
            };

            service.name[..service.name_len].copy_from_slice(&name.as_bytes()[..service.name_len]);
            service.path[..service.path_len].copy_from_slice(&path.as_bytes()[..service.path_len]);

            *slot = Some(service);
            crate::kinfo!("Registered service: {} -> {}", name, path);
            return;
        }
    }

    crate::kwarn!("Service table full, cannot register: {}", name);
}

/// Register a service with the init system (public API)
pub fn register_service(
    name: &str,
    path: &str,
    respawn: bool,
    runlevels: u8,
    priority: u8,
) -> Result<(), &'static str> {
    let mut state = INIT_STATE.lock();
    register_service_internal(&mut state, name, path, respawn, runlevels, priority);
    Ok(())
}

/// Start the init process (PID 1)
/// This should be called after kernel initialization is complete
pub fn start_init_process(init_path: &str) -> Result<Pid, &'static str> {
    crate::kinfo!("Starting init process: {}", init_path);

    // Try to load init binary from filesystem
    let init_data = load_system_file(init_path).ok_or("Init binary not found")?;

    // Create init process
    let mut init_proc = Process::from_elf(init_data)?;

    // Init process always has PID 1 and PPID 0 (no parent)
    // Note: We need to modify the process creation to ensure PID 1
    if init_proc.pid != INIT_PID {
        crate::kwarn!("Init process PID is {}, expected 1", init_proc.pid);
    }
    init_proc.ppid = 0; // Init has no parent

    // Add to scheduler with highest priority
    scheduler::add_process(init_proc, 0)?;

    // Register in init state
    let mut state = INIT_STATE.lock();
    state.init_process_pid = Some(init_proc.pid);

    crate::kinfo!("Init process started with PID {}", init_proc.pid);

    Ok(init_proc.pid)
}

/// Execute the init process directly (legacy single-process mode)
/// This is used when we only want to run one process
#[allow(unreachable_code)]
pub fn exec_init_process(init_path: &str) -> ! {
    crate::kinfo!("Executing init process in legacy mode: {}", init_path);

    let init_data = load_system_file(init_path).unwrap_or_else(|| {
        crate::kpanic!("Init binary not found: {}", init_path);
    });

    let mut proc = Process::from_elf(init_data).unwrap_or_else(|e| {
        crate::kpanic!("Failed to load init process '{}': {}", init_path, e);
    });

    crate::kinfo!("Init process loaded, switching to user mode...");
    proc.execute(); // Never returns

    // This line should never be reached, but the compiler doesn't know proc.execute() is divergent
    crate::arch::halt_loop()
}

/// Change system runlevel
pub fn change_runlevel(new_level: RunLevel) -> Result<(), &'static str> {
    let mut state = INIT_STATE.lock();
    let old_level = state.current_runlevel;

    crate::kinfo!("Changing runlevel: {:?} -> {:?}", old_level, new_level);

    // Stop services not needed in new runlevel
    let new_mask = 1 << (new_level as u8);
    for (_idx, service_opt) in state.services.iter_mut().enumerate() {
        if let Some(service) = service_opt {
            let should_run = (service.runlevels & new_mask) != 0;

            if !should_run && service.pid.is_some() {
                // Stop this service
                if let Some(_pid) = service.pid {
                    crate::kinfo!(
                        "Stopping service: {}",
                        core::str::from_utf8(&service.name[..service.name_len]).unwrap_or("???")
                    );

                    // Send SIGTERM to stop the service
                    // TODO: Implement send_signal when signal delivery is ready
                    // let _ = crate::signal::send_signal(pid, crate::signal::SIGTERM);
                    service.pid = None;
                }
            }
        }
    }

    state.current_runlevel = new_level;

    // Special handling for halt and reboot
    match new_level {
        RunLevel::Halt => {
            crate::kinfo!("System halting...");
            crate::arch::halt_loop();
        }
        RunLevel::Reboot => {
            crate::kinfo!("System rebooting...");
            system_reboot();
        }
        _ => {
            // Start services for new runlevel
            start_services_for_runlevel(&mut state, new_level);
        }
    }

    Ok(())
}

/// Start services appropriate for the given runlevel
fn start_services_for_runlevel(state: &mut InitState, level: RunLevel) {
    let level_mask = 1 << (level as u8);

    crate::kinfo!("Starting services for runlevel {:?}", level);

    // Sort by priority (lower number = higher priority)
    let mut service_indices: [(u8, usize); MAX_SERVICES] = [(255, 0); MAX_SERVICES];
    for (idx, service_opt) in state.services.iter().enumerate() {
        if let Some(service) = service_opt {
            if (service.runlevels & level_mask) != 0 {
                service_indices[idx] = (service.priority, idx);
            }
        }
    }

    // Simple bubble sort by priority
    for _i in 0..MAX_SERVICES {
        for j in 0..MAX_SERVICES - 1 {
            if service_indices[j].0 > service_indices[j + 1].0 {
                service_indices.swap(j, j + 1);
            }
        }
    }

    // Start services in priority order
    for (priority, idx) in service_indices.iter() {
        if *priority == 255 {
            continue; // Empty slot
        }

        if let Some(service) = &state.services[*idx] {
            if service.pid.is_none() {
                let _ = start_service(state, *idx);
            }
        }
    }
}

/// Start a specific service
fn start_service(state: &mut InitState, service_idx: usize) -> Result<Pid, &'static str> {
    let service = state.services[service_idx]
        .as_ref()
        .ok_or("Invalid service index")?;

    let path = core::str::from_utf8(&service.path[..service.path_len])
        .map_err(|_| "Invalid service path")?;

    crate::kinfo!("Starting service: {}", path);

    // Load service binary
    let binary = load_system_file(path).ok_or("Service binary not found")?;

    // Create process
    let proc = Process::from_elf(binary)?;
    let pid = proc.pid;

    // Add to scheduler
    scheduler::add_process(proc, service.priority)?;

    // Update service entry
    if let Some(service) = &mut state.services[service_idx] {
        service.pid = Some(pid);
    }

    Ok(pid)
}

/// Handle process death - respawn if configured
pub fn handle_process_exit(pid: Pid, exit_code: i32) {
    let mut state = INIT_STATE.lock();

    // Check if this is the init process
    if Some(pid) == state.init_process_pid {
        crate::kpanic!("Init process (PID 1) exited with code {}", exit_code);
    }

    // Find the service index first
    let mut found_idx: Option<usize> = None;
    for (idx, service_opt) in state.services.iter().enumerate() {
        if let Some(service) = service_opt {
            if service.pid == Some(pid) {
                found_idx = Some(idx);
                break;
            }
        }
    }

    // Handle the service if found
    if let Some(idx) = found_idx {
        let (should_respawn, name_copy, name_len_copy) = {
            let service = state.services[idx].as_mut().unwrap();
            let name = core::str::from_utf8(&service.name[..service.name_len]).unwrap_or("???");

            crate::kinfo!(
                "Service '{}' (PID {}) exited with code {}",
                name,
                pid,
                exit_code
            );

            service.pid = None;

            // Copy values before releasing the mutable borrow
            (service.respawn, service.name, service.name_len)
        };

        // Check respawn limit
        let respawn_count = state.respawn_counts[idx];

        if should_respawn && respawn_count < MAX_RESPAWN_COUNT {
            let name = core::str::from_utf8(&name_copy[..name_len_copy]).unwrap_or("???");
            crate::kinfo!("Respawning service '{}'", name);

            if let Ok(new_pid) = start_service(&mut state, idx) {
                state.respawn_counts[idx] += 1;
                crate::kinfo!("Service '{}' respawned with PID {}", name, new_pid);
            } else {
                crate::kerror!("Failed to respawn service '{}'", name);
            }
        } else if should_respawn {
            let name = core::str::from_utf8(&name_copy[..name_len_copy]).unwrap_or("???");
            crate::kerror!("Service '{}' respawn limit reached, giving up", name);
        }
    }
}

/// Get current runlevel
pub fn current_runlevel() -> RunLevel {
    INIT_STATE.lock().current_runlevel
}

/// Check if we are in single-user mode
pub fn is_single_user_mode() -> bool {
    current_runlevel() == RunLevel::SingleUser
}

/// Request system shutdown
pub fn shutdown() {
    crate::kinfo!("System shutdown requested");
    let _ = change_runlevel(RunLevel::Halt);
}

/// Request system reboot
pub fn reboot() {
    crate::kinfo!("System reboot requested");
    let _ = change_runlevel(RunLevel::Reboot);
}

/// Perform system reboot via keyboard controller or triple fault
fn system_reboot() -> ! {
    crate::kinfo!("Attempting keyboard controller reboot...");

    unsafe {
        use x86_64::instructions::port::Port;

        // Method 1: Keyboard controller reboot (traditional method)
        let mut port: Port<u8> = Port::new(0x64);

        // Wait for keyboard controller to be ready
        for _ in 0..1000 {
            if (port.read() & 0x02) == 0 {
                break;
            }
        }

        // Send reboot command
        port.write(0xFE);

        // Wait a bit
        for _ in 0..10000 {
            core::hint::spin_loop();
        }

        crate::kwarn!("Keyboard controller reboot failed, trying triple fault...");

        // Method 2: Triple fault by loading invalid IDT
        core::arch::asm!(
            "lidt [{}]",
            in(reg) &[0u8; 6],
            options(readonly, nostack)
        );

        // Trigger interrupt with invalid IDT
        core::arch::asm!("int 0x03", options(noreturn));
    }
}

/// Emergency sync and halt (called on panic)
pub fn emergency_halt() -> ! {
    crate::kfatal!("Emergency halt requested");

    // Try to sync filesystems (if we had a real filesystem)
    // sync_filesystems();

    // Halt the system
    crate::arch::halt_loop()
}

/// Parse inittab-style configuration (simplified)
/// Format: id:runlevels:action:process
/// Example: "1:2345:respawn:/sbin/getty"
pub fn parse_inittab_line(line: &str) -> Option<ServiceEntry> {
    let parts: [&str; 4] = {
        let mut iter = line.split(':');
        [iter.next()?, iter.next()?, iter.next()?, iter.next()?]
    };

    let _id = parts[0];
    let runlevels_str = parts[1];
    let action = parts[2];
    let process = parts[3];

    // Parse runlevels
    let mut runlevels: u8 = 0;
    for c in runlevels_str.chars() {
        if let Some(digit) = c.to_digit(10) {
            if digit <= 6 {
                runlevels |= 1 << digit;
            }
        }
    }

    // Parse action
    let respawn = action == "respawn";

    // Create service entry
    let mut service = ServiceEntry {
        name: [0; 32],
        name_len: 0,
        path: [0; 64],
        path_len: process.len().min(64),
        pid: None,
        respawn,
        runlevels,
        priority: 50,
    };

    service.path[..service.path_len].copy_from_slice(&process.as_bytes()[..service.path_len]);

    // Extract name from path
    if let Some(pos) = process.rfind('/') {
        let name = &process[pos + 1..];
        service.name_len = name.len().min(32);
        service.name[..service.name_len].copy_from_slice(&name.as_bytes()[..service.name_len]);
    }

    Some(service)
}

/// Load init configuration from /etc/inittab
pub fn load_inittab() -> Result<(), &'static str> {
    crate::kinfo!("Loading /etc/inittab");

    // Try to read /etc/inittab
    let inittab_data = load_system_file("/etc/inittab").ok_or("inittab not found")?;

    let inittab_str = core::str::from_utf8(inittab_data).map_err(|_| "Invalid UTF-8 in inittab")?;

    let mut state = INIT_STATE.lock();

    // Parse each line
    for line in inittab_str.lines() {
        let line = line.trim();

        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(service) = parse_inittab_line(line) {
            // Add to service table
            for slot in state.services.iter_mut() {
                if slot.is_none() {
                    *slot = Some(service);
                    break;
                }
            }
        }
    }

    Ok(())
}
