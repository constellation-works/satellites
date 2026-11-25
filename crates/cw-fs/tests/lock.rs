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

// `Lock` stays unwind safe, so a caller can hold one across `catch_unwind`.
const _: () = {
    const fn unwind_safe<T: std::panic::UnwindSafe + std::panic::RefUnwindSafe>() {}
    unwind_safe::<Lock>();
};

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

/// How long the parent waits for the child to take the lock, or to exit.
const CHILD_DEADLINE: Duration = Duration::from_secs(10);

/// A child process holding a lock. Dropping it, on any path out of a test
/// (a panic included), kills the child and reaps it.
struct HeldByChild {
    child: Child,
}

impl Drop for HeldByChild {
    fn drop(&mut self) {
        // Both fail harmlessly when the child was already reaped.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A child process holding the lock at `path`, once it says so within
/// [`CHILD_DEADLINE`]; it releases the lock and exits when its stdin closes.
fn hold_in_child(path: &Path) -> Result<HeldByChild, Box<dyn std::error::Error>> {
    let mut held = HeldByChild {
        child: Command::new(std::env::current_exe()?)
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
            .spawn()?,
    };
    let stdout = held.child.stdout.take().ok_or("no child stdout")?;
    let (ready, readiness) = std::sync::mpsc::channel();
    // Reads every line, so the child's last lines never meet a closed pipe,
    // and reports the readiness line when it comes.
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) if line.contains(READY) => {
                    let _ = ready.send(());
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    readiness
        .recv_timeout(CHILD_DEADLINE)
        .map_err(|_| "the child did not take the lock in time")?;
    Ok(held)
}

/// Close the child's stdin and wait, within [`CHILD_DEADLINE`], for it to
/// release the lock and exit successfully.
fn release(mut held: HeldByChild) -> TestResult {
    drop(held.child.stdin.take());
    let deadline = Instant::now() + CHILD_DEADLINE;
    loop {
        if let Some(status) = held.child.try_wait()? {
            assert!(status.success(), "the child failed: {status}");
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("the child did not exit in time".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
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
    let child_pid = child.child.id();
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
    // A lock file must be private, as `Lock` itself creates it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
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

#[test]
fn a_waiter_never_takes_the_lock_after_its_deadline() -> TestResult {
    // The holder lets go a little before the waiter's deadline, while the
    // waiter is in its last sleep, which ends at the deadline or later. A
    // waiter that tried once more on waking would take the lock late.
    let wait = Duration::from_millis(30);
    let release_at = Duration::from_millis(22);
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(".lock");
    let (held, holding) = std::sync::mpsc::channel();
    let holder_path = path.clone();
    let holder = std::thread::spawn(move || -> Result<(), Error> {
        let lock = Lock::acquire(&holder_path, "holder", Duration::ZERO)?;
        let started = Instant::now();
        let _ = held.send(());
        std::thread::sleep(release_at.saturating_sub(started.elapsed()));
        drop(lock);
        Ok(())
    });
    holding.recv_timeout(Duration::from_secs(10))?;
    let started = Instant::now();
    let result = Lock::acquire(&path, "waiter", wait);
    let elapsed = started.elapsed();
    holder.join().map_err(|_| "the holder panicked")??;
    match result {
        Err(Error::Locked { .. }) => {}
        Ok(_) => assert!(
            elapsed < wait,
            "the lock was taken after the deadline ({elapsed:?})"
        ),
        Err(other) => return Err(format!("expected Locked, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn a_long_label_is_cut_so_a_waiter_can_still_read_the_holder() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(".lock");
    let label = "é".repeat(5000);
    let _lock = Lock::acquire(&path, &label, Duration::ZERO)?;
    assert!(
        std::fs::metadata(&path)?.len() <= 4096,
        "the record outgrew the cap"
    );
    let other = path.clone();
    let result = std::thread::spawn(move || Lock::acquire(&other, "waiter", SHORT_WAIT).map(drop))
        .join()
        .map_err(|_| "the waiter panicked")?;
    let err = result.err().ok_or("the waiter took a held lock")?;
    let Error::Locked {
        holder: Some(holder),
        ..
    } = err
    else {
        return Err(format!("expected Locked naming the holder, got {err:?}").into());
    };
    assert_eq!(holder.pid, std::process::id());
    assert!(
        !holder.label.is_empty()
            && label.starts_with(&holder.label)
            && holder.label.len() < label.len(),
        "the recorded label is not a cut of the one given ({} bytes)",
        holder.label.len()
    );
    Ok(())
}

/// Linux identifies a holder by its pid and start time, so a record
/// without a start time (an older build's) names nobody, even when its pid
/// is alive: here it is this very process.
#[cfg(target_os = "linux")]
#[test]
fn a_record_without_a_start_time_is_not_named_on_linux() -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(".lock");
    std::fs::write(
        &path,
        format!(
            "{{\"pid\":{},\"time\":\"2026-01-01T00:00:00Z\",\"label\":\"legacy\"}}\n",
            std::process::id()
        ),
    )?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)?;
    held.try_lock()?;
    let holder = locked_holder(Lock::acquire(&path, "waiter", SHORT_WAIT))?;
    assert_eq!(holder, None, "a record of unknown identity was named");
    Ok(())
}

/// Set on the child [`a_lock_whose_holder_record_cannot_be_written_is_still_acquired`]
/// starts, which runs with no room to write any file.
#[cfg(unix)]
const NO_ROOM_ENV: &str = "CW_FS_TEST_NO_ROOM_FOR_THE_RECORD";

/// Not a test: the body of that child. Every file write fails there (the
/// file-size limit is 0 and `SIGXFSZ` is ignored), so the lock is taken but
/// its holder record cannot be written.
#[cfg(unix)]
#[test]
#[ignore = "run only as the child of a_lock_whose_holder_record_cannot_be_written_is_still_acquired"]
fn child_acquires_with_no_room_for_the_record() -> TestResult {
    if std::env::var_os(NO_ROOM_ENV).is_none() {
        return Ok(());
    }
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(".lock");
    let lock = Lock::acquire(&path, "no room", Duration::ZERO)?;
    let failure = lock
        .holder_error()
        .ok_or("the record was written after all")?;
    assert!(matches!(failure, Error::Io { .. }), "got {failure:?}");
    // Held all the same: another thread waits and times out.
    let other = path.clone();
    let result = std::thread::spawn(move || Lock::acquire(&other, "waiter", SHORT_WAIT).map(drop))
        .join()
        .map_err(|_| "the waiter panicked")?;
    assert!(
        matches!(result, Err(Error::Locked { holder: None, .. })),
        "got {result:?}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_lock_whose_holder_record_cannot_be_written_is_still_acquired() -> TestResult {
    use std::io::Read as _;
    let name = "child_acquires_with_no_room_for_the_record";
    // `sh` sets the limit and ignores the signal, and both survive the exec
    // into this test binary.
    let mut child = HeldByChild {
        child: Command::new("/bin/sh")
            .args(["-c", "ulimit -f 0 && trap '' XFSZ && exec \"$0\" \"$@\""])
            .arg(std::env::current_exe()?)
            .args([name, "--exact", "--ignored", "--test-threads=1"])
            .env(NO_ROOM_ENV, "1")
            .stdout(Stdio::piped())
            .spawn()?,
    };
    let mut stdout = child.child.stdout.take().ok_or("no child stdout")?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        out
    });
    release(child)?;
    let out = reader.join().map_err(|_| "the reader panicked")?;
    // The filter matched and the child's assertions ran.
    assert!(out.contains("1 passed"), "the child ran no test: {out}");
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
