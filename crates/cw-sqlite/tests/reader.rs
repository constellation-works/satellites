//! The reader through the public API, on temporary roots: it creates and
//! changes no file, refuses a write, and reads a copy with no WAL through
//! `immutable=1`.

#![cfg(unix)]

use cw_base::Kind;
use cw_sqlite::{Db, Error, Step};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::time::SystemTime;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Boxed = Box<dyn std::error::Error>;

const MIGRATIONS: &[Step] = &[Step::sql("CREATE TABLE t (v INTEGER NOT NULL) STRICT;")];

fn insert(db: &Db, v: i64) -> Result<(), Boxed> {
    db.write(|tx| {
        tx.execute("INSERT INTO t VALUES (?1)", [v])?;
        Ok(())
    })
}

fn count(reader: &cw_sqlite::Reader) -> Result<i64, Boxed> {
    Ok(reader.query_row("SELECT count(*) FROM t", [], |row| row.get(0))?)
}

/// Every entry in `dir` with its size and modification time.
fn snapshot(dir: &Path) -> std::io::Result<Vec<(String, u64, SystemTime)>> {
    let mut entries = std::fs::read_dir(dir)?
        .map(|entry| {
            let entry = entry?;
            let meta = entry.metadata()?;
            Ok((
                entry.file_name().to_string_lossy().into_owned(),
                meta.len(),
                meta.modified()?,
            ))
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();
    Ok(entries)
}

#[test]
fn read_creates_and_changes_no_file() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);

    let err = db.read().err().ok_or("a missing database was read")?;
    assert!(matches!(err, Error::Missing { .. }), "{err:?}");
    assert_eq!(err.kind(), Kind::NotFound);
    assert_eq!(snapshot(root.path())?, vec![]);

    insert(&db, 1)?;
    let before = snapshot(root.path())?;
    let reader = db.read()?;
    assert_eq!(count(&reader)?, 1);
    drop(reader);
    assert_eq!(snapshot(root.path())?, before);
    Ok(())
}

#[test]
fn a_write_through_a_reader_is_refused() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    insert(&db, 1)?;
    let reader = db.read()?;
    assert!(reader.execute("INSERT INTO t VALUES (2)", []).is_err());
    assert!(reader.execute_batch("CREATE TABLE u (v INTEGER)").is_err());
    assert_eq!(count(&reader)?, 1);
    Ok(())
}

/// Restores a directory's mode when dropped, so the temporary root can be
/// removed even when an assertion fails.
struct Writable<'a>(&'a Path);

impl Drop for Writable<'_> {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(self.0, std::fs::Permissions::from_mode(0o700));
    }
}

#[test]
fn a_copy_with_no_wal_in_a_read_only_directory_is_read_as_immutable() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    insert(&db, 1)?;
    insert(&db, 2)?;

    let copy_dir = root.path().join("copy");
    std::fs::create_dir(&copy_dir)?;
    std::fs::set_permissions(&copy_dir, std::fs::Permissions::from_mode(0o700))?;
    let copy = Db::new(copy_dir.join("store.db"), MIGRATIONS);
    std::fs::copy(db.path(), copy.path())?;
    std::fs::set_permissions(&copy_dir, std::fs::Permissions::from_mode(0o500))?;
    let _restore = Writable(&copy_dir);

    let reader = copy.read()?;
    assert!(reader.immutable(), "the copy was read without the fallback");
    assert_eq!(count(&reader)?, 2);
    assert!(!db.read()?.immutable());
    Ok(())
}
