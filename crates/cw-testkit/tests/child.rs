//! A child built by the kit sees only what the kit set and the test named.
#![allow(
    clippy::expect_used,
    reason = "tests assert fixture setup and child output"
)]

use std::collections::BTreeMap;

use cw_testkit::TempRoot;

/// The canonical parent of `path`, as git compares it.
fn canonical_parent(path: &std::path::Path) -> std::path::PathBuf {
    let canonical = path.canonicalize().expect("canonical path");
    canonical.parent().expect("a parent").to_path_buf()
}

/// Set on the child [`git_in_the_default_child_finds_no_enclosing_repository`]
/// starts with `TMPDIR` inside a repository.
const IN_REPO: &str = "CWTEST_TMPDIR_IN_REPO";

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
        (
            "GIT_CEILING_DIRECTORIES".to_owned(),
            canonical_parent(root.path()).display().to_string(),
        ),
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

#[test]
fn git_in_the_default_child_finds_no_enclosing_repository() {
    let name = "git_in_the_default_child_finds_no_enclosing_repository";
    if std::env::var_os(IN_REPO).is_none() {
        // Make a repository and rerun this test in a child whose TMPDIR is
        // inside it, so the child's temp roots are too.
        let outer = TempRoot::new("CWTEST").expect("create temp root");
        let repo = outer.elsewhere("repo");
        std::fs::create_dir_all(repo.join("tmp")).expect("create repo tmp");
        let init = outer
            .command("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .output()
            .expect("run git init");
        assert!(init.status.success(), "git init failed: {init:?}");
        let out = outer
            .command(std::env::current_exe().expect("test binary"))
            .args(["--exact", name, "--test-threads", "1"])
            .env("TMPDIR", repo.join("tmp"))
            .env(IN_REPO, &repo)
            .output()
            .expect("rerun the test");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "child run: {stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }

    let root = TempRoot::new("CWTEST").expect("create temp root");
    let repo = std::env::var_os(IN_REPO).expect("repository path");
    assert!(
        root.path()
            .canonicalize()
            .expect("canonical root")
            .starts_with(
                std::path::Path::new(&repo)
                    .canonicalize()
                    .expect("canonical repo")
            ),
        "the temp root is inside the repository"
    );
    let out = root
        .command("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("run git rev-parse");
    assert!(
        !out.status.success(),
        "git found a repository: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}
