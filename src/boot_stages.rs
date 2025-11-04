/// Boot stage management for rootfs initialization
/// 
/// This module implements a multi-stage boot process similar to Linux:
/// 1. Bootloader Stage (GRUB/Multiboot2)
/// 2. Kernel Init Stage (hardware detection, memory setup, initramfs unpacking)
/// 3. Initramfs Stage (/proc, /sys mounting, device detection, root mounting)
/// 4. Root Switch Stage (pivot_root, chroot to new root)
/// 5. Real Root Stage (remount rw, mount /usr, /home, start init)
/// 6. User Space Stage (login, shell, services)

use spin::Mutex;

/// Boot stage enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootStage {
    /// Stage 1: Bootloader has loaded kernel and initramfs
    Bootloader,
    /// Stage 2: Kernel is initializing (hardware, memory, GDT, IDT)
    KernelInit,
    /// Stage 3: Initramfs is active, preparing for real root
    InitramfsStage,
    /// Stage 4: Switching from initramfs to real root
    RootSwitch,
    /// Stage 5: Real root is mounted, starting init
    RealRoot,
    /// Stage 6: User space is active
    UserSpace,
    /// Emergency mode due to boot failure
    Emergency,
}

/// Boot configuration parsed from kernel command line
#[derive(Clone, Copy)]
pub struct BootConfig {
    /// Root device (e.g., "/dev/vda1", "UUID=...", "LABEL=...")
    pub root_device: Option<&'static str>,
    /// Root filesystem type (e.g., "ext2", "ext4")
    pub root_fstype: Option<&'static str>,
    /// Root mount options (e.g., "rw", "ro")
    pub root_options: Option<&'static str>,
    /// Init program path
    pub init_path: Option<&'static str>,
    /// Emergency mode flag
    pub emergency: bool,
}

impl BootConfig {
    pub const fn new() -> Self {
        Self {
            root_device: None,
            root_fstype: None,
            root_options: None,
            init_path: None,
            emergency: false,
        }
    }
}

/// Global boot state
struct BootState {
    current_stage: BootStage,
    config: BootConfig,
    initramfs_mounted: bool,
    real_root_mounted: bool,
    proc_mounted: bool,
    sys_mounted: bool,
    dev_mounted: bool,
}

static BOOT_STATE: Mutex<BootState> = Mutex::new(BootState {
    current_stage: BootStage::Bootloader,
    config: BootConfig::new(),
    initramfs_mounted: false,
    real_root_mounted: false,
    proc_mounted: false,
    sys_mounted: false,
    dev_mounted: false,
});

/// Initialize boot stage management
pub fn init() {
    let mut state = BOOT_STATE.lock();
    state.current_stage = BootStage::KernelInit;
    crate::kinfo!("Boot stage: {:?}", state.current_stage);
}

/// Get current boot stage
pub fn current_stage() -> BootStage {
    BOOT_STATE.lock().current_stage
}

/// Advance to next boot stage
pub fn advance_stage(next_stage: BootStage) {
    let mut state = BOOT_STATE.lock();
    let prev_stage = state.current_stage;
    state.current_stage = next_stage;
    crate::kinfo!("Boot stage transition: {:?} -> {:?}", prev_stage, next_stage);
}

/// Set boot configuration from kernel command line
pub fn parse_boot_config(cmdline: &str) {
    let mut state = BOOT_STATE.lock();
    
    crate::kinfo!("Parsing boot configuration from cmdline: {}", cmdline);
    
    for arg in cmdline.split_whitespace() {
        if let Some(value) = arg.strip_prefix("root=") {
            // Store in a static buffer to ensure 'static lifetime
            state.config.root_device = Some(store_static_str(value));
            crate::kinfo!("Boot config: root={}", value);
        } else if let Some(value) = arg.strip_prefix("rootfstype=") {
            state.config.root_fstype = Some(store_static_str(value));
            crate::kinfo!("Boot config: rootfstype={}", value);
        } else if let Some(value) = arg.strip_prefix("rootflags=") {
            state.config.root_options = Some(store_static_str(value));
            crate::kinfo!("Boot config: rootflags={}", value);
        } else if let Some(value) = arg.strip_prefix("init=") {
            state.config.init_path = Some(store_static_str(value));
            crate::kinfo!("Boot config: init={}", value);
        } else if arg == "emergency" || arg == "single" || arg == "1" {
            state.config.emergency = true;
            crate::kinfo!("Boot config: emergency mode enabled");
        }
    }
}

