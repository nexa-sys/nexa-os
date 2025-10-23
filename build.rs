use std::env;

fn main() {
    println!("cargo:rerun-if-changed=boot/long_mode.S");
    println!("cargo:rerun-if-changed=linker.ld");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    println!("cargo:rustc-link-arg=-T{}/linker.ld", manifest_dir);

    cc::Build::new()
        .file("boot/long_mode.S")
        .flag_if_supported("-fno-asynchronous-unwind-tables")
        .compile("boot");
}
