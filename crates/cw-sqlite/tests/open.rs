//! Opening a writer, through the public API on temporary roots: the files
//! it creates and refuses, and the pragmas it sets.

#![cfg(unix)]

use cw_base::Kind;
use cw_sqlite::{Db, Error, Step};
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const MIGRATIONS: &[Step] = &[Step::sql("CREATE TABLE t (v INTEGER) STRICT;")];

fn db_in(root: &Path) -> Db {
    Db::new(root.join("store.db"), MIGRATIONS)
}

fn sidecar(db: &Db, suffix: &str) -> PathBuf {
    let mut name = db.path().as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn touch(db: &Db) -> Result<(), Error> {
    db.write(|_| Ok::<_, Error>(()))
}

fn mode(path: &Path) -> std::io::Result<u32> {
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o777)
}

#[test]
fn a_new_database_is_created_owner_only() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = db_in(root.path());
    touch(&db)?;
    assert_eq!(mode(db.path())?, 0o600);
    assert_eq!(mode(&sidecar(&db, "-wal"))?, 0o600);
    assert_eq!(mode(&sidecar(&db, "-shm"))?, 0o600);
    Ok(())
}

#[test]
fn a_database_that_is_a_symlink_is_refused() -> TestResult {
    let root = tempfile::tempdir()?;
    let real = db_in(&root.path().join("real"));
    std::fs::create_dir(root.path().join("real"))?;
    touch(&real)?;
    let db = db_in(root.path());
    symlink(real.path(), db.path())?;
    let err = touch(&db).err().ok_or("a symlinked database was opened")?;
    assert!(
        matches!(err, Error::File(cw_fs::Error::Symlink { .. })),
        "{err:?}"
    );
    assert_eq!(err.kind(), Kind::Refused);
    Ok(())
}

#[test]
fn a_sidecar_that_is_a_symlink_is_refused() -> TestResult {
    for suffix in ["-wal", "-shm"] {
        let root = tempfile::tempdir()?;
        let db = db_in(root.path());
        touch(&db)?;
        let elsewhere = root.path().join("elsewhere");
        std::fs::write(&elsewhere, b"")?;
        std::fs::remove_file(sidecar(&db, suffix))?;
        symlink(&elsewhere, sidecar(&db, suffix))?;
        let err = touch(&db).err().ok_or("a symlinked sidecar was opened")?;
        assert!(
            matches!(err, Error::File(cw_fs::Error::Symlink { .. })),
            "{suffix}: {err:?}"
        );
        let err = db.read().err().ok_or("a symlinked sidecar was read")?;
        assert_eq!(err.kind(), Kind::Refused, "{suffix}");
    }
    Ok(())
}

#[test]
fn a_group_or_other_permission_bit_is_refused() -> TestResult {
    for (suffix, bits) in [("", 0o640), ("", 0o604), ("-wal", 0o620), ("-shm", 0o602)] {
        let root = tempfile::tempdir()?;
        let db = db_in(root.path());
        touch(&db)?;
        let file = sidecar(&db, suffix);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(bits))?;
        let err = touch(&db).err().ok_or("an open file was accepted")?;
        assert!(
            matches!(err, Error::File(cw_fs::Error::Permissions { .. })),
            "{suffix} {bits:o}: {err:?}"
        );
        assert_eq!(err.kind(), Kind::Refused);
        assert_eq!(db.read().err().map(|e| e.kind()), Some(Kind::Refused));
    }
    Ok(())
}

#[test]
fn a_writer_runs_in_wal_with_full_sync_and_foreign_keys() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = db_in(root.path());
    let (journal, synchronous, foreign_keys) = db.write(|tx| {
        let read = |sql: &str| tx.query_row(sql, [], |row| row.get::<_, String>(0));
        Ok::<_, Box<dyn std::error::Error>>((
            read("PRAGMA journal_mode")?,
            tx.query_row("PRAGMA synchronous", [], |row| row.get::<_, i64>(0))?,
            tx.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))?,
        ))
    })?;
    assert_eq!(journal.to_ascii_lowercase(), "wal");
    assert_eq!(synchronous, 2, "synchronous is FULL");
    assert_eq!(foreign_keys, 1);
    Ok(())
}

#[test]
fn the_wal_is_kept_after_the_last_connection_closes() -> TestResult {
    let root = tempfile::tempdir()?;
    let db = db_in(root.path());
    db.write(|tx| {
        tx.execute("INSERT INTO t VALUES (1)", [])?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    drop(db.read()?);
    assert!(sidecar(&db, "-wal").exists());
    assert!(sidecar(&db, "-shm").exists());
    Ok(())
}
