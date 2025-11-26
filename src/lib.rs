#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

mod acpi;
pub mod allocator;
pub mod arch;
pub mod auth;
pub mod boot_stages;
pub mod bootinfo;
pub mod elf;
pub mod framebuffer;
pub mod fs;
pub mod gdt;
pub mod init;
pub mod initramfs;
pub mod interrupts;
pub mod ipc;
pub mod keyboard;
pub mod lapic;
pub mod logger;
pub mod memory;
pub mod net;
pub mod numa;
pub mod paging;
pub mod pipe;
pub mod posix;
pub mod process;
pub mod safety;
pub mod scheduler;
pub mod serial;
pub mod signal;
pub mod smp;
pub mod syscalls;
pub mod uefi_compat;
pub mod vga_buffer;
pub mod vt;

use core::panic::PanicInfo;
use multiboot2::{BootInformation, BootInformationHeader};
use nexa_boot_info::{device_flags, BootInfo};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
pub const MULTIBOOT_BOOTLOADER_MAGIC: u32 = 0x2BADB002; // Multiboot v1
pub const MULTIBOOT2_BOOTLOADER_MAGIC: u32 = 0x36d76289; // Multiboot v2

pub fn kernel_main(multiboot_info_address: u64, magic: u32) -> ! {
    bootinfo::clear();
    uefi_compat::reset();

    // Stage 1: Bootloader has loaded us (GRUB/Multiboot2)
    let freq_hz = logger::init();
    let boot_info = unsafe {
        BootInformation::load(multiboot_info_address as *const BootInformationHeader)
            .expect("valid multiboot info structure")
    };

    let cmdline_multiboot = boot_info
        .command_line_tag()
        .and_then(|tag| tag.cmdline().ok())
        .map(|line| bootinfo::stash_cmdline(line));

    let cmdline_uefi = bootinfo::cmdline_str().map(|line| bootinfo::stash_cmdline(line));

    let cmdline_effective = cmdline_multiboot.or(cmdline_uefi);

    if let Some(line) = cmdline_effective {
        if let Some(level) = logger::parse_level_directive(line) {
            logger::set_max_level(level);
        }
    }

    framebuffer::early_init(&boot_info);
    vga_buffer::init();

    kinfo!("Kernel log level set to {}", logger::max_level().as_str());

    // Stage 2: Kernel Init - Initialize boot stage tracking
    boot_stages::init();

    kinfo!("==========================================================");
    kinfo!("NexaOS Kernel Bootstrap");
    kinfo!("==========================================================");
    kinfo!("Stage 1: Bootloader - Complete");
    kinfo!("Stage 2: Kernel Init - Starting...");
    kdebug!("Multiboot magic: {:#x}", magic);
    kdebug!("Multiboot info struct at: {:#x}", multiboot_info_address);

    if let Some(cmdline) = cmdline_effective {
        boot_stages::parse_boot_config(cmdline);
    }

    if logger::tsc_frequency_is_guessed() {
        kwarn!(
            "Falling back to default TSC frequency: {}.{:03} MHz",
            freq_hz / 1_000_000,
            (freq_hz % 1_000_000) / 1_000
        );
    } else {
        kinfo!(
            "Detected invariant TSC frequency: {}.{:03} MHz",
            freq_hz / 1_000_000,
            (freq_hz % 1_000_000) / 1_000
        );
    }

    if magic != MULTIBOOT2_BOOTLOADER_MAGIC && magic != MULTIBOOT_BOOTLOADER_MAGIC {
        kpanic!("Invalid Multiboot magic value: {:#x}", magic);
    }

    if magic == MULTIBOOT2_BOOTLOADER_MAGIC {
        memory::log_memory_overview(&boot_info);

        // Initialize Allocator
        // Try to find a region larger than 32MB
        if let Some((heap_start, heap_size)) =
            memory::find_heap_region(&boot_info, 32 * 1024 * 1024)
        {
            // Ensure we don't overwrite the kernel (loaded at 1MB).
            // We'll start the heap at least at 64MB to be safe.
            let min_heap_addr = 64 * 1024 * 1024;

            let effective_start = if heap_start < min_heap_addr {
                min_heap_addr
            } else {
                heap_start
            };

            if effective_start < heap_start + heap_size {
                let effective_size = (heap_start + heap_size) - effective_start;
                allocator::init_kernel_heap(effective_start, effective_size);
            } else {
                kwarn!(
                    "Heap region {:#x} is below safe threshold {:#x}",
                    heap_start,
                    min_heap_addr
                );
            }
        } else {
            kwarn!("No suitable memory region found for kernel heap!");
        }

        paging::ensure_nxe_enabled();

        configure_gs_base();

        // Load initramfs from multiboot module if present
        if let Some(modules_tag) = boot_info.module_tags().next() {
            let module_start = modules_tag.start_address() as *const u8;
            let module_size = (modules_tag.end_address() - modules_tag.start_address()) as usize;
            if module_size > 0 {
                kinfo!(
                    "Found initramfs module at {:#x}, size {} bytes",
                    module_start as usize,
                    module_size
                );
                kinfo!("About to call initramfs::init()");
                unsafe {
                    initramfs::init(module_start, module_size);
                }
                kinfo!("initramfs::init() completed");
            }
        } else {
            kwarn!("No initramfs module found, using built-in filesystem");
        }
    } else {
        kwarn!("Multiboot v1 detected; memory overview is not yet supported.");
    }

    proceed_after_initramfs(cmdline_effective)
}

