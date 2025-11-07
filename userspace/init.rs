//! /sbin/init - System initialization program (PID 1)
//! 
//! Hybrid kernel init system with process supervision
//! 
//! Features:
//! - PID 1 process management
//! - Service supervision and respawn
//! - systemd-style logging
//! - Automatic restart on failure
//! - Runlevel management
//!
//! POSIX/Unix-like compliance:
//! - Process hierarchy root (PPID = 0)
//! - Orphan process adoption
//! - Signal handling for system control
//! - Service respawn on failure

// Use #![no_main] to bypass Rust's standard main wrapper
// We provide our own main function with C ABI that crt.rs can call
#![no_main]

use std::arch::asm;
use std::io::{self, Write};

// System call numbers
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAIT4: u64 = 61;
const SYS_GETPID: u64 = 39;
const SYS_GETPPID: u64 = 110;
const SYS_RUNLEVEL: u64 = 231;
const SYS_USER_ADD: u64 = 220;
const SYS_USER_LOGIN: u64 = 221;
const SYS_GETERRNO: u64 = 201;

// Standard file descriptors
const STDIN: i32 = 0;
const STDOUT: i32 = 1;
const STDERR: i32 = 2;

// Service management constants
const MAX_RESPAWN_COUNT: u32 = 5;  // Max respawns within window
const RESPAWN_WINDOW_SEC: u64 = 60; // Respawn window in seconds
const RESTART_DELAY_MS: u64 = 1000; // Delay between restarts

// Direct syscall implementations (inline, no nrlib dependency)
#[inline(always)]
fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rax") ret,
            clobber_abi("sysv64"),
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
fn syscall2(n: u64, a1: u64, a2: u64) -> u64 {
    syscall3(n, a1, a2, 0)
}

#[inline(always)]
fn syscall1(n: u64, a1: u64) -> u64 {
    syscall3(n, a1, 0, 0)
}

#[inline(always)]
fn syscall0(n: u64) -> u64 {
    syscall3(n, 0, 0, 0)
}

/// Print string to stdout using direct syscall (for debugging)
fn print_raw(s: &str) {
    unsafe {
        syscall3(1, 1, s.as_ptr() as u64, s.len() as u64);
    }
}

/// Print string to stdout (uses std::io, may fail if std not initialized)
fn print(s: &str) {
    // Fallback to raw syscall if std::io fails
    match io::stdout().write_all(s.as_bytes()) {
        Ok(_) => {},
        Err(_) => print_raw(s),
    }
}

/// Print string to stderr
fn eprint(s: &str) {
    let _ = io::stderr().write_all(s.as_bytes());
}

/// Exit process
fn exit(code: i32) -> ! {
    std::process::exit(code)
}

/// Get process ID
fn getpid() -> i32 {
    syscall0(SYS_GETPID) as i32
}

/// Get parent process ID
fn getppid() -> i32 {
    syscall0(SYS_GETPPID) as i32
}

