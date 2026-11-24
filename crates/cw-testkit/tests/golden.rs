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

/// The goldens folder of a comparison test, inside its temp root.
fn goldens_in(root: &TempRoot) -> Goldens {
    let dir = root.elsewhere("goldens");
    std::fs::create_dir_all(dir.join("out")).unwrap();
    Goldens::new(dir)
}

/// Whether the comparison test `test` runs its body here. When this process
/// runs with the update variable set (someone regenerating a workspace's
/// goldens) it cannot compare, so it runs `test` again in a child, whose
/// environment the kit clears, requires that run to pass, and returns
/// `false`.
fn compares_here(test: &str) -> bool {
    if !Goldens::new("").is_updating() {
        return true;
    }
    let root = TempRoot::new("CWTEST").unwrap();
    let out = root
        .command(std::env::current_exe().unwrap())
        .args(["--exact", test, "--test-threads", "1"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "{test} in comparison mode: {stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    false
}

#[test]
fn matching_output_passes() {
    if !compares_here("matching_output_passes") {
        return;
    }
    let root = TempRoot::new("CWTEST").unwrap();
    let goldens = goldens_in(&root);
    std::fs::write(goldens.dir().join("out/list.txt"), "one\ntwo\n").unwrap();
    goldens.check("out/list.txt", "one\ntwo\n").expect("match");
}

#[test]
fn differing_output_fails_naming_the_file_and_the_update_variable() {
    if !compares_here("differing_output_fails_naming_the_file_and_the_update_variable") {
        return;
    }
    let root = TempRoot::new("CWTEST").unwrap();
    let goldens = goldens_in(&root);
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
    if !compares_here("outputs_differing_only_in_timestamps_compare_equal") {
        return;
    }
    let root = TempRoot::new("CWTEST").unwrap();
    let goldens = goldens_in(&root);
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

#[test]
fn lowercase_t_and_z_timestamps_are_redacted() {
    if !compares_here("lowercase_t_and_z_timestamps_are_redacted") {
        return;
    }
    let root = TempRoot::new("CWTEST").unwrap();
    let goldens = goldens_in(&root);
    let first = "at 2026-01-02t03:04:05z\n";
    let second = "at 2026-01-02t03:04:06Z\n";
    std::fs::write(goldens.dir().join("out/at.txt"), redact_timestamps(first)).unwrap();
    goldens
        .check("out/at.txt", &redact_timestamps(second))
        .expect("equal after redaction");
}

#[cfg(unix)]
#[test]
fn goldens_are_not_read_through_symbolic_links() {
    if !compares_here("goldens_are_not_read_through_symbolic_links") {
        return;
    }
    let root = TempRoot::new("CWTEST").unwrap();
    let goldens = goldens_in(&root);
    let outside = root.elsewhere("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("list.txt"), "sibling fixture\n").unwrap();
    std::os::unix::fs::symlink(outside.join("list.txt"), goldens.dir().join("out/list.txt"))
        .unwrap();
    std::os::unix::fs::symlink(&outside, goldens.dir().join("linked")).unwrap();

    for name in ["out/list.txt", "linked/list.txt"] {
        let err = goldens.check(name, "sibling fixture\n").unwrap_err();
        assert!(
            matches!(err, Error::GoldenSymlink { .. }),
            "{name}: {err:?}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(outside.join("list.txt")).unwrap(),
        "sibling fixture\n"
    );
}
