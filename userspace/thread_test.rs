//! NexaOS Multi-threading Test Program
//!
//! This program tests the LWP-based threading implementation using std.

use std::sync::atomic::{AtomicU32, Ordering};
use std::arch::asm;
use std::thread;
use std::time::Duration;

// Syscall numbers
const SYS_GETTID: u64 = 186;

// ============================================================================
// Syscall wrappers (for NexaOS-specific calls)
// ============================================================================

#[inline(always)]
fn syscall0(n: u64) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            lateout("rax") ret,
            clobber_abi("sysv64")
        );
    }
    ret
}

fn gettid() -> i32 {
    syscall0(SYS_GETTID) as i32
}

// ============================================================================
// Test functions
// ============================================================================

// Global counter for testing
static COUNTER: AtomicU32 = AtomicU32::new(0);

fn test_basic_thread() {
    println!("=== Test 1: Basic Thread Creation ===");
    COUNTER.store(0, Ordering::SeqCst);
    
    let handle = thread::spawn(|| {
        let tid = gettid();
        println!("[Thread {}] Started", tid);
        
        // Increment counter
        COUNTER.fetch_add(1, Ordering::SeqCst);
        
        // Do some work
        for i in 0..3 {
            println!("[Thread {}] Working... iteration {}", tid, i);
            thread::sleep(Duration::from_millis(100));
        }
        
        println!("[Thread {}] Done!", tid);
        42u64
    });
    
    println!("[Main] Thread created, waiting for completion...");
    let result = handle.join().expect("Thread panicked");
    println!("[Main] Thread returned: {}", result);
    
    let counter = COUNTER.load(Ordering::SeqCst);
    println!("[Main] Counter value: {}", counter);
    
    if result == 42 && counter == 1 {
        println!("[PASS] Basic thread test passed!");
    } else {
        println!("[FAIL] Basic thread test failed!");
    }
    println!();
}

fn test_multiple_threads() {
    println!("=== Test 2: Multiple Threads ===");
    COUNTER.store(0, Ordering::SeqCst);
    
    let mut handles = Vec::new();
    
    for i in 0..4 {
        let handle = thread::spawn(move || {
            let tid = gettid();
            println!("[Thread {} ({})] Starting increment", tid, i);
            
            for _ in 0..1000 {
                COUNTER.fetch_add(1, Ordering::SeqCst);
            }
            
            println!("[Thread {} ({})] Done incrementing", tid, i);
            i
        });
        handles.push(handle);
    }
    
    println!("[Main] Created 4 threads, waiting...");
    
    for handle in handles {
        let _ = handle.join();
    }
    
    let counter = COUNTER.load(Ordering::SeqCst);
    println!("[Main] Final counter value: {}", counter);
    
    if counter == 4000 {
        println!("[PASS] Multiple threads test passed!");
    } else {
        println!("[FAIL] Multiple threads test failed!");
        println!("  Expected: 4000, Got: {}", counter);
    }
    println!();
}

fn test_thread_id_consistency() {
    println!("=== Test 3: Thread ID Consistency ===");
    
    let handle = thread::spawn(|| {
        let tid1 = gettid();
        thread::sleep(Duration::from_millis(10));
        let tid2 = gettid();
        
        println!("[Thread] TID check: first={}, second={}", tid1, tid2);
        tid1 == tid2
    });
    
    let result = handle.join().expect("Thread panicked");
    if result {
        println!("[PASS] Thread ID consistency test passed!");
    } else {
        println!("[FAIL] Thread ID changed during execution!");
    }
    println!();
}

fn test_thread_arguments() {
    println!("=== Test 4: Thread Arguments ===");
    
    let value = 123u64;
    let message = String::from("Hello from main!");
    
    let handle = thread::spawn(move || {
        let tid = gettid();
        println!("[Thread {}] Received value: {}", tid, value);
        println!("[Thread {}] Received message: {}", tid, message);
        value * 2
    });
    
    let result = handle.join().expect("Thread panicked");
    println!("[Main] Thread returned: {}", result);
    
    if result == 246 {
        println!("[PASS] Thread arguments test passed!");
    } else {
        println!("[FAIL] Thread arguments test failed!");
    }
    println!();
}

fn test_nested_threads() {
    println!("=== Test 5: Nested Thread Creation ===");
    COUNTER.store(0, Ordering::SeqCst);
    
    let handle = thread::spawn(|| {
        let parent_tid = gettid();
        println!("[Parent Thread {}] Creating child thread", parent_tid);
        
        COUNTER.fetch_add(1, Ordering::SeqCst);
        
        let child_handle = thread::spawn(move || {
            let child_tid = gettid();
            println!("[Child Thread {}] Started (parent was {})", child_tid, parent_tid);
            COUNTER.fetch_add(1, Ordering::SeqCst);
            child_tid
        });
        
        let child_tid = child_handle.join().expect("Child panicked");
        println!("[Parent Thread {}] Child {} finished", parent_tid, child_tid);
        
        COUNTER.fetch_add(1, Ordering::SeqCst);
        parent_tid
    });
    
    let parent_tid = handle.join().expect("Parent panicked");
    println!("[Main] Parent thread {} finished", parent_tid);
    
    let counter = COUNTER.load(Ordering::SeqCst);
    println!("[Main] Counter value: {}", counter);
    
    if counter == 3 {
        println!("[PASS] Nested threads test passed!");
    } else {
        println!("[FAIL] Nested threads test failed!");
    }
    println!();
}

fn test_thread_local_data() {
    println!("=== Test 6: Thread Local Data (simulated) ===");
    
    let mut handles = Vec::new();
    
    for i in 0..3 {
        let handle = thread::spawn(move || {
            // Each thread has its own local variable
            let mut local_counter = 0u32;
            let tid = gettid();
            
            for _ in 0..100 {
                local_counter += 1;
            }
            
            println!("[Thread {} ({})] Local counter: {}", tid, i, local_counter);
            local_counter
        });
        handles.push(handle);
    }
    
    let mut all_correct = true;
    for handle in handles {
        let result = handle.join().expect("Thread panicked");
        if result != 100 {
            all_correct = false;
        }
    }
    
    if all_correct {
        println!("[PASS] Thread local data test passed!");
    } else {
        println!("[FAIL] Thread local data test failed!");
    }
    println!();
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    println!("========================================");
    println!("  NexaOS Multi-threading Test Program  ");
    println!("========================================");
    println!();
    
    let main_tid = gettid();
    println!("Main thread TID: {}", main_tid);
    println!();
    
    test_basic_thread();
    test_multiple_threads();
    test_thread_id_consistency();
    test_thread_arguments();
    test_nested_threads();
    test_thread_local_data();
    
    println!("========================================");
    println!("  All tests completed!                 ");
    println!("========================================");
}
