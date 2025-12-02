//! procfs - Process Information Pseudo-Filesystem
//!
//! This module implements a Linux-compatible /proc filesystem that provides
//! process information and kernel statistics through a virtual filesystem interface.
//!
//! Supported files:
//! - /proc/self -> symlink to current process directory
//! - /proc/[pid]/cmdline - Process command line
//! - /proc/[pid]/status - Process status information
//! - /proc/[pid]/stat - Process statistics
//! - /proc/[pid]/maps - Memory mappings
//! - /proc/[pid]/fd/ - File descriptors (directory)
//! - /proc/cpuinfo - CPU information
//! - /proc/meminfo - Memory information
//! - /proc/version - Kernel version
//! - /proc/uptime - System uptime
//! - /proc/loadavg - Load averages
//! - /proc/stat - Kernel/system statistics
//! - /proc/filesystems - Supported filesystems
//! - /proc/mounts - Current mounts
//! - /proc/cmdline - Kernel command line

use crate::mm;
use crate::posix::{FileType, Metadata};
use crate::process::{Pid, ProcessState, MAX_PROCESSES};
use crate::scheduler;
use crate::smp;
use core::fmt::Write;

/// Buffer size for dynamically generated procfs content
const PROC_BUF_SIZE: usize = 4096;

/// Static buffer for procfs content generation (protected by lock)
static PROC_BUFFER: spin::Mutex<[u8; PROC_BUF_SIZE]> = spin::Mutex::new([0u8; PROC_BUF_SIZE]);

/// A simple writer that writes to a fixed-size buffer
struct BufWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> BufWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn len(&self) -> usize {
        self.pos
    }
}

impl<'a> Write for BufWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.buf.len() - self.pos;
        let to_write = bytes.len().min(remaining);
        self.buf[self.pos..self.pos + to_write].copy_from_slice(&bytes[..to_write]);
        self.pos += to_write;
        Ok(())
    }
}

// =============================================================================
// CPUID Helper Functions for Real CPU Information
// =============================================================================

/// Get CPU vendor, family, model, stepping, model name, and frequency via CPUID
fn get_cpuid_info() -> (&'static str, u32, u32, u32, &'static str, u32) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::__cpuid;

        // Get vendor ID (CPUID leaf 0)
        let cpuid0 = __cpuid(0);
        let vendor_bytes: [u8; 12] = [
            (cpuid0.ebx & 0xFF) as u8,
            ((cpuid0.ebx >> 8) & 0xFF) as u8,
            ((cpuid0.ebx >> 16) & 0xFF) as u8,
            ((cpuid0.ebx >> 24) & 0xFF) as u8,
            (cpuid0.edx & 0xFF) as u8,
            ((cpuid0.edx >> 8) & 0xFF) as u8,
            ((cpuid0.edx >> 16) & 0xFF) as u8,
            ((cpuid0.edx >> 24) & 0xFF) as u8,
            (cpuid0.ecx & 0xFF) as u8,
            ((cpuid0.ecx >> 8) & 0xFF) as u8,
            ((cpuid0.ecx >> 16) & 0xFF) as u8,
            ((cpuid0.ecx >> 24) & 0xFF) as u8,
        ];

        let vendor = if &vendor_bytes == b"GenuineIntel" {
            "GenuineIntel"
        } else if &vendor_bytes == b"AuthenticAMD" {
            "AuthenticAMD"
        } else {
            "Unknown"
        };

        // Get family/model/stepping (CPUID leaf 1)
        let cpuid1 = __cpuid(1);
        let stepping = cpuid1.eax & 0xF;
        let base_model = (cpuid1.eax >> 4) & 0xF;
        let base_family = (cpuid1.eax >> 8) & 0xF;
        let ext_model = (cpuid1.eax >> 16) & 0xF;
        let ext_family = (cpuid1.eax >> 20) & 0xFF;

        let family = if base_family == 0xF {
            base_family + ext_family
        } else {
            base_family
        };

        let model = if base_family == 0x6 || base_family == 0xF {
            (ext_model << 4) | base_model
        } else {
            base_model
        };

        // Get processor brand string (CPUID leaves 0x80000002-0x80000004)
        let max_ext = __cpuid(0x80000000).eax;
        let model_name = if max_ext >= 0x80000004 {
            // Extended brand string available - use generic name based on vendor
            if vendor == "GenuineIntel" {
                "Intel(R) Core(TM) Processor"
            } else if vendor == "AuthenticAMD" {
                "AMD Processor"
            } else {
                "Unknown Processor"
            }
        } else {
            "Generic x86_64 Processor"
        };

        // Get CPU frequency (CPUID leaf 0x16 if available)
        let cpu_mhz = if cpuid0.eax >= 0x16 {
            let cpuid16 = __cpuid(0x16);
            if cpuid16.eax != 0 {
                cpuid16.eax // Base frequency in MHz
            } else {
                3000 // Default 3GHz
            }
        } else {
            3000 // Default 3GHz
        };

        (vendor, family, model, stepping, model_name, cpu_mhz)
    }

    #[cfg(not(target_arch = "x86_64"))]
    ("Unknown", 0, 0, 0, "Unknown Processor", 1000)
}

