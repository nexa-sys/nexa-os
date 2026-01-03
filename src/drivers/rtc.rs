//! rtc - Real Time Clock (CMOS) support
//!
//! Provides a minimal CMOS RTC reader for x86 and an optional boot-time
//! initialization hook for CLOCK_REALTIME.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::safety::{inb, outb};

#[derive(Clone, Copy, Debug)]
pub struct RtcDateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

static TIME_INIT_DONE: AtomicBool = AtomicBool::new(false);

#[inline]
fn cmos_read(reg: u8) -> u8 {
    // Disable NMI (bit 7) while selecting the register.
    outb(0x70, reg | 0x80);
    inb(0x71)
}

#[inline]
fn bcd_to_bin(v: u8) -> u8 {
    (v & 0x0f) + ((v >> 4) * 10)
}

fn normalize_hour(hour: u8, reg_b: u8) -> u8 {
    let is_24h = (reg_b & 0x02) != 0;
    if is_24h {
        return hour;
    }

    // 12-hour format: bit 7 indicates PM.
    let pm = (hour & 0x80) != 0;
    let mut h = hour & 0x7f;
    if h == 12 {
        h = 0;
    }
    if pm {
        h = h.saturating_add(12);
    }
    h
}

fn read_consistent() -> Option<RtcDateTime> {
    // Wait until update-in-progress (UIP) is clear.
    for _ in 0..1_000_000 {
        if (cmos_read(0x0a) & 0x80) == 0 {
            break;
        }
    }

    // Read twice and compare to avoid races across second boundaries.
    for _ in 0..8 {
        let sec1 = cmos_read(0x00);
        let min1 = cmos_read(0x02);
        let hour1 = cmos_read(0x04);
        let day1 = cmos_read(0x07);
        let mon1 = cmos_read(0x08);
        let year1 = cmos_read(0x09);
        let cent1 = cmos_read(0x32);
        let reg_b = cmos_read(0x0b);

        let sec2 = cmos_read(0x00);
        let min2 = cmos_read(0x02);
        let hour2 = cmos_read(0x04);
        let day2 = cmos_read(0x07);
        let mon2 = cmos_read(0x08);
        let year2 = cmos_read(0x09);
        let cent2 = cmos_read(0x32);

        if sec1 == sec2
            && min1 == min2
            && hour1 == hour2
            && day1 == day2
            && mon1 == mon2
            && year1 == year2
            && cent1 == cent2
        {
            let is_binary = (reg_b & 0x04) != 0;

            let mut second = sec1;
            let mut minute = min1;
            let mut hour = hour1;
            let mut day = day1;
            let mut month = mon1;
            let mut year = year1;
            let mut century = cent1;

            if !is_binary {
                second = bcd_to_bin(second);
                minute = bcd_to_bin(minute);
                // Hour keeps AM/PM bit in 12h mode; convert digits only.
                hour = bcd_to_bin(hour & 0x7f) | (hour & 0x80);
                day = bcd_to_bin(day);
                month = bcd_to_bin(month);
                year = bcd_to_bin(year);
                century = bcd_to_bin(century);
            }

            hour = normalize_hour(hour, reg_b);

            let full_year: u16 = if century != 0 {
                (century as u16) * 100 + (year as u16)
            } else {
                // Fallback when century is unavailable.
                2000 + (year as u16)
            };

            return Some(RtcDateTime {
                year: full_year,
                month,
                day,
                hour,
                minute,
                second,
            });
        }

        // If mismatch, try again after UIP clears.
        for _ in 0..100_000 {
            if (cmos_read(0x0a) & 0x80) == 0 {
                break;
            }
        }
    }

    None
}

/// Read current CMOS RTC datetime.
pub fn read_datetime() -> Option<RtcDateTime> {
    read_consistent()
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    // Howard Hinnant's algorithm, Gregorian calendar.
    let mut y = y;
    let m = m as i32;
    let d = d as i32;

    y -= if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = m + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era as i64) * 146097 + (doe as i64) - 719468
}

fn datetime_to_unix_seconds(dt: RtcDateTime) -> Option<i64> {
    if dt.month == 0 || dt.day == 0 {
        return None;
    }
    let days = days_from_civil(dt.year as i32, dt.month as u32, dt.day as u32);
    let secs = days
        .saturating_mul(86_400)
        .saturating_add((dt.hour as i64) * 3600)
        .saturating_add((dt.minute as i64) * 60)
        .saturating_add(dt.second as i64);
    Some(secs)
}

/// Initialize CLOCK_REALTIME from RTC once (best-effort).
///
/// This sets the `TIME_OFFSET_US` used by `clock_gettime(CLOCK_REALTIME)`.
pub fn init_system_time_from_rtc_once() {
    if TIME_INIT_DONE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let Some(dt) = read_datetime() else {
        crate::kwarn!("rtc: CMOS read failed; CLOCK_REALTIME not initialized");
        return;
    };

    let Some(sec) = datetime_to_unix_seconds(dt) else {
        crate::kwarn!("rtc: invalid CMOS datetime; CLOCK_REALTIME not initialized");
        return;
    };

    // CMOS RTC is assumed to be UTC here.
    let realtime_us = sec.saturating_mul(1_000_000);
    crate::syscalls::set_realtime_us(realtime_us);

    crate::kinfo!(
        "rtc: CLOCK_REALTIME initialized from CMOS: {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        dt.year,
        dt.month,
        dt.day,
        dt.hour,
        dt.minute,
        dt.second
    );
}
