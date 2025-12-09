//! Build script for ntcp2 library
//!
//! This configures the library name for C ABI compatibility

fn main() {
    // Rerun if source changes
    println!("cargo:rerun-if-changed=src/");
    
    // Set output library name to match ngtcp2
    // For rust-lld linker, we use --soname directly (no -Wl, prefix)
    println!("cargo:rustc-cdylib-link-arg=--soname=libngtcp2.so.1");
    
    // Note: nssl and ncryptolib are linked statically via Cargo rlib dependencies
    // If you need dynamic linking, use rustc-link-lib=dylib=ssl instead
}