pub fn kernel_main_uefi(boot_info_ptr: *const BootInfo) -> ! {
    if boot_info_ptr.is_null() {
        kpanic!("UEFI entry invoked with null boot info pointer");
    }

    let boot_info = unsafe { &*boot_info_ptr };

    kinfo!("UEFI boot info pointer: {:#x}", boot_info_ptr as u64);

    let mut signature_probe = [0u8; 8];
    unsafe {
        let raw = core::slice::from_raw_parts(boot_info_ptr as *const u8, signature_probe.len());
        signature_probe.copy_from_slice(raw);
    }
    kinfo!("UEFI boot info raw signature bytes: {:?}", signature_probe);

    if let Err(err) = bootinfo::set(boot_info) {
        match err {
            bootinfo::BootInfoError::InvalidSignature => {
                kpanic!("UEFI boot info signature mismatch")
            }
            bootinfo::BootInfoError::UnsupportedVersion(ver) => {
                kpanic!("Unsupported UEFI boot info version: {}", ver)
            }
        }
    }

    uefi_compat::reset();

    let freq_hz = logger::init();

    let cmdline_effective = bootinfo::cmdline_str().map(|line| bootinfo::stash_cmdline(line));

    if let Some(line) = cmdline_effective {
        if let Some(level) = logger::parse_level_directive(line) {
            logger::set_max_level(level);
        }
    }

    uefi_compat::init();

    if let Some(fb) = bootinfo::framebuffer_info() {
        framebuffer::install_from_bootinfo(&fb);
    }

    vga_buffer::init();

    kinfo!("Kernel log level set to {}", logger::max_level().as_str());

    boot_stages::init();

    kinfo!("==========================================================");
    kinfo!("NexaOS Kernel Bootstrap (UEFI)");
    kinfo!("==========================================================");
    kinfo!("Stage 1: UEFI Loader - Complete");
    kinfo!("Stage 2: Kernel Init - Starting...");

    if let Some(offset) = bootinfo::kernel_load_offset() {
        kinfo!("Kernel relocation offset reported by loader: {:#x}", offset);
    }
    if let Some((expected, actual)) = bootinfo::kernel_entry_points() {
        kdebug!(
            "Kernel entry points -> expected: {:#x}, actual: {:#x}",
            expected,
            actual
        );
    }
    if let Some(segments) = bootinfo::kernel_segments() {
        kdebug!("Loader reported {} relocated segment(s)", segments.len());
    }

    if let Some(cmdline) = cmdline_effective {
        boot_stages::parse_boot_config(cmdline);
    }

    if let Some(pci_iter) = bootinfo::pci_devices() {
        for dev in pci_iter {
            let blk = if (dev.device_flags & device_flags::BLOCK) != 0 {
                "y"
            } else {
                "n"
            };
            let net = if (dev.device_flags & device_flags::NETWORK) != 0 {
                "y"
            } else {
                "n"
            };
            let usb = if (dev.device_flags & device_flags::USB_HOST) != 0 {
                "y"
            } else {
                "n"
            };
            let gfx = if (dev.device_flags & device_flags::GRAPHICS) != 0 {
                "y"
            } else {
                "n"
            };
            kinfo!(
                "UEFI PCI {:04x}:{:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x}-{:02x}-{:02x} caps[blk={},net={},usb={},gfx={}]",
                dev.segment,
                dev.bus,
                dev.device,
                dev.function,
                dev.vendor_id,
                dev.device_id,
                dev.class_code,
                dev.subclass,
                dev.prog_if,
                blk,
                net,
                usb,
                gfx
            );
        }
    }

    // Initialize Allocator (UEFI path)
    // We don't have a memory map from BootInfo, so we hardcode a safe region.
    // Kernel is at 1MB, size ~35MB.
    // Page tables start at 128MB (0x0800_0000).
    // So 64MB (0x0400_0000) to 128MB is a safe 64MB region.
    let heap_start = 0x0400_0000;
    let heap_size = 64 * 1024 * 1024; // 64MB
    allocator::init_kernel_heap(heap_start, heap_size);

    if logger::tsc_frequency_is_guessed() {
        kwarn!(
            "Falling back to default TSC frequency: {}.{:03} MHz",
            freq_hz / 1_000_000,
            (freq_hz % 1_000_000) / 1_000
        );
    } else {
        kinfo!(
            "Detected invariant TSC frequency: {}.{:03} MHz",
            freq_hz / 1_000_000,
            (freq_hz % 1_000_000) / 1_000
        );
    }

    paging::ensure_nxe_enabled();
    configure_gs_base();

    if let Some(initramfs) = bootinfo::initramfs_slice() {
        kinfo!(
            "UEFI loader provided initramfs payload at {:#x} ({} bytes)",
            initramfs.as_ptr() as usize,
            initramfs.len()
        );
        unsafe {
            initramfs::init(initramfs.as_ptr(), initramfs.len());
        }
    } else {
        kwarn!("UEFI loader did not supply an initramfs payload");
    }

    if let Some(rootfs) = bootinfo::rootfs_slice() {
        kinfo!(
            "UEFI loader staged rootfs image at {:#x} ({} bytes)",
            rootfs.as_ptr() as usize,
            rootfs.len()
        );
    }

    proceed_after_initramfs(cmdline_effective)
}

