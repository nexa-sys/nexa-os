use crate::bootinfo;
use crate::safety::StaticArena;
use crate::uefi_compat;
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
    crate::kinfo!(
        "Boot stage transition: {:?} -> {:?}",
        prev_stage,
        next_stage
    );
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
    crate::kpanic!("No emergency shell available, system halted");
}

/// Storage for static strings parsed from cmdline
static CMDLINE_STORAGE: StaticArena<512> = StaticArena::new();

fn store_static_str(s: &str) -> &'static str {
    match CMDLINE_STORAGE.store_str(s) {
        Ok(value) => value,
        Err(err) => {
            crate::kwarn!("Boot cmdline arena error: {:?}", err);
            "(overflow)"
        }
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

    // Load kernel modules from initramfs
    // This is similar to Linux's initramfs module loading
    crate::kinfo!("Loading kernel modules from initramfs...");
    crate::kmod::load_initramfs_modules();

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

    // Create subdirectories that may be needed
    // The actual content is dynamically generated by procfs module
    // but we need placeholder directories for some tools

    // Create /proc/sys hierarchy for kernel parameters
    crate::fs::add_directory("/proc/sys");
    crate::fs::add_directory("/proc/sys/kernel");

    mark_mounted("proc");

    crate::kinfo!("/proc mounted successfully (Linux-compatible procfs enabled)");
    Ok(())
}

/// Mount /sys pseudo-filesystem
fn mount_sys() -> Result<(), &'static str> {
    crate::kinfo!("Mounting /sys...");

    // Create /sys directory and basic structure
    crate::fs::add_directory("/sys");
    crate::fs::add_directory("/sys/kernel");
    crate::fs::add_directory("/sys/kernel/random");
    crate::fs::add_directory("/sys/block");
    crate::fs::add_directory("/sys/class");
    crate::fs::add_directory("/sys/class/tty");
    crate::fs::add_directory("/sys/class/block");
    crate::fs::add_directory("/sys/class/net");
    crate::fs::add_directory("/sys/devices");
    crate::fs::add_directory("/sys/bus");
    crate::fs::add_directory("/sys/fs");
    crate::fs::add_directory("/sys/power");

    // Note: Actual content is dynamically generated by sysfs module
    // The old static file is no longer needed as generate_kernel_version() handles it

    mark_mounted("sys");

    crate::kinfo!("/sys mounted successfully (Linux-compatible sysfs enabled)");
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

    uefi_compat::install_device_nodes();

    // In a real system, we would have actual device nodes here
    // For now, just create placeholders

    mark_mounted("dev");

    crate::kinfo!("/dev mounted successfully");
    Ok(())
}

/// Wait for root device to appear (simplified)
///
/// TODO: This is a placeholder implementation that always succeeds.
/// A real implementation would:
/// 1. Poll /sys/block for the device node
/// 2. Use udev events for device hotplug
/// 3. Implement a timeout mechanism
/// 4. Verify device is accessible and has expected filesystem
fn wait_for_root_device(device: &str) -> bool {
    crate::kinfo!("Checking for root device: {}", device);

    // PLACEHOLDER: In a real implementation, this would:
    // - Poll /sys/block/*/dev for matching device
    // - Wait for udev to settle
    // - Verify block device is accessible
    // - Check filesystem signature

    // For now, we accept any device specification as a framework
    crate::kwarn!(
        "Device detection is placeholder - assuming {} exists",
        device
    );
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

    // Step 1: Scan for block device / ext2 image
    // In a real system, this would scan PCI for virtio-blk or AHCI controllers
    // For now, we look for an ext2 disk image in initramfs
    let disk_image = scan_for_block_device(root_dev)?;

    // Step 2: Detect and verify filesystem
    if root_fstype == "ext2" {
        // Check if ext2 module is loaded
        if !crate::fs::ext2_is_module_loaded() {
            crate::kerror!("ext2 module not loaded, cannot mount root filesystem");
            return Err("ext2 module not loaded");
        }

        // Initialize ext2 filesystem via module
        crate::fs::ext2_new(disk_image).map_err(|e| {
            crate::kerror!("Failed to parse ext2 filesystem: {:?}", e);
            "Invalid ext2 filesystem"
        })?;

        crate::kinfo!("Successfully parsed ext2 filesystem via module");

        // Mount the modular ext2 filesystem at /sysroot
        // The Ext2ModularFs is a zero-sized type that delegates to the module
        static EXT2_MODULAR_FS: crate::fs::ext2_modular::Ext2ModularFs =
            crate::fs::ext2_modular::Ext2ModularFs;

        crate::fs::mount_at("/sysroot", &EXT2_MODULAR_FS).map_err(|e| {
            crate::kerror!("Failed to mount filesystem: {:?}", e);
            "Mount failed"
        })?;

        mark_mounted("rootfs");
        crate::kinfo!("Real root mounted at /sysroot (ext2 via module, read-only)");

        Ok(())
    } else {
        crate::kerror!("Unsupported filesystem type: {}", root_fstype);
        Err("Unsupported filesystem type")
    }
}

