use core::fmt;
use spin::Mutex;
use uart_16550::SerialPort;

struct SerialPortWrapper {
    port: Option<SerialPort>,
}

impl SerialPortWrapper {
    const fn new() -> Self {
        Self { port: None }
    }

    fn ensure_init(&mut self) {
        if self.port.is_none() {
            let mut port = unsafe { SerialPort::new(0x3F8) };
            port.init();
            self.port = Some(port);
        }
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) {
        self.ensure_init();
        if let Some(ref mut port) = self.port {
            use core::fmt::Write;
            port.write_fmt(args).ok();
        }
    }

    fn with_port<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut SerialPort) -> R,
    {
        self.ensure_init();
        self.port.as_mut().map(f)
    }
}

static SERIAL1: Mutex<SerialPortWrapper> = Mutex::new(SerialPortWrapper::new());

pub fn init() {
    SERIAL1.lock().ensure_init();
}

pub(crate) fn _print(args: fmt::Arguments<'_>) {
    SERIAL1.lock().write_fmt(args);
}

pub fn write_byte(byte: u8) {
    let mut guard = SERIAL1.lock();
    guard.with_port(|port| port.send(byte));
}

pub fn write_str(s: &str) {
    write_bytes(s.as_bytes())
}

pub fn write_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }

    let mut guard = SERIAL1.lock();
    guard.with_port(|port| {
        for &byte in bytes {
            port.send(byte);
        }
    });
}
