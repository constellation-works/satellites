//! The temp root is private and is removed on drop.
#![allow(clippy::expect_used, reason = "tests assert fixture setup")]

use cw_testkit::TempRoot;

#[test]
fn temp_root_is_owner_only_and_gone_after_drop() {
    let root = TempRoot::new("CWTEST").expect("create temp root");
    let path = root.path().to_path_buf();
    assert!(path.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "temp root mode is {mode:o}");
    }
    drop(root);
    assert!(!path.exists(), "{} survived drop", path.display());
}
