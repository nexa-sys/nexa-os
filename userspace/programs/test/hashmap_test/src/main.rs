//! HashMap Test - Test that std::collections::HashMap works on NexaOS
//!
//! This program tests that HashMap can be used in userspace programs.
//! HashMap requires getrandom() for RandomState initialization.

use std::alloc::{alloc, dealloc, Layout};
use std::collections::HashMap;
use std::io::Write;

fn main() {
    println!("=== HashMap Free Bug Test ===");
    let _ = std::io::stdout().flush();

    // Test aligned allocation (HashMap uses this)
    println!("\nTest 1: Aligned allocation (64 byte alignment)");
    let _ = std::io::stdout().flush();
    unsafe {
        let layout = Layout::from_size_align(256, 64).unwrap();
        let ptr = alloc(layout);
        println!("  Allocated 256 bytes with 64-byte alignment at {:p}", ptr);
        let _ = std::io::stdout().flush();

        if !ptr.is_null() {
            dealloc(ptr, layout);
            println!("  Deallocated OK");
            let _ = std::io::stdout().flush();
        }
    }

    println!("\nTest 2: Multiple aligned allocations");
    let _ = std::io::stdout().flush();
    unsafe {
        let layout1 = Layout::from_size_align(128, 64).unwrap();
        let layout2 = Layout::from_size_align(64, 32).unwrap();
        let layout3 = Layout::from_size_align(32, 16).unwrap();

        let p1 = alloc(layout1);
        let p2 = alloc(layout2);
        let p3 = alloc(layout3);
        println!("  p1={:p}, p2={:p}, p3={:p}", p1, p2, p3);
        let _ = std::io::stdout().flush();

        // Free in different order
        dealloc(p2, layout2);
        println!("  p2 freed OK");
        let _ = std::io::stdout().flush();

        dealloc(p1, layout1);
        println!("  p1 freed OK");
        let _ = std::io::stdout().flush();

        dealloc(p3, layout3);
        println!("  p3 freed OK");
        let _ = std::io::stdout().flush();
    }

    println!("\nTest 3: Simulate hashbrown RawTable allocation");
    let _ = std::io::stdout().flush();
    unsafe {
        // hashbrown uses Group alignment which is typically 16 bytes
        // and allocates control bytes + data
        let layout = Layout::from_size_align(512, 16).unwrap();
        let ptr = alloc(layout);
        println!("  RawTable-like allocation at {:p}", ptr);
        let _ = std::io::stdout().flush();

        // Also allocate a String
        let s = "hello".to_string();
        println!("  String at {:p}", s.as_ptr());
        let _ = std::io::stdout().flush();

        // Drop string first (like HashMap does)
        drop(s);
        println!("  String dropped OK");
        let _ = std::io::stdout().flush();

        // Then free table
        dealloc(ptr, layout);
        println!("  Table freed OK");
        let _ = std::io::stdout().flush();
    }

    println!("\nTest 4: Actual HashMap<i32, String>");
    let _ = std::io::stdout().flush();

    let mut map: HashMap<i32, String> = HashMap::new();
    map.insert(1, "test".to_string());
    println!("  HashMap created, len = {}", map.len());
    let _ = std::io::stdout().flush();

    println!("  Dropping HashMap...");
    let _ = std::io::stdout().flush();
    drop(map);
    println!("  HashMap dropped OK!");
    let _ = std::io::stdout().flush();

    println!("\n=== All tests passed! ===");
}
