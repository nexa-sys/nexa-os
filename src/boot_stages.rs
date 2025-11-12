use crate::bootinfo;
use crate::uefi_compat;
use crate::safety::StaticArena;
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

    // Add system information
    // TODO: Read version from build-time constant
    const KERNEL_VERSION: &[u8] = b"NexaOS 0.0.1 (experimental)\n";
    crate::fs::add_file_bytes("/sys/kernel/version", KERNEL_VERSION, false);

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
        // Parse ext2 filesystem
        let ext2_fs = crate::fs::ext2::Ext2Filesystem::new(disk_image).map_err(|e| {
            crate::kerror!("Failed to parse ext2 filesystem: {:?}", e);
            "Invalid ext2 filesystem"
        })?;

        crate::kinfo!("Successfully parsed ext2 filesystem");

        // Step 3: Register and mount the filesystem
        let fs_ref = crate::fs::ext2::register_global(ext2_fs);

        // Mount at /sysroot
        crate::fs::mount_at("/sysroot", fs_ref).map_err(|e| {
            crate::kerror!("Failed to mount filesystem: {:?}", e);
            "Mount failed"
        })?;

        mark_mounted("rootfs");
        crate::kinfo!("Real root mounted at /sysroot (ext2, read-only)");

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

    // First, try to use UEFI-staged rootfs image
    if let Some(data) = bootinfo::rootfs_slice() {
        crate::kinfo!(
            "Using UEFI-staged rootfs image ({} bytes) for {}",
            data.len(),
            device_name
        );
        return Ok(data);
    }

    // Try to find a block device that was passed from UEFI
    if let Some(block_devices) = bootinfo::block_devices() {
        // In a real implementation, we would match the device_name (e.g. "/dev/vda1") 
        // with the actual block device information.
        // For now, we just take the first available block device.
        for block_device in block_devices {
            // Try to find the corresponding PCI device to get the memory mapping
            if let Some(pci_device) = bootinfo::pci_device_by_location(
                block_device.pci_segment,
                block_device.pci_bus,
                block_device.pci_device,
                block_device.pci_function
            ) {
                // Look for a valid MMIO BAR
                for bar in &pci_device.bars {
                    if bar.base != 0 && (bar.bar_flags & nexa_boot_info::bar_flags::IO_SPACE) == 0 {
                        // This is a valid MMIO region - in a real implementation we would
                        // map this region and read the block device data from it.
                        // For now, we'll just log that we found it.
                        crate::kinfo!(
                            "Found block device: PCI {:04x}:{:02x}:{:02x}.{} with MMIO BAR at {:#x}",
                            block_device.pci_segment,
                            block_device.pci_bus,
                            block_device.pci_device,
                            block_device.pci_function,
                            bar.base
                        );
                        
                        // Return a slice pointing to the MMIO region as our block device data
                        // Note: In a real implementation, we would need to properly initialize
                        // the storage controller (virtio-blk, AHCI) before accessing it.
                        // This is just a placeholder to demonstrate the concept.
                        // Compute total bytes from last_block (inclusive) and block_size.
                        // UEFI Block I/O reports `last_block` (0-based), so number of blocks = last_block + 1.
                        let num_blocks = (block_device.last_block as usize).saturating_add(1);
                        let total_bytes = num_blocks.saturating_mul(block_device.block_size as usize);

                        let data = unsafe {
                            core::slice::from_raw_parts(bar.base as *const u8, total_bytes)
                        };

                        crate::kinfo!(
                            "Returning MMIO-mapped block device data: {} blocks of {} bytes each",
                            num_blocks,
                            block_device.block_size
                        );
                        
                        return Ok(data);
                    }
                }
            }
        }
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
    if let Some(ext2_fs) = crate::fs::ext2::global() {
        crate::kinfo!("Remounting ext2 filesystem as new root");

        // Remount at root (this will override initramfs at /)
        crate::fs::remount_root(ext2_fs).map_err(|e| {
            crate::kerror!("Failed to remount root: {}", e);
            "Remount failed"
        })?;

        crate::kinfo!("Root filesystem switched successfully");
    } else {
        crate::kerror!("No ext2 filesystem registered");
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

    crate::kinfo!("Real root initialization complete");
    advance_stage(BootStage::UserSpace);

    Ok(())
}
