//! The strict read through the public API, on temporary roots, in
//! directories this process can write: it creates, rebuilds and changes no
//! file, refuses a database whose write-ahead log files are missing or need
//! recovery, and leaves a writer later in the same process to the rules the
//! docs state.

#![cfg(unix)]
#![expect(
    clippy::disallowed_methods,
    reason = "strict fixture: copies and chmods raw database and sidecar files, and opens one directly, to check what a strict read leaves alone"
)]
#![expect(
    clippy::disallowed_types,
    reason = "strict fixture: opens a raw database file directly, to check the bytes a strict read leaves alone"
)]

use cw_base::Kind;
use cw_sqlite::rusqlite::ErrorCode;
use cw_sqlite::rusqlite::config::DbConfig;
use cw_sqlite::{Db, Error, Reader, Step};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::time::{Duration, Instant};

mod child;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Boxed = Box<dyn std::error::Error>;

const MIGRATIONS: &[Step] = &[Step::sql("CREATE TABLE t (v INTEGER NOT NULL) STRICT;")];

fn insert(db: &Db, v: i64) -> Result<(), Boxed> {
    db.write(|tx| {
        tx.execute("INSERT INTO t VALUES (?1)", [v])?;
        Ok(())
    })
}

fn count(reader: &Reader) -> Result<i64, Boxed> {
    Ok(reader.query_row("SELECT count(*) FROM t", [], |row| row.get(0))?)
}