/// Fork process
fn fork() -> i64 {
    let ret = syscall0(SYS_FORK);
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

/// Execute program
fn execve(path: &str, argv: &[*const u8], envp: &[*const u8]) -> i64 {
    let ret = syscall3(
        SYS_EXECVE,
        path.as_ptr() as u64,
        argv.as_ptr() as u64,
        envp.as_ptr() as u64,
    );
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

/// Get current runlevel
fn get_runlevel() -> i32 {
    let ret = syscall1(SYS_RUNLEVEL, (-1i32) as u64);
    ret as i32
}

/// Open file
fn open(path: &str) -> u64 {
    syscall2(SYS_OPEN, path.as_ptr() as u64, path.len() as u64)
}

/// Read from file descriptor
fn read(fd: u64, buf: *mut u8, count: usize) -> u64 {
    syscall3(SYS_READ, fd, buf as u64, count as u64)
}

/// Close file descriptor
fn close(fd: u64) -> u64 {
    syscall1(SYS_CLOSE, fd)
}

/// Wait for process state change (wait4 syscall)
fn wait4(pid: i64, status: *mut i32, options: i32) -> i64 {
    let ret = syscall3(SYS_WAIT4, pid as u64, status as u64, options as u64);
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

/// Simple integer to string conversion
fn itoa(mut n: u64, buf: &mut [u8]) -> &str {
    if n == 0 {
        buf[0] = b'0';
        return std::str::from_utf8(&buf[0..1]).unwrap();
    }
    
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    
    // Reverse
    for j in 0..i/2 {
        buf.swap(j, i - 1 - j);
    }
    
    std::str::from_utf8(&buf[0..i]).unwrap()
}

#[allow(dead_code)]
fn itoa_hex(mut n: u64, buf: &mut [u8]) -> &str {
    if n == 0 {
        buf[0] = b'0';
        return std::str::from_utf8(&buf[0..1]).unwrap();
    }

    let mut i = 0;
    while n > 0 {
        let digit = (n & 0xf) as u8;
        buf[i] = match digit {
            0..=9 => b'0' + digit,
            _ => b'a' + (digit - 10),
        };
        n >>= 4;
        i += 1;
    }

    for j in 0..i / 2 {
        buf.swap(j, i - 1 - j);
    }

    std::str::from_utf8(&buf[..i]).unwrap()
}

const CONFIG_PATH: &str = "/etc/ni/ni.conf";
const CONFIG_BUFFER_SIZE: usize = 4096;
const MAX_SERVICES: usize = 12;
const DEFAULT_TARGET_NAME: &str = "multi-user.target";
const FALLBACK_TARGET_NAME: &str = "rescue.target";
const POWER_OFF_TARGET_NAME: &str = "poweroff.target";
const NETWORK_TARGET_NAME: &str = "network.target";
const EMPTY_STR: &str = "";
const MAX_FIELD_LEN: usize = 256;
const SERVICE_FIELD_COUNT: usize = 5; // name, description, exec, after, wants
const FIELD_IDX_NAME: usize = 0;
const FIELD_IDX_DESCRIPTION: usize = 1;
const FIELD_IDX_EXEC_START: usize = 2;
const FIELD_IDX_AFTER: usize = 3;
const FIELD_IDX_WANTS: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RestartPolicy {
    No,
    OnFailure,
    Always,
}

impl RestartPolicy {
    fn from_str(raw: &str) -> Self {
        let lower = raw.as_bytes();
        match lower {
            b"no" | b"none" | b"never" | b"false" => RestartPolicy::No,
            b"on-failure" | b"onfailure" | b"failure" => RestartPolicy::OnFailure,
            b"always" | b"true" | b"yes" => RestartPolicy::Always,
            _ => RestartPolicy::Always,
        }
    }
}

#[derive(Clone, Copy)]
struct RestartSettings {
    burst: u32,
    interval_sec: u64,
}

impl RestartSettings {
    const fn new() -> Self {
        Self {
            burst: MAX_RESPAWN_COUNT,
            interval_sec: RESPAWN_WINDOW_SEC,
        }
    }
}

#[derive(Clone, Copy)]
struct ServiceConfig {
    name: &'static str,
    description: &'static str,
    exec_start: &'static str,
    restart: RestartPolicy,
    restart_settings: RestartSettings,
    restart_delay_ms: u64,
    after: &'static str,
    wants: &'static str,
}

impl ServiceConfig {
    const fn empty() -> Self {
        Self {
            name: EMPTY_STR,
            description: EMPTY_STR,
            exec_start: EMPTY_STR,
            restart: RestartPolicy::Always,
            restart_settings: RestartSettings::new(),
            restart_delay_ms: RESTART_DELAY_MS,
            after: EMPTY_STR,
            wants: DEFAULT_TARGET_NAME,
        }
    }

    fn is_valid(&self) -> bool {
        !self.exec_start.is_empty()
    }
}

#[derive(Clone, Copy)]
struct ServiceCatalog {
    services: &'static [ServiceConfig],
    default_target: &'static str,
    fallback_target: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InitLogLevel {
    Info,
    Debug,
}

impl InitLogLevel {
    const fn default() -> Self {
        InitLogLevel::Info
    }

    fn from_str(raw: &'static str) -> Self {
        if raw.is_empty() {
            return InitLogLevel::Info;
        }
        let lower = to_ascii_lower(raw);
        if lower == "debug" {
            InitLogLevel::Debug
        } else {
            InitLogLevel::Info
        }
    }

    const fn allows_debug(self) -> bool {
        match self {
            InitLogLevel::Debug => true,
            InitLogLevel::Info => false,
        }
    }
}

static mut INIT_LOG_LEVEL: InitLogLevel = InitLogLevel::default();

fn debug_logs_enabled() -> bool {
    unsafe { INIT_LOG_LEVEL.allows_debug() }
}

static mut CONFIG_BUFFER: [u8; CONFIG_BUFFER_SIZE] = [0; CONFIG_BUFFER_SIZE];
static mut SERVICE_CONFIGS: [ServiceConfig; MAX_SERVICES] = [ServiceConfig::empty(); MAX_SERVICES];
static mut DEFAULT_BOOT_TARGET: &'static str = DEFAULT_TARGET_NAME;
static mut FALLBACK_BOOT_TARGET: &'static str = FALLBACK_TARGET_NAME;
static mut STRING_STORAGE: [[u8; MAX_FIELD_LEN]; MAX_SERVICES * SERVICE_FIELD_COUNT] =
    [[0; MAX_FIELD_LEN]; MAX_SERVICES * SERVICE_FIELD_COUNT];
static mut STRING_LENGTHS: [usize; MAX_SERVICES * SERVICE_FIELD_COUNT] =
    [0; MAX_SERVICES * SERVICE_FIELD_COUNT];

fn load_service_catalog() -> ServiceCatalog {
    unsafe {
        DEFAULT_BOOT_TARGET = DEFAULT_TARGET_NAME;
        FALLBACK_BOOT_TARGET = FALLBACK_TARGET_NAME;
        INIT_LOG_LEVEL = InitLogLevel::default();

        for slot in SERVICE_CONFIGS.iter_mut() {
            *slot = ServiceConfig::empty();
        }

        for bucket in STRING_STORAGE.iter_mut() {
            for byte in bucket.iter_mut() {
                *byte = 0;
            }
        }

        for len in STRING_LENGTHS.iter_mut() {
            *len = 0;
        }

        for byte in CONFIG_BUFFER.iter_mut() {
            *byte = 0;
        }

        use std::fs::File;
        use std::io::Read;
        
        let mut file = match File::open(CONFIG_PATH) {
            Ok(f) => f,
            Err(_) => {
                return ServiceCatalog {
                    services: &SERVICE_CONFIGS[0..0],
                    default_target: DEFAULT_BOOT_TARGET,
                    fallback_target: FALLBACK_BOOT_TARGET,
                };
            }
        };

        let read_count = match file.read(&mut CONFIG_BUFFER) {
            Ok(n) => n,
            Err(_) => 0,
        };

        if read_count == 0 {
            return ServiceCatalog {
                services: &SERVICE_CONFIGS[0..0],
                default_target: DEFAULT_BOOT_TARGET,
                fallback_target: FALLBACK_BOOT_TARGET,
            };
        }

        let usable = std::cmp::min(read_count, CONFIG_BUFFER.len());
        let service_count = parse_unit_file(usable);

        ServiceCatalog {
            services: &SERVICE_CONFIGS[0..service_count],
            default_target: DEFAULT_BOOT_TARGET,
            fallback_target: FALLBACK_BOOT_TARGET,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParserSection {
    None,
    Init,
    Service,
}

fn parse_unit_file(len: usize) -> usize {
    unsafe {
        let bytes = &CONFIG_BUFFER[..len];
        let mut section = ParserSection::None;
        let mut current = ServiceConfig::empty();
        let mut service_active = false;
        let mut line_start = 0usize;
        let mut service_count = 0usize;

        for (idx, &byte) in bytes.iter().enumerate() {
            if byte == b'\n' {
                process_line(
                    &bytes[line_start..idx],
                    &mut section,
                    &mut current,
                    &mut service_active,
                    &mut service_count,
                );
                line_start = idx + 1;
            }
        }

        if line_start < bytes.len() {
            process_line(
                &bytes[line_start..bytes.len()],
                &mut section,
                &mut current,
                &mut service_active,
                &mut service_count,
            );
        }

        if section == ParserSection::Service && service_active && current.is_valid() {
            finalize_service(&current, &mut service_count);
        }
        service_count
    }
}

fn process_line(
    raw_line: &[u8],
    section: &mut ParserSection,
    current: &mut ServiceConfig,
    service_active: &mut bool,
    service_count: &mut usize,
) {
    let line = trim_slice(raw_line);
    if line.is_empty() {
        return;
    }

    match line[0] {
        b'#' | b';' => return,
        b'[' => {
            handle_section_header(line, section, current, service_active, service_count);
        }
        _ => match section {
            ParserSection::Service => handle_service_key_value(line, current),
            ParserSection::Init => handle_init_key_value(line),
            ParserSection::None => {}
        },
    }
}

fn handle_section_header(
    line: &[u8],
    section: &mut ParserSection,
    current: &mut ServiceConfig,
    service_active: &mut bool,
    service_count: &mut usize,
) {
    let cleaned = strip_brackets(line);
    if cleaned.is_empty() {
        return;
    }

    match identify_section(cleaned) {
        ParserSection::Service => {
            if *section == ParserSection::Service && *service_active && current.is_valid() {
                finalize_service(current, service_count);
            }

            *current = ServiceConfig::empty();
            *service_active = true;
            *section = ParserSection::Service;

            if let Some(name_bytes) = extract_quoted_identifier(cleaned) {
                current.name = slice_to_static(name_bytes);
            }
        }
        ParserSection::Init => {
            if *section == ParserSection::Service && *service_active && current.is_valid() {
                finalize_service(current, service_count);
            }
            *section = ParserSection::Init;
            *service_active = false;
        }
        ParserSection::None => {
            if *section == ParserSection::Service && *service_active && current.is_valid() {
                finalize_service(current, service_count);
            }
            *section = ParserSection::None;
            *service_active = false;
        }
    }
}

fn handle_service_key_value(line: &[u8], current: &mut ServiceConfig) {
    if let Some((key, value)) = split_key_value(line) {
        let key_str = slice_to_static(key);
        let value_str = slice_to_static(value);
        let value_trimmed = strip_optional_quotes(value_str);

        if eq_ignore_ascii_case(key_str, "Description") {
            current.description = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "ExecStart") {
            current.exec_start = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "Restart") {
            current.restart = RestartPolicy::from_str(to_ascii_lower(value_trimmed));
        } else if eq_ignore_ascii_case(key_str, "RestartLimitIntervalSec") {
            current.restart_settings.interval_sec = parse_u64(value_trimmed, RESPAWN_WINDOW_SEC);
        } else if eq_ignore_ascii_case(key_str, "RestartLimitBurst") {
            current.restart_settings.burst = parse_u32(value_trimmed, MAX_RESPAWN_COUNT);
        } else if eq_ignore_ascii_case(key_str, "RestartSec") {
            let seconds = parse_u64(value_trimmed, RESTART_DELAY_MS / 1000);
            current.restart_delay_ms = seconds.saturating_mul(1000);
        } else if eq_ignore_ascii_case(key_str, "After") {
            current.after = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "WantedBy") {
            current.wants = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "Unit") && current.name.is_empty() {
            current.name = value_trimmed;
        }
    }
}

fn handle_init_key_value(line: &[u8]) {
    if let Some((key, value)) = split_key_value(line) {
        let key_str = slice_to_static(key);
        let value_str = slice_to_static(value);
        let trimmed = strip_optional_quotes(value_str);

        unsafe {
            if eq_ignore_ascii_case(key_str, "DefaultTarget") && !trimmed.is_empty() {
                DEFAULT_BOOT_TARGET = trimmed;
            } else if eq_ignore_ascii_case(key_str, "FallbackTarget") && !trimmed.is_empty() {
                FALLBACK_BOOT_TARGET = trimmed;
            } else if eq_ignore_ascii_case(key_str, "LogLevel") && !trimmed.is_empty() {
                INIT_LOG_LEVEL = InitLogLevel::from_str(trimmed);
            }
        }
    }
}

fn store_service_field(
    service_idx: usize,
    field_idx: usize,
    value: &'static str,
    label: &str,
) -> &'static str {
    if value.is_empty() {
        return EMPTY_STR;
    }

    unsafe {
        let slot = service_idx * SERVICE_FIELD_COUNT + field_idx;
        if slot >= STRING_STORAGE.len() {
            log_warn("Unit config storage exhausted");
            print("         Field: ");
            print(label);
            print("\n");
            return EMPTY_STR;
        }

        let dest = &mut STRING_STORAGE[slot];
        let bytes = value.as_bytes();
        let max_copy = if MAX_FIELD_LEN == 0 { 0 } else { MAX_FIELD_LEN - 1 };
        let mut copy_len = bytes.len();
        if copy_len > max_copy {
            copy_len = max_copy;
            log_warn("Unit config field truncated");
            print("         Field: ");
            print(label);
            print("\n");
        }

        for i in 0..copy_len {
            dest[i] = bytes[i];
        }
        if copy_len < MAX_FIELD_LEN {
            dest[copy_len] = 0;
        }
        STRING_LENGTHS[slot] = copy_len;

        std::str::from_utf8_unchecked(&dest[..copy_len])
    }
}

fn finalize_service(service: &ServiceConfig, service_count: &mut usize) {
    unsafe {
        if *service_count >= MAX_SERVICES || !service.is_valid() {
            return;
        }

        let idx = *service_count;
        let mut stored = *service;

        stored.name = store_service_field(idx, FIELD_IDX_NAME, stored.name, "Unit");
        stored.description = store_service_field(idx, FIELD_IDX_DESCRIPTION, stored.description, "Description");
        stored.exec_start = store_service_field(idx, FIELD_IDX_EXEC_START, stored.exec_start, "ExecStart");
        stored.after = store_service_field(idx, FIELD_IDX_AFTER, stored.after, "After");
        stored.wants = store_service_field(idx, FIELD_IDX_WANTS, stored.wants, "WantedBy");

        SERVICE_CONFIGS[idx] = stored;
        *service_count += 1;
    }
}

fn trim_slice(slice: &[u8]) -> &[u8] {
    let mut start = 0usize;
    let mut end = slice.len();

    while start < end && is_whitespace(slice[start]) {
        start += 1;
    }
    while end > start && is_whitespace(slice[end - 1]) {
        end -= 1;
    }

    &slice[start..end]
}

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

fn strip_brackets(line: &[u8]) -> &[u8] {
    if line.len() >= 2 && line[0] == b'[' && line[line.len() - 1] == b']' {
        trim_slice(&line[1..line.len() - 1])
    } else {
        line
    }
}

fn identify_section(header: &[u8]) -> ParserSection {
    if starts_with_ignore_case(header, b"Service") {
        ParserSection::Service
    } else if starts_with_ignore_case(header, b"Init") {
        ParserSection::Init
    } else {
        ParserSection::None
    }
}

fn extract_quoted_identifier(header: &[u8]) -> Option<&[u8]> {
    if let Some(start) = header.iter().position(|&b| b == b'"') {
        let rest = &header[start + 1..];
        if let Some(end) = rest.iter().position(|&b| b == b'"') {
            let candidate = &rest[..end];
            if !candidate.is_empty() {
                return Some(candidate);
            }
        }
    }
    None
}

fn split_key_value(line: &[u8]) -> Option<(&[u8], &[u8])> {
    if let Some(pos) = line.iter().position(|&b| b == b'=') {
        let key = trim_slice(&line[..pos]);
        let value = trim_slice(&line[pos + 1..]);
        if !key.is_empty() {
            return Some((key, value));
        }
    }
    None
}

fn slice_to_static(slice: &[u8]) -> &'static str {
    if slice.is_empty() {
        return EMPTY_STR;
    }
    unsafe {
        let base = CONFIG_BUFFER.as_ptr() as usize;
        let start = slice.as_ptr() as usize;
        if start < base {
            return EMPTY_STR;
        }
        let offset = start - base;
        if offset >= CONFIG_BUFFER.len() {
            return EMPTY_STR;
        }
        let end = match offset.checked_add(slice.len()) {
            Some(val) => val,
            None => return EMPTY_STR,
        };
        if end > CONFIG_BUFFER.len() {
            return EMPTY_STR;
        }
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(
            CONFIG_BUFFER.as_ptr().add(offset),
            slice.len(),
        ))
    }
}

fn strip_optional_quotes(value: &'static str) -> &'static str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        let inner = &bytes[1..bytes.len() - 1];
        slice_to_static(inner)
    } else {
        value
    }
}

fn eq_ignore_ascii_case(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (ac, bc) in a.bytes().zip(b.bytes()) {
        if ac.to_ascii_lowercase() != bc.to_ascii_lowercase() {
            return false;
        }
    }
    true
}

fn starts_with_ignore_case(haystack: &[u8], needle: &[u8]) -> bool {
    if haystack.len() < needle.len() {
        return false;
    }
    for (h, &n) in haystack.iter().zip(needle) {
        if h.to_ascii_lowercase() != n.to_ascii_lowercase() {
            return false;
        }
    }
    true
}

fn parse_u32(value: &'static str, default: u32) -> u32 {
    if value.is_empty() {
        return default;
    }
    let mut result: u32 = 0;
    for &b in value.as_bytes() {
        if b < b'0' || b > b'9' {
            return default;
        }
        result = result.saturating_mul(10).saturating_add((b - b'0') as u32);
    }
    result
}

fn parse_u64(value: &'static str, default: u64) -> u64 {
    if value.is_empty() {
        return default;
    }
    let mut result: u64 = 0;
    for &b in value.as_bytes() {
        if b < b'0' || b > b'9' {
            return default;
        }
        result = result.saturating_mul(10).saturating_add((b - b'0') as u64);
    }
    result
}

fn to_ascii_lower(value: &'static str) -> &'static str {
    // Values are stored in CONFIG_BUFFER, mutate in place for lowercase
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return value;
    }
    unsafe {
        let base = CONFIG_BUFFER.as_ptr() as usize;
        let start = value.as_ptr() as usize;
        if start < base {
            return value;
        }
        let offset = start - base;
        if offset >= CONFIG_BUFFER.len() {
            return value;
        }
        let end = match offset.checked_add(bytes.len()) {
            Some(val) => val,
            None => return value,
        };
        if end > CONFIG_BUFFER.len() {
            return value;
        }
        let ptr = CONFIG_BUFFER.as_mut_ptr().add(offset);
        for i in 0..bytes.len() {
            let b = ptr.add(i).read();
            ptr.add(i).write(b.to_ascii_lowercase());
        }
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, bytes.len()))
    }
}

fn select_boot_target(
    runlevel: i32,
    default_target: &'static str,
    fallback_target: &'static str,
) -> &'static str {
    match runlevel {
        0 => POWER_OFF_TARGET_NAME,
        1 => fallback_target,
        2 => default_target,
        3 => NETWORK_TARGET_NAME,
        _ => default_target,
    }
}

