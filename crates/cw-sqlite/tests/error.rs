//! Error classes through the public API, on temporary roots.

use cw_base::Kind;
use cw_sqlite::{Db, Error, Step};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const MIGRATIONS: &[Step] = &[Step::sql("CREATE TABLE t (v INTEGER NOT NULL) STRICT;")];

#[test]
fn a_file_that_is_not_a_database_is_corrupt() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    cw_fs::write_atomic(db.path(), &[b'x'; 4096])?;
    let err = db
        .write(|_| Ok::<_, Error>(()))
        .err()
        .ok_or("garbage was opened")?;
    assert!(matches!(err, Error::Corrupt { .. }), "{err:?}");
    assert_eq!(err.kind(), Kind::Corrupt);
    assert_eq!(db.read().err().map(|e| e.kind()), Some(Kind::Corrupt));
    Ok(())
}

#[test]
fn check_passes_on_a_healthy_database() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = Db::new(root.path().join("store.db"), MIGRATIONS);
    db.write(|tx| {
        tx.execute("INSERT INTO t VALUES (1)", [])?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    db.check()?;
    Ok(())
}
