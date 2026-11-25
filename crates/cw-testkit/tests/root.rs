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

/// Set on the child a regression test reruns itself in, to the fixture the
/// parent arranged, which the child checks it received.
#[cfg(unix)]
const RERUN: &str = "CWTEST_ROOT_RERUN";

/// The explicit argument a rerun child gets: an extra name filter that
/// matches no test. A test takes its child branch only when it has both
/// this argument and [`RERUN`], so a marker inherited from the developer's
/// shell never skips the regression inputs.
#[cfg(unix)]
const CHILD_ARG: &str = "cwtest-root-rerun-child";

/// The fixture this process was rerun with, or `None` in the parent.
#[cfg(unix)]
fn rerun_fixture() -> Option<std::ffi::OsString> {
    if !std::env::args_os().any(|arg| arg == CHILD_ARG) {
        return None;
    }
    Some(std::env::var_os(RERUN).expect("a rerun child has its fixture marker"))
}

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

/// Rerun the test `name` in a kit child, in `cwd` if given, with `TMPDIR`
/// at `tmpdir` (created unless empty) and
/// [`RERUN`] at `fixture`, through `sh -c <script>` so the script can change
/// the child's process state first, and require that it passed within
/// [`CHILD_DEADLINE`]. The kit command starts from a cleared environment, so
/// any marker this process inherited is not passed on; only `fixture` is.
#[cfg(unix)]
fn rerun(
    outer: &TempRoot,
    name: &str,
    tmpdir: &std::path::Path,
    script: &str,
    fixture: &std::ffi::OsStr,
    cwd: Option<&std::path::Path>,
) {
    use std::process::Stdio;

    if !tmpdir.as_os_str().is_empty() {
        std::fs::create_dir_all(tmpdir).expect("create the child's TMPDIR");
    }
    let mut command = outer.command("sh");
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let mut child = Reaped(
        command
            .args(["-c", script])
            .arg(std::env::current_exe().expect("test binary"))
            .args(["--exact", name, CHILD_ARG, "--test-threads", "1"])
            .env("TMPDIR", tmpdir)
            .env(RERUN, fixture)
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
    let Some(fixture) = rerun_fixture() else {
        let outer = TempRoot::new("CWTEST").expect("create temp root");
        let tmpdir = outer.elsewhere("tmp:split");
        rerun(
            &outer,
            name,
            &tmpdir,
            r#"exec "$0" "$@""#,
            tmpdir.as_os_str(),
            None,
        );
        return;
    };
    // The child got the fixture: its temporary directory is the one with a
    // colon that the parent made.
    let tmpdir = std::env::temp_dir();
    assert_eq!(
        tmpdir.as_os_str(),
        fixture,
        "the child's temporary directory"
    );
    assert!(
        tmpdir.to_string_lossy().contains(':'),
        "{}",
        tmpdir.display()
    );
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
    let Some(fixture) = rerun_fixture() else {
        // 0277 leaves a new root 0500 and 0477 leaves it 0300 (writable
        // but not readable), so each needs the explicit mode change.
        let outer = TempRoot::new("CWTEST").expect("create temp root");
        for umask in ["0277", "0477"] {
            rerun(
                &outer,
                name,
                &outer.elsewhere(format!("tmp-{umask}")),
                &format!(r#"umask {umask} && exec "$0" "$@""#),
                umask.as_ref(),
                None,
            );
        }
        return;
    };
    // The child got the fixture: a directory it asks to be 0777 comes out
    // with the umask's bits cleared, so that umask is in force.
    let umask =
        u32::from_str_radix(fixture.to_str().expect("utf-8 fixture"), 8).expect("an octal umask");
    let probe = std::env::temp_dir().join("umask-probe");
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o777)
            .create(&probe)
            .expect("create the umask probe");
    }
    let probed = std::fs::metadata(&probe)
        .expect("stat")
        .permissions()
        .mode()
        & 0o777;
    std::fs::remove_dir(&probe).expect("remove the umask probe");
    assert_eq!(
        probed,
        0o777 & !umask,
        "the child's umask leaves {probed:o}"
    );
    let root = TempRoot::new("CWTEST").expect("create temp root under the umask");
    for dir in [root.path().to_path_buf(), root.home(), root.bin()] {
        let mode = std::fs::metadata(&dir).expect("stat").permissions().mode() & 0o777;
        assert_eq!(
            mode,
            0o700,
            "{} mode is {mode:o} under umask {umask:o}",
            dir.display()
        );
    }
}

