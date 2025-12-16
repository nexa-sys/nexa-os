//! NKM format tests (from src/kmod/mod.rs)

use crate::kmod::{generate_nkm, NkmHeader, ModuleType};

#[test]
fn test_generate_and_parse_nkm() {
    let data = generate_nkm(
        "ext2",
        ModuleType::Filesystem,
        "1.0.0",
        "ext2 filesystem driver",
    );
    let header = NkmHeader::parse(&data).expect("parse failed");
    assert_eq!(header.name_str(), "ext2");
    assert_eq!(header.module_type(), ModuleType::Filesystem);
}
