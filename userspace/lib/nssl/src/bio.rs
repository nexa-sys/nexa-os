//! BIO (Basic I/O) Abstraction
//!
//! Provides I/O abstraction layer similar to OpenSSL's BIO.

use crate::{c_char, c_int, c_uchar};
use std::fs::File;
use std::io::{Read, Write};
use std::vec::Vec;

/// BIO method type
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BioMethodType {
    Null = 0,
    Socket = 1,
    File = 2,
    Memory = 3,
    Pair = 4,
    Filter = 5,
    Ssl = 6,
}

/// BIO method descriptor
#[repr(C)]
pub struct BioMethod {
    pub method_type: BioMethodType,
    pub name: &'static [u8],
}

impl BioMethod {
    pub const fn socket() -> Self {
        Self {
            method_type: BioMethodType::Socket,
            name: b"socket\0",
        }
    }

    pub const fn file() -> Self {
        Self {
            method_type: BioMethodType::File,
            name: b"FILE pointer\0",
        }
    }

    pub const fn memory() -> Self {
        Self {
            method_type: BioMethodType::Memory,
            name: b"memory buffer\0",
        }
    }
}

// Static method instances
pub static BIO_S_SOCKET: BioMethod = BioMethod::socket();
pub static BIO_S_FILE: BioMethod = BioMethod::file();
pub static BIO_S_MEM: BioMethod = BioMethod::memory();

/// BIO flags
pub mod bio_flags {
    pub const BIO_FLAGS_READ: i32 = 0x01;
    pub const BIO_FLAGS_WRITE: i32 = 0x02;
    pub const BIO_FLAGS_IO_SPECIAL: i32 = 0x04;
    pub const BIO_FLAGS_SHOULD_RETRY: i32 = 0x08;
    pub const BIO_CLOSE: i32 = 0x01;
    pub const BIO_NOCLOSE: i32 = 0x00;
}

/// BIO - Basic I/O abstraction
pub struct Bio {
    /// Method type
    method: *const BioMethod,
    /// Socket file descriptor (for socket BIO)
    fd: i32,
    /// File handle (for file BIO)
    file: Option<File>,
    /// Memory buffer (for memory BIO)
    mem_buf: Vec<u8>,
    /// Read position in memory buffer
    mem_pos: usize,
    /// Flags
    flags: i32,
    /// Close flag (whether to close fd/file on free)
    close_flag: i32,
    /// Next BIO in chain
    next: *mut Bio,
    /// Previous BIO in chain
    prev: *mut Bio,
    /// Reference count
    ref_count: u32,
    /// Retry reason
    retry_reason: i32,
}

impl Bio {
    /// Create new BIO with method
    pub fn new(method: *const BioMethod) -> *mut Bio {
        if method.is_null() {
            return core::ptr::null_mut();
        }

        let bio = Box::new(Self {
            method,
            fd: -1,
            file: None,
            mem_buf: Vec::new(),
            mem_pos: 0,
            flags: 0,
            close_flag: bio_flags::BIO_NOCLOSE,
            next: core::ptr::null_mut(),
            prev: core::ptr::null_mut(),
            ref_count: 1,
            retry_reason: 0,
        });

        Box::into_raw(bio)
    }

    /// Create socket BIO
    pub fn new_socket(sock: c_int, close_flag: c_int) -> *mut Bio {
        let bio = Self::new(&BIO_S_SOCKET as *const _);
        if bio.is_null() {
            return bio;
        }

        unsafe {
            (*bio).fd = sock;
            (*bio).close_flag = close_flag;
        }

        bio
    }

    /// Create file BIO
    pub fn new_file(filename: *const c_char, mode: *const c_char) -> *mut Bio {
        if filename.is_null() || mode.is_null() {
            return core::ptr::null_mut();
        }

        let path = unsafe {
            match core::ffi::CStr::from_ptr(filename as *const i8).to_str() {
                Ok(s) => s,
                Err(_) => return core::ptr::null_mut(),
            }
        };

        let mode_str = unsafe {
            match core::ffi::CStr::from_ptr(mode as *const i8).to_str() {
                Ok(s) => s,
                Err(_) => return core::ptr::null_mut(),
            }
        };

        let file = if mode_str.contains('w') {
            File::create(path).ok()
        } else {
            File::open(path).ok()
        };

        if file.is_none() {
            return core::ptr::null_mut();
        }

        let bio = Self::new(&BIO_S_FILE as *const _);
        if bio.is_null() {
            return bio;
        }

        unsafe {
            (*bio).file = file;
            (*bio).close_flag = bio_flags::BIO_CLOSE;
        }

        bio
    }

    /// Create memory BIO from buffer
    pub fn new_mem_buf(buf: *const c_uchar, len: c_int) -> *mut Bio {
        let bio = Self::new(&BIO_S_MEM as *const _);
        if bio.is_null() {
            return bio;
        }

        if !buf.is_null() && len != 0 {
            let actual_len = if len < 0 {
                // Null-terminated string
                unsafe {
                    let mut i = 0;
                    while *buf.add(i) != 0 {
                        i += 1;
                    }
                    i
                }
            } else {
                len as usize
            };

            unsafe {
                let slice = core::slice::from_raw_parts(buf, actual_len);
                (*bio).mem_buf = slice.to_vec();
            }
        }

        bio
    }

