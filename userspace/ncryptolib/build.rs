fn main() {
    // Link against NexaOS nrlib's libc
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    
    // CRITICAL: PIC sysroot MUST come FIRST for shared library builds!
    // The linker searches paths in order, and shared libraries need PIC-compiled libc.a
    // Using non-PIC libc.a causes R_X86_64_32S relocation errors
    let sysroot_pic = format!("{}/../../build/userspace-build/sysroot-pic/lib", manifest_dir);
    println!("cargo:rustc-link-search=native={}", sysroot_pic);
    
    // Non-PIC sysroot as fallback for static builds
    let sysroot = format!("{}/../../build/userspace-build/sysroot/lib", manifest_dir);
    println!("cargo:rustc-link-search=native={}", sysroot);
    
    println!("cargo:rustc-link-lib=c");
}
