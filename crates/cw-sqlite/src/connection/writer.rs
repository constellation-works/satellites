//! Opening a read-write connection.

use super::{create_if_missing, get_pragma, resolve, set_pragma};
use crate::error::{Error, Result, is_busy};
use crate::migration;
use crate::migration::Step;
use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::time::{Duration, Instant};

/// How often the switch to WAL is retried while another connection holds a
/// new database.
const WAL_RETRY: Duration = Duration::from_millis(20);

/// A read-write connection to the database at `path`, created `0600` when
/// missing, configured and migrated to the end of `migrations`.
pub(crate) fn open(
    path: &Path,
    migrations: &[Step],
    wait: Duration,
    secure_delete: bool,
) -> Result<Connection> {
    create_if_missing(path)?;
    let mut conn = Connection::open_with_flags(
        resolve(path)?,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|e| Error::sqlite("open", path, e))?;
    let configure = |e| Error::sqlite("configure", path, e);
    set_pragma(&conn, "busy_timeout", millis(wait)).map_err(configure)?;
    let mut persist = 1;
    conn.file_control(
        rusqlite::DEFAULT_NAME,
        rusqlite::FileControl::PersistWal(&mut persist),
    )
    .map_err(configure)?;
    // A database a newer build migrated is refused before anything here
    // changes it, the journal mode included. Until it passes, closing this
    // connection must not checkpoint either: a checkpoint after WAL recovery
    // rewrites the main file, even with the pages it already holds.
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
        .map_err(configure)?;
    migration::refuse_newer(&conn, path, migrations)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)
        .map_err(configure)?;
    switch_to_wal(&conn, path, wait)?;
    set_pragma(&conn, "synchronous", "FULL").map_err(configure)?;
    set_pragma(&conn, "foreign_keys", "ON").map_err(configure)?;
    if secure_delete {
        set_pragma(&conn, "secure_delete", "ON").map_err(configure)?;
    }
    migration::run(&mut conn, path, migrations)?;
    Ok(conn)
}

/// Put the database in WAL mode, which stays set in the file. SQLite does
/// not call the busy handler for a journal-mode change, so while another
/// connection holds a new database the switch is retried until `wait` runs
/// out. A database that stays out of WAL is an error.
fn switch_to_wal(conn: &Connection, path: &Path, wait: Duration) -> Result<()> {
    let started = Instant::now();
    loop {
        let mode = get_pragma::<String>(conn, "journal_mode", None).and_then(|mode| {
            if mode.eq_ignore_ascii_case("wal") {
                Ok(mode)
            } else {
                get_pragma(conn, "journal_mode", Some("WAL"))
            }
        });
        match mode {
            Ok(mode) if mode.eq_ignore_ascii_case("wal") => return Ok(()),
            Ok(mode) => {
                return Err(Error::NotWal {
                    path: path.to_path_buf(),
                    mode,
                });
            }
            Err(e) if is_busy(&e) && started.elapsed() < wait => {
                std::thread::sleep(WAL_RETRY.min(wait.saturating_sub(started.elapsed())));
            }
            Err(e) => return Err(Error::sqlite("switch to WAL", path, e)),
        }
    }
}

/// `wait` in whole milliseconds, as `busy_timeout` takes it.
pub(crate) fn millis(wait: Duration) -> i32 {
    i32::try_from(wait.as_millis()).unwrap_or(i32::MAX)
}