fn service_matches_target(service: &ServiceConfig, target: &str) -> bool {
    if service.wants.is_empty() {
        return true;
    }

    for token in service.wants.split_whitespace() {
        if token == target {
            return true;
        }
    }

    false
}

const FALLBACK_SERVICE: ServiceConfig = ServiceConfig {
    name: "fallback-shell",
    description: "Emergency fallback interactive shell",
    exec_start: "/bin/sh",
    restart: RestartPolicy::Always,
    restart_settings: RestartSettings {
        burst: MAX_RESPAWN_COUNT,
        interval_sec: RESPAWN_WINDOW_SEC,
    },
    restart_delay_ms: RESTART_DELAY_MS,
    after: EMPTY_STR,
    wants: DEFAULT_TARGET_NAME,
};

/// Service state tracking
#[derive(Clone, Copy)]
struct ServiceState {
    respawn_count: u32,
    window_start: u64,
    total_starts: u64,
    pid: i64, // Current PID of running service (0 if not running)
}

impl ServiceState {
    const fn new() -> Self {
        Self {
            respawn_count: 0,
            window_start: 0,
            total_starts: 0,
            pid: 0,
        }
    }

    fn allow_attempt(
        &mut self,
        current_time: u64,
        policy: RestartPolicy,
        settings: RestartSettings,
    ) -> bool {
        match policy {
            RestartPolicy::No => {
                if self.total_starts == 0 {
                    self.total_starts = 1;
                    true
                } else {
                    false
                }
            }
            RestartPolicy::OnFailure | RestartPolicy::Always => {
                if settings.interval_sec > 0 {
                    if self.window_start == 0 {
                        self.window_start = current_time;
                        self.respawn_count = 0;
                    } else if current_time.saturating_sub(self.window_start) >= settings.interval_sec {
                        self.window_start = current_time;
                        self.respawn_count = 0;
                    }
                }

                if settings.burst != 0 && self.respawn_count >= settings.burst {
                    return false;
                }

                self.respawn_count = self.respawn_count.saturating_add(1);
                self.total_starts = self.total_starts.saturating_add(1);
                true
            }
        }
    }
}

