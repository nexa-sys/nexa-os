//! Enhanced stdio support for NexaOS userspace (no_std)
//!
//! This module provides a buffered libc-compatible stdio layer for NexaOS
//! userspace programs. The implementation keeps the following guarantees:
//! - Standard `FILE` abstraction with per-stream buffering, error and EOF state
//! - Configurable buffering modes (unbuffered, line buffered, fully buffered)
//! - Correct `fflush` semantics and line-buffer flushing on newline writes
//! - A printf-style formatter with width/precision/length modifiers, pointer
//!   formatting and basic floating-point output
//! - Minimal spinlock-based synchronisation for use in multi-threaded programs

use core::{
    arch::asm,
    cmp,
    ffi::{c_void, VaListImpl},
    fmt::{self, Write},
    hint::spin_loop,
    marker::PhantomData,
    ptr, slice,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{get_errno, set_errno, translate_ret_isize, EINVAL};

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;

const STDIN: i32 = 0;
const STDOUT: i32 = 1;
const STDERR: i32 = 2;

const BUFFER_CAPACITY: usize = 512;
const INT_BUFFER_SIZE: usize = 128;
const FLOAT_BUFFER_SIZE: usize = 128;
const DEFAULT_FLOAT_PRECISION: usize = 6;
const MAX_FLOAT_PRECISION: usize = 18;

const FLAG_LEFT: u8 = 0x01;
const FLAG_PLUS: u8 = 0x02;
const FLAG_SPACE: u8 = 0x04;
const FLAG_ALT: u8 = 0x08;
const FLAG_ZERO: u8 = 0x10;

fn pow10(n: usize) -> u128 {
    let mut result = 1u128;
    for _ in 0..n {
        result *= 10;
    }
    result
}

fn trunc_f64(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    if x >= 0.0 {
        (x as i64) as f64
    } else {
        -(((-x) as i64) as f64)
    }
}

fn round_f64(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    let truncated = trunc_f64(x);
    let frac = x - truncated;
    if frac.abs() >= 0.5 {
        if x >= 0.0 {
            truncated + 1.0
        } else {
            truncated - 1.0
        }
    } else {
        truncated
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum BufferMode {
    Unbuffered = 0,
    Line = 1,
    Full = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum LastOp {
    None = 0,
    Read = 1,
    Write = 2,
}

struct FileBuffer {
    data: [u8; BUFFER_CAPACITY],
    pos: usize,
    len: usize,
}

impl FileBuffer {
    const fn new() -> Self {
        Self {
            data: [0; BUFFER_CAPACITY],
            pos: 0,
            len: 0,
        }
    }

    fn clear(&mut self) {
        self.pos = 0;
        self.len = 0;
    }
}

#[repr(C)]
pub struct FILE {
    fd: i32,
    flags: i32,
    mode: BufferMode,
    buffer: FileBuffer,
    last_op: LastOp,
    error: bool,
    eof: bool,
    lock: AtomicBool,
}

impl FILE {
    const fn new(fd: i32, mode: BufferMode) -> Self {
        Self {
            fd,
            flags: 0,
            mode,
            buffer: FileBuffer::new(),
            last_op: LastOp::None,
            error: false,
            eof: false,
            lock: AtomicBool::new(false),
        }
    }
}

struct FileGuard<'a> {
    file: *mut FILE,
    _marker: PhantomData<&'a mut FILE>,
}

impl<'a> FileGuard<'a> {
    fn file_mut(&mut self) -> &mut FILE {
        unsafe { &mut *self.file }
    }
}

impl Drop for FileGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (*self.file).lock.store(false, Ordering::Release);
        }
    }
}

static mut STDIN_FILE: FILE = FILE::new(STDIN, BufferMode::Full);
static mut STDOUT_FILE: FILE = FILE::new(STDOUT, BufferMode::Line);
static mut STDERR_FILE: FILE = FILE::new(STDERR, BufferMode::Unbuffered);

#[no_mangle]
pub static mut stdin: *mut FILE = ptr::addr_of_mut!(STDIN_FILE);
#[no_mangle]
pub static mut stdout: *mut FILE = ptr::addr_of_mut!(STDOUT_FILE);
#[no_mangle]
pub static mut stderr: *mut FILE = ptr::addr_of_mut!(STDERR_FILE);

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
fn write_fd(fd: i32, buf: *const u8, len: usize) -> isize {
    translate_ret_isize(syscall3(SYS_WRITE, fd as u64, buf as u64, len as u64))
}

#[inline(always)]
fn read_fd(fd: i32, buf: *mut u8, len: usize) -> isize {
    translate_ret_isize(syscall3(SYS_READ, fd as u64, buf as u64, len as u64))
}