/// Get boot configuration
pub fn boot_config() -> BootConfig {
    BOOT_STATE.lock().config
}

/// Mark filesystem as mounted
pub fn mark_mounted(fs_type: &str) {
    let mut state = BOOT_STATE.lock();
    match fs_type {
        "initramfs" => state.initramfs_mounted = true,
        "proc" => state.proc_mounted = true,
        "sys" => state.sys_mounted = true,
        "dev" => state.dev_mounted = true,
        "rootfs" => state.real_root_mounted = true,
        _ => {}
    }
    crate::kinfo!("Marked {} as mounted", fs_type);
}

/// Check if filesystem is mounted
pub fn is_mounted(fs_type: &str) -> bool {
    let state = BOOT_STATE.lock();
    match fs_type {
        "initramfs" => state.initramfs_mounted,
        "proc" => state.proc_mounted,
        "sys" => state.sys_mounted,
        "dev" => state.dev_mounted,
        "rootfs" => state.real_root_mounted,
        _ => false,
    }
}

/// Enter emergency mode
pub fn enter_emergency_mode(reason: &str) -> ! {
    advance_stage(BootStage::Emergency);
    crate::kerror!("==========================================================");
    crate::kerror!("EMERGENCY MODE: System cannot complete boot");
    crate::kerror!("Reason: {}", reason);
    crate::kerror!("==========================================================");
    crate::kerror!("");
    crate::kerror!("The system encountered a critical error during boot.");
    crate::kerror!("You may attempt manual recovery or inspect the system.");
    crate::kerror!("");
    crate::kerror!("Available actions:");
    crate::kerror!("  - Inspect /sys/block for available block devices");
    crate::kerror!("  - Check kernel log for error messages");
    crate::kerror!("  - Type 'exit' to attempt boot continuation");
    crate::kerror!("");
    
    // Try to spawn emergency shell
    if let Some(sh_data) = crate::initramfs::find_file("/bin/sh") {
        crate::kinfo!("Spawning emergency shell...");
        match crate::process::Process::from_elf(sh_data) {
            Ok(mut proc) => {
                crate::kinfo!("Emergency shell loaded, entering interactive mode");
                proc.execute(); // Never returns
            }
            Err(e) => {
                crate::kfatal!("Failed to load emergency shell: {}", e);
            }
        }
    }
    
    // If we can't spawn a shell, halt
    crate::kfatal!("No emergency shell available, system halted");
    crate::arch::halt_loop()
}

/// Storage for static strings parsed from cmdline
/// We use a simple bump allocator for storing boot config strings
const CMDLINE_BUF_SIZE: usize = 512;
static mut CMDLINE_STORAGE: [u8; CMDLINE_BUF_SIZE] = [0; CMDLINE_BUF_SIZE];
static mut CMDLINE_OFFSET: usize = 0;

fn store_static_str(s: &str) -> &'static str {
    unsafe {
        let len = s.len();
        if CMDLINE_OFFSET + len >= CMDLINE_BUF_SIZE {
            return "(overflow)";
        }
        
        let start = CMDLINE_OFFSET;
        let end = start + len;
        CMDLINE_STORAGE[start..end].copy_from_slice(s.as_bytes());
        CMDLINE_OFFSET = end + 1; // +1 for null terminator space
        
        core::str::from_utf8_unchecked(&CMDLINE_STORAGE[start..end])
    }
}

/// Stage 3: Initramfs stage - mount virtual filesystems and prepare for real root
pub fn initramfs_stage() -> Result<(), &'static str> {
    advance_stage(BootStage::InitramfsStage);
    
    crate::kinfo!("=== Initramfs Stage ===");
    crate::kinfo!("Mounting virtual filesystems...");
    
    // Mount /proc (process information pseudo-filesystem)
    mount_proc()?;
    
    // Mount /sys (sysfs for device and kernel information)
    mount_sys()?;
    
    // Mount /dev (device files)
    mount_dev()?;
    
    crate::kinfo!("Virtual filesystems mounted successfully");
    
    // Wait for root device to appear (simplified - no actual udev)
    // In a real system, this would use udevadm settle
    let config = boot_config();
    if let Some(root_dev) = config.root_device {
        crate::kinfo!("Waiting for root device: {}", root_dev);
        if !wait_for_root_device(root_dev) {
            return Err("Root device not found");
        }
    } else {
        crate::kwarn!("No root device specified, will use initramfs as root");
    }
    
    Ok(())
}