fn sidecar(db: &Db, suffix: &str) -> PathBuf {
    let mut name = db.path().as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Every entry in `dir` with its bytes and modification time.
fn snapshot(dir: &Path) -> std::io::Result<Vec<(String, Vec<u8>, SystemTime)>> {
    let mut entries = std::fs::read_dir(dir)?
        .map(|entry| {
            let entry = entry?;
            Ok((
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(entry.path())?,
                entry.metadata()?.modified()?,
            ))
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();
    Ok(entries)
}

/// Commit `v` and close every connection with the rows still in the `-wal`:
/// the state a database is in when its last connection dies, or closes
/// without a checkpoint. Both `-wal` and `-shm` stay.
fn commit_and_leave_wal(db: &Db, v: i64) -> TestResult {
    let conn = db.open_writer()?.into_connection();
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
    conn.execute("INSERT INTO t VALUES (?1)", [v])?;
    drop(conn);
    Ok(())
}

/// Every name in `dir`.
fn names(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut names = std::fs::read_dir(dir)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    Ok(names)
}

#[test]
fn a_strict_read_creates_and_changes_no_file() -> TestResult {
    // The files as a normal close leaves them, and as a close without a
    // checkpoint does, with committed rows only in the `-wal`.
    for leave_rows_in_wal in [false, true] {
        let root = tempfile::tempdir()?;
        let db = Db::new(root.path().join("store.db"), MIGRATIONS);
        insert(&db, 1)?;
        if leave_rows_in_wal {
            commit_and_leave_wal(&db, 2)?;
        }
        let rows = if leave_rows_in_wal { 2 } else { 1 };
        let before = snapshot(root.path())?;
        assert_eq!(
            names(root.path())?,
            ["store.db", "store.db-shm", "store.db-wal"]
        );

        let reader = db.read_strict()?;
        assert_eq!(count(&reader)?, rows, "committed rows are read");
        assert!(!reader.immutable());
        assert_eq!(reader.version(), 1);
        assert!(reader.execute("INSERT INTO t VALUES (9)", []).is_err());
        drop(reader);

        // Bytes, sizes, modification times and the names in the directory.
        assert_eq!(
            snapshot(root.path())?,
            before,
            "wal rows: {leave_rows_in_wal}"
        );
    }
    Ok(())
}

#[test]
fn a_strict_read_beside_a_live_writer_in_another_process_changes_no_file() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    insert(&db, 1)?;
    let env = [(DB_ENV, db.path().as_os_str())];
    let writer = child::Owned::spawn("child_holds_a_writer", &env)?;
    writer.wait_for(READY, Instant::now() + child::READY_WAIT)?;
    let before = snapshot(root.path())?;

    let reader = db.read_strict()?;
    assert_eq!(count(&reader)?, 2, "the writer's committed row is read");
    drop(reader);

    assert_eq!(snapshot(root.path())?, before);
    assert!(writer.finish(Instant::now() + Duration::from_secs(10))?);
    Ok(())
}

#[test]
fn a_strict_read_with_a_missing_wal_or_shm_is_refused_and_creates_nothing() -> TestResult {
    for missing in [&["-wal"][..], &["-shm"], &["-wal", "-shm"]] {
        let root = tempfile::tempdir()?;
        let db = Db::new(root.path().join("store.db"), MIGRATIONS);
        insert(&db, 1)?;
        // Committed rows only in the `-wal`: the main file alone is stale.
        commit_and_leave_wal(&db, 2)?;
        for suffix in missing {
            std::fs::remove_file(sidecar(&db, suffix))?;
        }
        let before = snapshot(root.path())?;

        let err = db.read_strict().err().ok_or("a strict read was served")?;
        let Error::WalUnavailable {
            path,
            missing: gone,
            source,
        } = &err
        else {
            return Err(format!("{missing:?}: {err:?}").into());
        };
        assert_eq!(path, db.path());
        let expected: Vec<PathBuf> = missing.iter().map(|s| sidecar(&db, s)).collect();
        assert_eq!(gone, &expected);
        assert!(source.is_none());
        assert_eq!(err.kind(), Kind::Refused);
        let message = err.to_string();
        assert!(
            message.contains(&db.path().display().to_string()),
            "{message}"
        );
        assert!(message.contains("missing or need recovery"), "{message}");

        // Nothing was created or changed, and no page was read.
        assert_eq!(snapshot(root.path())?, before, "{missing:?}");

        // An ordinary read creates the files, after which the strict read works.
        let ordinary = db.read()?;
        drop(ordinary);
        assert!(sidecar(&db, "-wal").exists() && sidecar(&db, "-shm").exists());
        assert!(count(&db.read_strict()?)? >= 1, "{missing:?}");
    }
    Ok(())
}

#[test]
fn a_strict_read_of_a_database_that_does_not_exist_or_holds_nothing_is_missing() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    let err = db
        .read_strict()
        .err()
        .ok_or("a missing database was read")?;
    assert!(matches!(err, Error::Missing { .. }), "{err:?}");
    assert_eq!(err.kind(), Kind::NotFound);
    assert!(names(root.path())?.is_empty());

    // What a first writer has made before its first step commits.
    std::fs::write(db.path(), b"")?;
    std::fs::set_permissions(db.path(), std::fs::Permissions::from_mode(0o600))?;
    let err = db.read_strict().err().ok_or("an empty database was read")?;
    assert!(matches!(err, Error::Missing { .. }), "{err:?}");
    assert_eq!(names(root.path())?, ["store.db"]);
    Ok(())
}

#[test]
fn a_strict_read_that_sqlite_cannot_open_without_recovery_is_refused() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    insert(&db, 1)?;
    let wal = sidecar(&db, "-wal");
    std::fs::set_permissions(&wal, std::fs::Permissions::from_mode(0o000))?;
    if std::fs::File::open(&wal).is_ok() {
        // The process ignores file modes (root): nothing to exercise.
        return Ok(());
    }
    let before = names(root.path())?;

    let err = db.read_strict().err().ok_or("a strict read was served")?;
    std::fs::set_permissions(&wal, std::fs::Permissions::from_mode(0o600))?;
    let Error::WalUnavailable {
        missing, source, ..
    } = &err
    else {
        return Err(format!("{err:?}").into());
    };
    assert!(missing.is_empty());
    assert!(source.is_some(), "SQLite's error is the source");
    assert_eq!(err.kind(), Kind::Refused);
    assert_eq!(names(root.path())?, before);
    Ok(())
}