/// Running service tracker (for parallel supervision)
#[derive(Clone, Copy)]
struct RunningService<'a> {
    config: &'a ServiceConfig,
    state: ServiceState,
}


/// systemd-style logging with colors
fn log_info(msg: &str) {
    print("\x1b[1;32m[  OK  ]\x1b[0m ");  // Green
    print(msg);
    print("\n");
}

fn log_start(msg: &str) {
    print("\x1b[1;36m[ .... ]\x1b[0m ");  // Cyan
    print(msg);
    print("\n");
}

fn log_fail(msg: &str) {
    print("\x1b[1;31m[FAILED]\x1b[0m ");  // Red
    print(msg);
    print("\n");
}

fn log_warn(msg: &str) {
    print("\x1b[1;33m[ WARN ]\x1b[0m ");  // Yellow
    print(msg);
    print("\n");
}

/// Simple timestamp (just a counter for now)
fn get_timestamp() -> u64 {
    static mut COUNTER: u64 = 0;
    unsafe {
        COUNTER += 1;
        COUNTER
    }
}

/// Delay function
fn delay_ms(ms: u64) {
    for _ in 0..(ms * 1000) {
        unsafe { asm!("pause") }
    }
}

/// Main init loop with service supervision
fn init_main() -> ! {
    // Now we can use std::io!
    print("\n");
    print("\x1b[1;34m=========================================\x1b[0m\n");  // Blue
    print("\x1b[1;34m  NexaOS Init (ni) - PID 1\x1b[0m\n");
    print("\x1b[1;34m  Hybrid Kernel - Process Supervisor\x1b[0m\n");
    print("\x1b[1;34m=========================================\x1b[0m\n");
    print("\n");
    
    // Verify we are PID 1
    let pid = getpid();
    let ppid = getppid();
    
    let mut buf = [0u8; 32];
    
    log_start("Verifying init process identity");
    print("         PID: ");
    print(itoa(pid as u64, &mut buf));
    print("\n");
    print("         PPID: ");
    print(itoa(ppid as u64, &mut buf));
    print("\n");
    
    if pid != 1 {
        log_fail("Not running as PID 1 - system unstable");
        exit(1);
    }
    
    if ppid != 0 {
        log_warn("PPID is not 0 - unusual configuration");
    } else {
        log_info("Init process identity verified");
    }
    
    // Get current runlevel
    log_start("Querying system runlevel");
    let runlevel = get_runlevel();
    if runlevel >= 0 {
        print("         Runlevel: ");
        print(itoa(runlevel as u64, &mut buf));
        print("\n");
        log_info("System runlevel configured");
    } else {
        log_warn("Failed to query runlevel");
    }
    
    print("\n");
    log_info("System initialization complete");
    print("\n");
    
    // Load service catalog
    print("\n");
    log_start("Loading unit catalog");
    let catalog = load_service_catalog();
    let services = catalog.services;
    if services.is_empty() {
        log_warn("No unit definitions found, preparing fallback shell");
    } else {
        log_info("Loaded units from /etc/ni/ni.conf");
        print("         Unit count: ");
        print(itoa(services.len() as u64, &mut buf));
        print("\n");
    }

    let boot_target = select_boot_target(runlevel, catalog.default_target, catalog.fallback_target);
    print("         Boot target: ");
    print(boot_target);
    print("\n");

    // Service supervision with parallel fork/exec/wait
    print("\n");
    log_start("Starting parallel unit supervision");
    log_info("Using fork/exec/wait4 supervision model");
    print("\n");

    // Build list of services to run
    let mut running_services: [Option<RunningService>; MAX_SERVICES] = [None; MAX_SERVICES];
    let mut service_count = 0usize;

    if services.is_empty() {
        log_warn("No configured units, using fallback shell");
        running_services[0] = Some(RunningService {
            config: &FALLBACK_SERVICE,
            state: ServiceState::new(),
        });
        service_count = 1;
    } else {
        for service in services {
            if service_matches_target(service, boot_target) {
                if service_count < MAX_SERVICES {
                    running_services[service_count] = Some(RunningService {
                        config: service,
                        state: ServiceState::new(),
                    });
                    service_count += 1;
                }
            }
        }

        if service_count == 0 {
            log_warn("No unit matched boot target; using fallback shell");
            running_services[0] = Some(RunningService {
                config: &FALLBACK_SERVICE,
                state: ServiceState::new(),
            });
            service_count = 1;
        }
    }

    print("         Active units: ");
    print(itoa(service_count as u64, &mut buf));
    print("\n\n");

    // NOTE: Since fork() is not fully implemented (returns fake PID),
    // we cannot run services as separate processes in parallel.
    // Instead, we show login prompt directly and exec into shell.
    
    print("\n");
    log_info("System ready - starting login sequence");
    print("\n");
    
    // Show login prompt and authenticate
    show_login_and_exec_shell(&mut buf);
}