/// Mount /proc pseudo-filesystem
fn mount_proc() -> Result<(), &'static str> {
    crate::kinfo!("Mounting /proc...");
    
    // Create /proc directory in filesystem
    crate::fs::add_directory("/proc");
    
    // In a real implementation, we would populate /proc with process info
    // For now, just mark it as mounted
    mark_mounted("proc");
    
    crate::kinfo!("/proc mounted successfully");
    Ok(())
}

/// Mount /sys pseudo-filesystem
fn mount_sys() -> Result<(), &'static str> {
    crate::kinfo!("Mounting /sys...");
    
    // Create /sys directory
    crate::fs::add_directory("/sys");
    
    // Create basic sysfs structure
    crate::fs::add_directory("/sys/block");
    crate::fs::add_directory("/sys/class");
    crate::fs::add_directory("/sys/devices");
    
    // Add some basic system information
    crate::fs::add_file_bytes("/sys/kernel/version", b"NexaOS 0.0.1\n", false);
    
    mark_mounted("sys");
    
    crate::kinfo!("/sys mounted successfully");
    Ok(())
}

/// Mount /dev device filesystem
fn mount_dev() -> Result<(), &'static str> {
    crate::kinfo!("Mounting /dev...");
    
    // Create /dev directory
    crate::fs::add_directory("/dev");
    
    // Create standard device nodes
    crate::fs::add_file_bytes("/dev/null", b"", false);
    crate::fs::add_file_bytes("/dev/zero", b"", false);
    crate::fs::add_file_bytes("/dev/console", b"", false);
    
    // In a real system, we would have actual device nodes here
    // For now, just create placeholders
    
    mark_mounted("dev");
    
    crate::kinfo!("/dev mounted successfully");
    Ok(())
}

/// Wait for root device to appear (simplified)
fn wait_for_root_device(device: &str) -> bool {
    crate::kinfo!("Checking for root device: {}", device);
    
    // In a real implementation, this would poll /sys/block or use udev events
    // For now, we just check if the device string makes sense
    
    // Simulate device detection delay
    for _ in 0..100 {
        core::hint::spin_loop();
    }
    
    // For now, accept any device specification
    // In a real system, we would check /sys/block/*/dev for the device
    crate::kinfo!("Root device {} detected (simulated)", device);
    true
}

/// Stage 4: Mount real root filesystem at /sysroot
pub fn mount_real_root() -> Result<(), &'static str> {
    let config = boot_config();
    
    if config.root_device.is_none() {
        crate::kinfo!("No root device specified, using initramfs as final root");
        advance_stage(BootStage::RealRoot);
        return Ok(());
    }
    
    let root_dev = config.root_device.unwrap();
    let root_fstype = config.root_fstype.unwrap_or("ext2");
    
    crate::kinfo!("=== Root Mounting Stage ===");
    crate::kinfo!("Mounting {} as {} at /sysroot", root_dev, root_fstype);
    
    // Create /sysroot mount point
    crate::fs::add_directory("/sysroot");
    
    // In a real implementation, we would:
    // 1. Open the block device
    // 2. Run fsck if needed
    // 3. Mount the filesystem
    // For now, just mark as mounted
    
    mark_mounted("rootfs");
    crate::kinfo!("Real root mounted at /sysroot");
    
    Ok(())
}

/// Stage 5: Pivot to real root
pub fn pivot_to_real_root() -> Result<(), &'static str> {
    advance_stage(BootStage::RootSwitch);
    
    crate::kinfo!("=== Root Switch Stage ===");
    crate::kinfo!("Performing pivot_root /sysroot /sysroot/initrd");
    
    // In a real implementation, we would:
    // 1. Move mount points to new root
    // 2. Change root directory with pivot_root syscall
    // 3. Move /proc, /sys, /dev to new root
    // 4. Unmount old initramfs
    
    // For now, we simulate this by just changing boot stage
    advance_stage(BootStage::RealRoot);
    
    crate::kinfo!("Root switch completed");
    Ok(())
}

/// Stage 6: Start init process in real root
pub fn start_real_root_init() -> Result<(), &'static str> {
    advance_stage(BootStage::RealRoot);
    
    crate::kinfo!("=== Real Root Stage ===");
    
    let config = boot_config();
    
    // Remount root as read-write if needed
    if config.root_options == Some("rw") || config.root_options.is_none() {
        crate::kinfo!("Remounting root as read-write");
    }
    
    crate::kinfo!("Real root initialization complete");
    advance_stage(BootStage::UserSpace);
    
    Ok(())
}