#[test]
fn a_strict_read_does_not_fall_back_to_the_main_file_in_a_read_only_directory() -> TestResult {
    struct Restore<'a>(&'a Path);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(self.0, std::fs::Permissions::from_mode(0o700));
        }
    }

    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    insert(&db, 1)?;
    let copy_dir = root.path().join("copy");
    std::fs::create_dir(&copy_dir)?;
    let copy = Db::new(copy_dir.join("store.db"), MIGRATIONS);
    std::fs::copy(db.path(), copy.path())?;
    std::fs::set_permissions(&copy_dir, std::fs::Permissions::from_mode(0o500))?;
    let _restore = Restore(&copy_dir);

    // `read` opens the copy `immutable=1`; `read_strict` trusts no file alone.
    assert!(copy.read()?.immutable());
    let err = copy
        .read_strict()
        .err()
        .ok_or("the copy was read strictly")?;
    assert!(matches!(err, Error::WalUnavailable { .. }), "{err:?}");
    Ok(())
}

#[test]
fn a_newer_schema_is_refused_by_a_strict_read() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    insert(&db, 1)?;
    let older = Db::new(db.path(), &[]);
    let err = older.read_strict().err().ok_or("a newer schema was read")?;
    assert!(matches!(err, Error::Incompatible { .. }), "{err:?}");
    assert_eq!(err.kind(), Kind::Incompatible);
    Ok(())
}

#[test]
fn a_writer_in_the_same_process_is_refused_while_a_strict_reader_holds_the_index() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    insert(&db, 1)?;
    let reader = db.read_strict()?;

    // The index was mapped read-only first, so the write lock is refused,
    // through `write` and through a writer handle alike.
    let err = insert(&db, 2).err().ok_or("a write was committed")?;
    let Some(Error::Sqlite { source, .. }) = err.downcast_ref::<Error>() else {
        return Err(format!("{err:?}").into());
    };
    assert_eq!(source.sqlite_error_code(), Some(ErrorCode::ReadOnly));
    let writer = db.open_writer()?;
    assert!(writer.execute("INSERT INTO t VALUES (3)", []).is_err());
    drop(writer);
    assert_eq!(count(&reader)?, 1, "the reader is unaffected");
    drop(reader);

    // The index is mapped again, writable, once the process's last
    // connection to the database closed.
    insert(&db, 4)?;
    assert_eq!(count(&db.read()?)?, 2, "only the later write committed");
    Ok(())
}

#[test]
fn a_writer_before_a_strict_reader_in_the_same_process_keeps_the_index_writable() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    insert(&db, 1)?;
    let writer = db.open_writer()?;
    let reader = db.read_strict()?;
    writer.execute("INSERT INTO t VALUES (2)", [])?;
    assert_eq!(count(&reader)?, 1, "the reader keeps its snapshot");
    drop(reader);
    insert(&db, 3)?;
    drop(writer);
    assert_eq!(count(&db.read()?)?, 3);
    Ok(())
}

/// Set on a child this file starts: the database it opens.
const DB_ENV: &str = "CW_SQLITE_STRICT_DB";
/// What the child prints once its writer has committed a row.
const READY: &str = "cw-sqlite-strict-child-ready";

/// Not a test: the body of the child process
/// [`a_strict_read_beside_a_live_writer_in_another_process_changes_no_file`]
/// starts. It opens a writer, commits a row, says so, and holds the writer
/// until its stdin closes.
#[test]
#[ignore = "run only as a child of a_strict_read_beside_a_live_writer_in_another_process_changes_no_file"]
fn child_holds_a_writer() -> TestResult {
    use std::io::Write as _;
    let Some(path) = std::env::var_os(DB_ENV) else {
        return Ok(());
    };
    let writer = Db::new(Path::new(&path), MIGRATIONS).open_writer()?;
    writer.execute("INSERT INTO t VALUES (7)", [])?;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{READY}")?;
    out.flush()?;
    drop(out);
    child::until_stdin_closes()?;
    drop(writer);
    Ok(())
}
