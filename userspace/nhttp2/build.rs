//! Build script for nhttp2
//!
//! This configures the library name for C ABI compatibility

fn main() {
    // Set output library name to match nghttp2
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libnghttp2.so.14");
    
    // Enable position independent code
    println!("cargo:rustc-link-arg=-fPIC");
}
