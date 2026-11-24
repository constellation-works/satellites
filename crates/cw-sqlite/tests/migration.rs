//! Migrations through the public API, on temporary roots: which steps run,
//! racing openers, a failing step, the foreign-key guard, and a database a
//! newer build migrated.

use cw_base::Kind;
use cw_sqlite::rusqlite::Transaction;
use cw_sqlite::{Db, Error, Step};
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

mod child;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Each step records itself, so a step applied twice shows as two rows.
const STEPS: &[Step] = &[
    Step::sql(
        "CREATE TABLE applied (step INTEGER NOT NULL) STRICT; INSERT INTO applied VALUES (1);",
    ),
    Step::sql("INSERT INTO applied VALUES (2);"),
    Step::sql("INSERT INTO applied VALUES (3);"),
];

fn version_and_steps(db: &Db) -> Result<(u32, Vec<i64>), Box<dyn std::error::Error>> {
    let reader = db.read()?;
    let mut statement = reader.prepare("SELECT step FROM applied ORDER BY step")?;
    let steps = statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;
    Ok((reader.version(), steps))
}

fn touch(db: &Db) -> Result<(), Error> {
    db.write(|_| Ok::<_, Error>(()))
}

/// Create the database at `path` at `version`: the first `version` steps.
fn stage(path: &Path, steps: &'static [Step], version: usize) -> Result<(), Error> {
    touch(&Db::new(path, &steps[..version]))
}

#[test]
fn a_fresh_database_runs_every_step() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), STEPS);
    touch(&db)?;
    assert_eq!(version_and_steps(&db)?, (3, vec![1, 2, 3]));
    Ok(())
}

#[test]
fn an_older_database_runs_only_the_later_steps() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    stage(&path, STEPS, 1)?;
    let db = Db::new(&path, STEPS);
    assert_eq!(version_and_steps(&db)?, (1, vec![1]));
    // Mark the staged state: a step run again would add a second 1.
    touch(&db)?;
    assert_eq!(version_and_steps(&db)?, (3, vec![1, 2, 3]));
    Ok(())
}

/// Set on a child this file starts: the database it opens.
const DB_ENV: &str = "CW_SQLITE_TEST_DB";
/// Set on a racing child: the directory the parent and the children
/// coordinate through.
const RACE_ENV: &str = "CW_SQLITE_TEST_RACE_DIR";
/// What a child prints once it is ready to be told to go.
const READY: &str = "cw-sqlite-test-child-ready";

/// The race's steps: the same as [`STEPS`], but the first holds its write
/// lock until the parent says to go on, so the other child reaches the
/// version check while the step is uncommitted and re-reads it after.
const RACE_STEPS: &[Step] = &[Step::function(hold_step_one), STEPS[1], STEPS[2]];

/// Step 1 for a racing child: apply it, record which process ran it, and
/// wait for the parent's `proceed` file before committing.
fn hold_step_one(tx: &Transaction<'_>) -> cw_sqlite::rusqlite::Result<()> {
    let failed = |e: Box<dyn std::error::Error + Send + Sync>| {
        cw_sqlite::rusqlite::Error::ToSqlConversionFailure(e)
    };
    tx.execute_batch(
        "CREATE TABLE applied (step INTEGER NOT NULL) STRICT; INSERT INTO applied VALUES (1);",
    )?;
    let Some(dir) = std::env::var_os(RACE_ENV) else {
        return Ok(());
    };
    let dir = Path::new(&dir);
    let marker = dir.join(format!("step-one-{}", std::process::id()));
    std::fs::write(marker, b"").map_err(|e| failed(e.into()))?;
    wait_for_file(&dir.join("proceed")).map_err(|e| failed(e.into()))
}

