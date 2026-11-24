//! Running the steps a database has not had.

use super::Step;
use crate::connection::{get_pragma, set_pragma};
use crate::error::{Error, Result};
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;

/// How many steps `migrations` has: the newest version this build knows.
fn supported(migrations: &[Step]) -> u32 {
    u32::try_from(migrations.len()).unwrap_or(u32::MAX)
}

/// The database's `user_version`, refused as [`Error::Incompatible`] when it
/// is past the end of `migrations`.
pub(crate) fn refuse_newer(conn: &Connection, path: &Path, migrations: &[Step]) -> Result<u32> {
    let found: u32 = get_pragma(conn, "user_version", None)
        .map_err(|e| Error::sqlite("read the schema version of", path, e))?;
    refuse_past(path, found, migrations)
}

/// `found`, refused as [`Error::Incompatible`] when it is past the end of
/// `migrations`.
pub(crate) fn refuse_past(path: &Path, found: u32, migrations: &[Step]) -> Result<u32> {
    let supported = supported(migrations);
    if found > supported {
        return Err(Error::Incompatible {
            path: path.to_path_buf(),
            found,
            supported,
        });
    }
    Ok(found)
}

/// Run every step past the database's version, each in its own `IMMEDIATE`
/// transaction that re-reads the version under the write lock, so a step
/// another process applied meanwhile is not applied again.
pub(crate) fn run(conn: &mut Connection, path: &Path, migrations: &[Step]) -> Result<()> {
    loop {
        let version = refuse_newer(conn, path, migrations)?;
        let index = usize::try_from(version).unwrap_or(usize::MAX);
        let Some(step) = migrations.get(index) else {
            return Ok(());
        };
        // SQLite ignores `foreign_keys` inside a transaction, so the guard
        // goes around it, and is lifted whether the step committed or not.
        if step.runs_with_foreign_keys_off() {
            set_pragma(conn, "foreign_keys", "OFF")
                .map_err(|e| Error::sqlite("turn off foreign keys for a migration of", path, e))?;
        }
        let applied = apply_one(conn, path, migrations, version, step);
        if step.runs_with_foreign_keys_off() {
            let restored = set_pragma(conn, "foreign_keys", "ON")
                .map_err(|e| Error::sqlite("turn foreign keys back on in", path, e));
            applied?;
            restored?;
        } else {
            applied?;
        }
    }
}

/// Apply `step`, which moves the database from `expected` to `expected + 1`,
/// unless another connection moved it first.
fn apply_one(
    conn: &mut Connection,
    path: &Path,
    migrations: &[Step],
    expected: u32,
    step: &Step,
) -> Result<()> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| Error::sqlite("begin a migration of", path, e))?;
    let version = refuse_newer(&tx, path, migrations)?;
    if version != expected {
        // Another connection applied this step while this one waited for
        // the lock. Dropping the transaction rolls back nothing it did.
        return Ok(());
    }
    let next = expected.saturating_add(1);
    step.apply(&tx)
        .and_then(|()| set_pragma(&tx, "user_version", next))
        .map_err(|e| Error::migration(path, next, e))?;
    tx.commit()
        .map_err(|e| Error::sqlite("commit a migration of", path, e))
}