/// Parallel service supervisor - manages multiple services simultaneously
fn parallel_service_supervisor(running_services: &mut [Option<RunningService>; MAX_SERVICES], service_count: usize, buf: &mut [u8]) -> ! {
    // Start all services initially
    for i in 0..service_count {
        if let Some(ref mut rs) = running_services[i] {
            let service = rs.config;
            let state = &mut rs.state;
            
            log_start("Starting unit");
            print("         Unit: ");
            if service.name.is_empty() {
                print(service.exec_start);
            } else {
                print(service.name);
            }
            print("\n");
            
            let pid = start_service(service, buf);
            if pid > 0 {
                state.pid = pid;
                state.total_starts = state.total_starts.saturating_add(1);
                log_info("Unit started");
                print("         PID: ");
                print(itoa(pid as u64, buf));
                print("\n\n");
            } else {
                log_fail("Failed to start unit");
                print("\n");
            }
        }
    }

    // Main supervision loop - wait for any child process to exit and restart if needed
    loop {
        let mut status: i32 = 0;
        let pid = wait4(-1, &mut status as *mut i32, 0); // Wait for any child
        
        if pid < 0 {
            // No children or error, delay and retry
            delay_ms(1000);
            continue;
        }

        let timestamp = get_timestamp();
        
        // Find which service exited
        for i in 0..service_count {
            if let Some(ref mut rs) = running_services[i] {
                if rs.state.pid == pid {
                    let service = rs.config;
                    let state = &mut rs.state;
                    
                    log_warn("Unit terminated");
                    print("         Unit: ");
                    if service.name.is_empty() {
                        print(service.exec_start);
                    } else {
                        print(service.name);
                    }
                    print("\n");
                    print("         PID: ");
                    print(itoa(pid as u64, buf));
                    print("\n");
                    print("         Exit status: ");
                    print(itoa(status as u64, buf));
                    print("\n");
                    
                    state.pid = 0;
                    
                    // Check if we should restart
                    let should_restart = match service.restart {
                        RestartPolicy::No => false,
                        RestartPolicy::Always => true,
                        RestartPolicy::OnFailure => status != 0,
                    };
                    
                    if should_restart && state.allow_attempt(timestamp, service.restart, service.restart_settings) {
                        delay_ms(service.restart_delay_ms);
                        
                        log_start("Restarting unit");
                        print("         Unit: ");
                        if service.name.is_empty() {
                            print(service.exec_start);
                        } else {
                            print(service.name);
                        }
                        print("\n");
                        
                        let new_pid = start_service(service, buf);
                        if new_pid > 0 {
                            state.pid = new_pid;
                            log_info("Unit restarted");
                            print("         PID: ");
                            print(itoa(new_pid as u64, buf));
                            print("\n\n");
                        } else {
                            log_fail("Failed to restart unit");
                            print("\n");
                        }
                    } else if should_restart {
                        log_fail("Restart limit exceeded for unit");
                        print("         Unit: ");
                        if service.name.is_empty() {
                            print(service.exec_start);
                        } else {
                            print(service.name);
                        }
                        print("\n\n");
                    } else {
                        log_info("Unit will not be restarted (policy: no restart)");
                        print("\n");
                    }
                    
                    break;
                }
            }
        }
    }
}

