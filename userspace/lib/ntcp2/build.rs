//! Build script for ntcp2 library
//!
//! This configures:
//! - Library name for C ABI compatibility (libngtcp2.so)
//! - Library search path for NexaOS libraries

fn main() {
    // Rerun if source changes
    println!("cargo:rerun-if-changed=src/");

    // Set output library name to match ngtcp2
    // For rust-lld linker, we use --soname directly (no -Wl, prefix)
    println!("cargo:rustc-cdylib-link-arg=--soname=libngtcp2.so.1");

    // Add library search path for our custom libraries (nssl, ncryptolib)
    // These are built in the userspace target directory
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        // Compute the path to the library output directory
        let lib_path = std::path::Path::new(&manifest_dir)
            .parent() // lib/
            .and_then(|p| p.parent()) // userspace/
            .map(|p| p.join("target/x86_64-nexaos-userspace-lib/release"));

        if let Some(path) = lib_path {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
    }
}
