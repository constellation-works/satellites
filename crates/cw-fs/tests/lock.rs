//! The lock through the public API, on temporary roots.
//!
//! These tests measure wall-clock time, so each has an explicit deadline and
//! `.config/nextest.toml` runs them alone.

use cw_fs::{Error, Holder, Lock, Result as FsResult};
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Set on a child this file starts, naming the lock file it is to hold.
const HOLD_ENV: &str = "CW_FS_TEST_HOLD_LOCK";

/// What the child prints once it holds the lock. libtest may print its own
/// text on the same line, so the parent looks for it anywhere in a line.
const READY: &str = "cw-fs-test-child-holds-the-lock";

/// The label the child records.
const CHILD_LABEL: &str = "child holder";

/// How long a waiter waits for a lock that will not be released.
const SHORT_WAIT: Duration = Duration::from_millis(100);

/// Not a test: the body of the child process [`hold_in_child`] starts. It
/// takes the lock, says so on stdout, and holds it until stdin closes.
#[test]
#[ignore = "run only as the child of hold_in_child"]
fn child_holds_the_lock_until_stdin_closes() -> TestResult {
    let Some(path) = std::env::var_os(HOLD_ENV) else {
        return Ok(());
    };
    let _lock = Lock::acquire(Path::new(&path), CHILD_LABEL, Duration::from_secs(10))?;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{READY}")?;
    out.flush()?;
    let mut line = String::new();
    while std::io::stdin().read_line(&mut line)? > 0 {}
    Ok(())
}

/// A child process holding the lock at `path`; it releases it and exits
/// when its stdin is closed.
fn hold_in_child(path: &Path) -> Result<Child, Box<dyn std::error::Error>> {
    let mut child = Command::new(std::env::current_exe()?)
        .args([
            "child_holds_the_lock_until_stdin_closes",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(HOLD_ENV, path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("no child stdout")?;
    let mut lines = BufReader::new(stdout).lines();
    loop {
        let Some(line) = lines.next() else {
            return Err("the child exited before taking the lock".into());
        };
        if line?.contains(READY) {
            // Keep reading, so the child's last lines never meet a closed
            // pipe.
            std::thread::spawn(move || lines.for_each(drop));
            return Ok(child);
        }
    }
}

fn release(mut child: Child) -> TestResult {
    drop(child.stdin.take());
    let status = child.wait()?;
    assert!(status.success(), "the child failed: {status}");
    Ok(())
}

fn locked_holder(result: FsResult<Lock>) -> Result<Option<Holder>, Box<dyn std::error::Error>> {
    match result {
        Err(Error::Locked { holder, .. }) => Ok(holder),
        Err(other) => Err(format!("expected Locked, got {other:?}").into()),
        Ok(_) => Err("the lock was taken while held".into()),
    }
}

#[test]
fn a_second_process_waits_and_times_out_naming_the_live_holder() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(".lock");
    let child = hold_in_child(&path)?;
    let child_pid = child.id();
    let started = Instant::now();
    let result = Lock::acquire(&path, "waiter", SHORT_WAIT);
    let waited = started.elapsed();
    let holder = locked_holder(result);
    release(child)?;
    let holder = holder?.ok_or("the live holder was not named")?;
    assert_eq!(
        (holder.pid, holder.label.as_str()),
        (child_pid, CHILD_LABEL)
    );
    assert!(waited >= SHORT_WAIT, "gave up after {waited:?}");
    // Released when the child exits.
    drop(Lock::acquire(&path, "after", Duration::from_secs(5))?);
    Ok(())
}

#[test]
fn the_holder_record_is_written_on_acquire_and_cleared_on_release() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(".lock");
    let lock = Lock::acquire(&path, "writer", Duration::ZERO)?;
    let record = std::fs::read_to_string(&path)?;
    assert!(
        record.contains(&format!("\"pid\":{}", std::process::id()))
            && record.contains("\"label\":\"writer\""),
        "record: {record:?}"
    );
    drop(lock);
    assert_eq!(std::fs::read_to_string(&path)?, "");
    Ok(())
}

#[test]
fn a_record_left_by_an_exited_process_is_not_named() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(".lock");
    // A process that has exited: this test binary, running no test.
    let mut gone = Command::new(std::env::current_exe()?)
        .args(["--exact", "no test has this name"])
        .stdout(Stdio::null())
        .spawn()?;
    let gone_pid = gone.id();
    assert!(gone.wait()?.success());
    std::fs::write(
        &path,
        format!(
            "{{\"pid\":{gone_pid},\"time\":\"2026-01-01T00:00:00Z\",\"label\":\"finished\"}}\n"
        ),
    )?;
    // Held outside `Lock`, as a holder that died before clearing would
    // leave it, but with the lock still taken.
    let held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)?;
    held.try_lock()?;
    let holder = locked_holder(Lock::acquire(&path, "waiter", SHORT_WAIT))?;
    assert_eq!(holder, None, "an exited process was named");
    Ok(())
}

