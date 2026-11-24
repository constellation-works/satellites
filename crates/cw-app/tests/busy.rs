//! A command that failed because another holder kept the program's own
//! lock still records its failure, within one deadline, because the audit
//! log has a lock of its own.
//!
//! The other holder is a second process. The test measures wall-clock
//! time, so it runs alone (`.config/nextest.toml`).

use cw_app::{App, AuditMeta, Ctx, Origin, Runtime, Status, Surface, exec};
use cw_base::{Error, Kind};
use cw_fs::{Cap, Lock};
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// How long the command waits for the program's lock.
const STORE_WAIT: Duration = Duration::from_millis(100);
/// How long the audit append may wait for the audit lock.
const AUDIT_WAIT: Duration = Duration::from_secs(1);
/// Both waits, with room for a slow runner: past this the audit line was
/// held up by the program's lock.
/// Set on a child this file starts, naming the lock file it is to hold.
const HOLD_ENV: &str = "CW_APP_TEST_HOLD_LOCK";

/// What the child prints once it holds the lock. libtest may print its own
/// text on the same line, so the parent looks for it anywhere in a line.
const READY: &str = "cw-app-test-child-holds-the-lock";

const DEADLINE: Duration = Duration::from_secs(3);

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
struct StoreError(cw_fs::Error);

impl From<StoreError> for Error {
    fn from(error: StoreError) -> Self {
        Error::new(error.0.kind(), "store_busy", error)
    }
}

struct Store;

impl App for Store {
    type State = PathBuf;
    type Command = ();
    type Outcome = ();
    type Error = StoreError;

    fn audit((): &()) -> Option<AuditMeta> {
        Some(AuditMeta::new("store write"))
    }

    fn run(ctx: &Ctx<'_, PathBuf>, (): ()) -> Result<(), StoreError> {
        let _lock = Lock::acquire(ctx.state(), "store write", STORE_WAIT).map_err(StoreError)?;
        Ok(())
    }

    fn target((): &()) -> Option<String> {
        None
    }
}

/// Not a test: the body of the child process [`hold_in_child`] starts. It
/// takes the program's lock, says so on stdout, and holds it until stdin
/// closes.
#[test]
#[ignore = "run only as the child of hold_in_child"]
fn child_holds_the_lock_until_stdin_closes() -> TestResult {
    let Some(path) = std::env::var_os(HOLD_ENV) else {
        return Ok(());
    };
    let _lock = Lock::acquire(Path::new(&path), "other process", Duration::from_secs(10))?;
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

#[test]
fn a_command_blocked_on_the_program_lock_still_records_its_failure() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("data");
    cw_fs::create_private_dir(&root)?;
    let store_lock = root.join(".lock");

    // Another process holds the program's lock until the line is checked.
    let mut holder = hold_in_child(&store_lock)?;

    let runtime = Runtime::new(&root, store_lock).with_audit_wait(AUDIT_WAIT);
    let origin = Origin::new(Surface::CLI, Some("u".to_owned()));
    let started = Instant::now();
    let (result, warning) = exec::<Store>(&runtime, &origin, ()).settle();
    let elapsed = started.elapsed();

    assert_eq!(result.err().map(|e| e.kind()), Some(Kind::Busy));
    assert!(warning.is_none(), "audit line not written: {warning:?}");
    assert!(elapsed < DEADLINE, "took {elapsed:?}");
    let lines = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = lines.as_slice() else {
        panic!("one readable line expected, got {lines:?}");
    };
    assert_eq!(record.status, Status::Failure);
    assert_eq!(record.error_code.as_deref(), Some("store_busy"));

    drop(holder.stdin.take());
    let status = holder.wait()?;
    assert!(status.success(), "the child failed: {status}");
    Ok(())
}