    /// Read from BIO
    pub fn read(&mut self, buf: *mut c_uchar, len: c_int) -> c_int {
        if buf.is_null() || len <= 0 {
            return -1;
        }

        let method_type = unsafe { (*self.method).method_type };

        match method_type {
            BioMethodType::Socket => self.read_socket(buf, len),
            BioMethodType::File => self.read_file(buf, len),
            BioMethodType::Memory => self.read_memory(buf, len),
            _ => -1,
        }
    }

    /// Write to BIO
    pub fn write(&mut self, buf: *const c_uchar, len: c_int) -> c_int {
        if buf.is_null() || len <= 0 {
            return -1;
        }

        let method_type = unsafe { (*self.method).method_type };

        match method_type {
            BioMethodType::Socket => self.write_socket(buf, len),
            BioMethodType::File => self.write_file(buf, len),
            BioMethodType::Memory => self.write_memory(buf, len),
            _ => -1,
        }
    }

    /// Read from socket
    fn read_socket(&mut self, buf: *mut c_uchar, len: c_int) -> c_int {
        if self.fd < 0 {
            return -1;
        }

        // Use read syscall
        unsafe {
            let result = libc_read(self.fd, buf as *mut u8, len as usize);
            if result < 0 {
                self.flags |= bio_flags::BIO_FLAGS_SHOULD_RETRY;
                self.retry_reason = -1; // errno would go here
            }
            result as c_int
        }
    }

    /// Write to socket
    fn write_socket(&mut self, buf: *const c_uchar, len: c_int) -> c_int {
        if self.fd < 0 {
            return -1;
        }

        unsafe {
            let result = libc_write(self.fd, buf as *const u8, len as usize);
            if result < 0 {
                self.flags |= bio_flags::BIO_FLAGS_SHOULD_RETRY;
                self.retry_reason = -1;
            }
            result as c_int
        }
    }

    /// Read from file
    fn read_file(&mut self, buf: *mut c_uchar, len: c_int) -> c_int {
        if let Some(ref mut file) = self.file {
            let slice = unsafe { core::slice::from_raw_parts_mut(buf, len as usize) };
            match file.read(slice) {
                Ok(n) => n as c_int,
                Err(_) => -1,
            }
        } else {
            -1
        }
    }

    /// Write to file
    fn write_file(&mut self, buf: *const c_uchar, len: c_int) -> c_int {
        if let Some(ref mut file) = self.file {
            let slice = unsafe { core::slice::from_raw_parts(buf, len as usize) };
            match file.write(slice) {
                Ok(n) => n as c_int,
                Err(_) => -1,
            }
        } else {
            -1
        }
    }

    /// Read from memory buffer
    fn read_memory(&mut self, buf: *mut c_uchar, len: c_int) -> c_int {
        let available = self.mem_buf.len() - self.mem_pos;
        if available == 0 {
            return 0; // EOF
        }

        let to_read = (len as usize).min(available);
        unsafe {
            core::ptr::copy_nonoverlapping(self.mem_buf[self.mem_pos..].as_ptr(), buf, to_read);
        }
        self.mem_pos += to_read;

        to_read as c_int
    }

    /// Write to memory buffer
    fn write_memory(&mut self, buf: *const c_uchar, len: c_int) -> c_int {
        let slice = unsafe { core::slice::from_raw_parts(buf, len as usize) };
        self.mem_buf.extend_from_slice(slice);
        len
    }

    /// Get memory buffer contents
    pub fn get_mem_data(&self) -> &[u8] {
        &self.mem_buf
    }

    /// Free BIO
    pub fn free(bio: *mut Bio) -> c_int {
        if bio.is_null() {
            return 0;
        }

        unsafe {
            (*bio).ref_count -= 1;
            if (*bio).ref_count == 0 {
                // Close socket if BIO_CLOSE
                if (*bio).close_flag == bio_flags::BIO_CLOSE && (*bio).fd >= 0 {
                    libc_close((*bio).fd);
                }
                drop(Box::from_raw(bio));
            }
        }

        1
    }

    /// Free entire BIO chain
    pub fn free_all(mut bio: *mut Bio) {
        while !bio.is_null() {
            let next = unsafe { (*bio).next };
            Self::free(bio);
            bio = next;
        }
    }

    /// Check if should retry
    pub fn should_retry(&self) -> bool {
        (self.flags & bio_flags::BIO_FLAGS_SHOULD_RETRY) != 0
    }

    /// Get retry reason
    pub fn get_retry_reason(&self) -> c_int {
        self.retry_reason
    }
}

// Syscall wrappers (would use actual NexaOS syscalls)
unsafe fn libc_read(fd: i32, buf: *mut u8, count: usize) -> isize {
    // NexaOS read syscall
    extern "C" {
        fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
    }
    read(fd, buf, count)
}

unsafe fn libc_write(fd: i32, buf: *const u8, count: usize) -> isize {
    extern "C" {
        fn write(fd: i32, buf: *const u8, count: usize) -> isize;
    }
    write(fd, buf, count)
}

unsafe fn libc_close(fd: i32) -> i32 {
    extern "C" {
        fn close(fd: i32) -> i32;
    }
    close(fd)
}

// C ABI exports for BIO methods
#[no_mangle]
pub extern "C" fn BIO_s_socket() -> *const BioMethod {
    &BIO_S_SOCKET as *const _
}

#[no_mangle]
pub extern "C" fn BIO_s_file() -> *const BioMethod {
    &BIO_S_FILE as *const _
}

#[no_mangle]
pub extern "C" fn BIO_s_mem() -> *const BioMethod {
    &BIO_S_MEM as *const _
}