fn proceed_after_initramfs(cmdline_opt: Option<&'static str>) -> ! {
    // Initialize paging (required for user mode)
    paging::init();
    framebuffer::activate();

    // Bring up the multi-terminal console before user processes start
    const DEFAULT_VT_COUNT: usize = 4;
    vt::init(DEFAULT_VT_COUNT);

    enable_floating_point_unit();

    // Check INITRAMFS after paging::init()
    kinfo!(
        "INITRAMFS after paging::init(): {}",
        crate::initramfs::get().is_some()
    );

    // Initialize GDT for user/kernel mode
    gdt::init();

    // Check INITRAMFS after gdt::init()
    kinfo!(
        "INITRAMFS after gdt::init(): {}",
        crate::initramfs::get().is_some()
    );

    kinfo!("About to call interrupts::init_interrupts()");

    // Initialize interrupts and system calls
    // This is a critical section - any failure here will cause a triple fault
    // Note: Currently assumes single-core initialization (BSP only)
    // TODO: Implement per-core IDT for SMP support
    crate::interrupts::init_interrupts();

    kinfo!("interrupts::init() completed successfully");

    // Enable interrupts now that essential handlers and PIC configuration are in place
    x86_64::instructions::interrupts::enable();
    kinfo!("CPU interrupts enabled");

    // Debug: Check initramfs state before filesystem init
    kinfo!("Checking initramfs state before fs::init()...");
    kinfo!(
        "INITRAMFS check before fs::init(): {}",
        crate::initramfs::get().is_some()
    );

    // Initialize subsystems in dependency order
    auth::init(); // User authentication system
    ipc::init(); // Inter-process communication
    signal::init(); // POSIX signal handling
    pipe::init(); // Pipe system

    // Initialize SMP after interrupts are enabled but before scheduler
    // This allows AP cores to come online safely
    crate::smp::init();

    // Initialize NUMA topology detection after SMP/ACPI init
    // This must come before scheduler for NUMA-aware load balancing
    if let Err(e) = numa::init() {
        kwarn!("NUMA initialization failed: {}", e);
    }

    // Initialize NUMA-aware memory allocator (uses NUMA topology)
    allocator::init_numa_allocator();

    scheduler::init(); // Process scheduler
    fs::init(); // Filesystem
    init::init(); // Init system (PID 1 management)

    let elapsed_us = logger::boot_time_us();
    kinfo!(
        "Kernel initialization completed in {}.{:03} ms",
        elapsed_us / 1_000,
        elapsed_us % 1_000
    );

    kinfo!("Stage 2: Kernel Init - Complete");
    boot_stages::mark_mounted("initramfs");

    // Stage 3: Initramfs Stage - Mount virtual filesystems and prepare for real root
    kinfo!("Stage 3: Initramfs Stage - Starting...");
    if let Err(e) = boot_stages::initramfs_stage() {
        boot_stages::enter_emergency_mode(e);
    }
    kinfo!("Stage 3: Initramfs Stage - Complete");

    // Initialize network stack (before mounting real root)
    kinfo!("Initializing network subsystem...");
    crate::net::init();
    kinfo!("Network subsystem initialized");

    // Stage 4 & 5: Mount real root (if specified) or use initramfs
    let config = boot_stages::boot_config();
    if config.root_device.is_some() {
        kinfo!("Stage 4: Root Mounting - Starting...");
        if let Err(e) = boot_stages::mount_real_root() {
            boot_stages::enter_emergency_mode(e);
        }
        kinfo!("Stage 4: Root Mounting - Complete");

        kinfo!("Stage 5: Root Switch - Starting...");
        if let Err(e) = boot_stages::pivot_to_real_root() {
            boot_stages::enter_emergency_mode(e);
        }
        kinfo!("Stage 5: Root Switch - Complete");

        if let Err(e) = boot_stages::start_real_root_init() {
            boot_stages::enter_emergency_mode(e);
        }
    } else {
        kinfo!("No root device specified, using initramfs as final root");
        boot_stages::advance_stage(boot_stages::BootStage::RealRoot);
        boot_stages::advance_stage(boot_stages::BootStage::UserSpace);
    }

    // Try to load init configuration (Unix-like /etc/inittab)
    if let Err(e) = init::load_inittab() {
        kwarn!("Failed to load /etc/inittab: {}", e);
        kwarn!("Using default init configuration");
        init::register_default_gettys();
    }

    // Stage 6: User Space - Start init process
    kinfo!("==========================================================");
    kinfo!("Stage 6: User Space - Starting init process");
    kinfo!("==========================================================");

    // Parse kernel command line for init= parameter (POSIX convention)
    let cmdline = cmdline_opt.unwrap_or("");
    let cmd_init_path = parse_init_from_cmdline(cmdline).unwrap_or("(none)");

    // Standard Unix init search paths (in order of preference)
    // Following FHS (Filesystem Hierarchy Standard) and POSIX conventions
    static INIT_PATHS: &[&str] = &[
        "/sbin/ni",   // Nexa Init (primary)
        "/sbin/init", // Traditional init location (fallback)
        "/etc/init",  // Alternative init location
        "/bin/init",  // Fallback init location
        "/bin/sh",    // Emergency shell (minimal init)
    ];

    kinfo!(
        "Searching for init in {} standard locations",
        INIT_PATHS.len()
    );

    // Try to load init process into scheduler
    let mut init_pid: Option<u64> = None;

    // Try custom init path first (if specified on command line)
    if cmd_init_path != "(none)" {
        kinfo!("Custom init path from cmdline: {}", cmd_init_path);
        init_pid = try_load_init(cmd_init_path);
    }

    // If custom init failed, try standard paths
    if init_pid.is_none() {
        for &path in INIT_PATHS.iter() {
            kinfo!("Trying init program: {}", path);
            if let Some(pid) = try_load_init(path) {
                init_pid = Some(pid);
                break;
            }

            // If /bin/sh is not found, this is a critical failure
            if path == "/bin/sh" {
                kfatal!("Critical: No init program found in initramfs");
                kfatal!("Searched paths: /sbin/ni, /sbin/init, /etc/init, /bin/init, /bin/sh");
                kfatal!("Cannot continue without init process (PID 1)");
                boot_stages::enter_emergency_mode("No init program found");
            }
        }
    }

    // If we have init loaded, start the scheduler
    // All processes (including init) run through the scheduler
    if let Some(pid) = init_pid {
        kinfo!("==========================================================");
        kinfo!("Init process loaded (PID {}), starting scheduler", pid);
        kinfo!("==========================================================");

        // Set init as current process
        scheduler::set_current_pid(Some(pid));

        // Mark init as Ready (scheduler will pick it up)
        let _ = scheduler::set_process_state(pid, process::ProcessState::Ready);

        // 标记 init 已启动 - 此后内核日志将只输出到环形缓冲区
        logger::mark_init_started();

        // Start the scheduler - this will switch to init and never return
        kinfo!("Starting process scheduler");
        scheduler::do_schedule();

        // Should never reach here
        kfatal!("Scheduler returned to kernel_main!");
    }

    // If we reach here, all init programs failed to load
    boot_stages::enter_emergency_mode("Failed to load any init program")
}

