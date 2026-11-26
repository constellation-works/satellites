//! The owned [`cw_sqlite::Writer`] handle and outside-transaction WAL
//! `TRUNCATE` checkpointing through the public API, on temporary roots.

#![expect(
    clippy::disallowed_methods,
    reason = "writer fixture: reads and changes the raw files a writer leaves, to check their bytes and modes"
)]

use cw_base::Kind;
use cw_sqlite::rusqlite::TransactionBehavior;
use cw_sqlite::{Db, Error, Step};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod child;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Boxed = Box<dyn std::error::Error>;

const MIGRATIONS: &[Step] = &[
    Step::sql("CREATE TABLE note (id INTEGER PRIMARY KEY, body TEXT NOT NULL) STRICT;"),
    Step::sql("CREATE INDEX note_body ON note (body);"),
];

/// Set on a child this file starts: the database it holds a write lock on.
const HOLD_ENV: &str = "CW_SQLITE_TEST_WRITER_HOLD";
/// What the child prints once it holds `BEGIN IMMEDIATE`.
const READY: &str = "cw-sqlite-test-writer-child-ready";
/// Short wait for lock-timeout assertions.
const SHORT_WAIT: Duration = Duration::from_millis(200);

fn sidecar(db: &Db, suffix: &str) -> PathBuf {
    let mut name = db.path().as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
#[ignore = "run only as the child of hold_write_in_child"]
fn child_holds_write_for_writer_tests() -> TestResult {
    let Some(path) = std::env::var_os(HOLD_ENV) else {
        return Ok(());
    };
    let db = Db::new(Path::new(&path), MIGRATIONS).lock_wait(Duration::from_secs(10));
    let held: Result<(), Boxed> = db.write(|tx| {
        tx.execute("INSERT INTO note (body) VALUES ('held')", [])?;
        let mut out = std::io::stdout().lock();
        writeln!(out, "{READY}")?;
        out.flush()?;
        drop(out);
        child::until_stdin_closes()?;
        Err("released without committing".into())
    });
    assert!(held.is_err());
    Ok(())
}

fn hold_write_in_child(path: &Path) -> Result<child::Owned, Boxed> {
    let held = child::Owned::spawn(
        "child_holds_write_for_writer_tests",
        &[(HOLD_ENV, path.as_os_str())],
    )?;
    held.wait_for(READY, Instant::now() + child::READY_WAIT)?;
    Ok(held)
}

fn release(held: child::Owned) -> TestResult {
    let exited = held.finish(Instant::now() + child::READY_WAIT)?;
    assert!(exited, "the child failed or did not exit");
    Ok(())
}

#[cfg(unix)]
#[test]
fn open_writer_enforces_owner_only_files_and_refuses_symlinks_and_open_modes() -> TestResult {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    let mode = |p: &Path| -> std::io::Result<u32> {
        Ok(std::fs::metadata(p)?.permissions().mode() & 0o777)
    };

    // Fresh database created 0600 with 0600 sidecars.
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    let writer = db.open_writer()?;
    assert_eq!(writer.version(), 2);
    drop(writer);
    assert_eq!(mode(db.path())?, 0o600);
    assert_eq!(mode(&sidecar(&db, "-wal"))?, 0o600);
    assert_eq!(mode(&sidecar(&db, "-shm"))?, 0o600);

    // Symlinked database is refused.
    let link_root = tempfile::tempdir()?;
    let link_db = Db::new(link_root.path().join("store.db"), MIGRATIONS);
    symlink(db.path(), link_db.path())?;
    let err = link_db
        .open_writer()
        .err()
        .ok_or("symlinked database was opened")?;
    assert!(
        matches!(err, Error::File(cw_fs::Error::Symlink { .. })),
        "{err:?}"
    );
    assert_eq!(err.kind(), Kind::Refused);

    // Symlinked sidecars and open permissions are refused.
    for suffix in ["-wal", "-shm"] {
        let dir = tempfile::tempdir()?;
        let target = Db::new(dir.path().join("store.db"), MIGRATIONS);
        drop(target.open_writer()?);
        let elsewhere = dir.path().join("elsewhere");
        std::fs::write(&elsewhere, b"")?;
        std::fs::remove_file(sidecar(&target, suffix))?;
        symlink(&elsewhere, sidecar(&target, suffix))?;
        let err = target
            .open_writer()
            .err()
            .ok_or("symlinked sidecar was opened")?;
        assert!(
            matches!(err, Error::File(cw_fs::Error::Symlink { .. })),
            "{suffix}: {err:?}"
        );
    }

    for (suffix, bits) in [("", 0o640), ("-wal", 0o620), ("-shm", 0o604)] {
        let dir = tempfile::tempdir()?;
        let target = Db::new(dir.path().join("store.db"), MIGRATIONS);
        drop(target.open_writer()?);
        let file = sidecar(&target, suffix);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(bits))?;
        let err = target
            .open_writer()
            .err()
            .ok_or("open-mode file was accepted")?;
        assert!(
            matches!(err, Error::File(cw_fs::Error::Permissions { .. })),
            "{suffix} {bits:o}: {err:?}"
        );
        assert_eq!(err.kind(), Kind::Refused);
    }

    Ok(())
}