/// Start a single service (fork and exec)
fn start_service(service: &ServiceConfig, _buf: &mut [u8]) -> i64 {
    let pid = fork();
    
    if pid < 0 {
        // Fork failed
        return -1;
    }
    
    if pid == 0 {
        // Child process - exec the service
        let exec_bytes = service.exec_start.as_bytes();
        if exec_bytes.is_empty() {
            exit(1);
        }
        
        // Prepare null-terminated path
        let mut path_with_null = [0u8; 256];
        if exec_bytes.len() >= path_with_null.len() {
            exit(1);
        }
        
        for (i, &b) in exec_bytes.iter().enumerate() {
            path_with_null[i] = b;
        }
        path_with_null[exec_bytes.len()] = 0;
        
        let exec_path = unsafe {
            std::str::from_utf8_unchecked(&path_with_null[..exec_bytes.len()])
        };
        
        let argv: [*const u8; 2] = [
            path_with_null.as_ptr(),
            std::ptr::null(),
        ];
        let envp: [*const u8; 1] = [
            std::ptr::null(),
        ];
        
        execve(exec_path, &argv, &envp);
        
        // If execve returns, it failed
        exit(1);
    }
    
    // Parent process - return child PID
    pid
}

/// Old single-service loop (kept for reference but not used)
#[allow(dead_code)]
fn run_service_loop(service_state: &mut ServiceState, service: &ServiceConfig, buf: &mut [u8]) -> ! {
    loop {
        let timestamp = get_timestamp();
        
        if !service_state.allow_attempt(timestamp, service.restart, service.restart_settings) {
            log_fail("Restart limit reached");
            print("         Unit: ");
            if service.name.is_empty() {
                print(service.exec_start);
            } else {
                print(service.name);
            }
            print("\n");
            eprint("ni: CRITICAL: Restart limit exceeded for unit ");
            if service.name.is_empty() {
                eprint(service.exec_start);
            } else {
                eprint(service.name);
            }
            eprint("\n");
            eprint("ni: Restart policy: ");
            match service.restart {
                RestartPolicy::No => eprint("none\n"),
                RestartPolicy::OnFailure => eprint("on-failure\n"),
                RestartPolicy::Always => eprint("always\n"),
            }
            eprint("ni: Restart burst: ");
            print(itoa(service.restart_settings.burst as u64, buf));
            eprint(" in ");
            print(itoa(service.restart_settings.interval_sec, buf));
            eprint(" seconds\n");
            eprint("ni: Total attempts: ");
            print(itoa(service_state.total_starts, buf));
            eprint("\n\n");

            match service.restart {
                RestartPolicy::No => exit(1),
                _ => {
                    delay_ms(service.restart_delay_ms.max(RESTART_DELAY_MS));
                    continue;
                }
            }
        }

        log_start("Spawning unit");
        print("         Unit: ");
        if service.name.is_empty() {
            print(service.exec_start);
        } else {
            print(service.name);
        }
        print("\n");
        if !service.description.is_empty() {
            print("         Description: ");
            print(service.description);
            print("\n");
        }
        print("         Attempt: ");
        print(itoa(service_state.total_starts, buf));
        print("\n");

        // Fork and execute unit
        let pid = fork();

        if pid < 0 {
            log_fail("fork() failed");
            print("         Unit: ");
            if service.name.is_empty() {
                print(service.exec_start);
            } else {
                print(service.name);
            }
            print("\n");
            delay_ms(service.restart_delay_ms);
            continue;
        }

        log_info("Unit process created");
        print("         Child PID: ");
        print(itoa(pid as u64, buf));
        print("\n\n");

        // Add null terminator to ExecStart for execve path parameter
        let mut path_with_null = [0u8; 256];
        let exec_bytes = service.exec_start.as_bytes();
        if exec_bytes.is_empty() {
            log_fail("ExecStart not defined for unit");
            delay_ms(service.restart_delay_ms);
            continue;
        }
        if exec_bytes.len() >= path_with_null.len() {
            log_fail("ExecStart exceeds maximum length (255 bytes)");
            delay_ms(service.restart_delay_ms);
            continue;
        }

        for (i, &b) in exec_bytes.iter().enumerate() {
            path_with_null[i] = b;
        }
        path_with_null[exec_bytes.len()] = 0;

        // Execute unit directly - this jumps and never returns normally
        let argv: [*const u8; 2] = [
            path_with_null.as_ptr(),
            std::ptr::null(),
        ];
        let envp: [*const u8; 1] = [
            std::ptr::null(),
        ];

        let exec_path = unsafe {
            std::str::from_utf8_unchecked(&path_with_null[..exec_bytes.len()])
        };

        execve(exec_path, &argv, &envp);

        // If execve returns, it failed
        log_fail("execve failed");
        print("         Unit: ");
        if service.name.is_empty() {
            print(service.exec_start);
        } else {
            print(service.name);
        }
        print("\n");
        delay_ms(service.restart_delay_ms);
    }
}