#[test]
fn a_second_thread_of_the_same_process_waits() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(".lock");
    let lock = Lock::acquire(&path, "first thread", Duration::ZERO)?;
    let other = path.clone();
    let result = std::thread::spawn(move || {
        let started = Instant::now();
        let result = Lock::acquire(&other, "second thread", SHORT_WAIT).map(drop);
        (result, started.elapsed())
    })
    .join()
    .map_err(|_| "the second thread panicked")?;
    let (result, waited) = result;
    let err = result.err().ok_or("the second thread took a held lock")?;
    assert!(
        matches!(&err, Error::Locked { holder: Some(h), .. } if h.label == "first thread"),
        "got {err:?}"
    );
    assert!(waited >= SHORT_WAIT, "gave up after {waited:?}");
    drop(lock);
    let other = path.clone();
    std::thread::spawn(move || Lock::acquire(&other, "second thread", Duration::ZERO).map(drop))
        .join()
        .map_err(|_| "the second thread panicked")??;
    Ok(())
}

#[test]
fn the_same_thread_acquiring_a_held_lock_is_refused_at_once() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(".lock");
    let _lock = Lock::acquire(&path, "outer", Duration::ZERO)?;
    let deadline = Duration::from_secs(30);
    let started = Instant::now();
    let err = Lock::acquire(&path, "inner", deadline)
        .err()
        .ok_or("the lock was taken twice")?;
    let took = started.elapsed();
    assert!(matches!(err, Error::Reentrant { .. }), "got {err:?}");
    assert!(took < Duration::from_secs(5), "refused only after {took:?}");
    Ok(())
}

#[cfg(unix)]
mod unix {
    use super::TestResult;
    use cw_fs::{EntryKind, Error, Lock};
    use std::time::Duration;

    #[test]
    fn a_lock_file_that_is_a_symlink_is_refused() -> TestResult {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("precious");
        std::fs::write(&target, b"keep")?;
        let path = dir.path().join(".lock");
        std::os::unix::fs::symlink(&target, &path)?;
        let err = Lock::acquire(&path, "t", Duration::ZERO)
            .err()
            .ok_or("a linked lock was taken")?;
        assert!(matches!(err, Error::Symlink { .. }), "got {err:?}");
        assert_eq!(std::fs::read(&target)?, b"keep");
        Ok(())
    }

    #[test]
    fn a_lock_file_with_a_second_hard_link_is_refused() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join(".lock");
        drop(Lock::acquire(&path, "t", Duration::ZERO)?);
        std::fs::hard_link(&path, dir.path().join("alias"))?;
        let err = Lock::acquire(&path, "t", Duration::ZERO)
            .err()
            .ok_or("a hard-linked lock was taken")?;
        assert!(
            matches!(
                err,
                Error::NotRegular {
                    found: EntryKind::HardLinked { links: 2 },
                    ..
                }
            ),
            "got {err:?}"
        );
        Ok(())
    }
}
