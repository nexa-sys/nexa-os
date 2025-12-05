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

use core::cell::UnsafeCell;
use std::arch::asm;
use std::fs;
use std::io::{self, Read, Write};
use std::panic;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn install_minimal_panic_hook() {
    panic::set_hook(Box::new(|_info| {
        let _ = eprintln!("[ni] panic");
        std::process::abort();
    }));
}

fn announce_runtime_start() {
    println!("NI_RUNNING");
}

// System call numbers
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_WAIT4: u64 = 61;

// Service management constants
const MAX_RESPAWN_COUNT: u32 = 5; // Max respawns within window
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
fn syscall0(n: u64) -> u64 {
    syscall3(n, 0, 0, 0)
}

fn flush_stdout() {
    let _ = io::stdout().flush();
}

/// Exit process using std::process
fn exit(code: i32) -> ! {
    std::process::exit(code)
}

/// Fork process - wraps kernel syscall
fn fork() -> i64 {
    let ret = syscall0(SYS_FORK);
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

/// Execute program - wraps kernel syscall
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

/// Wait for process state change (wait4 syscall)
fn wait4(pid: i64, status: *mut i32, options: i32) -> i64 {
    let ret = syscall3(SYS_WAIT4, pid as u64, status as u64, options as u64);
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

// POSIX wait status macros
fn wexitstatus(status: i32) -> i32 {
    (status >> 8) & 0xff
}

fn wifexited(status: i32) -> bool {
    (status & 0x7f) == 0
}

fn wifsignaled(status: i32) -> bool {
    ((status & 0x7f) + 1) as i8 >= 2
}

fn wtermsig(status: i32) -> i32 {
    status & 0x7f
}

/// Simple integer to string conversion
fn itoa(n: u64, buf: &mut [u8]) -> &str {
    if buf.is_empty() {
        return "";
    }
    
    if n == 0 {
        buf[0] = b'0';
        return std::str::from_utf8(&buf[0..1]).unwrap();
    }

    let mut i = 0;
    let mut num = n;
    while num > 0 && i < buf.len() {
        buf[i] = b'0' + (num % 10) as u8;
        num /= 10;
        i += 1;
    }

    // Reverse
    for j in 0..i / 2 {
        buf.swap(j, i - 1 - j);
    }

    std::str::from_utf8(&buf[0..i]).unwrap()
}

const CONFIG_BUFFER_SIZE: usize = 4096;
const MAX_SERVICES: usize = 12;
const DEFAULT_TARGET_NAME: &str = "multi-user.target";
const FALLBACK_TARGET_NAME: &str = "rescue.target";
const EMPTY_STR: &str = "";
const MAX_FIELD_LEN: usize = 256;
const SERVICE_FIELD_COUNT: usize = 13; // Increased for new fields
const FIELD_IDX_NAME: usize = 0;
const FIELD_IDX_DESCRIPTION: usize = 1;
const FIELD_IDX_EXEC_START: usize = 2;
const FIELD_IDX_EXEC_STOP: usize = 3;
const FIELD_IDX_AFTER: usize = 4;
const FIELD_IDX_BEFORE: usize = 5;
const FIELD_IDX_WANTS: usize = 6;
const FIELD_IDX_REQUIRES: usize = 7;
const FIELD_IDX_USER: usize = 8;
const FIELD_IDX_GROUP: usize = 9;
const FIELD_IDX_WORKING_DIR: usize = 10;
const FIELD_IDX_STANDARD_OUTPUT: usize = 11;
const FIELD_IDX_RESERVED: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ServiceType {
    Simple,     // Default: service main process started directly
    Oneshot,    // Service process terminates, init continues
    Forking,    // Service forks, parent exits
    Dbus,       // Service acquires D-Bus name
    Notify,     // Service sends readiness notification
}

impl ServiceType {
    fn from_str(raw: &str) -> Self {
        let lower = raw.as_bytes();
        match lower {
            b"simple" => ServiceType::Simple,
            b"oneshot" => ServiceType::Oneshot,
            b"forking" => ServiceType::Forking,
            b"dbus" => ServiceType::Dbus,
            b"notify" => ServiceType::Notify,
            _ => ServiceType::Simple,
        }
    }
}

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ServiceState {
    Inactive,   // Not running
    Activating, // Being started
    Active,     // Running
    Deactivating, // Being stopped
    Failed,     // Failed or exited with error
}

impl ServiceState {
    fn as_str(&self) -> &'static str {
        match self {
            ServiceState::Inactive => "inactive",
            ServiceState::Activating => "activating",
            ServiceState::Active => "active",
            ServiceState::Deactivating => "deactivating",
            ServiceState::Failed => "failed",
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
    exec_stop: &'static str,
    service_type: ServiceType,
    restart: RestartPolicy,
    restart_settings: RestartSettings,
    restart_delay_ms: u64,
    timeout_start_sec: u64,
    timeout_stop_sec: u64,
    after: &'static str,
    before: &'static str,
    wants: &'static str,
    requires: &'static str,
    user: &'static str,
    group: &'static str,
    working_dir: &'static str,
    standard_output: &'static str,
}

impl ServiceConfig {
    const fn empty() -> Self {
        Self {
            name: EMPTY_STR,
            description: EMPTY_STR,
            exec_start: EMPTY_STR,
            exec_stop: EMPTY_STR,
            service_type: ServiceType::Simple,
            restart: RestartPolicy::Always,
            restart_settings: RestartSettings::new(),
            restart_delay_ms: RESTART_DELAY_MS,
            timeout_start_sec: 90,
            timeout_stop_sec: 90,
            after: EMPTY_STR,
            before: EMPTY_STR,
            wants: DEFAULT_TARGET_NAME,
            requires: EMPTY_STR,
            user: EMPTY_STR,
            group: EMPTY_STR,
            working_dir: EMPTY_STR,
            standard_output: "journal",
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

struct ConfigBuffer {
    inner: UnsafeCell<[u8; CONFIG_BUFFER_SIZE]>,
}

impl ConfigBuffer {
    const fn new() -> Self {
        Self {
            inner: UnsafeCell::new([0; CONFIG_BUFFER_SIZE]),
        }
    }

    unsafe fn as_mut_ptr(&self) -> *mut u8 {
        (*self.inner.get()).as_mut_ptr()
    }

    unsafe fn as_ptr(&self) -> *const u8 {
        (*self.inner.get()).as_ptr()
    }
}

unsafe impl Sync for ConfigBuffer {}

static CONFIG_BUFFER: ConfigBuffer = ConfigBuffer::new();
static mut SERVICE_CONFIGS: [ServiceConfig; MAX_SERVICES] = [ServiceConfig::empty(); MAX_SERVICES];
static mut DEFAULT_BOOT_TARGET: &'static str = DEFAULT_TARGET_NAME;
static mut FALLBACK_BOOT_TARGET: &'static str = FALLBACK_TARGET_NAME;
static mut STRING_STORAGE: [[u8; MAX_FIELD_LEN]; MAX_SERVICES * SERVICE_FIELD_COUNT] =
    [[0; MAX_FIELD_LEN]; MAX_SERVICES * SERVICE_FIELD_COUNT];
static mut STRING_LENGTHS: [usize; MAX_SERVICES * SERVICE_FIELD_COUNT] =
    [0; MAX_SERVICES * SERVICE_FIELD_COUNT];

#[inline]
fn config_buffer_capacity() -> usize {
    CONFIG_BUFFER_SIZE
}

#[inline]
unsafe fn config_buffer_ptr() -> *mut u8 {
    CONFIG_BUFFER.as_mut_ptr()
}

#[inline]
unsafe fn config_buffer_const_ptr() -> *const u8 {
    CONFIG_BUFFER.as_ptr()
}

#[inline]
unsafe fn config_buffer_slice(len: usize) -> &'static [u8] {
    core::slice::from_raw_parts(config_buffer_const_ptr(), len)
}

fn load_service_catalog() -> ServiceCatalog {
    unsafe {
        DEFAULT_BOOT_TARGET = DEFAULT_TARGET_NAME;
        FALLBACK_BOOT_TARGET = FALLBACK_TARGET_NAME;
        
        // Use std::fs for file operations - nrlib provides std I/O support
        match fs::read("/etc/ni/ni.conf") {
            Ok(content) => {
                log_info("Unit catalog file opened");
                
                let usable = core::cmp::min(content.len(), config_buffer_capacity());
                
                // Copy content to CONFIG_BUFFER
                let buffer_ptr = config_buffer_ptr();
                for i in 0..usable {
                    buffer_ptr.add(i).write(content[i]);
                }
                
                let mut diag_buf = [0u8; 32];
                let bytes_read = itoa(usable as u64, &mut diag_buf);
                println!("         Bytes read: {}", bytes_read);
                
                let service_count = parse_unit_file(usable);
                log_info("Unit catalog parsed successfully");
                
                ServiceCatalog {
                    services: &SERVICE_CONFIGS[0..service_count],
                    default_target: DEFAULT_BOOT_TARGET,
                    fallback_target: FALLBACK_BOOT_TARGET,
                }
            }
            Err(e) => {
                eprintln!("         File open failed: {:?}", e);
                ServiceCatalog {
                    services: &SERVICE_CONFIGS[0..0],
                    default_target: DEFAULT_BOOT_TARGET,
                    fallback_target: FALLBACK_BOOT_TARGET,
                }
            }
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
        let bytes = config_buffer_slice(len);
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
        } else if eq_ignore_ascii_case(key_str, "ExecStop") {
            current.exec_stop = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "Type") {
            current.service_type = ServiceType::from_str(value_trimmed);
        } else if eq_ignore_ascii_case(key_str, "Restart") {
            current.restart = RestartPolicy::from_str(to_ascii_lower(value_trimmed));
        } else if eq_ignore_ascii_case(key_str, "RestartLimitIntervalSec") {
            current.restart_settings.interval_sec = parse_u64(value_trimmed, RESPAWN_WINDOW_SEC);
        } else if eq_ignore_ascii_case(key_str, "RestartLimitBurst") {
            current.restart_settings.burst = parse_u32(value_trimmed, MAX_RESPAWN_COUNT);
        } else if eq_ignore_ascii_case(key_str, "RestartSec") {
            let seconds = parse_u64(value_trimmed, RESTART_DELAY_MS / 1000);
            current.restart_delay_ms = seconds.saturating_mul(1000);
        } else if eq_ignore_ascii_case(key_str, "TimeoutStartSec") {
            current.timeout_start_sec = parse_u64(value_trimmed, 90);
        } else if eq_ignore_ascii_case(key_str, "TimeoutStopSec") {
            current.timeout_stop_sec = parse_u64(value_trimmed, 90);
        } else if eq_ignore_ascii_case(key_str, "After") {
            current.after = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "Before") {
            current.before = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "WantedBy") {
            current.wants = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "RequiredBy") {
            current.requires = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "User") {
            current.user = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "Group") {
            current.group = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "WorkingDirectory") {
            current.working_dir = value_trimmed;
        } else if eq_ignore_ascii_case(key_str, "StandardOutput") {
            current.standard_output = value_trimmed;
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
            println!("         Field: {}", label);
            return EMPTY_STR;
        }

        let dest = &mut STRING_STORAGE[slot];
        let bytes = value.as_bytes();
        let max_copy = if MAX_FIELD_LEN == 0 {
            0
        } else {
            MAX_FIELD_LEN - 1
        };
        let mut copy_len = bytes.len();
        if copy_len > max_copy {
            copy_len = max_copy;
            log_warn("Unit config field truncated");
            println!("         Field: {}", label);
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
        stored.description = store_service_field(
            idx,
            FIELD_IDX_DESCRIPTION,
            stored.description,
            "Description",
        );
        stored.exec_start =
            store_service_field(idx, FIELD_IDX_EXEC_START, stored.exec_start, "ExecStart");
        stored.exec_stop =
            store_service_field(idx, FIELD_IDX_EXEC_STOP, stored.exec_stop, "ExecStop");
        stored.after = store_service_field(idx, FIELD_IDX_AFTER, stored.after, "After");
        stored.before = store_service_field(idx, FIELD_IDX_BEFORE, stored.before, "Before");
        stored.wants = store_service_field(idx, FIELD_IDX_WANTS, stored.wants, "WantedBy");
        stored.requires = store_service_field(idx, FIELD_IDX_REQUIRES, stored.requires, "RequiredBy");
        stored.user = store_service_field(idx, FIELD_IDX_USER, stored.user, "User");
        stored.group = store_service_field(idx, FIELD_IDX_GROUP, stored.group, "Group");
        stored.working_dir =
            store_service_field(idx, FIELD_IDX_WORKING_DIR, stored.working_dir, "WorkingDirectory");
        stored.standard_output = store_service_field(
            idx,
            FIELD_IDX_STANDARD_OUTPUT,
            stored.standard_output,
            "StandardOutput",
        );

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
        let base = config_buffer_const_ptr() as usize;
        let start = slice.as_ptr() as usize;
        if start < base {
            return EMPTY_STR;
        }
        let offset = start - base;
        if offset >= config_buffer_capacity() {
            return EMPTY_STR;
        }
        let end = match offset.checked_add(slice.len()) {
            Some(val) => val,
            None => return EMPTY_STR,
        };
        if end > config_buffer_capacity() {
            return EMPTY_STR;
        }
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(
            config_buffer_const_ptr().add(offset),
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
        let base = config_buffer_const_ptr() as usize;
        let start = value.as_ptr() as usize;
        if start < base {
            return value;
        }
        let offset = start - base;
        if offset >= config_buffer_capacity() {
            return value;
        }
        let end = match offset.checked_add(bytes.len()) {
            Some(val) => val,
            None => return value,
        };
        if end > config_buffer_capacity() {
            return value;
        }
        let ptr = config_buffer_ptr().add(offset);
        for i in 0..bytes.len() {
            let b = ptr.add(i).read();
            ptr.add(i).write(b.to_ascii_lowercase());
        }
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr as *const u8, bytes.len()))
    }
}

fn service_label(service: &ServiceConfig) -> &str {
    if service.name.is_empty() {
        service.exec_start
    } else {
        service.name
    }
}

const FALLBACK_SERVICE: ServiceConfig = ServiceConfig {
    name: "fallback-shell",
    description: "Emergency fallback interactive shell",
    exec_start: "/bin/sh",
    exec_stop: EMPTY_STR,
    service_type: ServiceType::Simple,
    restart: RestartPolicy::Always,
    restart_settings: RestartSettings {
        burst: MAX_RESPAWN_COUNT,
        interval_sec: RESPAWN_WINDOW_SEC,
    },
    restart_delay_ms: RESTART_DELAY_MS,
    timeout_start_sec: 90,
    timeout_stop_sec: 90,
    after: EMPTY_STR,
    before: EMPTY_STR,
    wants: DEFAULT_TARGET_NAME,
    requires: EMPTY_STR,
    user: EMPTY_STR,
    group: EMPTY_STR,
    working_dir: EMPTY_STR,
    standard_output: "journal",
};

/// Service state tracking with lifecycle management
#[derive(Clone, Copy)]
struct UnitState {
    state: ServiceState,
    respawn_count: u32,
    window_start: Option<Instant>,
    total_starts: u64,
    pid: i64, // Current PID of running service (0 if not running)
    start_time: Option<Instant>,
}

impl UnitState {
    const fn new() -> Self {
        Self {
            state: ServiceState::Inactive,
            respawn_count: 0,
            window_start: None,
            total_starts: 0,
            pid: 0,
            start_time: None,
        }
    }

    fn transition_to(&mut self, new_state: ServiceState) {
        self.state = new_state;
    }

    fn is_running(&self) -> bool {
        self.state == ServiceState::Active && self.pid > 0
    }

    fn set_active(&mut self, pid: i64) {
        self.state = ServiceState::Active;
        self.pid = pid;
        self.start_time = Some(Instant::now());
    }

    fn set_inactive(&mut self) {
        self.state = ServiceState::Inactive;
        self.pid = 0;
        self.start_time = None;
    }

    fn set_failed(&mut self) {
        self.state = ServiceState::Failed;
        self.pid = 0;
        self.start_time = None;
    }

    fn uptime(&self) -> u64 {
        match self.start_time {
            Some(start) => start.elapsed().as_secs(),
            None => 0,
        }
    }

    fn allow_attempt(
        &mut self,
        current_time: Instant,
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
                    let interval = Duration::from_secs(settings.interval_sec);
                    match self.window_start {
                        Some(start) => {
                            let elapsed = current_time
                                .checked_duration_since(start)
                                .unwrap_or_default();
                            if elapsed >= interval {
                                self.window_start = Some(current_time);
                                self.respawn_count = 0;
                            }
                        }
                        None => {
                            self.window_start = Some(current_time);
                            self.respawn_count = 0;
                        }
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
    state: UnitState,
}

/// systemd-style logging with colors
fn log_info(msg: &str) {
    println!("\x1b[1;32m[  OK  ]\x1b[0m {}", msg); // Green
}

fn log_start(msg: &str) {
    println!("\x1b[1;36m[ .... ]\x1b[0m {}", msg); // Cyan
}

fn log_fail(msg: &str) {
    println!("\x1b[1;31m[FAILED]\x1b[0m {}", msg); // Red
}

fn log_warn(msg: &str) {
    println!("\x1b[1;33m[ WARN ]\x1b[0m {}", msg); // Yellow
}

fn log_detail(key: &str, value: &str) {
    println!("         {} = {}", key, value);
}

fn log_state_change(unit: &str, old_state: &str, new_state: &str) {
    println!(
        "\x1b[1;35m[STATE ]\x1b[0m {} state: {} -> {}",
        unit, old_state, new_state
    ); // Magenta
}

/// Simple timestamp (just a counter for now)
fn uptime_seconds() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs()
}

/// Delay function with std thread sleeping support
fn delay_ms(ms: u64) {
    // Try to use std::thread::sleep if available, fallback to spin loop
    #[cfg(target_os = "none")]
    {
        // No std::thread in bare metal, use spin loop
        for _ in 0..(ms * 1000) {
            unsafe { asm!("pause") }
        }
    }
    
    #[cfg(not(target_os = "none"))]
    {
        // In normal environments, use proper sleep
        std::thread::sleep(Duration::from_millis(ms));
    }
}
fn init_main() -> ! {
    let catalog = load_service_catalog();
    
    // Simple initialization to avoid complex const expressions
    let empty_service: Option<RunningService> = None;
    let mut running_services: [Option<RunningService>; MAX_SERVICES] = [
        empty_service, empty_service, empty_service, empty_service,
        empty_service, empty_service, empty_service, empty_service,
        empty_service, empty_service, empty_service, empty_service,
    ];
    
    let service_count = catalog.services.len();

    for i in 0..service_count {
        running_services[i] = Some(RunningService {
            config: &catalog.services[i],
            state: UnitState::new(),
        });
    }

    let mut buf = [0u8; 256];

    if service_count == 0 {
        log_warn("No units configured, starting fallback shell");
        
        // Use FALLBACK_SERVICE configuration
        running_services[0] = Some(RunningService {
            config: &FALLBACK_SERVICE,
            state: UnitState::new(),
        });
        
        parallel_service_supervisor(&mut running_services, 1, &mut buf);
    }

    parallel_service_supervisor(&mut running_services, service_count, &mut buf);
}

/// Parallel service supervisor - manages multiple services simultaneously
fn parallel_service_supervisor(
    running_services: &mut [Option<RunningService>; MAX_SERVICES],
    service_count: usize,
    buf: &mut [u8],
) -> ! {
    // Start all services initially
    println!("\x1b[1;34m[INIT]\x1b[0m Starting {} services", service_count);
    println!();
    
    for i in 0..service_count {
        if let Some(ref mut rs) = running_services[i] {
            let service = rs.config;
            let state = &mut rs.state;

            log_start(&format!("Starting unit: {}", service_label(service)));
            
            let pid = start_service(service, buf);
            
            if pid > 0 {
                state.set_active(pid);
                state.total_starts = state.total_starts.saturating_add(1);
                log_info(&format!("Unit started: {} (PID: {})", service_label(service), pid));
                
                // For oneshot services, don't wait for them
                if service.service_type == ServiceType::Oneshot {
                    log_detail("Type", "oneshot");
                }
                println!();
            } else {
                state.set_failed();
                log_fail(&format!("Failed to start unit: {}", service_label(service)));
                log_detail("ExecStart", service.exec_start);
                println!();
            }
        }
    }
    
    // Main supervision loop - wait for any child process to exit and restart if needed
    loop {
        let mut status: i32 = 0;
        let pid = wait4(-1, &mut status as *mut i32, 0); // Wait for any child

        if pid < 0 {
            log_warn("wait4 failed, retrying");
            delay_ms(100);
            continue;
        }

        let now_marker = Instant::now();
        let uptime = uptime_seconds();

        // Find which service exited
        for i in 0..service_count {
            if let Some(ref mut rs) = running_services[i] {
                if rs.state.pid == pid {
                    let service = rs.config;
                    let state = &mut rs.state;
                    let old_state = state.state.as_str();

                    log_warn(&format!("Unit terminated: {}", service_label(service)));
                    let pid_str = itoa(pid as u64, buf);
                    log_detail("PID", pid_str);
                    
                    // Decode POSIX wait status
                    if wifexited(status) {
                        let exit_code = wexitstatus(status);
                        let exit_str = itoa(exit_code as u64, buf);
                        log_detail("Exit code", exit_str);
                    } else if wifsignaled(status) {
                        let signal = wtermsig(status);
                        let sig_str = itoa(signal as u64, buf);
                        log_detail("Terminated by signal", sig_str);
                    }

                    state.set_inactive();
                    log_state_change(
                        service_label(service),
                        old_state,
                        state.state.as_str(),
                    );

                    // Check if we should restart
                    let exited_with_failure = if wifexited(status) {
                        wexitstatus(status) != 0
                    } else {
                        true // Terminated by signal counts as failure
                    };
                    
                    let should_restart = match service.restart {
                        RestartPolicy::No => false,
                        RestartPolicy::Always => true,
                        RestartPolicy::OnFailure => exited_with_failure,
                    };

                    if should_restart
                        && state.allow_attempt(now_marker, service.restart, service.restart_settings)
                    {
                        delay_ms(service.restart_delay_ms);

                        log_start(&format!("Restarting unit: {}", service_label(service)));

                        let new_pid = start_service(service, buf);
                        if new_pid > 0 {
                            let old_state_str = state.state.as_str();
                            state.set_active(new_pid);
                            log_info(&format!(
                                "Unit restarted: {} (PID: {})",
                                service_label(service),
                                new_pid
                            ));
                            log_state_change(
                                service_label(service),
                                old_state_str,
                                state.state.as_str(),
                            );
                            println!();
                        } else {
                            state.set_failed();
                            log_fail(&format!("Failed to restart unit: {}", service_label(service)));
                            println!();
                        }
                    } else if should_restart {
                        state.set_failed();
                        log_fail(&format!(
                            "Restart limit exceeded for unit: {}",
                            service_label(service)
                        ));
                        log_state_change(
                            service_label(service),
                            old_state,
                            state.state.as_str(),
                        );
                        println!();
                    } else {
                        log_info(&format!(
                            "Unit will not be restarted: {} (policy: no restart)",
                            service_label(service)
                        ));
                        println!();
                    }

                    let uptime_str = itoa(uptime, buf);
                    log_detail("System uptime", &format!("{}s", uptime_str));

                    break;
                }
            }
        }
    }
}

/// Start a single service (fork and exec)
fn start_service(service: &ServiceConfig, _buf: &mut [u8]) -> i64 {
    // 拷贝 service.exec_start 到一个本地 buffer，避免子进程访问父进程的只读数据
    let mut exec_path_buf = [0u8; MAX_FIELD_LEN];
    let exec_start_bytes = service.exec_start.as_bytes();
    let exec_path_len = core::cmp::min(exec_start_bytes.len(), exec_path_buf.len() - 1);
    exec_path_buf[..exec_path_len].copy_from_slice(&exec_start_bytes[..exec_path_len]);
    exec_path_buf[exec_path_len] = 0; // null-terminate

    let exec_start_str = std::str::from_utf8(&exec_path_buf[..exec_path_len]).unwrap_or("/bin/sh");
    eprintln!("[ni] start_service: service.exec_start='{}'", exec_start_str);
    
    // Parse exec_start into program path and arguments
    // Split by whitespace to get argv components
    let mut argv_ptrs: [*const u8; 16] = [core::ptr::null(); 16]; // Max 15 args + NULL
    let mut argv_bufs: [[u8; 128]; 16] = [[0u8; 128]; 16]; // Storage for each arg
    let mut argc = 0usize;
    
    // Split exec_start by spaces
    let mut start = 0;
    let bytes = exec_start_str.as_bytes();
    let mut in_arg = false;
    
    for i in 0..=bytes.len() {
        let is_space = i == bytes.len() || bytes[i] == b' ' || bytes[i] == b'\t';
        
        if is_space && in_arg {
            // End of argument
            let arg_len = i - start;
            if arg_len > 0 && argc < 15 {
                let copy_len = core::cmp::min(arg_len, 127);
                argv_bufs[argc][..copy_len].copy_from_slice(&bytes[start..start + copy_len]);
                argv_bufs[argc][copy_len] = 0; // null-terminate
                argv_ptrs[argc] = argv_bufs[argc].as_ptr();
                argc += 1;
            }
            in_arg = false;
        } else if !is_space && !in_arg {
            // Start of argument
            start = i;
            in_arg = true;
        }
    }
    
    // Ensure NULL terminator for argv
    argv_ptrs[argc] = core::ptr::null();
    
    if argc == 0 {
        eprintln!("[ni] start_service: empty exec_start");
        return -1;
    }
    
    // First argument is the program path
    let program_path = std::str::from_utf8(&argv_bufs[0][..]).unwrap_or("/bin/sh");
    let program_path = program_path.trim_end_matches('\0');
    eprintln!("[ni] start_service: program='{}', argc={}", program_path, argc);
    
    let pid = fork();

    if pid < 0 {
        // Fork failed
        eprintln!("[ni] start_service: fork failed");
        return -1;
    }

    if pid == 0 {
        // Child process - exec the service
        eprintln!("[ni] start_service: child process (PID 0 from fork), about to exec");
        
        let envp: [*const u8; 1] = [core::ptr::null()];
        
        let result = execve(
            program_path,
            &argv_ptrs,
            &envp,
        );
        
        // If execve returns, it failed
        eprintln!("[ni] start_service: execve failed with code {}", result);
        exit(127);  // Standard exit code for "command not found"
    }

    // Parent process - return child PID
    eprintln!("[ni] start_service: parent continuing, child PID={}", pid);
    pid
}

// Entry point for the init program
// Using standard main() to ensure std runtime initialization
// This allows std::io, TLS, and other std features to work correctly
// Using extern "C" to provide the C ABI main function directly
// argc/argv are ignored since we don't use command-line arguments
fn main() -> ! {
    install_minimal_panic_hook();
    announce_runtime_start();
    init_main()
}
