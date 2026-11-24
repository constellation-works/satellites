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

#[cfg(unix)]
#[test]
fn close_reports_a_tree_it_cannot_remove() {
    use std::os::unix::fs::PermissionsExt;

    let root = TempRoot::new("CWTEST").expect("create temp root");
    let locked = root.elsewhere("locked");
    std::fs::create_dir_all(&locked).expect("create locked dir");
    std::fs::write(locked.join("kept"), "x").expect("write file");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).expect("chmod");
    let path = root.path().to_path_buf();

    // A process that may write anyway (root) cannot show the failure.
    let privileged = std::fs::write(locked.join("probe"), "x").is_ok();
    let closed = root.close();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).ok();
    let _ = std::fs::remove_dir_all(&path);
    if privileged {
        return;
    }
    let err = closed.expect_err("close must report the read-only directory");
    assert!(matches!(err, cw_testkit::Error::Io { .. }), "{err:?}");
}
