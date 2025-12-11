//! Build script for nhttp2
//!
//! This configures the library name for C ABI compatibility

fn main() {
    // Set output library name to match nghttp2
    // For rust-lld linker, we use --soname directly (no -Wl, prefix)
    println!("cargo:rustc-cdylib-link-arg=--soname=libnghttp2.so.14");

    // Note: PIC is set via RUSTFLAGS in the build script, not here
    // rust-lld doesn't support -fPIC as a linker argument
}