pub(crate) fn configure_gs_base() {
    unsafe {
        let gs_data_addr = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;
        use x86_64::registers::model_specific::Msr;
        Msr::new(0xc0000101).write(gs_data_addr);
        kinfo!("GS base set to {:#x}", gs_data_addr);
    }
}

/// Try to load init program from given path
/// Returns Some(pid) if successful, None otherwise
fn try_load_init(path: &str) -> Option<u64> {
    // Try root filesystem first
    if let Some(init_data) = fs::read_file_bytes(path) {
        kinfo!(
            "Found init file '{}' in root filesystem ({} bytes), loading...",
            path,
            init_data.len()
        );

        match process::Process::from_elf(init_data) {
            Ok(proc) => {
                let pid = proc.pid;
                kinfo!(
                    "Successfully loaded '{}' from root filesystem as PID {}",
                    path,
                    pid
                );
                kinfo!("Adding init process to scheduler...");

                // Add init process to scheduler
                if let Err(e) = scheduler::add_process(proc, 0) {
                    kwarn!("Failed to add init process to scheduler: {}", e);
                    return None;
                }

                kinfo!("Init process (PID {}) added to scheduler", pid);
                // Set init as current process
                scheduler::set_current_pid(Some(pid));
                return Some(pid);
            }
            Err(e) => {
                kwarn!("Failed to load '{}' from root filesystem: {}", path, e);
            }
        }
    }

    // Try initramfs
    if let Some(init_data) = initramfs::find_file(path) {
        kinfo!(
            "Found init file '{}' in initramfs ({} bytes), loading...",
            path,
            init_data.len()
        );

        match process::Process::from_elf(init_data) {
            Ok(proc) => {
                let pid = proc.pid;
                kinfo!(
                    "Successfully loaded '{}' from initramfs as PID {}",
                    path,
                    pid
                );
                kinfo!("Adding init process to scheduler...");

                // Add init process to scheduler
                if let Err(e) = scheduler::add_process(proc, 0) {
                    kwarn!("Failed to add init process to scheduler: {}", e);
                    return None;
                }

                kinfo!("Init process (PID {}) added to scheduler", pid);
                // Set init as current process
                scheduler::set_current_pid(Some(pid));
                return Some(pid);
            }
            Err(e) => {
                kwarn!("Failed to load '{}' from initramfs: {}", path, e);
            }
        }
    } else {
        kwarn!(
            "Init file '{}' not found on root filesystem or in initramfs",
            path
        );
    }

    None
}