/// Get CPU feature flags as a string
fn get_cpu_flags() -> &'static str {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::__cpuid;

        let cpuid1 = __cpuid(1);
        let edx = cpuid1.edx;
        let ecx = cpuid1.ecx;

        // Extended features (CPUID leaf 7)
        let cpuid0 = __cpuid(0);
        let (ebx7, ecx7) = if cpuid0.eax >= 7 {
            let cpuid7 = __cpuid(7);
            (cpuid7.ebx, cpuid7.ecx)
        } else {
            (0, 0)
        };

        // Extended CPUID leaf 0x80000001
        let max_ext = __cpuid(0x80000000).eax;
        let (edx_ext, ecx_ext) = if max_ext >= 0x80000001 {
            let cpuid_ext = __cpuid(0x80000001);
            (cpuid_ext.edx, cpuid_ext.ecx)
        } else {
            (0, 0)
        };

        // Build flags string based on detected features
        // This is a representative subset - real Linux shows many more
        let mut flags = "fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2";

        // Add common modern features based on actual detection
        if ecx & (1 << 0) != 0 {
            flags = "fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 sse3";
        }
        if ecx & (1 << 9) != 0 {
            flags = "fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 sse3 ssse3";
        }
        if ecx & (1 << 19) != 0 {
            flags = "fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 sse3 ssse3 sse4_1";
        }
        if ecx & (1 << 20) != 0 {
            flags = "fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 sse3 ssse3 sse4_1 sse4_2";
        }
        if edx_ext & (1 << 29) != 0 {
            flags = "fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 sse3 ssse3 sse4_1 sse4_2 lm";
        }
        if edx_ext & (1 << 20) != 0 {
            flags = "fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 sse3 ssse3 sse4_1 sse4_2 lm nx";
        }

        flags
    }

    #[cfg(not(target_arch = "x86_64"))]
    "fpu"
}

