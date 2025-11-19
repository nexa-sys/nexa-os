use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=boot/long_mode.S");
    println!("cargo:rerun-if-changed=boot/ap_trampoline.S");
    println!("cargo:rerun-if-changed=linker.ld");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let target = env::var("TARGET").expect("TARGET is set by cargo");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));

    // 只有当目标不是UEFI时才应用主内核的链接参数
    if target != "x86_64-unknown-uefi" {
        // Resolve and canonicalize linker.ld so we emit an absolute path that is
        // robust if the project directory has been moved or symlinked.
        let manifest_path = PathBuf::from(&manifest_dir);
        let linker_path = manifest_path.join("linker.ld");
        let linker_abs = std::fs::canonicalize(&linker_path)
            .unwrap_or_else(|_| panic!("Failed to canonicalize linker.ld at {}", linker_path.display()));

        // Emit an informational message to help debug path issues during the
        // build; cargo will display this as a warning.
        println!("cargo:warning=Resolved linker path: {}", linker_abs.display());

        println!(
            "cargo:rustc-link-arg-bin=nexa-os=-T{}",
            linker_abs.display()
        );

        cc::Build::new()
            .file("boot/long_mode.S")
            .file("boot/ap_trampoline.S")
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
    } else {
        // 为UEFI目标构建时，只编译汇编文件
        cc::Build::new()
            .file("boot/long_mode.S")
            .file("boot/ap_trampoline.S")
            .flag_if_supported("-fno-asynchronous-unwind-tables")
            .compile("boot");
    }
}
