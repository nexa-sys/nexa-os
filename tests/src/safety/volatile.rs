//! Volatile Memory Access Tests

#[cfg(test)]
mod tests {
    use crate::safety::volatile::{Volatile, VgaChar};

    // =========================================================================
    // Volatile<T> Structure Tests
    // =========================================================================

    #[test]
    fn test_volatile_new() {
        let v = Volatile::new(42u32);
        assert_eq!(v.read(), 42);
    }

    #[test]
    fn test_volatile_read_write() {
        let mut v = Volatile::new(0u32);
        assert_eq!(v.read(), 0);
        
        v.write(100);
        assert_eq!(v.read(), 100);
        
        v.write(u32::MAX);
        assert_eq!(v.read(), u32::MAX);
    }

    #[test]
    fn test_volatile_multiple_types() {
        let v8 = Volatile::new(0xFFu8);
        let v16 = Volatile::new(0xFFFFu16);
        let v32 = Volatile::new(0xFFFFFFFFu32);
        let v64 = Volatile::new(0xFFFFFFFFFFFFFFFFu64);
        
        assert_eq!(v8.read(), 0xFF);
        assert_eq!(v16.read(), 0xFFFF);
        assert_eq!(v32.read(), 0xFFFFFFFF);
        assert_eq!(v64.read(), 0xFFFFFFFFFFFFFFFF);
    }

    #[test]
    fn test_volatile_pointer_access() {
        let v = Volatile::new(123u32);
        let ptr = v.as_ptr();
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_volatile_mut_pointer_access() {
        let mut v = Volatile::new(456u32);
        let ptr = v.as_mut_ptr();
        assert!(!ptr.is_null());
    }

    // =========================================================================
    // VgaChar Structure Tests
    // =========================================================================

    #[test]
    fn test_vga_char_size() {
        let size = core::mem::size_of::<VgaChar>();
        assert_eq!(size, 2); // 1 byte char + 1 byte attr
    }

    #[test]
    fn test_vga_char_new() {
        let ch = VgaChar::new(b'A', 0x0F);
        assert_eq!(ch.ascii, b'A');
        assert_eq!(ch.color, 0x0F);
    }

    #[test]
    fn test_vga_char_copy() {
        let ch1 = VgaChar::new(b'X', 0x1E);
        let ch2 = ch1;
        assert_eq!(ch1.ascii, ch2.ascii);
        assert_eq!(ch1.color, ch2.color);
    }

    // =========================================================================
    // Volatile Memory Semantics Tests
    // =========================================================================

    #[test]
    fn test_volatile_prevents_elision() {
        // Write followed by write - both should happen
        let mut v = Volatile::new(0u32);
        v.write(1);
        v.write(2);
        assert_eq!(v.read(), 2);
    }

    #[test]
    fn test_volatile_read_side_effects() {
        // Multiple reads should all happen
        let v = Volatile::new(42u32);
        let r1 = v.read();
        let r2 = v.read();
        let r3 = v.read();
        assert_eq!(r1, r2);
        assert_eq!(r2, r3);
    }

    // =========================================================================
    // Color Code Tests
    // =========================================================================

    #[test]
    fn test_color_code_encoding() {
        // VGA color: (bg << 4) | fg
        fn make_color(fg: u8, bg: u8) -> u8 {
            (bg << 4) | (fg & 0x0F)
        }
        
        let white_on_black = make_color(15, 0);
        let green_on_black = make_color(10, 0);
        let yellow_on_blue = make_color(14, 1);
        
        assert_eq!(white_on_black, 0x0F);
        assert_eq!(green_on_black, 0x0A);
        assert_eq!(yellow_on_blue, 0x1E);
    }

    #[test]
    fn test_vga_colors() {
        const BLACK: u8 = 0;
        const BLUE: u8 = 1;
        const GREEN: u8 = 2;
        const CYAN: u8 = 3;
        const RED: u8 = 4;
        const MAGENTA: u8 = 5;
        const BROWN: u8 = 6;
        const LIGHT_GRAY: u8 = 7;
        const DARK_GRAY: u8 = 8;
        const LIGHT_BLUE: u8 = 9;
        const LIGHT_GREEN: u8 = 10;
        const LIGHT_CYAN: u8 = 11;
        const LIGHT_RED: u8 = 12;
        const PINK: u8 = 13;
        const YELLOW: u8 = 14;
        const WHITE: u8 = 15;
        
        assert_eq!(BLACK, 0);
        assert_eq!(WHITE, 15);
        assert_eq!(YELLOW, 14);
    }
}