/// Wait until `path` exists, failing at a deadline.
fn wait_for_file(path: &Path) -> Result<(), String> {
    let deadline = Instant::now() + child::READY_WAIT;
    while !path.exists() {
        if Instant::now() > deadline {
            return Err(format!("{} never appeared", path.display()));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

/// Not a test: the body of each child process
/// [`two_processes_opening_a_new_database_apply_each_step_once`] starts. It
/// says it is ready, waits for the `go` file, then opens the database for a
/// write.
#[test]
#[ignore = "run only as a child of two_processes_opening_a_new_database_apply_each_step_once"]
fn child_opens_the_database_when_told() -> TestResult {
    let (Some(path), Some(dir)) = (std::env::var_os(DB_ENV), std::env::var_os(RACE_ENV)) else {
        return Ok(());
    };
    let mut out = std::io::stdout().lock();
    writeln!(out, "{READY}")?;
    out.flush()?;
    drop(out);
    wait_for_file(&Path::new(&dir).join("go"))?;
    touch(&Db::new(Path::new(&path), RACE_STEPS).lock_wait(Duration::from_secs(10)))?;
    Ok(())
}

#[test]
fn two_processes_opening_a_new_database_apply_each_step_once() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    let dir = root.path().join("race");
    std::fs::create_dir(&dir)?;
    let env = [(DB_ENV, path.as_os_str()), (RACE_ENV, dir.as_os_str())];
    let children = [
        child::Owned::spawn("child_opens_the_database_when_told", &env)?,
        child::Owned::spawn("child_opens_the_database_when_told", &env)?,
    ];
    let deadline = Instant::now() + child::READY_WAIT;
    for racer in &children {
        racer.wait_for(READY, deadline)?;
    }
    std::fs::write(dir.join("go"), b"")?;
    // One child is inside step 1, holding the write lock; give the other
    // time to read version 0 and wait on the lock, then let the first go on.
    while !std::fs::read_dir(&dir)?.any(|entry| {
        entry.is_ok_and(|entry| entry.file_name().to_string_lossy().starts_with("step-one-"))
    }) {
        if Instant::now() > deadline {
            return Err("neither child reached step 1".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    std::thread::sleep(Duration::from_millis(200));
    std::fs::write(dir.join("proceed"), b"")?;
    for racer in children {
        assert!(
            racer.finish(Instant::now() + child::READY_WAIT)?,
            "a child failed"
        );
    }
    let ran_step_one = std::fs::read_dir(&dir)?
        .filter(|entry| {
            entry
                .as_ref()
                .is_ok_and(|entry| entry.file_name().to_string_lossy().starts_with("step-one-"))
        })
        .count();
    assert_eq!(ran_step_one, 1, "step 1 ran in both children");
    assert_eq!(
        version_and_steps(&Db::new(&path, STEPS))?,
        (3, vec![1, 2, 3])
    );
    Ok(())
}

/// A newer build's steps: [`STEPS`], whose third step holds the write lock
/// until stdin closes.
const NEWER_STEPS: &[Step] = &[STEPS[0], STEPS[1], Step::function(hold_step_three)];

fn hold_step_three(tx: &Transaction<'_>) -> cw_sqlite::rusqlite::Result<()> {
    let failed = |e: std::io::Error| cw_sqlite::rusqlite::Error::ToSqlConversionFailure(e.into());
    let mut out = std::io::stdout().lock();
    writeln!(out, "{READY}")
        .and_then(|()| out.flush())
        .map_err(failed)?;
    drop(out);
    child::until_stdin_closes().map_err(failed)?;
    tx.execute_batch("INSERT INTO applied VALUES (3);")
}

/// Not a test: the body of the child process
/// [`a_write_is_refused_when_a_newer_build_migrates_while_it_waits`] starts:
/// a newer build migrating to version 3, holding its lock in step 3.
#[test]
#[ignore = "run only as the child of a_write_is_refused_when_a_newer_build_migrates_while_it_waits"]
fn child_migrates_to_three_until_stdin_closes() -> TestResult {
    let Some(path) = std::env::var_os(DB_ENV) else {
        return Ok(());
    };
    touch(&Db::new(Path::new(&path), NEWER_STEPS).lock_wait(Duration::from_secs(10)))?;
    Ok(())
}

#[test]
fn a_write_is_refused_when_a_newer_build_migrates_while_it_waits() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    stage(&path, STEPS, 2)?;
    let mut newer = child::Owned::spawn(
        "child_migrates_to_three_until_stdin_closes",
        &[(DB_ENV, path.as_os_str())],
    )?;
    newer.wait_for(READY, Instant::now() + child::READY_WAIT)?;
    // The older build opens while version 3 is uncommitted, so it sees 2
    // and waits on the write lock; the newer one commits meanwhile.
    let stdin = newer.take_stdin();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        drop(stdin);
    });
    let mut ran = false;
    let result = Db::new(&path, &STEPS[..2])
        .lock_wait(Duration::from_secs(10))
        .write(|_| {
            ran = true;
            Ok::<_, Error>(())
        });
    release.join().map_err(|_| "the release thread panicked")?;
    assert!(
        newer.finish(Instant::now() + child::READY_WAIT)?,
        "the newer build failed"
    );
    let err = result.err().ok_or("the older build wrote to version 3")?;
    assert_eq!(err.kind(), Kind::Incompatible, "{err:?}");
    assert!(!ran, "the callback ran against a newer schema");
    Ok(())
}

const ENDS_ITS_TRANSACTION: &[Step] = &[
    STEPS[0],
    Step::sql("INSERT INTO applied VALUES (2); COMMIT;"),
];

#[test]
fn a_step_that_ends_its_transaction_fails_and_the_version_stays() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    stage(&path, ENDS_ITS_TRANSACTION, 1)?;
    let db = Db::new(&path, ENDS_ITS_TRANSACTION);
    let err = touch(&db)
        .err()
        .ok_or("a step that committed itself was accepted")?;
    assert!(matches!(err, Error::Migration { step: 2, .. }), "{err:?}");
    assert_eq!(db.read()?.version(), 1);
    Ok(())
}

fn fail_after_a_change(tx: &Transaction<'_>) -> cw_sqlite::rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE half (v INTEGER) STRICT; INSERT INTO applied VALUES (2);")?;
    tx.execute_batch("THIS IS NOT SQL")
}

const FAILING: &[Step] = &[STEPS[0], Step::function(fail_after_a_change)];

fn schema(db: &Db) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let reader = db.read()?;
    let mut statement = reader.prepare("SELECT sql FROM sqlite_schema ORDER BY name")?;
    let rows = statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(rows)
}