#[test]
fn open_writer_configures_pragmas_persist_wal_and_optional_secure_delete() -> TestResult {
    let root = tempfile::tempdir()?;
    let default_db = Db::new(root.path().join("default.db"), MIGRATIONS);
    let writer = default_db.open_writer()?;
    assert!(writer.is_autocommit(), "returned outside any transaction");
    assert_eq!(writer.version(), 2);
    let journal: String = writer.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
    let synchronous: i64 = writer.query_row("PRAGMA synchronous", [], |r| r.get(0))?;
    let foreign_keys: i64 = writer.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    let secure_delete: i64 = writer.query_row("PRAGMA secure_delete", [], |r| r.get(0))?;
    assert_eq!(journal.to_ascii_lowercase(), "wal");
    assert_eq!(synchronous, 2);
    assert_eq!(foreign_keys, 1);
    assert_eq!(secure_delete, 0);
    drop(writer);
    assert!(sidecar(&default_db, "-wal").exists());
    assert!(sidecar(&default_db, "-shm").exists());

    let secure_db = Db::new(root.path().join("secure.db"), MIGRATIONS).secure_delete(true);
    let writer = secure_db.open_writer()?;
    let secure_delete: i64 = writer.query_row("PRAGMA secure_delete", [], |r| r.get(0))?;
    assert_eq!(secure_delete, 1);
    Ok(())
}

#[test]
fn open_writer_refuses_a_newer_database_without_modifying_files() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    Db::new(&path, MIGRATIONS).write(|tx| {
        tx.execute("INSERT INTO note (body) VALUES ('v2')", [])?;
        Ok::<_, Boxed>(())
    })?;
    let snapshot = |p: &Path| -> std::io::Result<Vec<(Vec<u8>, std::time::SystemTime)>> {
        ["", "-wal"]
            .into_iter()
            .map(|suffix| {
                let mut name = p.as_os_str().to_os_string();
                name.push(suffix);
                Ok((std::fs::read(&name)?, std::fs::metadata(&name)?.modified()?))
            })
            .collect()
    };
    let before = snapshot(&path)?;
    let older = Db::new(&path, &MIGRATIONS[..1]);
    let err = older
        .open_writer()
        .err()
        .ok_or("older build opened newer database")?;
    assert!(
        matches!(
            err,
            Error::Incompatible {
                found: 2,
                supported: 1,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(err.kind(), Kind::Incompatible);
    assert_eq!(snapshot(&path)?, before);
    Ok(())
}

#[test]
fn writer_supports_transactions_savepoints_and_into_connection() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    let mut writer = db.open_writer()?;
    assert!(writer.connection().is_autocommit());
    {
        let mut tx = writer
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO note (body) VALUES ('kept')", [])?;
        {
            let sp = tx.savepoint()?;
            sp.execute("INSERT INTO note (body) VALUES ('rolled back')", [])?;
            // Drop without commit to roll back savepoint.
        }
        tx.commit()?;
    }

    // Shared ownership through Rc<Writer> supports SQL savepoints and checkpoint_truncate.
    let shared = std::rc::Rc::new(writer);
    shared.execute_batch("BEGIN IMMEDIATE")?;
    shared.execute("INSERT INTO note (body) VALUES ('via rc')", [])?;
    shared.execute_batch("SAVEPOINT sp1")?;
    shared.execute("INSERT INTO note (body) VALUES ('sp1 rolled back')", [])?;
    shared.execute_batch("ROLLBACK TO sp1; RELEASE sp1; COMMIT;")?;
    shared.checkpoint_truncate()?;

    // Consume the Writer into a raw Connection and keep writing through it.
    let mut raw = std::rc::Rc::try_unwrap(shared)
        .map_err(|_| "expected sole Rc owner")?
        .into_connection();
    let tx = raw.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute("INSERT INTO note (body) VALUES ('via raw')", [])?;
    tx.commit()?;
    drop(raw);

    let reader = db.read()?;
    let mut stmt = reader.prepare("SELECT body FROM note ORDER BY id")?;
    let bodies = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(bodies, vec!["kept", "via rc", "via raw"]);
    Ok(())
}

#[test]
fn writer_honors_zero_and_per_wait_lock_wait_even_after_open_deadline_passes() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    Db::new(&path, MIGRATIONS).write(|_| Ok::<_, Error>(()))?;

    // Open a writer with a short lock_wait, then sleep past that lock_wait
    // before acquiring a contended write lock: per-wait busy handler must
    // still wait for lock_wait rather than failing immediately.
    let wait = Duration::from_millis(120);
    let writer = Db::new(&path, MIGRATIONS).lock_wait(wait).open_writer()?;
    std::thread::sleep(wait + Duration::from_millis(30));

    let child = hold_write_in_child(&path)?;
    let started = Instant::now();
    let res = writer.execute_batch("BEGIN IMMEDIATE");
    let waited = started.elapsed();

    // A zero-wait writer fails immediately without waiting.
    let zero_writer = Db::new(&path, MIGRATIONS)
        .lock_wait(Duration::ZERO)
        .open_writer()?;
    let zero_started = Instant::now();
    let zero_res = zero_writer.execute_batch("BEGIN IMMEDIATE");
    let zero_waited = zero_started.elapsed();
    release(child)?;

    let err = Error::sqlite(
        "begin write",
        &path,
        res.err().ok_or("contended BEGIN IMMEDIATE succeeded")?,
    );
    assert!(matches!(err, Error::Locked { .. }), "{err:?}");
    assert!(waited >= wait, "gave up too early after {waited:?}");
    assert!(
        waited < wait + Duration::from_secs(3),
        "waited too long: {waited:?}"
    );

    let zero_err = Error::sqlite(
        "begin write",
        &path,
        zero_res
            .err()
            .ok_or("zero-wait BEGIN IMMEDIATE succeeded")?,
    );
    assert!(matches!(zero_err, Error::Locked { .. }), "{zero_err:?}");
    assert!(
        zero_waited < Duration::from_secs(1),
        "zero wait took {zero_waited:?}"
    );
    Ok(())
}