fn write_all_fd(fd: i32, mut buf: &[u8]) -> Result<(), i32> {
    while !buf.is_empty() {
        let written = write_fd(fd, buf.as_ptr(), buf.len());
        if written < 0 {
            return Err(get_errno());
        }
        if written == 0 {
            return Err(get_errno());
        }
        buf = &buf[written as usize..];
    }
    Ok(())
}

unsafe fn lock_stream<'a>(stream: *mut FILE) -> Result<FileGuard<'a>, ()> {
    if stream.is_null() {
        set_errno(EINVAL);
        return Err(());
    }
    let file = &*stream;
    while file
        .lock
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spin_loop();
    }
    Ok(FileGuard {
        file: stream,
        _marker: PhantomData,
    })
}

fn file_prepare_write(file: &mut FILE) -> Result<(), i32> {
    if matches!(file.last_op, LastOp::Read) {
        file.buffer.clear();
        file.last_op = LastOp::None;
    }
    Ok(())
}

fn file_prepare_read(file: &mut FILE) -> Result<(), i32> {
    if matches!(file.last_op, LastOp::Write) {
        file_flush(file)?;
        file.buffer.clear();
        file.last_op = LastOp::None;
    }
    Ok(())
}

fn file_flush(file: &mut FILE) -> Result<(), i32> {
    if matches!(file.last_op, LastOp::Write) && file.buffer.len > 0 {
        let data = &file.buffer.data[..file.buffer.len];
        if let Err(err) = write_all_fd(file.fd, data) {
            file.error = true;
            set_errno(err);
            return Err(err);
        }
        file.buffer.clear();
    }
    Ok(())
}

fn file_write_bytes(file: &mut FILE, bytes: &[u8]) -> Result<(), i32> {
    if bytes.is_empty() {
        return Ok(());
    }

    file_prepare_write(file)?;

    if matches!(file.mode, BufferMode::Unbuffered) {
        if let Err(err) = write_all_fd(file.fd, bytes) {
            file.error = true;
            set_errno(err);
            return Err(err);
        }
        file.last_op = LastOp::Write;
        return Ok(());
    }

    let mut remaining = bytes;
    while !remaining.is_empty() {
        let available = BUFFER_CAPACITY.saturating_sub(file.buffer.len);
        if available == 0 {
            file_flush(file)?;
            continue;
        }
        let chunk = cmp::min(available, remaining.len());
        let end = file.buffer.len + chunk;
        file.buffer.data[file.buffer.len..end].copy_from_slice(&remaining[..chunk]);
        file.buffer.len = end;
        let newline_written = if matches!(file.mode, BufferMode::Line) {
            remaining[..chunk].iter().any(|b| *b == b'\n')
        } else {
            false
        };
        if newline_written || file.buffer.len == BUFFER_CAPACITY {
            file_flush(file)?;
        }
        remaining = &remaining[chunk..];
    }

    file.last_op = LastOp::Write;
    Ok(())
}

fn file_write_byte(file: &mut FILE, byte: u8) -> Result<(), i32> {
    let buf = [byte];
    file_write_bytes(file, &buf)
}

fn write_repeat(file: &mut FILE, byte: u8, mut count: usize) -> Result<(), i32> {
    const TEMP_BUF_SIZE: usize = 32;
    let temp = [byte; TEMP_BUF_SIZE];
    while count > 0 {
        let take = cmp::min(count, TEMP_BUF_SIZE);
        file_write_bytes(file, &temp[..take])?;
        count -= take;
    }
    Ok(())
}

fn file_read_bytes(file: &mut FILE, out: &mut [u8]) -> Result<usize, i32> {
    if out.is_empty() {
        return Ok(0);
    }

    file_prepare_read(file)?;

    if matches!(file.mode, BufferMode::Unbuffered) {
        let read = read_fd(file.fd, out.as_mut_ptr(), out.len());
        if read < 0 {
            file.error = true;
            return Err(get_errno());
        }
        if read == 0 {
            file.eof = true;
            return Ok(0);
        }
        file.eof = false;
        file.last_op = LastOp::Read;
        return Ok(read as usize);
    }

    let mut copied = 0usize;
    while copied < out.len() {
        if file.buffer.pos == file.buffer.len {
            let read = read_fd(file.fd, file.buffer.data.as_mut_ptr(), BUFFER_CAPACITY);
            if read < 0 {
                file.error = true;
                return Err(get_errno());
            }
            if read == 0 {
                file.eof = true;
                break;
            }
            file.buffer.pos = 0;
            file.buffer.len = read as usize;
            file.eof = false;
        }

        let available = file.buffer.len - file.buffer.pos;
        let take = cmp::min(available, out.len() - copied);
        out[copied..copied + take]
            .copy_from_slice(&file.buffer.data[file.buffer.pos..file.buffer.pos + take]);
        copied += take;
        file.buffer.pos += take;
        file.last_op = LastOp::Read;

        if take == 0 {
            break;
        }
    }

    Ok(copied)
}

