//! Safety module tests

use crate::safety::{layout_of, layout_array};

#[test]
fn test_layout_of() {
    let layout = layout_of::<u64>();
    assert_eq!(layout.size(), 8);
    assert_eq!(layout.align(), 8);
}

#[test]
fn test_layout_array() {
    let layout = layout_array::<u32>(10).unwrap();
    assert_eq!(layout.size(), 40);
    assert_eq!(layout.align(), 4);
}

#[test]
fn test_layout_array_zero() {
    let layout = layout_array::<u64>(0).unwrap();
    assert_eq!(layout.size(), 0);
}
