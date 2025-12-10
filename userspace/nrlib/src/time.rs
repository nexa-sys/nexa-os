use core::arch::x86_64::{__cpuid, __cpuid_count, _rdtsc};
use core::hint::spin_loop;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const NSEC_PER_SEC: u128 = 1_000_000_000;
const DEFAULT_TSC_FREQ_HZ: u64 = 1_000_000_000; // 1 GHz fallback matches kernel logger
const MAX_SLEEP_CHUNK_NS: u64 = 100_000_000; // 100 ms per busy-wait chunk to avoid overflow

// Syscall numbers
const SYS_CLOCK_GETTIME: u64 = 228;
const SYS_NANOSLEEP: u64 = 35;

// Clock IDs
const CLOCK_REALTIME: i32 = 0;
const CLOCK_MONOTONIC: i32 = 1;
const CLOCK_BOOTTIME: i32 = 7;

#[repr(C)]
#[derive(Clone, Copy)]
struct TimeSpec {
    tv_sec: i64,
    tv_nsec: i64,
}

static TIME_INIT: AtomicBool = AtomicBool::new(false);
static TSC_FREQ_HZ: AtomicU64 = AtomicU64::new(DEFAULT_TSC_FREQ_HZ);
static TSC_BASE_CYCLES: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
fn read_tsc() -> u64 {
    unsafe { _rdtsc() }
}

fn detect_tsc_frequency_hz() -> u64 {
    unsafe {
        let max_leaf = __cpuid(0).eax;
        if max_leaf >= 0x15 {
            let leaf = __cpuid_count(0x15, 0);
            let denom = leaf.eax as u64;
            let numer = leaf.ebx as u64;
            let crystal_hz = leaf.ecx as u64;
            if denom != 0 && numer != 0 && crystal_hz != 0 {
                let freq = (crystal_hz as u128 * numer as u128) / denom as u128;
                if freq != 0 {
                    return freq as u64;
                }
            }
        }

        if max_leaf >= 0x16 {
            let leaf = __cpuid_count(0x16, 0);
            let mhz = leaf.eax as u64;
            if mhz != 0 {
                return mhz * 1_000_000;
            }
        }
    }

    DEFAULT_TSC_FREQ_HZ
}

fn ensure_time_state() {
    if TIME_INIT.load(Ordering::Acquire) {
        return;
    }

    let freq = detect_tsc_frequency_hz().max(1);
    let base = read_tsc();
    TSC_FREQ_HZ.store(freq, Ordering::Release);
    TSC_BASE_CYCLES.store(base, Ordering::Release);
    TIME_INIT.store(true, Ordering::Release);
}

fn cycles_to_timespec(cycles: u64, freq: u64) -> (i64, i64) {
    if freq == 0 {
        return (0, 0);
    }

    let nanos = (cycles as u128 * NSEC_PER_SEC) / freq as u128;
    let secs = (nanos / NSEC_PER_SEC) as i64;
    let nsec = (nanos % NSEC_PER_SEC) as i64;
    (secs, nsec)
}

fn nanos_to_cycles(ns: u64, freq: u64) -> u128 {
    if freq == 0 || ns == 0 {
        return 0;
    }
    (ns as u128 * freq as u128 + (NSEC_PER_SEC - 1)) / NSEC_PER_SEC
}

/// Syscall wrapper for clock_gettime
fn syscall_clock_gettime(clk_id: i32, tp: *mut TimeSpec) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_CLOCK_GETTIME,
            in("rdi") clk_id,
            in("rsi") tp,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    ret
}

/// Syscall wrapper for nanosleep
fn syscall_nanosleep(req: *const TimeSpec, rem: *mut TimeSpec) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_NANOSLEEP,
            in("rdi") req,
            in("rsi") rem,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    ret
}

/// Get current uptime in seconds (uses kernel syscall)
#[no_mangle]
pub extern "C" fn get_uptime() -> u64 {
    let mut ts = TimeSpec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if syscall_clock_gettime(CLOCK_BOOTTIME, &mut ts as *mut TimeSpec) == 0 {
        ts.tv_sec as u64
    } else {
        // Fallback to TSC-based time
        let (sec, _) = monotonic_timespec();
        sec as u64
    }
}

/// Sleep for specified number of seconds (uses kernel syscall)
#[no_mangle]
pub extern "C" fn sleep(seconds: u32) {
    let req = TimeSpec {
        tv_sec: seconds as i64,
        tv_nsec: 0,
    };
    syscall_nanosleep(&req as *const TimeSpec, core::ptr::null_mut());
}

fn busy_wait_ns_chunk(ns: u64) {
    if ns == 0 {
        return;
    }

    ensure_time_state();
    let freq = TSC_FREQ_HZ.load(Ordering::Relaxed).max(1);
    let target_cycles = nanos_to_cycles(ns, freq);
    let start = read_tsc();

    loop {
        let elapsed = read_tsc().wrapping_sub(start) as u128;
        if elapsed >= target_cycles {
            break;
        }
        spin_loop();
    }
}

pub fn sleep_ns(mut total_ns: u64) {
    if total_ns == 0 {
        return;
    }

    while total_ns > 0 {
        let chunk = total_ns.min(MAX_SLEEP_CHUNK_NS);
        busy_wait_ns_chunk(chunk);
        total_ns -= chunk;
    }
}

pub fn monotonic_timespec() -> (i64, i64) {
    ensure_time_state();
    let freq = TSC_FREQ_HZ.load(Ordering::Relaxed).max(1);
    let base = TSC_BASE_CYCLES.load(Ordering::Relaxed);
    let current = read_tsc();
    let delta = current.wrapping_sub(base);
    cycles_to_timespec(delta, freq)
}

pub fn realtime_timespec() -> (i64, i64) {
    // We currently lack a real-time reference clock, so reuse monotonic time.
    // This still provides consistent, non-decreasing timestamps since boot.
    monotonic_timespec()
}

pub fn resolution_ns() -> i64 {
    ensure_time_state();
    let freq = TSC_FREQ_HZ.load(Ordering::Relaxed).max(1);
    let nanos = (NSEC_PER_SEC + freq as u128 - 1) / freq as u128;
    nanos.max(1) as i64
}
