//! PTY smoke test
//!
//! Opens /dev/ptmx, uses ioctl to get PTY number + unlock, opens /dev/pts/<n>,
//! then verifies bidirectional read/write between master and slave.

use std::ffi::CString;
use libc::{c_int, c_ulong, c_void};

const O_RDWR: c_int = 2;

// ioctl numbers (must match kernel/userspace)
const TIOCGPTN: u64 = 0x8004_5430;
const TIOCSPTLCK: u64 = 0x4004_5431;

fn main() {
    unsafe {
        let ptmx = CString::new("/dev/ptmx").unwrap();
        let m = libc::open(ptmx.as_ptr(), O_RDWR, 0);
        if m < 0 {
            let msg = CString::new("open /dev/ptmx").unwrap();
            libc::perror(msg.as_ptr());
            std::process::exit(1);
        }

        let mut ptn: c_int = -1;
        if libc::ioctl(m, TIOCGPTN as c_ulong, &mut ptn as *mut _ as *mut c_void) != 0 {
            let msg = CString::new("ioctl TIOCGPTN").unwrap();
            libc::perror(msg.as_ptr());
            let _ = libc::close(m);
            std::process::exit(2);
        }

        let mut unlock: c_int = 0;
        if libc::ioctl(m, TIOCSPTLCK as c_ulong, &mut unlock as *mut _ as *mut c_void) != 0 {
            let msg = CString::new("ioctl TIOCSPTLCK").unwrap();
            libc::perror(msg.as_ptr());
            let _ = libc::close(m);
            std::process::exit(3);
        }

        let path = CString::new(format!("/dev/pts/{ptn}")).unwrap();
        let s = libc::open(path.as_ptr(), O_RDWR, 0);
        if s < 0 {
            let msg = CString::new("open /dev/pts/N").unwrap();
            libc::perror(msg.as_ptr());
            let _ = libc::close(m);
            std::process::exit(4);
        }

        println!("[pty-smoke] allocated pty: {}", ptn);

        // Slave -> Master
        let msg1 = b"hello-from-slave";
        if libc::write(s, msg1.as_ptr() as *const c_void, msg1.len()) != msg1.len() as isize {
            let msg = CString::new("write slave").unwrap();
            libc::perror(msg.as_ptr());
            let _ = libc::close(s);
            let _ = libc::close(m);
            std::process::exit(5);
        }

        let mut buf = [0u8; 64];
        let r = libc::read(m, buf.as_mut_ptr() as *mut c_void, buf.len());
        if r <= 0 {
            let msg = CString::new("read master").unwrap();
            libc::perror(msg.as_ptr());
            let _ = libc::close(s);
            let _ = libc::close(m);
            std::process::exit(6);
        }

        // Master -> Slave
        let msg2 = b"hello-from-master";
        if libc::write(m, msg2.as_ptr() as *const c_void, msg2.len()) != msg2.len() as isize {
            let msg = CString::new("write master").unwrap();
            libc::perror(msg.as_ptr());
            let _ = libc::close(s);
            let _ = libc::close(m);
            std::process::exit(7);
        }
        let r2 = libc::read(s, buf.as_mut_ptr() as *mut c_void, buf.len());
        if r2 <= 0 {
            let msg = CString::new("read slave").unwrap();
            libc::perror(msg.as_ptr());
            let _ = libc::close(s);
            let _ = libc::close(m);
            std::process::exit(8);
        }

        let _ = libc::close(s);
        let _ = libc::close(m);
        println!("[pty-smoke] PASS");
    }
}