#[test]
fn checkpoint_truncate_on_db_and_writer_clears_deleted_bytes_from_wal_and_db() -> TestResult {
    for via_writer in [false, true] {
        let root = tempfile::tempdir()?;
        let db = Db::new(root.path().join("store.db"), MIGRATIONS).secure_delete(true);
        let secret = "top-secret-redacted-text-0123456789abcdef";

        if via_writer {
            let mut writer = db.open_writer()?;
            {
                let tx = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
                tx.execute("INSERT INTO note (body) VALUES (?1)", [secret])?;
                tx.commit()?;
            }
            {
                let tx = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
                tx.execute("DELETE FROM note", [])?;
                tx.commit()?;
            }
            // Before checkpointing, the persisted -wal still holds the inserted page.
            let wal_before = std::fs::read(sidecar(&db, "-wal"))?;
            assert!(
                contains_bytes(&wal_before, secret.as_bytes()),
                "expected secret bytes in -wal before truncate checkpoint"
            );
            writer.checkpoint_truncate()?;
            drop(writer);
        } else {
            db.write(|tx| {
                tx.execute("INSERT INTO note (body) VALUES (?1)", [secret])?;
                Ok::<_, Boxed>(())
            })?;
            db.write(|tx| {
                tx.execute("DELETE FROM note", [])?;
                Ok::<_, Boxed>(())
            })?;
            let wal_before = std::fs::read(sidecar(&db, "-wal"))?;
            assert!(
                contains_bytes(&wal_before, secret.as_bytes()),
                "expected secret bytes in -wal before truncate checkpoint"
            );
            db.checkpoint_truncate()?;
        }

        let wal_after = std::fs::read(sidecar(&db, "-wal"))?;
        let main_after = std::fs::read(db.path())?;
        assert_eq!(
            wal_after.len(),
            0,
            "expected -wal to be truncated to 0 bytes (via_writer={via_writer})"
        );
        assert!(
            !contains_bytes(&wal_after, secret.as_bytes()),
            "secret still present in -wal"
        );
        assert!(
            !contains_bytes(&main_after, secret.as_bytes()),
            "secret still present in main database file"
        );
    }
    Ok(())
}