/// Show login prompt and exec into shell after successful authentication
/// This works within single-process model without needing fork()
fn show_login_and_exec_shell(buf: &mut [u8]) -> ! {
    // Display welcome banner
    print("\n");
    print("\x1b[1;36m╔════════════════════════════════════════╗\x1b[0m\n");
    print("\x1b[1;36m║                                        ║\x1b[0m\n");
    print("\x1b[1;36m║          \x1b[1;37mWelcome to NexaOS\x1b[1;36m                     ║\x1b[0m\n");
    print("\x1b[1;36m║                                        ║\x1b[0m\n");
    print("\x1b[1;36m║    \x1b[0mHybrid Kernel Operating System\x1b[1;36m      ║\x1b[0m\n");
    print("\x1b[1;36m║                                        ║\x1b[0m\n");
    print("\x1b[1;36m╚════════════════════════════════════════╝\x1b[0m\n");
    print("\n");
    print("\x1b[1;32mNexaOS Login\x1b[0m\n");
    print("\x1b[0;36mDefault credentials: root/root\x1b[0m\n");
    print("\n");
    
    // Ensure default root user exists
    ensure_default_user();
    
    // Read username
    print("login: ");
    let mut username_buf = [0u8; 64];
    let username_len = read_line_input(&mut username_buf);

    debug_print_len("username_len_raw", username_len);
    debug_print_ptr("username_buf_ptr", username_buf.as_ptr() as u64);
    
    if username_len == 0 {
        print("\n\x1b[1;31mLogin failed: empty username\x1b[0m\n");
        exit(1);
    }
    
    if username_len > 64 {
        print("\n\x1b[1;31mLogin failed: username too long\x1b[0m\n");
        exit(1);
    }
    
    // Read password
    print("password: ");
    let mut password_buf = [0u8; 64];
    let password_len = read_password_input(&mut password_buf);

    debug_print_len("password_len_raw", password_len);
    debug_print_ptr("password_buf_ptr", password_buf.as_ptr() as u64);
    
    if password_len > 64 {
        print("\n\x1b[1;31mLogin failed: password too long\x1b[0m\n");
        exit(1);
    }
    
    // Authenticate - use safe indexing
    let username_slice = if username_len <= username_buf.len() {
        &username_buf[..username_len]
    } else {
        &username_buf[..]
    };
    
    let password_slice = if password_len <= password_buf.len() {
        &password_buf[..password_len]
    } else {
        &password_buf[..]
    };
    
    if debug_logs_enabled() {
        print("\n[DEBUG] Calling authenticate_user...\n");
    }
    let login_success = authenticate_user(username_slice, password_slice);
    if debug_logs_enabled() {
        print("[DEBUG] authenticate_user returned\n");
    }
    
    if login_success {
        print("\n\x1b[1;32mLogin successful!\x1b[0m\n");
        print("Starting user session...\n\n");
        
        if debug_logs_enabled() {
            print("[DEBUG] About to call execve...\n");
        }
        
        // Exec into shell (ensure C string semantics for kernel syscall)
        let shell_bytes = b"/bin/sh";
        let mut path_with_null = [0u8; 256];

        if shell_bytes.len() >= path_with_null.len() {
            print("\n\x1b[1;31mShell path too long\x1b[0m\n");
            exit(1);
        }

        path_with_null[..shell_bytes.len()].copy_from_slice(shell_bytes);
        path_with_null[shell_bytes.len()] = 0;

        let exec_path = unsafe {
            std::str::from_utf8_unchecked(&path_with_null[..shell_bytes.len()])
        };

        let argv: [*const u8; 2] = [
            path_with_null.as_ptr(),
            std::ptr::null(),
        ];
        let envp: [*const u8; 1] = [
            std::ptr::null(),
        ];

        execve(exec_path, &argv, &envp);
        
        // If exec fails, show error
        print("\n\x1b[1;31mFailed to start shell\x1b[0m\n");
        exit(1);
    } else {
        print("\n\x1b[1;31mLogin incorrect\x1b[0m\n");
        exit(1);
    }
}

