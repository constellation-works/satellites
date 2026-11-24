//! A command that failed because another holder kept the program's own
//! lock still records its failure, within one deadline, because the audit
//! log has a lock of its own.
//!
//! Measures wall-clock time, so it runs alone (`.config/nextest.toml`).

use cw_app::{App, AuditMeta, Ctx, Origin, Runtime, Status, Surface, exec};
use cw_base::{Error, Kind};
use cw_fs::{Cap, Lock};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// How long the command waits for the program's lock.
const STORE_WAIT: Duration = Duration::from_millis(100);
/// How long the audit append may wait for the audit lock.
const AUDIT_WAIT: Duration = Duration::from_secs(1);
/// Both waits, with room for a slow runner: past this the audit line was
/// held up by the program's lock.
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

#[test]
fn a_command_blocked_on_the_program_lock_still_records_its_failure() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("data");
    cw_fs::create_private_dir(&root)?;
    let store_lock = root.join(".lock");

    // Another holder keeps the program's lock for the whole test.
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let path = store_lock.clone();
    let holder = std::thread::spawn(move || {
        let lock = Lock::acquire(&path, "other process", Duration::from_secs(5));
        let _ = held_tx.send(lock.is_ok());
        let _ = release_rx.recv();
        drop(lock);
    });
    if !held_rx.recv()? {
        return Err("the other holder could not take the program's lock".into());
    }

    let runtime = Runtime::new(&root, store_lock).with_audit_wait(AUDIT_WAIT);
    let origin = Origin::new(Surface::CLI, Some("u".to_owned()));
    let started = Instant::now();
    let (result, warning) = exec::<Store>(&runtime, &origin, ()).settle();
    let elapsed = started.elapsed();

    release_tx.send(())?;
    holder.join().map_err(|_| "the other holder panicked")?;

    assert_eq!(result.err().map(|e| e.kind()), Some(Kind::Busy));
    assert!(warning.is_none(), "audit line not written: {warning:?}");
    assert!(elapsed < DEADLINE, "took {elapsed:?}");
    let lines = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = lines.as_slice() else {
        panic!("one readable line expected, got {lines:?}");
    };
    assert_eq!(record.status, Status::Failure);
    assert_eq!(record.error_code.as_deref(), Some("store_busy"));
    Ok(())
}
