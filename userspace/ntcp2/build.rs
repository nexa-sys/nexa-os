//! Build script for ntcp2 library
//!
//! This configures:
//! - Library name for C ABI compatibility (libngtcp2.so)
//! - Dynamic linking against nssl (libssl.so) for TLS support

fn main() {
    // Rerun if source changes
    println!("cargo:rerun-if-changed=src/");

    // Set output library name to match ngtcp2
    // For rust-lld linker, we use --soname directly (no -Wl, prefix)
    println!("cargo:rustc-cdylib-link-arg=--soname=libngtcp2.so.1");

    // Dynamically link against nssl (libssl.so)
    // nssl provides OpenSSL-compatible C ABI for TLS 1.2/1.3 support
    // nssl internally links against ncryptolib (libcrypto.so)
    println!("cargo:rustc-link-lib=dylib=ssl");
    println!("cargo:rustc-link-lib=dylib=crypto");
}
