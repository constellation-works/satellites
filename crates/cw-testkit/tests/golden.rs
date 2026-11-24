//! The golden harness: compare, fail with a useful message, update, redact.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "tests assert fixture setup"
)]

use cw_testkit::{Error, Goldens, TempRoot, UPDATE_VAR, redact_timestamps};

/// Where [`update_helper`] writes, passed by [`update_variable_rewrites_the_file`].
const HELPER_DIR: &str = "CWTEST_GOLDEN_DIR";
const HELPER: &str = "update_helper";

/// The goldens of a test that compares, or `None` when this process runs
/// with the update variable set (someone regenerating a workspace's goldens),
/// where a comparison cannot be tested.
fn comparing_goldens_in(root: &TempRoot) -> Option<Goldens> {
    let dir = root.elsewhere("goldens");
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let goldens = Goldens::new(dir);
    (!goldens.is_updating()).then_some(goldens)
}

#[test]
fn matching_output_passes() {
    let root = TempRoot::new("CWTEST").unwrap();
    let Some(goldens) = comparing_goldens_in(&root) else {
        return;
    };
    std::fs::write(goldens.dir().join("out/list.txt"), "one\ntwo\n").unwrap();
    goldens.check("out/list.txt", "one\ntwo\n").expect("match");
}

#[test]
fn differing_output_fails_naming_the_file_and_the_update_variable() {
    let root = TempRoot::new("CWTEST").unwrap();
    let Some(goldens) = comparing_goldens_in(&root) else {
        return;
    };
    let file = goldens.dir().join("out/list.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();

    let err = goldens.check("out/list.txt", "one\nthree\n").unwrap_err();
    assert!(matches!(err, Error::GoldenMismatch { .. }), "{err:?}");
    let message = err.to_string();
    assert!(message.contains(&file.display().to_string()), "{message}");
    assert!(message.contains(&format!("{UPDATE_VAR}=1")), "{message}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n");
}

#[test]
fn update_variable_rewrites_the_file() {
    let root = TempRoot::new("CWTEST").unwrap();
    let dir = root.elsewhere("goldens");
    std::fs::create_dir_all(dir.join("out")).unwrap();
    std::fs::write(dir.join("out/list.txt"), "stale\n").unwrap();

    // The variable is read from the process environment, so the check runs
    // in a child: this test binary, running only the ignored helper below.
    let exe = std::env::current_exe().unwrap();
    let out = root
        .command(&exe)
        .args(["--exact", HELPER, "--ignored", "--test-threads", "1"])
        .env(UPDATE_VAR, "1")
        .env(HELPER_DIR, &dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "helper failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("out/list.txt")).unwrap(),
        "fresh\n"
    );
}

/// Run by [`update_variable_rewrites_the_file`] in a child; does nothing on
/// its own.
#[test]
#[ignore = "run as a child by update_variable_rewrites_the_file"]
fn update_helper() {
    let Some(dir) = std::env::var_os(HELPER_DIR) else {
        return;
    };
    let goldens = Goldens::new(dir);
    assert!(goldens.is_updating());
    goldens.assert("out/list.txt", "fresh\n");
}

#[test]
fn outputs_differing_only_in_timestamps_compare_equal() {
    let root = TempRoot::new("CWTEST").unwrap();
    let Some(goldens) = comparing_goldens_in(&root) else {
        return;
    };
    let first = "note 1 created 2026-01-02T03:04:05Z\nnote 2 at 2026-01-02T03:04:05.120+09:00\n";
    let second = "note 1 created 2031-12-30T23:59:58Z\nnote 2 at 2031-12-30T23:59:58.999-04:00\n";
    assert_ne!(first, second);
    std::fs::write(
        goldens.dir().join("out/notes.txt"),
        redact_timestamps(first),
    )
    .unwrap();
    goldens
        .check("out/notes.txt", &redact_timestamps(second))
        .expect("equal after redaction");
}
