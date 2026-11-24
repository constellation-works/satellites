//! A child built by the kit sees only what the kit set and the test named.
#![allow(
    clippy::expect_used,
    reason = "tests assert fixture setup and child output"
)]

use std::collections::BTreeMap;

use cw_testkit::TempRoot;

#[test]
fn child_sees_only_the_kit_variables_and_the_named_ones() {
    // Cargo and nextest set this in every test process, so the parent
    // certainly has a variable the child must not inherit.
    let inherited = "CARGO_MANIFEST_DIR";
    assert!(
        std::env::var_os(inherited).is_some(),
        "the parent test process has {inherited}"
    );

    let root = TempRoot::new("CWTEST").expect("create temp root");
    let out = root
        .command("env")
        .env("CWTEST_NAMED", "named by the test")
        .output()
        .expect("run env");
    assert!(out.status.success(), "env failed: {out:?}");

    let seen: BTreeMap<String, String> = String::from_utf8(out.stdout)
        .expect("utf-8 environment")
        .lines()
        .map(|line| {
            let (name, value) = line.split_once('=').expect("NAME=value");
            (name.to_owned(), value.to_owned())
        })
        .collect();
    let path = format!("{}:/usr/bin:/bin", root.bin().display());
    let expected = BTreeMap::from([
        ("HOME".to_owned(), root.home().display().to_string()),
        ("PATH".to_owned(), path),
        (
            "CWTEST_ROOT".to_owned(),
            root.app_root().display().to_string(),
        ),
        ("CWTEST_NAMED".to_owned(), "named by the test".to_owned()),
    ]);
    assert_eq!(seen, expected);
    assert!(!seen.contains_key(inherited));
}

#[test]
fn child_runs_in_the_temp_root_unless_the_test_says_otherwise() {
    let root = TempRoot::new("CWTEST").expect("create temp root");
    let cwd = |cmd: &mut std::process::Command| {
        let out = cmd.args(["-c", "pwd -P"]).output().expect("run sh");
        assert!(out.status.success(), "sh failed: {out:?}");
        std::path::PathBuf::from(String::from_utf8(out.stdout).expect("utf-8").trim_end())
    };
    let canonical = |p: std::path::PathBuf| p.canonicalize().expect("canonical path");

    assert_eq!(
        cwd(&mut root.command("sh")),
        canonical(root.path().to_path_buf())
    );
    assert_eq!(
        cwd(root.command("sh").current_dir(root.home())),
        canonical(root.home())
    );
}