/// Scan for block device (simplified - looks for disk image in initramfs)
///
/// In a real implementation, this would:
/// 1. Scan PCI bus for storage controllers (virtio-blk, AHCI)
/// 2. Initialize discovered controllers
/// 3. Enumerate block devices
/// 4. Match device by name/UUID/LABEL
fn scan_for_block_device(device_name: &str) -> Result<&'static [u8], &'static str> {
    crate::kinfo!("Scanning for block device: {}", device_name);

    if let Some(data) = bootinfo::rootfs_slice() {
        crate::kinfo!(
            "Using UEFI-staged rootfs image ({} bytes) for {}",
            data.len(),
            device_name
        );
        return Ok(data);
    }

    // Strategy 1: Look for rootfs.ext2 in initramfs
    if let Some(data) = crate::initramfs::find_file("/rootfs.ext2") {
        crate::kinfo!("Found rootfs.ext2 in initramfs ({} bytes)", data.len());
        return Ok(data);
    }

    // Strategy 2: Look for disk.ext2 in initramfs
    if let Some(data) = crate::initramfs::find_file("/disk.ext2") {
        crate::kinfo!("Found disk.ext2 in initramfs ({} bytes)", data.len());
        return Ok(data);
    }

    // Strategy 3: Look for any .ext2 file in initramfs
    let mut found_image: Option<&'static [u8]> = None;
    crate::initramfs::for_each_entry(|entry| {
        if found_image.is_none() && (entry.name.ends_with(".ext2") || entry.name.ends_with(".img"))
        {
            crate::kinfo!(
                "Found disk image: {} ({} bytes)",
                entry.name,
                entry.data.len()
            );
            found_image = Some(entry.data);
        }
    });

    if let Some(data) = found_image {
        return Ok(data);
    }

    // TODO: In a real implementation, scan actual hardware:
    // - Scan PCI for virtio-blk (vendor 0x1AF4, device 0x1001/0x1042)
    // - Scan for AHCI controllers (class 0x01, subclass 0x06)
    // - Initialize controller and read device

    crate::kerror!("No block device or disk image found for '{}'", device_name);
    crate::kerror!("Searched: /rootfs.ext2, /disk.ext2, *.ext2, *.img in initramfs");
    Err("Block device not found")
}