#[cfg(unix)]
#[test]
fn a_temp_parent_others_could_replace_entries_in_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let name = "a_temp_parent_others_could_replace_entries_in_is_refused";
    let Some(fixture) = rerun_fixture() else {
        let outer = TempRoot::new("CWTEST").expect("create temp root");
        let open = outer.elsewhere("open");
        std::fs::create_dir_all(&open).expect("create the open parent");
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777))
            .expect("make the parent 0777 without the sticky bit");
        let script = r#"exec "$0" "$@""#;
        let fixture = |case: &str| {
            let mut value = std::ffi::OsString::from(format!("{case} "));
            value.push(&open);
            value
        };
        // TMPDIR names the open directory.
        rerun(&outer, name, &open, script, &fixture("named"), None);
        // An empty TMPDIR, run in the open directory: the check applies to
        // the current directory it resolves to.
        rerun(
            &outer,
            name,
            std::path::Path::new(""),
            script,
            &fixture("empty"),
            Some(&open),
        );
        // TMPDIR is private, and tempfile's own default is overridden to the
        // open directory: the root is made where the check looked.
        rerun(
            &outer,
            name,
            &outer.elsewhere("private"),
            script,
            &fixture("override"),
            None,
        );
        return;
    };
    let fixture = fixture.to_str().expect("utf-8 fixture");
    let (case, open) = fixture.split_once(' ').expect("case and path");
    let open = std::path::Path::new(open);
    let mode = std::fs::metadata(open).expect("stat").permissions().mode() & 0o7777;
    assert_eq!(mode, 0o777, "the open directory's mode");
    let canonical = |p: &std::path::Path| p.canonicalize().expect("canonical path");
    let parent = match case {
        "named" => std::env::temp_dir(),
        "empty" => {
            assert_eq!(std::env::temp_dir().as_os_str(), "", "TMPDIR is empty");
            let cwd = std::env::current_dir().expect("current directory");
            assert_eq!(
                canonical(&cwd),
                canonical(open),
                "run in the open directory"
            );
            cwd
        }
        "override" => {
            tempfile::env::override_temp_dir(open).expect("override tempfile's default");
            let parent = std::env::temp_dir();
            assert_ne!(
                canonical(&parent),
                canonical(open),
                "TMPDIR is not the open one"
            );
            let root = TempRoot::new("CWTEST").expect("a private TMPDIR is accepted");
            assert_eq!(
                root.path().parent(),
                Some(parent.as_path()),
                "made in TMPDIR"
            );
            let entries = std::fs::read_dir(open).expect("list the open dir").count();
            assert_eq!(entries, 0, "nothing was made in the override");
            return;
        }
        other => panic!("unknown case {other}"),
    };
    assert_eq!(
        canonical(&parent),
        canonical(open),
        "the child's temporary directory"
    );
    let err = TempRoot::new("CWTEST").expect_err("a replaceable parent must be refused");
    let cw_testkit::Error::ReplaceableTempParent { path } = &err else {
        panic!("unexpected error: {err:?}");
    };
    assert_eq!(path, &parent);
    assert!(
        err.to_string().contains(&parent.display().to_string()),
        "the message names the parent: {err}"
    );
    let entries = std::fs::read_dir(&parent).expect("list the parent").count();
    assert_eq!(entries, 0, "nothing was created in the refused parent");
}
