//! Migrations through the public API, on temporary roots: which steps run,
//! racing openers, a failing step, the foreign-key guard, and a database a
//! newer build migrated.

use cw_base::Kind;
use cw_sqlite::rusqlite::Transaction;
use cw_sqlite::{Db, Error, Step};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
const RACE_ENV: &str = "CW_SQLITE_TEST_RACE_DB";
/// Set on the child: a file whose appearance starts it.
const GO_ENV: &str = "CW_SQLITE_TEST_RACE_GO";

/// Not a test: the body of each child process
/// [`two_processes_opening_a_new_database_apply_each_step_once`] starts. It
/// waits for the start file, then opens the database for a write.
#[test]
#[ignore = "run only as a child of two_processes_opening_a_new_database_apply_each_step_once"]
fn child_opens_the_database_when_told() -> TestResult {
    let (Some(path), Some(go)) = (std::env::var_os(RACE_ENV), std::env::var_os(GO_ENV)) else {
        return Ok(());
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !Path::new(&go).exists() {
        if Instant::now() > deadline {
            return Err("the parent never said go".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    touch(&Db::new(Path::new(&path), STEPS).lock_wait(Duration::from_secs(10)))?;
    Ok(())
}

#[test]
fn two_processes_opening_a_new_database_apply_each_step_once() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("store.db");
    let go = root.path().join("go");
    let spawn = || {
        Command::new(std::env::current_exe()?)
            .args([
                "child_opens_the_database_when_told",
                "--exact",
                "--ignored",
                "--test-threads=1",
            ])
            .env(RACE_ENV, &path)
            .env(GO_ENV, &go)
            .stdout(Stdio::null())
            .spawn()
    };
    let children = [spawn()?, spawn()?];
    // Both are waiting on the file; let them go together.
    std::thread::sleep(Duration::from_millis(200));
    std::fs::write(&go, b"")?;
    for child in children {
        let status = child.wait_with_output()?.status;
        assert!(status.success(), "a child failed: {status}");
    }
    assert_eq!(
        version_and_steps(&Db::new(&path, STEPS))?,
        (3, vec![1, 2, 3])
    );
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