/// Read a line from stdin
fn read_line_input(buf: &mut [u8]) -> usize {
    use std::io::Read;
    let mut stdin = io::stdin();
    let mut pos = 0;
    let mut tmp = [0u8; 1];
    
    while pos < buf.len() {
        let n = match stdin.read(&mut tmp) {
            Ok(n) => n,
            Err(_) => {
                debug_print_len("read_line_input_err", pos);
                break;
            }
        };
        if n == 0 {
            debug_print_len("read_line_input_retry", pos);
            continue;
        }
        
        let ch = tmp[0];
        debug_print_ptr("read_line_input_byte", ch as u64);
        
        // Handle backspace
        if ch == 8 || ch == 127 {
            if pos > 0 {
                pos -= 1;
                print("\x08 \x08");
            }
            continue;
        }
        
        // Handle newline
        if ch == b'\n' || ch == b'\r' {
            if pos == 0 {
                debug_print_len("read_line_input_skip_newline", pos);
                // Ignore stray newline before any input arrives.
                continue;
            }
            print("\n");
            break;
        }
        
        // Printable characters
        if ch >= 32 && ch < 127 {
            buf[pos] = ch;
            pos += 1;
            let _ = io::stdout().write_all(&[ch]);
        }
    }
    
    pos
}

/// Read password (masked input)
fn read_password_input(buf: &mut [u8]) -> usize {
    use std::io::Read;
    let mut stdin = io::stdin();
    let mut pos = 0;
    let mut tmp = [0u8; 1];
    
    while pos < buf.len() {
        let n = match stdin.read(&mut tmp) {
            Ok(n) => n,
            Err(_) => {
                debug_print_len("read_password_input_err", pos);
                break;
            }
        };
        if n == 0 {
            debug_print_len("read_password_input_retry", pos);
            continue;
        }
        
        let ch = tmp[0];
        debug_print_ptr("read_password_input_byte", ch as u64);
        
        // Handle backspace
        if ch == 8 || ch == 127 {
            if pos > 0 {
                pos -= 1;
                let _ = io::stdout().write_all(b"\x08 \x08");
            }
            continue;
        }
        
        // Handle newline
        if ch == b'\n' || ch == b'\r' {
            if pos == 0 {
                debug_print_len("read_password_input_skip_newline", pos);
                continue;
            }
            let _ = io::stdout().write_all(b"\n");
            break;
        }
        
        // Printable characters (but don't echo)
        if ch >= 32 && ch < 127 {
            buf[pos] = ch;
            pos += 1;
            let _ = io::stdout().write_all(b"*");
        }
    }
    
    pos
}

/// Ensure default root user exists
fn ensure_default_user() {
    let username = b"root";
    let password = b"root";
    
    #[repr(C)]
    struct UserRequest {
        username_ptr: u64,
        username_len: u64,
        password_ptr: u64,
        password_len: u64,
        flags: u64,
    }
    
    let req = UserRequest {
        username_ptr: username.as_ptr() as u64,
        username_len: username.len() as u64,
        password_ptr: password.as_ptr() as u64,
        password_len: password.len() as u64,
        flags: 1, // Admin flag
    };
    
    syscall1(SYS_USER_ADD, &req as *const UserRequest as u64);
}

/// Authenticate user
fn authenticate_user(username: &[u8], password: &[u8]) -> bool {
    #[repr(C)]
    struct UserRequest {
        username_ptr: u64,
        username_len: u64,
        password_ptr: u64,
        password_len: u64,
        flags: u64,
    }
    
    let req = UserRequest {
        username_ptr: username.as_ptr() as u64,
        username_len: username.len() as u64,
        password_ptr: password.as_ptr() as u64,
        password_len: password.len() as u64,
        flags: 0,
    };
    
    debug_print_ptr("username_ptr", req.username_ptr);
    debug_print_len("username_len", username.len());
    debug_print_ptr("password_ptr", req.password_ptr);
    debug_print_len("password_len", password.len());

    let result = syscall1(SYS_USER_LOGIN, &req as *const UserRequest as u64);
    if result == 0 {
        true
    } else {
        let errno = syscall1(SYS_GETERRNO, 0);
        if debug_logs_enabled() {
            let mut buf = [0u8; 32];
            print("[DEBUG] login errno: ");
            print(itoa(errno, &mut buf));
            print("\n");
        }
        false
    }
}

// Entry point for the init program
// Using standard main() to ensure std runtime initialization
// This allows std::io, TLS, and other std features to work correctly
// Using extern "C" to provide the C ABI main function directly
// argc/argv are ignored since we don't use command-line arguments
#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    // First test: Try to write directly via syscall BEFORE any std::io
    print_raw("[DEBUG] Entered main(), before std initialization\n");
    
    // Try to use std::io
    print_raw("[DEBUG] Attempting std::io::stdout()...\n");
    let _ = io::stdout().write_all(b"[DEBUG] std::io works!\n");
    print_raw("[DEBUG] std::io succeeded\n");
    
    // Now call the actual init main
    init_main();
    // Never reached (init_main has infinite loop)
    0
}

fn debug_print_ptr(label: &str, value: u64) {
    if !debug_logs_enabled() {
        return;
    }
    print("[DEBUG] ");
    print(label);
    print(": 0x");

    let mut buf = [0u8; 16];
    for i in 0..16 {
        let shift = (15 - i) * 4;
        let nibble = ((value >> shift) & 0xF) as u8;
        buf[i] = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + (nibble - 10)
        };
    }

    let _ = io::stdout().write_all(&buf);
    print("\n");
}

fn debug_print_len(label: &str, value: usize) {
    if !debug_logs_enabled() {
        return;
    }
    print("[DEBUG] ");
    print(label);
    print(": ");

    let mut buf = [0u8; 32];
    print(itoa(value as u64, &mut buf));
    print("\n");
}