#[test]
fn checkpoint_truncate_edge_cases_missing_inside_tx_empty_and_non_wal() -> TestResult {
    let root = tempfile::tempdir()?;

    // Missing database returns Error::Missing and creates no file.
    let missing = Db::new(root.path().join("missing.db"), MIGRATIONS);
    let err = missing
        .checkpoint_truncate()
        .err()
        .ok_or("missing DB checkpoint succeeded")?;
    assert!(matches!(err, Error::Missing { .. }), "{err:?}");
    assert_eq!(err.kind(), Kind::NotFound);
    assert!(!missing.path().exists());

    // Calling checkpoint_truncate on a Writer inside an open transaction fails fast with Error::Sqlite.
    let db = Db::new(root.path().join("tx.db"), MIGRATIONS).lock_wait(Duration::from_secs(5));
    let writer = db.open_writer()?;
    writer.execute_batch("BEGIN IMMEDIATE")?;
    let started = Instant::now();
    let err = writer
        .checkpoint_truncate()
        .err()
        .ok_or("checkpoint inside transaction succeeded")?;
    assert!(matches!(err, Error::Sqlite { .. }), "{err:?}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "should fail fast without waiting for lock_wait"
    );
    writer.execute_batch("ROLLBACK")?;
    writer.checkpoint_truncate()?;
    drop(writer);

    // Brand-new 0-page WAL database (no migrations) succeeds cleanly.
    let empty = Db::new(root.path().join("empty.db"), &[]);
    let empty_writer = empty.open_writer()?;
    empty_writer.checkpoint_truncate()?;
    drop(empty_writer);

    // Manually switched out of WAL mode fails closed with Error::NotWal.
    let non_wal_writer = db.open_writer()?;
    let mode: String =
        non_wal_writer.query_row("PRAGMA journal_mode = DELETE", [], |r| r.get(0))?;
    assert_eq!(mode.to_ascii_lowercase(), "delete");
    let err = non_wal_writer
        .checkpoint_truncate()
        .err()
        .ok_or("checkpoint succeeded outside WAL mode")?;
    assert!(matches!(err, Error::NotWal { .. }), "{err:?}");
    Ok(())
}

#[test]
fn checkpoint_truncate_waits_for_blocking_reader_and_writer_and_retries_until_unblocked()
-> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    let db = Db::new(&path, MIGRATIONS);
    db.write(|tx| {
        tx.execute("INSERT INTO note (body) VALUES ('v1')", [])?;
        Ok::<_, Boxed>(())
    })?;

    // Blocked by a live reader past lock_wait -> Error::Locked on both Db and Writer.
    let reader = db.read()?;
    let short_db = db.clone().lock_wait(SHORT_WAIT);
    let started = Instant::now();
    let err = short_db
        .checkpoint_truncate()
        .err()
        .ok_or("Db::checkpoint_truncate succeeded while reader held snapshot")?;
    let waited = started.elapsed();
    assert!(matches!(err, Error::Locked { .. }), "{err:?}");
    assert_eq!(err.kind(), Kind::Busy);
    assert!(waited >= SHORT_WAIT, "gave up after {waited:?}");
    assert!(
        waited < SHORT_WAIT + Duration::from_secs(3),
        "waited {waited:?}"
    );

    let writer = short_db.open_writer()?;
    let started = Instant::now();
    let err = writer
        .checkpoint_truncate()
        .err()
        .ok_or("Writer::checkpoint_truncate succeeded while reader held snapshot")?;
    let waited = started.elapsed();
    assert!(matches!(err, Error::Locked { .. }), "{err:?}");
    assert!(waited >= SHORT_WAIT, "gave up after {waited:?}");
    assert!(
        waited < SHORT_WAIT + Duration::from_secs(3),
        "waited {waited:?}"
    );

    // Drop the reader mid-wait: retry succeeds and truncates -wal to 0 bytes.
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        drop(reader);
    });
    let long_writer = db.clone().lock_wait(Duration::from_secs(5)).open_writer()?;
    long_writer.checkpoint_truncate()?;
    releaser.join().map_err(|_| "releaser thread panicked")?;
    drop(long_writer);
    drop(writer);
    assert_eq!(std::fs::metadata(sidecar(&db, "-wal"))?.len(), 0);

    // Write again so -wal is non-empty, then block with another writer in a child process.
    db.write(|tx| {
        tx.execute("INSERT INTO note (body) VALUES ('v2')", [])?;
        Ok::<_, Boxed>(())
    })?;
    let child = hold_write_in_child(&path)?;
    let started = Instant::now();
    let err = short_db
        .checkpoint_truncate()
        .err()
        .ok_or("checkpoint succeeded while another writer held lock")?;
    let waited = started.elapsed();
    release(child)?;
    assert!(matches!(err, Error::Locked { .. }), "{err:?}");
    assert!(waited >= SHORT_WAIT, "gave up after {waited:?}");
    assert!(
        waited < SHORT_WAIT + Duration::from_secs(3),
        "waited {waited:?}"
    );
    Ok(())
}
