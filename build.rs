use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=boot/long_mode.S");
    println!("cargo:rerun-if-changed=linker.ld");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));

    println!(
        "cargo:rustc-link-arg-bin=nexa-os=-T{}/linker.ld",
        manifest_dir
    );

    cc::Build::new()
        .file("boot/long_mode.S")
        .flag_if_supported("-fno-asynchronous-unwind-tables")
        .cargo_metadata(false)
        .compile("boot");

    // Force linking of libboot.a exactly once for the binary
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-arg-bin=nexa-os=--whole-archive");
    println!(
        "cargo:rustc-link-arg-bin=nexa-os={}/libboot.a",
        out_dir.display()
    );
    println!("cargo:rustc-link-arg-bin=nexa-os=--no-whole-archive");
}
