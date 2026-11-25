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

/// Set on the child a regression test reruns itself in; its value is what
/// the child checks.
#[cfg(unix)]
const RERUN: &str = "CWTEST_ROOT_RERUN";

/// How long a rerun child may take before the parent kills it and fails.
#[cfg(unix)]
const CHILD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

/// A child process, killed and reaped when dropped, so a test that fails or
/// times out never leaves it running.
#[cfg(unix)]
struct Reaped(std::process::Child);

#[cfg(unix)]
impl Drop for Reaped {
    fn drop(&mut self) {
        // Already exited on the clean path: both calls then do nothing.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Read all of `pipe` on a thread, so the child never blocks on a full pipe
/// while the parent waits with a deadline.
#[cfg(unix)]
fn drain(pipe: impl std::io::Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut pipe = pipe;
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut pipe, &mut text);
        text
    })
}

/// Rerun the test `name` in a kit child with `TMPDIR` at `tmpdir`, through
/// `sh -c <script>` so the script can change the child's process state
/// first, and require that it passed within [`CHILD_DEADLINE`].
#[cfg(unix)]
fn rerun(outer: &TempRoot, name: &str, tmpdir: &std::path::Path, script: &str) {
    use std::process::Stdio;

    std::fs::create_dir_all(tmpdir).expect("create the child's TMPDIR");
    let mut child = Reaped(
        outer
            .command("sh")
            .args(["-c", script])
            .arg(std::env::current_exe().expect("test binary"))
            .args(["--exact", name, "--test-threads", "1"])
            .env("TMPDIR", tmpdir)
            .env(RERUN, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("rerun the test"),
    );
    let stdout = drain(child.0.stdout.take().expect("child stdout"));
    let stderr = drain(child.0.stderr.take().expect("child stderr"));
    let deadline = std::time::Instant::now() + CHILD_DEADLINE;
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("poll the child") {
            break status;
        }
        // Failing here drops the guard, which kills and reaps the child.
        assert!(
            std::time::Instant::now() < deadline,
            "the child test ran past {CHILD_DEADLINE:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let stdout = stdout.join().expect("read child stdout");
    let stderr = stderr.join().expect("read child stderr");
    assert!(
        status.success() && stdout.contains("1 passed"),
        "child run ({status}): {stdout}{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn a_temp_root_whose_bin_path_would_split_path_is_refused() {
    let name = "a_temp_root_whose_bin_path_would_split_path_is_refused";
    if std::env::var_os(RERUN).is_none() {
        let outer = TempRoot::new("CWTEST").expect("create temp root");
        rerun(
            &outer,
            name,
            &outer.elsewhere("tmp:split"),
            r#"exec "$0" "$@""#,
        );
        return;
    }
    let err = TempRoot::new("CWTEST").expect_err("a bin path with ':' must be refused");
    let cw_testkit::Error::PathSeparatorInRoot { path } = &err else {
        panic!("unexpected error: {err:?}");
    };
    assert!(path.ends_with("bin"), "{}", path.display());
    assert!(
        err.to_string().contains(&path.display().to_string()),
        "the message names the path: {err}"
    );
}

#[cfg(unix)]
#[test]
fn the_kit_directories_are_owner_only_under_a_restrictive_umask() {
    use std::os::unix::fs::PermissionsExt;

    let name = "the_kit_directories_are_owner_only_under_a_restrictive_umask";
    if std::env::var_os(RERUN).is_none() {
        let outer = TempRoot::new("CWTEST").expect("create temp root");
        rerun(
            &outer,
            name,
            &outer.elsewhere("tmp"),
            r#"umask 0277 && exec "$0" "$@""#,
        );
        return;
    }
    let root = TempRoot::new("CWTEST").expect("create temp root under umask 0277");
    for dir in [root.path().to_path_buf(), root.home(), root.bin()] {
        let mode = std::fs::metadata(&dir).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{} mode is {mode:o}", dir.display());
    }
}
