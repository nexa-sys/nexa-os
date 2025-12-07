fn main() {
    // Link against NexaOS nrlib's libc
    // 
    // NOTE: Link search paths are now handled by build-libs.sh via RUSTFLAGS
    // to ensure proper PIC/non-PIC separation. The build script uses:
    // - sysroot-pic/lib for shared library builds (PIC code required)
    // - sysroot/lib for static library builds (non-PIC is fine)
    //
    // We only need to declare the dependency on libc here.
    println!("cargo:rustc-link-lib=c");
    
    // Link against ncryptolib (dependency handled by build-libs.sh build order)
}