#[test]
fn a_failing_step_leaves_the_version_and_the_schema_as_they_were() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    stage(&path, FAILING, 1)?;
    let db = Db::new(&path, FAILING);
    let before = (version_and_steps(&db)?, schema(&db)?);
    let err = touch(&db).err().ok_or("the failing step succeeded")?;
    assert!(matches!(err, Error::Migration { step: 2, .. }), "{err:?}");
    assert_eq!((version_and_steps(&db)?, schema(&db)?), before);
    Ok(())
}

const FOREIGN_KEYS: &[Step] = &[Step::sql(
    "CREATE TABLE seen (foreign_keys INTEGER NOT NULL) STRICT;
     INSERT INTO seen SELECT foreign_keys FROM pragma_foreign_keys;",
)
.foreign_keys_off()];

#[test]
fn a_step_can_run_with_foreign_keys_off_and_they_are_on_again_after() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), FOREIGN_KEYS);
    let (during, after) = db.write(|tx| {
        let during: i64 = tx.query_row("SELECT foreign_keys FROM seen", [], |row| row.get(0))?;
        let after: i64 = tx.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        Ok::<_, Box<dyn std::error::Error>>((during, after))
    })?;
    assert_eq!((during, after), (0, 1));
    Ok(())
}

#[test]
fn a_database_newer_than_the_steps_is_refused_and_left_unchanged() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    stage(&path, STEPS, 3)?;
    let files = |path: &Path| -> std::io::Result<Vec<(Vec<u8>, std::time::SystemTime)>> {
        ["", "-wal"]
            .into_iter()
            .map(|suffix| {
                let mut name = path.as_os_str().to_os_string();
                name.push(suffix);
                Ok((std::fs::read(&name)?, std::fs::metadata(&name)?.modified()?))
            })
            .collect()
    };
    let before = files(&path)?;
    let older = Db::new(&path, &STEPS[..2]);
    let write = touch(&older).err().ok_or("an older build wrote")?;
    assert_eq!(write.kind(), Kind::Incompatible);
    assert!(
        matches!(
            write,
            Error::Incompatible {
                found: 3,
                supported: 2,
                ..
            }
        ),
        "{write:?}"
    );
    let read = older.read().err().ok_or("an older build read")?;
    assert_eq!(read.kind(), Kind::Incompatible);
    assert_eq!(files(&path)?, before);
    Ok(())
}