/// Generate /proc/version content
pub fn generate_version() -> (&'static [u8], usize) {
    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let _ = writeln!(
        writer,
        "NexaOS version 0.1.0 (rust@nexaos) (rustc 1.75.0) #1 SMP PREEMPT_DYNAMIC"
    );

    let len = writer.len();
    // SAFETY: Buffer has static lifetime, content is valid until next call
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /proc/uptime content
pub fn generate_uptime() -> (&'static [u8], usize) {
    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Get tick count (in ms), convert to seconds
    let tick_ms = scheduler::get_tick();
    let uptime_secs = tick_ms / 1000;
    let uptime_frac = (tick_ms % 1000) / 10; // Two decimal places

    // Idle time (simplified - assume 10% idle)
    let idle_secs = uptime_secs / 10;
    let idle_frac = uptime_frac / 10;

    let _ = writeln!(
        writer,
        "{}.{:02} {}.{:02}",
        uptime_secs, uptime_frac, idle_secs, idle_frac
    );

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /proc/loadavg content
pub fn generate_loadavg() -> (&'static [u8], usize) {
    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let (load1, load5, load15) = scheduler::get_load_average();
    let (ready, running, _sleeping, _zombie) = scheduler::get_process_counts();
    let total_procs = ready + running;

    // Format: load1 load5 load15 running/total last_pid
    // Load averages are floats, convert to integer parts
    let load1_int = load1 as u32;
    let load1_frac = ((load1 - load1_int as f32) * 100.0) as u32;
    let load5_int = load5 as u32;
    let load5_frac = ((load5 - load5_int as f32) * 100.0) as u32;
    let load15_int = load15 as u32;
    let load15_frac = ((load15 - load15_int as f32) * 100.0) as u32;

    let _ = writeln!(
        writer,
        "{}.{:02} {}.{:02} {}.{:02} {}/{} {}",
        load1_int,
        load1_frac,
        load5_int,
        load5_frac,
        load15_int,
        load15_frac,
        running,
        total_procs,
        scheduler::get_current_pid().unwrap_or(0)
    );

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /proc/meminfo content
pub fn generate_meminfo() -> (&'static [u8], usize) {
    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Get real memory statistics from the kernel heap
    let (heap_stats, buddy_stats, slab_stats) = mm::get_memory_stats();

    // Calculate real values from buddy allocator
    // Each page is 4KB
    let page_size_kb: u64 = 4;
    let total_pages = buddy_stats.pages_allocated + buddy_stats.pages_free;
    let total_kb = total_pages * page_size_kb;
    let free_kb = buddy_stats.pages_free * page_size_kb;
    let used_kb = buddy_stats.pages_allocated * page_size_kb;

    // Heap usage from HeapStats
    let heap_used_kb = (heap_stats
        .bytes_allocated
        .saturating_sub(heap_stats.bytes_freed))
        / 1024;
    let heap_peak_kb = heap_stats.peak_usage / 1024;

    // Slab allocator stats
    let slab_active = slab_stats.allocations.saturating_sub(slab_stats.frees);

    let available_kb = free_kb;
    let buffers_kb: u64 = 0;
    let cached_kb = heap_peak_kb.saturating_sub(heap_used_kb);

    let _ = writeln!(writer, "MemTotal:       {:8} kB", total_kb);
    let _ = writeln!(writer, "MemFree:        {:8} kB", free_kb);
    let _ = writeln!(writer, "MemAvailable:   {:8} kB", available_kb);
    let _ = writeln!(writer, "Buffers:        {:8} kB", buffers_kb);
    let _ = writeln!(writer, "Cached:         {:8} kB", cached_kb);
    let _ = writeln!(writer, "SwapCached:     {:8} kB", 0u64);
    let _ = writeln!(writer, "Active:         {:8} kB", heap_used_kb);
    let _ = writeln!(writer, "Inactive:       {:8} kB", cached_kb);
    let _ = writeln!(writer, "SwapTotal:      {:8} kB", 0u64);
    let _ = writeln!(writer, "SwapFree:       {:8} kB", 0u64);
    let _ = writeln!(writer, "Dirty:          {:8} kB", 0u64);
    let _ = writeln!(writer, "Writeback:      {:8} kB", 0u64);
    let _ = writeln!(writer, "AnonPages:      {:8} kB", heap_used_kb);
    let _ = writeln!(writer, "Mapped:         {:8} kB", used_kb);
    let _ = writeln!(writer, "Shmem:          {:8} kB", 0u64);
    let _ = writeln!(writer, "Slab:           {:8} kB", slab_active * 64 / 1024); // Estimate slab usage
    let _ = writeln!(writer, "KernelStack:    {:8} kB", 64u64);
    let _ = writeln!(
        writer,
        "PageTables:     {:8} kB",
        (buddy_stats.allocations * 4) as u64
    );
    let _ = writeln!(writer, "VmallocTotal:   {:8} kB", total_kb);
    let _ = writeln!(writer, "VmallocUsed:    {:8} kB", used_kb);
    let _ = writeln!(writer, "VmallocChunk:   {:8} kB", free_kb);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /proc/cpuinfo content
pub fn generate_cpuinfo() -> (&'static [u8], usize) {
    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Get real CPU count from SMP module
    let cpu_count = smp::cpu_count().max(1);
    let online_cpus = smp::online_cpus().max(1);

    // Get CPU info via CPUID instruction
    let (vendor_id, family, model, stepping, model_name, cpu_mhz) = get_cpuid_info();
    let flags = get_cpu_flags();

    for cpu_id in 0..online_cpus {
        let _ = writeln!(writer, "processor\t: {}", cpu_id);
        let _ = writeln!(writer, "vendor_id\t: {}", vendor_id);
        let _ = writeln!(writer, "cpu family\t: {}", family);
        let _ = writeln!(writer, "model\t\t: {}", model);
        let _ = writeln!(writer, "model name\t: {}", model_name);
        let _ = writeln!(writer, "stepping\t: {}", stepping);
        let _ = writeln!(writer, "cpu MHz\t\t: {}.000", cpu_mhz);
        let _ = writeln!(writer, "cache size\t: 256 KB");
        let _ = writeln!(writer, "physical id\t: 0");
        let _ = writeln!(writer, "siblings\t: {}", cpu_count);
        let _ = writeln!(writer, "core id\t\t: {}", cpu_id);
        let _ = writeln!(writer, "cpu cores\t: {}", cpu_count);
        let _ = writeln!(writer, "fpu\t\t: yes");
        let _ = writeln!(writer, "fpu_exception\t: yes");
        let _ = writeln!(writer, "cpuid level\t: 20");
        let _ = writeln!(writer, "wp\t\t: yes");
        let _ = writeln!(writer, "flags\t\t: {}", flags);
        let _ = writeln!(writer, "bogomips\t: {}.00", cpu_mhz * 2);
        let _ = writeln!(writer, "clflush size\t: 64");
        let _ = writeln!(writer, "cache_alignment\t: 64");
        let _ = writeln!(writer, "address sizes\t: 48 bits physical, 48 bits virtual");
        let _ = writeln!(writer, "");
    }

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /proc/stat content
pub fn generate_stat() -> (&'static [u8], usize) {
    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let tick = scheduler::get_tick();
    let stats = scheduler::get_stats();
    let (ready, running, sleeping, zombie) = scheduler::get_process_counts();

    // CPU statistics (simplified)
    // Format: cpu user nice system idle iowait irq softirq steal guest guest_nice
    let user_time = tick / 2;
    let system_time = tick / 4;
    let idle_time = tick / 4;

    let _ = writeln!(
        writer,
        "cpu  {} 0 {} {} 0 0 0 0 0 0",
        user_time, system_time, idle_time
    );
    let _ = writeln!(
        writer,
        "cpu0 {} 0 {} {} 0 0 0 0 0 0",
        user_time, system_time, idle_time
    );

    // Context switches
    let _ = writeln!(writer, "ctxt {}", stats.total_context_switches);

    // Boot time (placeholder)
    let _ = writeln!(writer, "btime 0");

    // Processes
    let _ = writeln!(writer, "processes {}", ready + running + sleeping + zombie);
    let _ = writeln!(writer, "procs_running {}", running);
    let _ = writeln!(writer, "procs_blocked {}", sleeping);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /proc/filesystems content
pub fn generate_filesystems() -> (&'static [u8], usize) {
    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let _ = writeln!(writer, "nodev\tproc");
    let _ = writeln!(writer, "nodev\tsysfs");
    let _ = writeln!(writer, "nodev\tdevtmpfs");
    let _ = writeln!(writer, "nodev\ttmpfs");
    let _ = writeln!(writer, "\text2");
    let _ = writeln!(writer, "\text3");
    let _ = writeln!(writer, "\text4");

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /proc/mounts content
pub fn generate_mounts() -> (&'static [u8], usize) {
    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Format: device mountpoint fstype options dump pass
    let _ = writeln!(writer, "rootfs / rootfs rw 0 0");
    let _ = writeln!(
        writer,
        "proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0"
    );
    let _ = writeln!(
        writer,
        "sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0"
    );
    let _ = writeln!(
        writer,
        "devtmpfs /dev devtmpfs rw,nosuid,relatime,size=0k,nr_inodes=0,mode=755 0 0"
    );

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /proc/cmdline content
pub fn generate_cmdline() -> (&'static [u8], usize) {
    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Kernel command line (placeholder)
    let _ = writeln!(writer, "root=/dev/vda1 console=ttyS0 quiet");

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /proc/self symlink target (returns current PID as string)
pub fn generate_self() -> (&'static [u8], usize) {
    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let pid = scheduler::get_current_pid().unwrap_or(1);
    let _ = write!(writer, "{}", pid);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /proc/[pid]/status content
pub fn generate_pid_status(pid: Pid) -> Option<(&'static [u8], usize)> {
    let process = scheduler::get_process(pid)?;

    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let state_char = match process.state {
        ProcessState::Running => 'R',
        ProcessState::Ready => 'R',
        ProcessState::Sleeping => 'S',
        ProcessState::Zombie => 'Z',
    };

    let state_name = match process.state {
        ProcessState::Running => "running",
        ProcessState::Ready => "runnable",
        ProcessState::Sleeping => "sleeping",
        ProcessState::Zombie => "zombie",
    };

    let _ = writeln!(writer, "Name:\tprocess");
    let _ = writeln!(writer, "Umask:\t0022");
    let _ = writeln!(writer, "State:\t{} ({})", state_char, state_name);
    let _ = writeln!(writer, "Tgid:\t{}", pid);
    let _ = writeln!(writer, "Ngid:\t0");
    let _ = writeln!(writer, "Pid:\t{}", pid);
    let _ = writeln!(writer, "PPid:\t{}", process.ppid);
    let _ = writeln!(writer, "TracerPid:\t0");
    let _ = writeln!(writer, "Uid:\t0\t0\t0\t0");
    let _ = writeln!(writer, "Gid:\t0\t0\t0\t0");
    let _ = writeln!(writer, "FDSize:\t64");
    let _ = writeln!(writer, "Groups:\t0");
    let _ = writeln!(writer, "VmPeak:\t{} kB", process.memory_size / 1024);
    let _ = writeln!(writer, "VmSize:\t{} kB", process.memory_size / 1024);
    let _ = writeln!(writer, "VmRSS:\t{} kB", process.memory_size / 1024);
    let _ = writeln!(
        writer,
        "VmData:\t{} kB",
        (process.heap_end - process.heap_start) / 1024
    );
    let _ = writeln!(writer, "VmStk:\t{} kB", crate::process::STACK_SIZE / 1024);
    let _ = writeln!(writer, "VmExe:\t0 kB");
    let _ = writeln!(writer, "VmLib:\t0 kB");
    let _ = writeln!(writer, "Threads:\t1");
    let _ = writeln!(writer, "SigPnd:\t{:016x}", 0u64);
    let _ = writeln!(writer, "ShdPnd:\t{:016x}", 0u64);
    let _ = writeln!(writer, "SigBlk:\t{:016x}", 0u64); // Signal blocked mask (internal field)
    let _ = writeln!(writer, "SigIgn:\t{:016x}", 0u64);
    let _ = writeln!(writer, "SigCgt:\t{:016x}", 0u64);
    let _ = writeln!(writer, "voluntary_ctxt_switches:\t0");
    let _ = writeln!(writer, "nonvoluntary_ctxt_switches:\t0");

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

/// Generate /proc/[pid]/stat content (single line format)
pub fn generate_pid_stat(pid: Pid) -> Option<(&'static [u8], usize)> {
    let process = scheduler::get_process(pid)?;

    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let state_char = match process.state {
        ProcessState::Running => 'R',
        ProcessState::Ready => 'R',
        ProcessState::Sleeping => 'S',
        ProcessState::Zombie => 'Z',
    };

    // Format: pid (comm) state ppid pgrp session tty_nr tpgid flags ...
    let _ = writeln!(
        writer,
        "{} (process) {} {} {} {} {} 0 0 0 0 0 0 0 0 0 0 1 0 0 {} 0 -1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        pid,
        state_char,
        process.ppid,
        pid, // pgrp
        pid, // session
        0,   // tty_nr
        process.memory_size
    );

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

/// Generate /proc/[pid]/cmdline content
pub fn generate_pid_cmdline(pid: Pid) -> Option<(&'static [u8], usize)> {
    let process = scheduler::get_process(pid)?;

    let mut buf = PROC_BUFFER.lock();

    // Copy the process cmdline to the buffer
    let len = process.cmdline_len.min(PROC_BUF_SIZE);
    buf[..len].copy_from_slice(&process.cmdline[..len]);

    // Return the slice with actual cmdline data
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

/// Generate /proc/[pid]/maps content
pub fn generate_pid_maps(pid: Pid) -> Option<(&'static [u8], usize)> {
    let process = scheduler::get_process(pid)?;

    let mut buf = PROC_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Memory mapping format:
    // address           perms offset  dev   inode pathname
    // 00400000-00452000 r-xp 00000000 08:02 173521 /usr/bin/dbus-daemon

    use crate::process::{STACK_BASE, STACK_SIZE, USER_VIRT_BASE};

    // Code segment
    let code_end = process.heap_start;
    let _ = writeln!(
        writer,
        "{:08x}-{:08x} r-xp 00000000 00:00 0 [text]",
        USER_VIRT_BASE, code_end
    );

    // Heap
    if process.heap_end > process.heap_start {
        let _ = writeln!(
            writer,
            "{:08x}-{:08x} rw-p 00000000 00:00 0 [heap]",
            process.heap_start, process.heap_end
        );
    }

    // Stack
    let stack_bottom = STACK_BASE;
    let stack_top = STACK_BASE + STACK_SIZE;
    let _ = writeln!(
        writer,
        "{:08x}-{:08x} rw-p 00000000 00:00 0 [stack]",
        stack_bottom, stack_top
    );

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

/// Check if a PID exists in the process table
pub fn pid_exists(pid: Pid) -> bool {
    scheduler::get_process(pid).is_some()
}

/// Get list of all PIDs for /proc directory listing
pub fn get_all_pids() -> [Option<Pid>; MAX_PROCESSES] {
    let mut pids = [None; MAX_PROCESSES];
    let table = scheduler::process_table_lock();
    let mut idx = 0;

    for slot in table.iter() {
        if let Some(entry) = slot {
            if idx < MAX_PROCESSES {
                pids[idx] = Some(entry.process.pid);
                idx += 1;
            }
        }
    }

    pids
}

/// Metadata for procfs entries
pub fn proc_file_metadata(size: u64) -> Metadata {
    let mut meta = Metadata::empty()
        .with_type(FileType::Regular)
        .with_mode(0o444);
    meta.size = size;
    meta.nlink = 1;
    meta
}

pub fn proc_dir_metadata() -> Metadata {
    let mut meta = Metadata::empty()
        .with_type(FileType::Directory)
        .with_mode(0o555);
    meta.nlink = 2;
    meta
}

pub fn proc_link_metadata() -> Metadata {
    let mut meta = Metadata::empty()
        .with_type(FileType::Symlink)
        .with_mode(0o777);
    meta.nlink = 1;
    meta
}

/// Convert a PID to a String for directory listing
/// Works with radix tree PIDs (up to 262143)
pub fn get_pid_string(pid: Pid) -> alloc::string::String {
    alloc::format!("{}", pid)
}
