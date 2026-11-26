//! The golden harness: compare, fail with a useful message, update, redact.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "tests assert fixture setup"
)]
#![expect(
    clippy::disallowed_methods,
    reason = "golden fixture: writes and reads raw golden files and runs the helper child that updates them"
)]

use cw_testkit::{Error, Goldens, TempRoot, UPDATE_VAR, redact_timestamps};

/// Where [`update_helper`] writes, passed by [`update_variable_rewrites_the_file`].
const HELPER_DIR: &str = "CWTEST_GOLDEN_DIR";
const HELPER: &str = "update_helper";
/// Set to `refused` when [`update_helper`] must see its update refused.
const HELPER_EXPECT: &str = "CWTEST_GOLDEN_EXPECT";

/// Run [`update_helper`] in a child with the update variable set, updating
/// the goldens in `dir`, and require the child to pass.
fn run_update_helper(root: &TempRoot, dir: &std::path::Path, expect: &str) {
    let out = root
        .command(std::env::current_exe().unwrap())
        .args(["--exact", HELPER, "--ignored", "--test-threads", "1"])
        .env(UPDATE_VAR, "1")
        .env(HELPER_DIR, dir)
        .env(HELPER_EXPECT, expect)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "helper failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

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
    run_update_helper(&root, &dir, "written");
    assert_eq!(
        std::fs::read_to_string(dir.join("out/list.txt")).unwrap(),
        "fresh\n"
    );
}

#[cfg(unix)]
#[test]
fn an_update_through_a_linked_goldens_folder_leaves_its_target_unchanged() {
    let root = TempRoot::new("CWTEST").unwrap();
    let sibling = root.elsewhere("sibling");
    std::fs::create_dir_all(sibling.join("out")).unwrap();
    std::fs::write(sibling.join("out/list.txt"), "sibling fixture\n").unwrap();
    let linked = root.elsewhere("goldens");
    std::os::unix::fs::symlink(&sibling, &linked).unwrap();

    run_update_helper(&root, &linked, "refused");
    assert_eq!(
        std::fs::read_to_string(sibling.join("out/list.txt")).unwrap(),
        "sibling fixture\n"
    );
}

/// Run by [`run_update_helper`] in a child; does nothing on its own.
#[test]
#[ignore = "run as a child by run_update_helper"]
fn update_helper() {
    let Some(dir) = std::env::var_os(HELPER_DIR) else {
        return;
    };
    let goldens = Goldens::new(dir);
    assert!(goldens.is_updating());
    if std::env::var_os(HELPER_EXPECT).is_some_and(|v| v == "refused") {
        let err = goldens.check("out/list.txt", "fresh\n").unwrap_err();
        assert!(matches!(err, Error::GoldenSymlink { .. }), "{err:?}");
    } else {
        goldens.assert("out/list.txt", "fresh\n");
    }
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
