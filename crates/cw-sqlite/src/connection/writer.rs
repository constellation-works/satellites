//! Opening a read-write connection.

use super::files::{header, sidecars};
use super::{create_if_missing, get_pragma, inspect, open_reader, resolve, set_pragma};
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
    let real = resolve(path)?;
    if inspect(&real)? {
        refuse_newer_without_writing(path, &real, migrations, wait)?;
    } else {
        create_if_missing(&real)?;
    }
    let mut conn = Connection::open_with_flags(
        &real,
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
    // Checked again on this connection, for a newer build that migrated
    // since the probe above, before anything here changes the database, the
    // journal mode included. Until it passes, closing this connection must
    // not checkpoint either: a checkpoint after WAL recovery rewrites the
    // main file, even with the pages it already holds.
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

/// Refuse a database a newer build migrated without opening it for writing,
/// which would create or rebuild its `-wal` and `-shm`. With nothing in the
/// WAL the main file's header holds the version; with WAL content and an
/// index, a [`super::Reader`] reads it and changes nothing. With WAL content
/// and no index there is no way to read it that writes nothing, so the check
/// is left to the read-write connection, which recreates the `-shm`.
fn refuse_newer_without_writing(
    path: &Path,
    real: &Path,
    migrations: &[Step],
    wait: Duration,
) -> Result<()> {
    let sidecars = sidecars(real)?;
    if !sidecars.wal_has_content {
        let found = header(real)?.map_or(0, |header| header.user_version);
        return migration::refuse_past(path, found, migrations).map(|_| ());
    }
    if sidecars.shm {
        return open_reader(path, migrations, wait).map(drop);
    }
    Ok(())
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
