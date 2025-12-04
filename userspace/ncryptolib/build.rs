fn main() {
    // Link against NexaOS nrlib's libc
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let sysroot = format!("{}/../../build/userspace-build/sysroot/lib", manifest_dir);
    
    println!("cargo:rustc-link-search=native={}", sysroot);
    println!("cargo:rustc-link-lib=c");
    
    // Also try alternative path
    let sysroot_pic = format!("{}/../../build/userspace-build/sysroot-pic/lib", manifest_dir);
    println!("cargo:rustc-link-search=native={}", sysroot_pic);
}
