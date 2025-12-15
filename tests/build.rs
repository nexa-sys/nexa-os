//! Build script for nexa-os-tests
//!
//! This script sets up the environment for testing kernel code.
//! It exports the kernel source path so tests can include kernel modules directly.

fn main() {
    // Export kernel source path for use in tests
    let kernel_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src");
    
    println!("cargo:rustc-env=KERNEL_SRC={}", kernel_src.display());
    println!("cargo:rerun-if-changed=build.rs");
    
    // Rerun if kernel source changes
    println!("cargo:rerun-if-changed=../src");
}
