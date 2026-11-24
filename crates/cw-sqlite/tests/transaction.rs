//! The write transaction through the public API, on temporary roots:
//! commit, rollback, a second writer waiting out the lock, and a read during
//! a write.
//!
//! The lock test measures wall-clock time, so it has an explicit deadline
//! and `.config/nextest.toml` runs this file's tests alone.

use cw_base::Kind;
use cw_sqlite::{Db, Error, Step};
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Boxed = Box<dyn std::error::Error>;

const MIGRATIONS: &[Step] = &[Step::sql("CREATE TABLE t (v INTEGER NOT NULL) STRICT;")];

/// Set on a child this file starts: the database it writes to.
const HOLD_ENV: &str = "CW_SQLITE_TEST_HOLD_WRITE";

/// What the child prints once its write transaction holds the lock, with a
/// row inserted and not committed.
const READY: &str = "cw-sqlite-test-child-holds-a-write";

/// How long the second writer waits for a lock that will not be released.
const SHORT_WAIT: Duration = Duration::from_millis(300);

fn values(db: &Db) -> Result<Vec<i64>, Boxed> {
    let reader = db.read()?;
    let mut statement = reader.prepare("SELECT v FROM t ORDER BY v")?;
    let rows = statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(rows)
}

fn insert(db: &Db, v: i64) -> Result<(), Boxed> {
    db.write(|tx| {
        tx.execute("INSERT INTO t VALUES (?1)", [v])?;
        Ok(())
    })
}

#[test]
fn write_commits_on_ok() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    insert(&db, 1)?;
    assert_eq!(values(&db)?, vec![1]);
    Ok(())
}

#[test]
fn write_rolls_back_on_err() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    insert(&db, 1)?;
    let result: Result<(), Boxed> = db.write(|tx| {
        tx.execute("INSERT INTO t VALUES (2)", [])?;
        Err("the caller changed its mind".into())
    });
    assert!(result.is_err());
    assert_eq!(values(&db)?, vec![1]);
    Ok(())
}

/// Not a test: the body of the child process [`hold_in_child`] starts. It
/// inserts a row in a write transaction, says so on stdout, and holds the
/// transaction open until stdin closes; then it rolls back.
#[test]
#[ignore = "run only as the child of hold_in_child"]
fn child_holds_a_write_until_stdin_closes() -> TestResult {
    let Some(path) = std::env::var_os(HOLD_ENV) else {
        return Ok(());
    };
    let db = Db::new(Path::new(&path), MIGRATIONS).lock_wait(Duration::from_secs(10));
    let held: Result<(), Boxed> = db.write(|tx| {
        tx.execute("INSERT INTO t VALUES (99)", [])?;
        let mut out = std::io::stdout().lock();
        writeln!(out, "{READY}")?;
        out.flush()?;
        let mut line = String::new();
        while std::io::stdin().read_line(&mut line)? > 0 {}
        Err("released without committing".into())
    });
    assert!(held.is_err());
    Ok(())
}

/// A child process holding a write transaction on the database at `path`;
/// it rolls back and exits when its stdin is closed.
fn hold_in_child(path: &Path) -> Result<Child, Boxed> {
    let mut child = Command::new(std::env::current_exe()?)
        .args([
            "child_holds_a_write_until_stdin_closes",
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
        let line = lines
            .next()
            .ok_or("the child exited before holding the write")??;
        if line.contains(READY) {
            // Keep reading, so the child's last lines meet an open pipe.
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

#[test]
fn a_second_writer_waits_the_lock_wait_then_fails_locked() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    insert(&Db::new(&path, MIGRATIONS), 1)?;
    let child = hold_in_child(&path)?;
    let started = Instant::now();
    let result = Db::new(&path, MIGRATIONS)
        .lock_wait(SHORT_WAIT)
        .write(|_| Ok::<_, Error>(()));
    let waited = started.elapsed();
    release(child)?;
    let err = result.err().ok_or("the second writer did not wait")?;
    assert!(matches!(err, Error::Locked { .. }), "{err:?}");
    assert_eq!(err.kind(), Kind::Busy);
    assert!(waited >= SHORT_WAIT, "gave up after {waited:?}");
    // Deadline: the wait plus a margin a busy runner stays within.
    assert!(
        waited < SHORT_WAIT + Duration::from_secs(3),
        "waited {waited:?}"
    );
    Ok(())
}

#[test]
fn a_read_during_a_write_sees_the_last_committed_state() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    let db = Db::new(&path, MIGRATIONS);
    insert(&db, 1)?;
    let child = hold_in_child(&path)?;
    let during = values(&db);
    release(child)?;
    assert_eq!(during?, vec![1]);
    assert_eq!(values(&db)?, vec![1]);
    Ok(())
}
