//! Dynamic Linking Test - Hello World
//!
//! Simple test program for dynamic linker (ld-nrlib)

fn main() {
    println!("===========================================");
    println!("  Hello from dynamically linked program!   ");
    println!("===========================================");
    println!();
    println!("Dynamic linker: ld-nrlib-x86_64.so.1");
    println!("C library:      libnrlib.so");
    println!();
    println!("If you see this message, dynamic linking works!");
}