pub fn panic(info: &PanicInfo) -> ! {
    kpanic!("{}", info);
}

fn enable_floating_point_unit() {
    unsafe {
        let mut cr0 = Cr0::read();
        cr0.remove(Cr0Flags::EMULATE_COPROCESSOR | Cr0Flags::TASK_SWITCHED);
        cr0.insert(Cr0Flags::MONITOR_COPROCESSOR);
        Cr0::write(cr0);

        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSFXSR | Cr4Flags::OSXMMEXCPT_ENABLE);
        Cr4::write(cr4);
    }

    kinfo!("Enabled FPU/SSE support for user mode execution");
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::vga_buffer::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! serial_println {
    () => { $crate::serial_print!("\n") };
    ($($arg:tt)*) => {{
        $crate::serial::_print(format_args!($($arg)*));
        $crate::serial::_print(format_args!("\n"));
    }};
}

#[macro_export]
macro_rules! klog {
    ($level:expr, $($arg:tt)*) => {{
        $crate::logger::log($level, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! kpanic {
    ($($arg:tt)*) => {{
        use core::arch::asm;
        let loc = core::panic::Location::caller();
        let message = format_args!($($arg)*);

        let cpu_id: u32 = unsafe {
            #[cfg(target_arch = "x86_64")]
            {
                use core::arch::x86_64::__cpuid;
                (__cpuid(1).ebx >> 24) as u32
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                0
            }
        };

        $crate::klog!(
            $crate::logger::LogLevel::PANIC,
            "------------[ cut here ]------------"
        );

        $crate::logger::log(
            $crate::logger::LogLevel::PANIC,
            format_args!("Kernel panic - not syncing: {}", message)
        );

        $crate::klog!(
            $crate::logger::LogLevel::PANIC,
            "CPU: {cpu} PID: 0 Comm: kernel Tainted: N/A",
            cpu = cpu_id
        );

        $crate::klog!(
            $crate::logger::LogLevel::PANIC,
            "Hardware name: NexaOS experimental"
        );

        $crate::klog!(
            $crate::logger::LogLevel::PANIC,
            "Call Trace: <panic> at {file}:{line}:{column}",
            file = loc.file(),
            line = loc.line(),
            column = loc.column(),
        );

        unsafe {
            let cr0: u64;
            let cr2: u64;
            let cr3: u64;
            let cr4: u64;
            asm!("mov {0}, cr0", out(reg) cr0);
            asm!("mov {0}, cr2", out(reg) cr2);
            asm!("mov {0}, cr3", out(reg) cr3);
            asm!("mov {0}, cr4", out(reg) cr4);
            $crate::klog!(
                $crate::logger::LogLevel::PANIC,
                "Control: CR0={cr0:#018x} CR2={cr2:#018x} CR3={cr3:#018x} CR4={cr4:#018x}",
                cr0 = cr0,
                cr2 = cr2,
                cr3 = cr3,
                cr4 = cr4,
            );
        }

        {
            let (rip, rsp, rbp, rflags): (u64, u64, u64, u64);
            unsafe {
                asm!("lea {0}, [rip + 0]", out(reg) rip);
                asm!("mov {0}, rsp", out(reg) rsp);
                asm!("mov {0}, rbp", out(reg) rbp);
                asm!("pushf; pop {0}", out(reg) rflags);
            }
            let interrupt_enabled = (rflags & (1 << 9)) != 0;
            $crate::klog!(
                $crate::logger::LogLevel::PANIC,
                "RIP: {rip:#018x} RSP: {rsp:#018x} RBP: {rbp:#018x} RFLAGS: {rflags:#018x} (IF={})",
                interrupt_enabled,
                rip = rip,
                rsp = rsp,
                rbp = rbp,
                rflags = rflags,
            );
        }

        $crate::klog!(
            $crate::logger::LogLevel::PANIC,
            "------------[ end Kernel panic ]------------"
        );
        $crate::arch::halt_loop()
    }};
}

#[macro_export]
macro_rules! kfatal {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::FATAL, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kerror {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::ERROR, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kwarn {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::WARN, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kinfo {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::INFO, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kdebug {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::DEBUG, $($arg)*);
    }};
}

#[macro_export]
macro_rules! ktrace {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::TRACE, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {{
        $crate::vga_buffer::_print(format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! kprintln {
    () => { $crate::kprint!("\n") };
    ($($arg:tt)*) => {{
        $crate::kprint!($($arg)*);
        $crate::kprint!("\n");
    }};
}

fn parse_init_from_cmdline(cmdline: &str) -> Option<&str> {
    for arg in cmdline.split_whitespace() {
        if let Some(value) = arg.strip_prefix("init=") {
            return Some(value);
        }
    }
    None
}
