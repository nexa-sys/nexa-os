//! Source Code Analysis Tests for Keyboard Interrupt Bug
//! 
//! These tests analyze the actual source code to verify that the keyboard
//! interrupt handler has the correct rescheduling logic.
//! 
//! A good test should FAIL when the bug exists and PASS when fixed.

/// Test: Verify keyboard_interrupt_handler checks resched_pending and calls do_schedule
/// 
/// This test will FAIL if keyboard_interrupt_handler does NOT call do_schedule_from_interrupt
/// based on the resched_pending flag.
/// 
/// Expected code pattern (like timer_interrupt_handler):
/// ```
/// let resched_pending = crate::smp::leave_interrupt();
/// if resched_pending {
///     do_schedule_from_interrupt();
/// }
/// ```
#[test]
fn test_keyboard_handler_has_resched_check() {
    // Read the keyboard interrupt handler source code
    let handler_source = include_str!("../../../../src/interrupts/handlers.rs");
    
    // Find the keyboard_interrupt_handler function
    let kbd_handler_start = handler_source.find("fn keyboard_interrupt_handler")
        .expect("keyboard_interrupt_handler function should exist");
    
    // Find the end of the function (next 'pub extern' or end of file)
    let remaining = &handler_source[kbd_handler_start..];
    let kbd_handler_end = remaining[50..].find("pub extern")
        .map(|i| i + 50)
        .unwrap_or(remaining.len());
    
    let kbd_handler_code = &remaining[..kbd_handler_end];
    
    // Check if the handler has the correct pattern:
    // 1. It should check resched_pending (not ignore it with let _)
    // 2. It should call do_schedule_from_interrupt when resched_pending is true
    
    let has_resched_pending_check = kbd_handler_code.contains("if") 
        && kbd_handler_code.contains("resched_pending")
        && !kbd_handler_code.contains("let _ = ");
    
    let calls_do_schedule = kbd_handler_code.contains("do_schedule_from_interrupt");
    
    // This assertion will FAIL if the bug exists!
    // The bug is: keyboard handler does NOT check resched_pending
    assert!(
        has_resched_pending_check && calls_do_schedule,
        "\n\n\
        =========== BUG DETECTED ===========\n\
        keyboard_interrupt_handler does NOT properly handle rescheduling!\n\
        \n\
        Current behavior:\n\
        - leave_interrupt() returns resched_pending but it's IGNORED with 'let _ = '\n\
        - do_schedule_from_interrupt() is NEVER called\n\
        \n\
        Expected behavior (like timer_interrupt_handler):\n\
        - let resched_pending = crate::smp::leave_interrupt();\n\
        - if resched_pending {{ crate::scheduler::do_schedule_from_interrupt(); }}\n\
        \n\
        Impact:\n\
        - When keyboard input wakes a sleeping shell process, it sets need_resched\n\
        - But keyboard ISR ignores this flag and returns immediately\n\
        - Shell must wait for next timer tick (up to 1ms) to run\n\
        - This causes noticeable input lag in interactive programs\n\
        \n\
        Fix location: src/interrupts/handlers.rs, keyboard_interrupt_handler()\n\
        ===================================\n"
    );
}

/// Test: Compare timer and keyboard handlers to show the inconsistency
#[test]
fn test_handler_consistency_timer_vs_keyboard() {
    let source = include_str!("../../../../src/interrupts/handlers.rs");
    
    // Find timer_interrupt_handler
    let timer_start = source.find("fn timer_interrupt_handler")
        .expect("timer_interrupt_handler should exist");
    let timer_section = &source[timer_start..];
    let timer_end = timer_section[50..].find("pub extern").unwrap_or(timer_section.len()) + 50;
    let timer_code = &timer_section[..timer_end];
    
    // Find keyboard_interrupt_handler  
    let kbd_start = source.find("fn keyboard_interrupt_handler")
        .expect("keyboard_interrupt_handler should exist");
    let kbd_section = &source[kbd_start..];
    let kbd_end = kbd_section[50..].find("pub extern").unwrap_or(kbd_section.len()) + 50;
    let kbd_code = &kbd_section[..kbd_end];
    
    // Timer handler should have proper resched handling
    let timer_has_resched_if = timer_code.contains("if should_resched || resched_pending")
        || timer_code.contains("if resched_pending");
    let timer_calls_schedule = timer_code.contains("do_schedule_from_interrupt");
    
    // Keyboard handler should ALSO have proper resched handling
    let kbd_has_resched_if = kbd_code.contains("if") 
        && (kbd_code.contains("resched_pending") || kbd_code.contains("should_resched"))
        && !kbd_code.contains("let _ = ");
    let kbd_calls_schedule = kbd_code.contains("do_schedule_from_interrupt");
    
    // Show what each handler does
    println!("Timer handler has resched check: {}", timer_has_resched_if);
    println!("Timer handler calls do_schedule: {}", timer_calls_schedule);
    println!("Keyboard handler has resched check: {}", kbd_has_resched_if);
    println!("Keyboard handler calls do_schedule: {}", kbd_calls_schedule);
    
    // This will FAIL if keyboard handler is missing the resched logic
    assert!(
        timer_has_resched_if == kbd_has_resched_if,
        "\n\nINCONSISTENT HANDLER BEHAVIOR!\n\
        Timer handler checks resched_pending: {}\n\
        Keyboard handler checks resched_pending: {}\n\
        \n\
        Both handlers should behave the same way regarding rescheduling.\n\
        When a process is woken up (by timer sleep expiry OR keyboard input),\n\
        the interrupt handler should check if rescheduling is needed.\n",
        timer_has_resched_if,
        kbd_has_resched_if
    );
    
    assert!(
        timer_calls_schedule == kbd_calls_schedule,
        "\n\nINCONSISTENT HANDLER BEHAVIOR!\n\
        Timer handler calls do_schedule: {}\n\
        Keyboard handler calls do_schedule: {}\n\
        \n\
        Both handlers should be able to trigger rescheduling.\n",
        timer_calls_schedule,
        kbd_calls_schedule
    );
}

/// Test: Verify the exact buggy pattern exists (for documentation)
#[test]
fn test_buggy_pattern_exists() {
    let source = include_str!("../../../../src/interrupts/handlers.rs");
    
    // Find keyboard_interrupt_handler
    let kbd_start = source.find("fn keyboard_interrupt_handler")
        .expect("keyboard_interrupt_handler should exist");
    let kbd_section = &source[kbd_start..];
    let kbd_end = kbd_section[50..].find("pub extern").unwrap_or(kbd_section.len()) + 50;
    let kbd_code = &kbd_section[..kbd_end];
    
    // The buggy pattern: "let _ = crate::smp::leave_interrupt();"
    // This means the resched_pending return value is intentionally discarded!
    let has_ignored_resched = kbd_code.contains("let _ = crate::smp::leave_interrupt()") 
        || kbd_code.contains("let _ = leave_interrupt()");
    
    // If this test PASSES, the bug exists!
    // If this test FAILS, the bug was fixed!
    if has_ignored_resched {
        panic!(
            "\n\n\
            =========== CONFIRMED BUG ===========\n\
            Found 'let _ = crate::smp::leave_interrupt()' in keyboard_interrupt_handler!\n\
            \n\
            This pattern IGNORES the resched_pending return value.\n\
            The handler should instead:\n\
            1. let resched_pending = crate::smp::leave_interrupt();\n\
            2. if resched_pending {{ do_schedule_from_interrupt(); }}\n\
            =====================================\n"
        );
    }
}
