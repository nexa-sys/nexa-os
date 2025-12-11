//! Build script for nh3 library
//!
//! This configures:
//! - Library name for C ABI compatibility (libnghttp3.so)
//! - Dynamic linking against ntcp2 (libngtcp2.so) for QUIC support

fn main() {
    // Rerun if source changes
    println!("cargo:rerun-if-changed=src/");

    // Set output library name to match nghttp3
    // For rust-lld linker, we use --soname directly (no -Wl, prefix)
    println!("cargo:rustc-cdylib-link-arg=--soname=libnghttp3.so.1");

    // Dynamically link against ntcp2 (libngtcp2.so)
    // ntcp2 provides ngtcp2-compatible C ABI for QUIC protocol support
    println!("cargo:rustc-link-lib=dylib=ngtcp2");

    // ntcp2 internally links against nssl (libssl.so) and ncryptolib (libcrypto.so)
    // but we don't need to link them directly as they're transitive dependencies
}