fn file_read_byte(file: &mut FILE) -> Result<Option<u8>, i32> {
    let mut buf = [0u8; 1];
    match file_read_bytes(file, &mut buf) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(buf[0])),
        Err(err) => Err(err),
    }
}

fn write_stream_bytes(stream: *mut FILE, bytes: &[u8]) -> Result<(), i32> {
    unsafe {
        let mut guard = lock_stream(stream).map_err(|_| get_errno())?;
        let file = guard.file_mut();
        if file.error {
            set_errno(EINVAL);
            return Err(EINVAL);
        }
        file_write_bytes(file, bytes)
    }
}

fn flush_stream(stream: *mut FILE) -> Result<(), i32> {
    unsafe {
        let mut guard = lock_stream(stream).map_err(|_| get_errno())?;
        let file = guard.file_mut();
        file_flush(file)
    }
}

fn read_stream_byte(stream: *mut FILE) -> Result<Option<u8>, i32> {
    unsafe {
        let mut guard = lock_stream(stream).map_err(|_| get_errno())?;
        let file = guard.file_mut();
        file_read_byte(file)
    }
}

fn echo_bytes(bytes: &[u8]) {
    let _ = write_stream_bytes(unsafe { stdout }, bytes);
}

fn read_blocking_byte() -> Result<u8, i32> {
    loop {
        match read_stream_byte(unsafe { stdin }) {
            Ok(Some(b)) => return Ok(b),
            Ok(None) => return Err(0),
            Err(err) => {
                if err == 0 || err == 4 || err == 11 {
                    continue;
                }
                return Err(err);
            }
        }
    }
}

enum EchoMode {
    None,
    Plain,
    Mask(u8),
}

fn read_line_internal(buf: &mut [u8], mode: EchoMode, skip_empty: bool) -> Result<usize, i32> {
    if buf.is_empty() {
        return Ok(0);
    }

    let max = buf.len().saturating_sub(1);
    let mut len = 0usize;

    loop {
        let byte = match read_blocking_byte() {
            Ok(b) => b,
            Err(err) => return Err(err),
        };

        match byte {
            b'\r' | b'\n' => {
                if len == 0 && skip_empty {
                    continue;
                }
                echo_bytes(b"\n");
                break;
            }
            8 | 127 => {
                if len > 0 {
                    len -= 1;
                    buf[len] = 0;
                    if !matches!(mode, EchoMode::None) {
                        echo_bytes(b"\x08 \x08");
                    }
                }
            }
            b if (0x20..=0x7e).contains(&b) => {
                if len < max {
                    buf[len] = b;
                    len += 1;
                    match mode {
                        EchoMode::Plain => echo_bytes(&[b]),
                        EchoMode::Mask(mask) => echo_bytes(&[mask]),
                        EchoMode::None => {}
                    }
                }
            }
            _ => {}
        }
    }

    buf[len] = 0;
    Ok(len)
}

