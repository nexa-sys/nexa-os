//! fstab tests (from src/fs/fstab.rs)

use crate::fs::fstab::parse_fstab;

#[test]
fn test_parse_fstab() {
    let content = r#"
# Comment line
/dev/vda1   /       ext2    defaults    0   1
tmpfs       /tmp    tmpfs   size=64M    0   0
"#;
    let entries = parse_fstab(content);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].device, "/dev/vda1");
    assert_eq!(entries[0].mount_point, "/");
    assert_eq!(entries[1].fs_type, "tmpfs");
}
