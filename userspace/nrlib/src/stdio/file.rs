//! FILE structure and low-level file operations
//!
//! This module provides the core FILE type and internal file manipulation functions.

use core::{
    cmp,
    hint::spin_loop,
    marker::PhantomData,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{get_errno, set_errno, EINVAL};

use super::buffer::{BufferMode, FileBuffer, LastOp};
use super::constants::{BUFFER_CAPACITY, EAGAIN, STDERR, STDIN, STDOUT, SYS_WRITE};
use super::helpers::{read_fd, syscall3, write_all_fd};

/// C-compatible FILE structure for stdio streams
#[repr(C)]
pub struct FILE {
    pub(crate) fd: i32,
    pub(crate) flags: i32,
    pub(crate) mode: BufferMode,
    pub(crate) buffer: FileBuffer,
    pub(crate) last_op: LastOp,
    pub(crate) error: bool,
    pub(crate) eof: bool,
    pub(crate) lock: AtomicBool,
}

impl FILE {
    /// Create a new FILE with the specified fd and buffering mode
    pub(crate) const fn new(fd: i32, mode: BufferMode) -> Self {
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

/// RAII guard for FILE lock
pub(crate) struct FileGuard<'a> {
    file: *mut FILE,
    _marker: PhantomData<&'a mut FILE>,
}

impl<'a> FileGuard<'a> {
    pub(crate) fn file_mut(&mut self) -> &mut FILE {
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

// ============================================================================
// Global stdio streams
// ============================================================================

pub(crate) static mut STDIN_FILE: FILE = FILE::new(STDIN, BufferMode::Full);
pub(crate) static mut STDOUT_FILE: FILE = FILE::new(STDOUT, BufferMode::Line);
pub(crate) static mut STDERR_FILE: FILE = FILE::new(STDERR, BufferMode::Unbuffered);

#[no_mangle]
pub static mut stdin: *mut FILE = ptr::addr_of_mut!(STDIN_FILE);
#[no_mangle]
pub static mut stdout: *mut FILE = ptr::addr_of_mut!(STDOUT_FILE);
#[no_mangle]
pub static mut stderr: *mut FILE = ptr::addr_of_mut!(STDERR_FILE);

// ============================================================================
// Lock acquisition
// ============================================================================

/// Acquire lock on a FILE stream with timeout and backoff
pub(crate) unsafe fn lock_stream<'a>(stream: *mut FILE) -> Result<FileGuard<'a>, ()> {
    if stream.is_null() {
        set_errno(EINVAL);
        return Err(());
    }

    let file = &*stream;

    // Production-grade lock acquisition with:
    // 1. Bounded wait time to detect deadlocks
    // 2. Exponential backoff to reduce CPU contention
    // 3. Diagnostic capability for debugging lock issues
    //
    // In practice, in single-threaded mode (__libc_single_threaded=1),
    // the lock should almost never be contested. If it is, it indicates
    // reentrancy or a serious logic error that must be diagnosed.

    const MAX_SPIN_COUNT: u32 = 10000; // ~10ms on modern CPUs
    const BACKOFF_CAP: u32 = 256; // Cap exponential backoff at 256 iterations

    let mut spin_count = 0u32;
    let mut backoff = 1u32;

    loop {
        match file
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        {
            Ok(_) => {
                // Successfully acquired lock
                return Ok(FileGuard {
                    file: stream,
                    _marker: PhantomData,
                });
            }
            Err(_) => {
                // Lock is already held, apply exponential backoff
                spin_count += 1;

                // Exponential backoff: gradually increase spin count to reduce CPU usage
                // and improve fairness if multiple threads were competing
                for _ in 0..backoff {
                    spin_loop();
                }

                // Increase backoff for next iteration (with cap to prevent overflow)
                backoff = (backoff * 2).min(BACKOFF_CAP);

                // DIAGNOSTIC: Use direct syscall to report lock contention
                // This helps debug the actual lock issue without causing more contention
                if spin_count == 100 {
                    let diag_msg = b"[nrlib] WARNING: lock contention detected on stdio\n";
                    let _ = syscall3(
                        SYS_WRITE,
                        2,
                        diag_msg.as_ptr() as u64,
                        diag_msg.len() as u64,
                    );
                }

                // Safety check: if we've been spinning too long, something is definitely wrong
                // This protects against deadlock scenarios (which shouldn't happen in single-threaded
                // mode but could indicate a serious bug like reentrancy or corruption)
                if spin_count >= MAX_SPIN_COUNT {
                    // CRITICAL: Failed to acquire lock after excessive spinning
                    let err_msg =
                        b"[nrlib] CRITICAL: Lock acquisition timeout - possible deadlock\n";
                    let _ = syscall3(SYS_WRITE, 2, err_msg.as_ptr() as u64, err_msg.len() as u64);

                    set_errno(EAGAIN);
                    return Err(());
                }
            }
        }
    }
}

// ============================================================================
// Internal file operations
// ============================================================================

/// Prepare FILE for writing (handle mode switch from read)
pub(crate) fn file_prepare_write(file: &mut FILE) -> Result<(), i32> {
    if matches!(file.last_op, LastOp::Read) {
        file.buffer.clear();
        file.last_op = LastOp::None;
    }
    Ok(())
}

/// Prepare FILE for reading (handle mode switch from write)
pub(crate) fn file_prepare_read(file: &mut FILE) -> Result<(), i32> {
    if matches!(file.last_op, LastOp::Write) {
        file_flush(file)?;
        file.buffer.clear();
        file.last_op = LastOp::None;
    }
    Ok(())
}

/// Flush pending write data from buffer to fd
pub(crate) fn file_flush(file: &mut FILE) -> Result<(), i32> {
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

/// Write bytes to FILE (with buffering)
pub(crate) fn file_write_bytes(file: &mut FILE, bytes: &[u8]) -> Result<(), i32> {
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
            // Buffer is full, need to flush. Set last_op first!
            file.last_op = LastOp::Write;
            file_flush(file)?;
            continue;
        }
        let chunk = cmp::min(available, remaining.len());
        let end = file.buffer.len + chunk;
        file.buffer.data[file.buffer.len..end].copy_from_slice(&remaining[..chunk]);
        file.buffer.len = end;

        // Set last_op BEFORE flushing so that file_flush can see it
        file.last_op = LastOp::Write;

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

    Ok(())
}

/// Write a single byte to FILE
pub(crate) fn file_write_byte(file: &mut FILE, byte: u8) -> Result<(), i32> {
    let buf = [byte];
    file_write_bytes(file, &buf)
}

/// Write a byte repeated `count` times
pub(crate) fn write_repeat(file: &mut FILE, byte: u8, mut count: usize) -> Result<(), i32> {
    const TEMP_BUF_SIZE: usize = 32;
    let temp = [byte; TEMP_BUF_SIZE];
    while count > 0 {
        let take = cmp::min(count, TEMP_BUF_SIZE);
        file_write_bytes(file, &temp[..take])?;
        count -= take;
    }
    Ok(())
}

/// Read bytes from FILE into buffer
pub(crate) fn file_read_bytes(file: &mut FILE, out: &mut [u8]) -> Result<usize, i32> {
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

/// Read a single byte from FILE
pub(crate) fn file_read_byte(file: &mut FILE) -> Result<Option<u8>, i32> {
    let mut buf = [0u8; 1];
    match file_read_bytes(file, &mut buf) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(buf[0])),
        Err(err) => Err(err),
    }
}
