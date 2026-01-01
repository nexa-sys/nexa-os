//! Build script for nexa-os-tests
//!
//! This script preprocesses kernel source files for testing:
//! 1. Copies kernel source to a build directory
//! 2. Removes #[global_allocator] and #[alloc_error_handler] attributes
//!    (these conflict with std's allocator)
//! 3. The preprocessed source can then be included in tests

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

fn main() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let kernel_src = Path::new(manifest_dir).parent().unwrap().join("src");
    let build_dir = Path::new(manifest_dir).join("build").join("kernel_src");
    
    // Clean and recreate build directory
    let _ = fs::remove_dir_all(&build_dir);
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");
    
    // Copy and preprocess kernel source
    preprocess_dir(&kernel_src, &build_dir).expect("Failed to preprocess kernel source");
    
    // Export preprocessed source path
    println!("cargo:rustc-env=KERNEL_SRC={}", build_dir.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../src");
}

/// Recursively preprocess a directory
fn preprocess_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);
        
        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            preprocess_dir(&src_path, &dst_path)?;
        } else if src_path.extension().map_or(false, |e| e == "rs") {
            preprocess_file(&src_path, &dst_path)?;
        } else {
            // Copy non-Rust files as-is
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Preprocess a single Rust file
/// Removes #[global_allocator], #[alloc_error_handler], and extern crate alloc
/// Removes #[cfg(test)] guards (test framework always runs in test mode)
/// Replaces alloc:: with std:: for std compatibility
/// Replaces x86_64 crate hardware calls with mock HAL calls
fn preprocess_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    let file = fs::File::open(src)?;
    let reader = BufReader::new(file);
    let mut output = fs::File::create(dst)?;
    
    let mut skip_until_balanced = false;
    let mut brace_depth = 0;
    
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        
        // Remove extern crate alloc - tests use std's alloc
        if trimmed == "extern crate alloc;" {
            writeln!(output, "// REMOVED FOR TESTING: {}", line)?;
            continue;
        }
        
        // Remove #[cfg(test)] guards - test framework is always in test mode
        // This makes test-only types like UdpMessage available
        if trimmed == "#[cfg(test)]" {
            writeln!(output, "// REMOVED FOR TESTING (cfg test): {}", line)?;
            continue;
        }
        
        // Check for attributes we need to remove
        if trimmed == "#[global_allocator]" || trimmed == "#[alloc_error_handler]" {
            // Skip this attribute and the following item
            skip_until_balanced = true;
            brace_depth = 0;
            writeln!(output, "// REMOVED FOR TESTING: {}", line)?;
            continue;
        }
        
        if skip_until_balanced {
            writeln!(output, "// REMOVED FOR TESTING: {}", line)?;
            
            // Track braces to find end of item
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        if brace_depth > 0 {
                            brace_depth -= 1;
                        }
                    }
                    _ => {}
                }
            }
            
            // For static items ending with semicolon (no braces)
            if brace_depth == 0 {
                if trimmed.ends_with(';') || trimmed == "}" {
                    skip_until_balanced = false;
                }
            }
            continue;
        }
        
        // Replace alloc crate references with std for std compatibility
        // std re-exports everything from alloc, so this works
        // Note: Don't replace "pub use alloc::" if it refers to local mod alloc
        let processed_line = if trimmed.starts_with("pub use alloc::") && 
            !trimmed.contains("vec::") && 
            !trimmed.contains("string::") && 
            !trimmed.contains("boxed::") &&
            !trimmed.contains("collections::") &&
            !trimmed.contains("fmt::") {
            // This is likely re-exporting from local alloc module, keep as is
            line.clone()
        } else {
            line
                // use statements
                .replace("use alloc::vec", "use std::vec")
                .replace("use alloc::string", "use std::string")
                .replace("use alloc::boxed", "use std::boxed")
                .replace("use alloc::collections", "use std::collections")
                .replace("use alloc::sync", "use std::sync")
                .replace("use alloc::rc", "use std::rc")
                .replace("use alloc::borrow", "use std::borrow")
                .replace("use alloc::fmt", "use std::fmt")
                .replace("use alloc::format", "use std::format")
                .replace("use alloc::alloc", "use std::alloc")
                // Inline usage (alloc::format!, alloc::vec!, etc.)
                .replace("alloc::format!", "std::format!")
                .replace("alloc::vec!", "std::vec!")
                .replace("alloc::vec::", "std::vec::")
                .replace("alloc::string::", "std::string::")
                .replace("alloc::boxed::", "std::boxed::")
                .replace("alloc::collections::", "std::collections::")
                // Inline alloc::alloc::func calls (from safety/alloc.rs)
                .replace("alloc::alloc::alloc", "std::alloc::alloc")
                .replace("alloc::alloc::alloc_zeroed", "std::alloc::alloc_zeroed")
                .replace("alloc::alloc::dealloc", "std::alloc::dealloc")
                .replace("alloc::alloc::realloc", "std::alloc::realloc")
                // Replace x86_64 crate Cr3 hardware calls with mock HAL
                // Must replace full path to avoid partial matches
                .replace("x86_64::registers::control::Cr3::read()", "crate::mock::hal::mock_cr3_read()")
                .replace("Cr3::read()", "crate::mock::hal::mock_cr3_read()")
                .replace("Cr3::write(", "crate::mock::hal::mock_cr3_write(")
        };
        
        writeln!(output, "{}", processed_line)?;
    }
    
    Ok(())
}