/// Stage 5: Pivot to real root
pub fn pivot_to_real_root() -> Result<(), &'static str> {
    advance_stage(BootStage::RootSwitch);

    crate::kinfo!("=== Root Switch Stage ===");
    crate::kinfo!("Performing pivot_root /sysroot /sysroot/initrd");

    // Step 1: Verify /sysroot is mounted
    if !is_mounted("rootfs") {
        crate::kerror!("Cannot pivot: /sysroot not mounted");
        return Err("Root not mounted");
    }

    // Step 2: Remount root filesystem at /
    // This effectively makes /sysroot the new root
    // In a real implementation, we would use the pivot_root syscall
    // For now, we remount the ext2 filesystem at root
    if crate::fs::ext2_is_module_loaded() && crate::fs::ext2_global().is_some() {
        crate::kinfo!("Remounting ext2 filesystem as new root (via module)");

        // Use the modular ext2 filesystem
        static EXT2_MODULAR_FS: crate::fs::ext2_modular::Ext2ModularFs =
            crate::fs::ext2_modular::Ext2ModularFs;

        // Remount at root (this will override initramfs at /)
        crate::fs::remount_root(&EXT2_MODULAR_FS).map_err(|e| {
            crate::kerror!("Failed to remount root: {}", e);
            "Remount failed"
        })?;

        crate::kinfo!("Root filesystem switched successfully");
    } else {
        crate::kerror!("No ext2 filesystem registered (module not loaded or not initialized)");
        return Err("No filesystem to pivot to");
    }

    // Step 3: Update boot stage
    advance_stage(BootStage::RealRoot);

    // Note: In a full implementation, we would:
    // - Move /proc, /sys, /dev mount points to new root
    // - Create /sysroot/initrd and move old root there
    // - Free initramfs memory

    crate::kinfo!("Root switch completed - now running from real root");
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

    // Load and process /etc/fstab
    process_fstab();

    crate::kinfo!("Real root initialization complete");
    advance_stage(BootStage::UserSpace);

    Ok(())
}

/// Process /etc/fstab and mount configured filesystems
fn process_fstab() {
    crate::kinfo!("Processing /etc/fstab...");

    // Try to load fstab
    match crate::fs::load_fstab() {
        Ok(count) => {
            if count > 0 {
                crate::kinfo!("Loaded {} fstab entries", count);

                // Mount all auto-mount filesystems
                match crate::fs::fstab_mount_all() {
                    Ok(mounted) => {
                        crate::kinfo!("Successfully mounted {} filesystems from fstab", mounted);
                    }
                    Err(e) => {
                        crate::kwarn!("Error mounting filesystems from fstab: {}", e);
                    }
                }
            } else {
                crate::kinfo!("No fstab entries found, using built-in defaults");
                mount_default_tmpfs();
            }
        }
        Err(e) => {
            crate::kwarn!("Failed to load fstab: {}, using defaults", e);
            mount_default_tmpfs();
        }
    }
}

/// Mount default tmpfs filesystems when fstab is not available
fn mount_default_tmpfs() {
    use crate::fs::{mount_tmpfs, TmpfsMountOptions};

    crate::kinfo!("Mounting default tmpfs filesystems...");

    // /tmp - temporary files
    if !crate::fs::file_exists("/tmp") {
        crate::fs::add_directory("/tmp");
    }
    let tmp_opts = TmpfsMountOptions {
        size: 64 * 1024 * 1024, // 64 MiB
        mode: 0o1777,           // sticky bit
        uid: 0,
        gid: 0,
    };
    if let Err(e) = mount_tmpfs("/tmp", tmp_opts) {
        crate::kwarn!("Failed to mount /tmp: {:?}", e);
    }

    // /run - runtime data
    if !crate::fs::file_exists("/run") {
        crate::fs::add_directory("/run");
    }
    let run_opts = TmpfsMountOptions {
        size: 32 * 1024 * 1024, // 32 MiB
        mode: 0o0755,
        uid: 0,
        gid: 0,
    };
    if let Err(e) = mount_tmpfs("/run", run_opts) {
        crate::kwarn!("Failed to mount /run: {:?}", e);
    }

    // /dev/shm - shared memory
    if !crate::fs::file_exists("/dev/shm") {
        crate::fs::add_directory("/dev/shm");
    }
    let shm_opts = TmpfsMountOptions {
        size: 64 * 1024 * 1024, // 64 MiB
        mode: 0o1777,           // sticky bit
        uid: 0,
        gid: 0,
    };
    if let Err(e) = mount_tmpfs("/dev/shm", shm_opts) {
        crate::kwarn!("Failed to mount /dev/shm: {:?}", e);
    }

    crate::kinfo!("Default tmpfs filesystems mounted");
}