#[no_mangle]
pub extern "C" fn putchar(c: i32) -> i32 {
    let byte = c as u8;
    match write_stream_bytes(unsafe { stdout }, &[byte]) {
        Ok(()) => c,
        Err(err) => {
            set_errno(err);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn fputc(c: i32, stream: *mut FILE) -> i32 {
    match write_stream_bytes(stream, &[c as u8]) {
        Ok(()) => c,
        Err(err) => {
            set_errno(err);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn puts(s: *const u8) -> i32 {
    if s.is_null() {
        set_errno(EINVAL);
        return -1;
    }
    unsafe {
        let mut len = 0usize;
        while ptr::read(s.add(len)) != 0 {
            len += 1;
        }
        let slice = slice::from_raw_parts(s, len);
        if write_stream_bytes(stdout, slice).is_err() || write_stream_bytes(stdout, b"\n").is_err() {
            return -1;
        }
    }
    1
}

#[no_mangle]
pub extern "C" fn fputs(s: *const u8, stream: *mut FILE) -> i32 {
    if s.is_null() {
        set_errno(EINVAL);
        return -1;
    }
    unsafe {
        let mut len = 0usize;
        while ptr::read(s.add(len)) != 0 {
            len += 1;
        }
        let slice = slice::from_raw_parts(s, len);
        if write_stream_bytes(stream, slice).is_err() {
            return -1;
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn getchar() -> i32 {
    match read_stream_byte(unsafe { stdin }) {
        Ok(Some(byte)) => byte as i32,
        Ok(None) => {
            set_errno(0);
            -1
        }
        Err(err) => {
            set_errno(err);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn fgets(buf: *mut u8, size: i32, stream: *mut FILE) -> *mut u8 {
    if buf.is_null() || size <= 1 {
        return ptr::null_mut();
    }

    unsafe {
        let mut guard = match lock_stream(stream) {
            Ok(g) => g,
            Err(()) => return ptr::null_mut(),
        };
        let file = guard.file_mut();
        let mut i = 0usize;
        while i < (size - 1) as usize {
            match file_read_byte(file) {
                Ok(Some(b)) => {
                    ptr::write(buf.add(i), b);
                    i += 1;
                    if b == b'\n' {
                        break;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    set_errno(err);
                    return ptr::null_mut();
                }
            }
        }
        ptr::write(buf.add(i), 0);
        if i == 0 {
            ptr::null_mut()
        } else {
            buf
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn gets(buf: *mut u8) -> *mut u8 {
    if buf.is_null() {
        return ptr::null_mut();
    }
    let mut i = 0usize;
    loop {
        let c = getchar();
        if c <= 0 {
            break;
        }
        if c as u8 == b'\n' {
            break;
        }
        ptr::write(buf.add(i), c as u8);
        i += 1;
    }
    ptr::write(buf.add(i), 0);
    if i == 0 {
        ptr::null_mut()
    } else {
        buf
    }
}

pub fn stdout_write_all(buf: &[u8]) -> Result<(), i32> {
    write_stream_bytes(unsafe { stdout }, buf)
}

pub fn stdout_write_str(s: &str) -> Result<(), i32> {
    stdout_write_all(s.as_bytes())
}

pub fn stdout_write_fmt(args: fmt::Arguments<'_>) -> Result<(), i32> {
    struct StdoutWriter {
        error: Option<i32>,
    }

    impl Write for StdoutWriter {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            match stdout_write_str(s) {
                Ok(()) => Ok(()),
                Err(err) => {
                    self.error = Some(err);
                    Err(fmt::Error)
                }
            }
        }
    }

    let mut writer = StdoutWriter { error: None };
    match writer.write_fmt(args) {
        Ok(()) => Ok(()),
        Err(_) => Err(writer.error.unwrap_or_else(get_errno)),
    }
}

pub fn stdout_flush() -> Result<(), i32> {
    flush_stream(unsafe { stdout })
}

pub fn stderr_write_all(buf: &[u8]) -> Result<(), i32> {
    write_stream_bytes(unsafe { stderr }, buf)
}

pub fn stderr_write_str(s: &str) -> Result<(), i32> {
    stderr_write_all(s.as_bytes())
}

pub fn stdin_read_line(buf: &mut [u8], skip_empty: bool) -> Result<usize, i32> {
    read_line_internal(buf, EchoMode::Plain, skip_empty)
}

pub fn stdin_read_line_masked(buf: &mut [u8], mask: u8, skip_empty: bool) -> Result<usize, i32> {
    read_line_internal(buf, EchoMode::Mask(mask), skip_empty)
}

pub fn stdin_read_line_noecho(buf: &mut [u8], skip_empty: bool) -> Result<usize, i32> {
    read_line_internal(buf, EchoMode::None, skip_empty)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LengthModifier {
    None,
    HH,
    H,
    L,
    LL,
    Z,
    T,
    J,
}

impl Default for LengthModifier {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Default)]
struct FormatSpec {
    flags: u8,
    width: Option<usize>,
    precision: Option<usize>,
    length: LengthModifier,
    specifier: u8,
}

fn parse_length(fmt: &[u8], idx: &mut usize) -> LengthModifier {
    if *idx >= fmt.len() {
        return LengthModifier::None;
    }
    match fmt[*idx] {
        b'h' => {
            if *idx + 1 < fmt.len() && fmt[*idx + 1] == b'h' {
                *idx += 2;
                LengthModifier::HH
            } else {
                *idx += 1;
                LengthModifier::H
            }
        }
        b'l' => {
            if *idx + 1 < fmt.len() && fmt[*idx + 1] == b'l' {
                *idx += 2;
                LengthModifier::LL
            } else {
                *idx += 1;
                LengthModifier::L
            }
        }
        b'z' => {
            *idx += 1;
            LengthModifier::Z
        }
        b't' => {
            *idx += 1;
            LengthModifier::T
        }
        b'j' => {
            *idx += 1;
            LengthModifier::J
        }
        _ => LengthModifier::None,
    }
}

fn format_unsigned(mut value: u128, base: u32, min_digits: usize) -> ([u8; INT_BUFFER_SIZE], usize) {
    let mut buf = [0u8; INT_BUFFER_SIZE];
    let mut index = INT_BUFFER_SIZE;
    if value == 0 {
        index -= 1;
        buf[index] = b'0';
    } else {
        while value > 0 {
            let digit = (value % base as u128) as u8;
            let ch = if digit < 10 {
                b'0' + digit
            } else {
                b'a' + (digit - 10)
            };
            index -= 1;
            buf[index] = ch;
            value /= base as u128;
        }
    }
    let digits = INT_BUFFER_SIZE - index;
    if min_digits > digits {
        let zeros = min_digits - digits;
        for _ in 0..zeros {
            index -= 1;
            buf[index] = b'0';
        }
    }
    (buf, index)
}

fn format_signed(value: i128, base: u32, min_digits: usize) -> (bool, [u8; INT_BUFFER_SIZE], usize) {
    if value < 0 {
        let unsigned = value.wrapping_neg() as u128;
        let (buf, index) = format_unsigned(unsigned, base, min_digits);
        (true, buf, index)
    } else {
        let (buf, index) = format_unsigned(value as u128, base, min_digits);
        (false, buf, index)
    }
}

fn emit_formatted_integer(
    file: &mut FILE,
    spec: &FormatSpec,
    negative: bool,
    digits: &[u8],
    uppercase: bool,
    value_is_zero: bool,
) -> Result<usize, i32> {
    let sign_char = if negative {
        Some(b'-')
    } else if spec.flags & FLAG_PLUS != 0 {
        Some(b'+')
    } else if spec.flags & FLAG_SPACE != 0 {
        Some(b' ')
    } else {
        None
    };

    let mut precision_buf = [0u8; INT_BUFFER_SIZE];
    let mut digits_prec_slice = digits;
    if let Some(mut precision) = spec.precision {
        if precision > INT_BUFFER_SIZE {
            precision = INT_BUFFER_SIZE;
        }
        if precision == 0 && digits.len() == 1 && digits[0] == b'0' && spec.specifier != b'p' {
            digits_prec_slice = &[];
        } else if precision > digits.len() {
            let start = INT_BUFFER_SIZE - precision;
            let zero_count = precision - digits.len();
            for idx in 0..zero_count {
                precision_buf[start + idx] = b'0';
            }
            precision_buf[start + zero_count..start + precision].copy_from_slice(digits);
            digits_prec_slice = &precision_buf[start..start + precision];
        }
    }

    let mut uppercase_buf = [0u8; INT_BUFFER_SIZE];
    let digits_case = if uppercase {
        let start = INT_BUFFER_SIZE - digits_prec_slice.len();
        uppercase_buf[start..start + digits_prec_slice.len()].copy_from_slice(digits_prec_slice);
        for byte in &mut uppercase_buf[start..start + digits_prec_slice.len()] {
            byte.make_ascii_uppercase();
        }
        &uppercase_buf[start..start + digits_prec_slice.len()]
    } else {
        digits_prec_slice
    };

    let mut prefix: &[u8] = b"";
    if spec.flags & FLAG_ALT != 0 {
        match spec.specifier {
            b'x' if !value_is_zero && !digits_case.is_empty() => prefix = b"0x",
            b'X' if !value_is_zero && !digits_case.is_empty() => prefix = b"0X",
            b'o' => {
                if digits_case.is_empty() || digits_case[0] != b'0' {
                    prefix = b"0";
                }
            }
            _ => {}
        }
    } else if spec.specifier == b'p' {
        prefix = b"0x";
    }

    write_formatted_block(file, spec, sign_char, prefix, digits_case)
}

fn write_formatted_block(
    file: &mut FILE,
    spec: &FormatSpec,
    sign: Option<u8>,
    prefix: &[u8],
    body: &[u8],
) -> Result<usize, i32> {
    let sign_len = sign.map(|_| 1).unwrap_or(0);
    let total_len = sign_len + prefix.len() + body.len();
    let width = spec.width.unwrap_or(0);
    let left = spec.flags & FLAG_LEFT != 0;
    let zero_allowed = spec.flags & FLAG_ZERO != 0 && !left && spec.precision.is_none();
    let pad_char = if zero_allowed && matches!(spec.specifier, b'd' | b'i' | b'u' | b'x' | b'X' | b'o' | b'p') {
        b'0'
    } else {
        b' '
    };
    let padding = width.saturating_sub(total_len);

    if !left {
        if pad_char == b' ' {
            write_repeat(file, b' ', padding)?;
        }
        if let Some(ch) = sign {
            file_write_byte(file, ch)?;
        }
        if !prefix.is_empty() {
            file_write_bytes(file, prefix)?;
        }
        if pad_char == b'0' {
            write_repeat(file, b'0', padding)?;
        }
        if !body.is_empty() {
            file_write_bytes(file, body)?;
        }
    } else {
        if let Some(ch) = sign {
            file_write_byte(file, ch)?;
        }
        if !prefix.is_empty() {
            file_write_bytes(file, prefix)?;
        }
        if !body.is_empty() {
            file_write_bytes(file, body)?;
        }
        if padding > 0 {
            write_repeat(file, b' ', padding)?;
        }
    }

    Ok(total_len + padding)
}

fn read_signed_arg(args: &mut VaListImpl<'_>, length: LengthModifier) -> i128 {
    unsafe {
        match length {
            LengthModifier::HH => args.arg::<i32>() as i8 as i128,
            LengthModifier::H => args.arg::<i32>() as i16 as i128,
            LengthModifier::L | LengthModifier::LL => args.arg::<i64>() as i128,
            LengthModifier::Z | LengthModifier::T => args.arg::<isize>() as i128,
            LengthModifier::J => args.arg::<i64>() as i128,
            LengthModifier::None => args.arg::<i32>() as i128,
        }
    }
}

fn read_unsigned_arg(args: &mut VaListImpl<'_>, length: LengthModifier) -> u128 {
    unsafe {
        match length {
            LengthModifier::HH => args.arg::<u32>() as u8 as u128,
            LengthModifier::H => args.arg::<u32>() as u16 as u128,
            LengthModifier::L | LengthModifier::LL => args.arg::<u64>() as u128,
            LengthModifier::Z | LengthModifier::T => args.arg::<usize>() as u128,
            LengthModifier::J => args.arg::<u64>() as u128,
            LengthModifier::None => args.arg::<u32>() as u128,
        }
    }
}

fn handle_float(spec: &FormatSpec, file: &mut FILE, value: f64, uppercase: bool) -> Result<usize, i32> {
    if value.is_nan() {
        let txt = if uppercase { b"NAN" } else { b"nan" };
        return write_formatted_block(file, spec, None, b"", txt);
    }
    if value.is_infinite() {
        let sign = if value.is_sign_negative() {
            Some(b'-')
        } else if spec.flags & FLAG_PLUS != 0 {
            Some(b'+')
        } else if spec.flags & FLAG_SPACE != 0 {
            Some(b' ')
        } else {
            None
        };
        let txt = if uppercase { b"INF" } else { b"inf" };
        return write_formatted_block(file, spec, sign, b"", txt);
    }

    let precision = spec
        .precision
        .unwrap_or(DEFAULT_FLOAT_PRECISION)
        .min(MAX_FLOAT_PRECISION);
    let decimal_point = precision > 0 || (spec.flags & FLAG_ALT != 0);

    let negative = value.is_sign_negative();
    let abs_value = if negative { -value } else { value };

    let scale = pow10(precision);
    let scaled = round_f64(abs_value * scale as f64)
        .min(u128::MAX as f64);
    let scaled_u128 = scaled as u128;

    let int_part = scaled_u128 / scale;
    let frac_part = scaled_u128 % scale;

    let (int_buf, int_idx) = format_unsigned(int_part, 10, 1);
    let integer_digits = &int_buf[int_idx..];

    let mut float_buf = [0u8; FLOAT_BUFFER_SIZE];
    let mut cursor = 0usize;
    float_buf[cursor..cursor + integer_digits.len()].copy_from_slice(integer_digits);
    cursor += integer_digits.len();

    if decimal_point {
        float_buf[cursor] = b'.';
        cursor += 1;
        if precision > 0 {
            let (frac_buf, _) = format_unsigned(frac_part, 10, precision.max(1));
            let start = INT_BUFFER_SIZE - precision;
            float_buf[cursor..cursor + precision]
                .copy_from_slice(&frac_buf[start..start + precision]);
            cursor += precision;
        }
    }

    let mut body_slice = &float_buf[..cursor];
    let mut upper_buf = [0u8; FLOAT_BUFFER_SIZE];
    if uppercase {
        upper_buf[..body_slice.len()].copy_from_slice(body_slice);
        for byte in &mut upper_buf[..body_slice.len()] {
            byte.make_ascii_uppercase();
        }
        body_slice = &upper_buf[..body_slice.len()];
    }

    let sign = if negative {
        Some(b'-')
    } else if spec.flags & FLAG_PLUS != 0 {
        Some(b'+')
    } else if spec.flags & FLAG_SPACE != 0 {
        Some(b' ')
    } else {
        None
    };

    write_formatted_block(file, spec, sign, b"", body_slice)
}

unsafe fn write_formatted(stream: *mut FILE, fmt_ptr: *const u8, args: &mut VaListImpl<'_>) -> Result<i32, i32> {
    if fmt_ptr.is_null() {
        set_errno(EINVAL);
        return Err(EINVAL);
    }

    let len = {
        let mut idx = 0usize;
        while ptr::read(fmt_ptr.add(idx)) != 0 {
            idx += 1;
        }
        idx
    };

    let fmt = slice::from_raw_parts(fmt_ptr, len);
    let mut total_written = 0i32;

    let mut guard = lock_stream(stream).map_err(|_| get_errno())?;
    let file = guard.file_mut();

    let mut i = 0usize;
    while i < fmt.len() {
        let ch = fmt[i];
        if ch != b'%' {
            file_write_byte(file, ch)?;
            total_written += 1;
            i += 1;
            continue;
        }

        i += 1;
        if i >= fmt.len() {
            break;
        }

        let mut spec = FormatSpec::default();

        while i < fmt.len() {
            let flag = fmt[i];
            let recognized = match flag {
                b'-' => {
                    spec.flags |= FLAG_LEFT;
                    true
                }
                b'+' => {
                    spec.flags |= FLAG_PLUS;
                    true
                }
                b' ' => {
                    spec.flags |= FLAG_SPACE;
                    true
                }
                b'#' => {
                    spec.flags |= FLAG_ALT;
                    true
                }
                b'0' => {
                    spec.flags |= FLAG_ZERO;
                    true
                }
                _ => false,
            };
            if recognized {
                i += 1;
            } else {
                break;
            }
        }

        if i < fmt.len() && fmt[i] == b'*' {
            i += 1;
            let w: i32 = args.arg();
            if w < 0 {
                spec.flags |= FLAG_LEFT;
                spec.width = Some((-w) as usize);
            } else {
                spec.width = Some(w as usize);
            }
        } else {
            let mut width: usize = 0;
            let mut has_digit = false;
            while i < fmt.len() {
                let ch = fmt[i];
                if !(b'0'..=b'9').contains(&ch) {
                    break;
                }
                has_digit = true;
                width = width.saturating_mul(10).saturating_add((ch - b'0') as usize);
                i += 1;
            }
            if has_digit {
                spec.width = Some(width);
            }
        }

        if i < fmt.len() && fmt[i] == b'.' {
            i += 1;
            if i < fmt.len() && fmt[i] == b'*' {
                i += 1;
                let p: i32 = args.arg();
                if p >= 0 {
                    spec.precision = Some(p as usize);
                } else {
                    spec.precision = None;
                }
            } else {
                let mut precision: usize = 0;
                let mut has_digit = false;
                while i < fmt.len() {
                    let ch = fmt[i];
                    if !(b'0'..=b'9').contains(&ch) {
                        break;
                    }
                    has_digit = true;
                    precision = precision.saturating_mul(10).saturating_add((ch - b'0') as usize);
                    i += 1;
                }
                spec.precision = if has_digit { Some(precision) } else { Some(0) };
            }
        }

        spec.length = parse_length(fmt, &mut i);

        if i >= fmt.len() {
            break;
        }

        spec.specifier = fmt[i];
        i += 1;

        match spec.specifier {
            b'%' => {
                let percent = [b'%'];
                let written = write_formatted_block(file, &spec, None, b"", &percent)?;
                total_written += written as i32;
            }
            b'c' => {
                let v: i32 = args.arg();
                let byte = (v & 0xFF) as u8;
                let written = write_formatted_block(file, &spec, None, b"", &[byte])?;
                total_written += written as i32;
            }
            b's' => {
                let ptr: *const u8 = args.arg();
                let slice = if ptr.is_null() {
                    b"(null)"
                } else {
                    let mut len = 0usize;
                    while ptr::read(ptr.add(len)) != 0 {
                        len += 1;
                    }
                    slice::from_raw_parts(ptr, len)
                };
                let truncated = if let Some(precision) = spec.precision {
                    &slice[..cmp::min(precision, slice.len())]
                } else {
                    slice
                };
                let written = write_formatted_block(file, &spec, None, b"", truncated)?;
                total_written += written as i32;
            }
            b'd' | b'i' => {
                let value = read_signed_arg(args, spec.length);
                let (negative, buf, idx) = format_signed(value, 10, 1);
                let digits = &buf[idx..];
                let written = emit_formatted_integer(file, &spec, negative, digits, false, value == 0)?;
                total_written += written as i32;
            }
            b'u' => {
                let value = read_unsigned_arg(args, spec.length);
                let (buf, idx) = format_unsigned(value, 10, 1);
                let digits = &buf[idx..];
                let written = emit_formatted_integer(file, &spec, false, digits, false, value == 0)?;
                total_written += written as i32;
            }
            b'x' | b'X' => {
                let value = read_unsigned_arg(args, spec.length);
                let uppercase = spec.specifier == b'X';
                let (buf, idx) = format_unsigned(value, 16, 1);
                let digits = &buf[idx..];
                let written = emit_formatted_integer(file, &spec, false, digits, uppercase, value == 0)?;
                total_written += written as i32;
            }
            b'o' => {
                let value = read_unsigned_arg(args, spec.length);
                let (buf, idx) = format_unsigned(value, 8, 1);
                let digits = &buf[idx..];
                let written = emit_formatted_integer(file, &spec, false, digits, false, value == 0)?;
                total_written += written as i32;
            }
            b'p' => {
                let value: *const c_void = args.arg();
                let addr = value as usize as u128;
                let (buf, idx) = format_unsigned(addr, 16, 1);
                let digits = &buf[idx..];
                let mut pointer_spec = spec;
                pointer_spec.specifier = b'p';
                pointer_spec.flags &= !(FLAG_ALT | FLAG_ZERO);
                let written = emit_formatted_integer(file, &pointer_spec, false, digits, false, addr == 0)?;
                total_written += written as i32;
            }
            b'f' | b'F' => {
                let value = args.arg::<f64>();
                let uppercase = spec.specifier == b'F';
                let written = handle_float(&spec, file, value, uppercase)?;
                total_written += written as i32;
            }
            _ => {
                file_write_byte(file, spec.specifier)?;
                total_written += 1;
            }
        }
    }

    Ok(total_written)
}

#[no_mangle]
pub unsafe extern "C" fn printf(fmt: *const u8, mut args: ...) -> i32 {
    match write_formatted(stdout, fmt, &mut args) {
        Ok(count) => count,
        Err(_) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fprintf(stream: *mut FILE, fmt: *const u8, mut args: ...) -> i32 {
    match write_formatted(stream, fmt, &mut args) {
        Ok(count) => count,
        Err(_) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fflush(stream: *mut FILE) -> i32 {
    if stream.is_null() {
        let mut result = 0;
        let mut error_code = 0;
        if let Err(err) = flush_stream(stdout) {
            result = -1;
            error_code = err;
        }
        if let Err(err) = flush_stream(stderr) {
            result = -1;
            if error_code == 0 {
                error_code = err;
            }
        }
        if result == 0 {
            set_errno(0);
        } else {
            set_errno(error_code);
        }
        return result;
    }

    match flush_stream(stream) {
        Ok(()) => {
            set_errno(0);
            0
        }
        Err(err) => {
            set_errno(err);
            -1
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn fwrite(
    ptr: *const c_void,
    size: usize,
    nmemb: usize,
    stream: *mut FILE,
) -> usize {
    if size == 0 || nmemb == 0 {
        return 0;
    }
    if ptr.is_null() {
        set_errno(EINVAL);
        return 0;
    }

    let total = match size.checked_mul(nmemb) {
        Some(v) => v,
        None => {
            set_errno(EINVAL);
            return 0;
        }
    };

    set_errno(0);
    let mut guard = match lock_stream(stream) {
        Ok(g) => g,
        Err(()) => return 0,
    };
    let file = guard.file_mut();

    let data = slice::from_raw_parts(ptr as *const u8, total);
    let mut written = 0usize;

    while written < total {
        match file_write_bytes(file, &data[written..]) {
            Ok(()) => {
                written = total;
            }
            Err(err) => {
                set_errno(err);
                break;
            }
        }
    }

    written / size
}

#[no_mangle]
pub unsafe extern "C" fn fread(
    ptr: *mut c_void,
    size: usize,
    nmemb: usize,
    stream: *mut FILE,
) -> usize {
    if size == 0 || nmemb == 0 {
        return 0;
    }
    if ptr.is_null() {
        set_errno(EINVAL);
        return 0;
    }

    let total = match size.checked_mul(nmemb) {
        Some(v) => v,
        None => {
            set_errno(EINVAL);
            return 0;
        }
    };

    set_errno(0);
    let mut guard = match lock_stream(stream) {
        Ok(g) => g,
        Err(()) => return 0,
    };
    let file = guard.file_mut();

    let mut read_total = 0usize;
    while read_total < total {
        let buffer = slice::from_raw_parts_mut((ptr as *mut u8).add(read_total), total - read_total);
        match file_read_bytes(file, buffer) {
            Ok(0) => break,
            Ok(n) => read_total += n,
            Err(err) => {
                set_errno(err);
                break;
            }
        }
    }

    read_total / size
}
