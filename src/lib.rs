#![no_std]
#![feature(abi_x86_interrupt)]

pub mod arch;
pub mod auth;
pub mod elf;
pub mod framebuffer;
pub mod fs;
pub mod gdt;
pub mod initramfs;
pub mod interrupts;
pub mod ipc;
pub mod keyboard;
pub mod logger;
pub mod memory;
pub mod paging;
pub mod posix;
pub mod process;
pub mod serial;
pub mod syscall;
pub mod vga_buffer;

use core::panic::PanicInfo;
use multiboot2::{BootInformation, BootInformationHeader};
pub const MULTIBOOT_BOOTLOADER_MAGIC: u32 = 0x2BADB002; // Multiboot v1
pub const MULTIBOOT2_BOOTLOADER_MAGIC: u32 = 0x36d76289; // Multiboot v2

pub fn kernel_main(multiboot_info_address: u64, magic: u32) -> ! {
    let freq_hz = logger::init();
    let boot_info = unsafe {
        BootInformation::load(multiboot_info_address as *const BootInformationHeader)
            .expect("valid multiboot info structure")
    };

    let cmdline_opt = boot_info
        .command_line_tag()
        .and_then(|tag| tag.cmdline().ok());

    if let Some(line) = cmdline_opt {
        if let Some(level) = logger::parse_level_directive(line) {
            logger::set_max_level(level);
        }
    }

    framebuffer::early_init(&boot_info);
    vga_buffer::init();

    kinfo!("Kernel log level set to {}", logger::max_level().as_str());

    kinfo!("NexaOS kernel bootstrap start");
    kdebug!("Multiboot magic: {:#x}", magic);
    kdebug!("Multiboot info struct at: {:#x}", multiboot_info_address);

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
        kerror!("Invalid Multiboot magic value: {:#x}", magic);
        arch::halt_loop();
    }

    if magic == MULTIBOOT2_BOOTLOADER_MAGIC {
        memory::log_memory_overview(&boot_info);

        paging::ensure_nxe_enabled();

        // Set GS base EARLY to avoid corrupting static variables during initramfs loading
        unsafe {
            // Get GS_DATA address without creating a reference that might corrupt nearby statics
            let gs_data_addr = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;
            use x86_64::registers::model_specific::Msr;
            Msr::new(0xc0000101).write(gs_data_addr); // GS base
            kinfo!("GS base set to {:#x}", gs_data_addr);
        }

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

    // Initialize paging (required for user mode)
    paging::init();
    framebuffer::activate();

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

    auth::init();
    ipc::init();

    // Initialize filesystem
    fs::init();

    let elapsed_us = logger::boot_time_us();
    kinfo!(
        "Kernel initialization completed in {}.{:03} ms",
        elapsed_us / 1_000,
        elapsed_us % 1_000
    );

    let cmdline = cmdline_opt.unwrap_or("");
    let cmd_init_path = parse_init_from_cmdline(cmdline).unwrap_or("(none)");

    if cmd_init_path != "(none)" {
        kinfo!("Custom init path: {}", cmd_init_path);
        try_init_exec!(cmd_init_path);
    }

    static INIT_PATHS: &[&str] = &["/sbin/init", "/etc/init", "/bin/init", "/bin/sh"];

    kinfo!("Using default init file list: {}", INIT_PATHS.len());
    kinfo!("Pausing briefly before starting init");
    for &path in INIT_PATHS.iter() {
        kinfo!("Trying init file: {}", path);
        try_init_exec!(path);
        if path == "/bin/sh" {
            kfatal!("'/bin/sh' not found in initramfs;");
            kfatal!("cannot initialize user mode.");
            kpanic!("Final fallback init program not found.");
        }
    }

    // Try to load /bin/sh from initramfs and
    // If we reach here, /bin/sh executed successfully, but it should never return
    kpanic!("Unexpected return from user mode process");
}

pub fn panic(info: &PanicInfo) -> ! {
    kfatal!("KERNEL PANIC: {}", info);

    arch::halt_loop()
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::vga_buffer::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => {{
        $crate::vga_buffer::_print(format_args!($($arg)*));
        $crate::vga_buffer::_print(format_args!("\n"));
    }};
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

#[macro_export]
macro_rules! try_init_exec {
    ($path:expr) => {{
        let path: &str = $path; // 强制类型为 &str，提供一定类型安全
        if let Some(init_data) = initramfs::find_file(path) {
            kinfo!(
                "Found init file '{}' in initramfs ({} bytes), loading...",
                path,
                init_data.len()
            );

            kinfo!(
                "Init file '{}' data ptr={:#x}",
                path,
                init_data.as_ptr() as usize
            );
            unsafe {
                let ptr = init_data.as_ptr();
                let before = if ptr as usize > 0 { *ptr.offset(-1) } else { 0 };
                if let Some(ramfs) = crate::initramfs::get() {
                    let base = ramfs.base_ptr() as usize;
                    let offset = ptr as usize - base;
                    kinfo!("Initramfs base={:#x}, data offset={:#x}", base, offset);
                }
                kinfo!("Byte before data: {:02x}", before);
            }

            // Debug: Check first few bytes of ELF
            let mut header = [0u8; 8];
            let to_copy = core::cmp::min(header.len(), init_data.len());
            for i in 0..to_copy {
                header[i] = init_data[i];
            }
            kinfo!(
                "ELF header bytes 0-7: {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
                header[0],
                header[1],
                header[2],
                header[3],
                header[4],
                header[5],
                header[6],
                header[7],
            );

            match process::Process::from_elf(init_data) {
                Ok(mut proc) => {
                    kinfo!("Successfully loaded '{}' as PID {}", path, proc.pid);
                    kinfo!("Switching to REAL user mode (Ring 3)...");
                    proc.execute(); // Never returns
                }
                Err(e) => {
                    kpanic!("Failed to load '{}': {}", path, e);
                }
            }
        } else {
            kwarn!("Init file '{}' not found in initramfs", path);
        }
    }};
}
