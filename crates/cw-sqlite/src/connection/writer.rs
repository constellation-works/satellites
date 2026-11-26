//! Opening a read-write connection, the owned [`Writer`] handle, and WAL
//! `TRUNCATE` checkpointing.

use super::deadline::{Deadline, DeadlineSlot};
use super::reader::open_resolved;
use super::{create_if_missing, get_pragma, inspect, resolve, set_pragma};
use crate::error::{Error, Result, checkpoint_busy, inside_transaction, is_busy};
use crate::migration;
use crate::migration::Step;
use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often the switch to WAL is retried while another connection holds a
/// new database.
const WAL_RETRY: Duration = Duration::from_millis(20);

/// An owned, configured, migrated read-write connection, from
/// [`crate::Db::open_writer`]. It dereferences to the [`Connection`].
///
/// Use [`crate::Db::write`] for a single `IMMEDIATE` write transaction. Use
/// `Writer` when a caller needs to manage its own transaction scope across
/// savepoints, in-transaction reads, or post-commit WAL truncation.
///
/// The connection is returned outside any transaction (autocommit mode), with
/// WAL mode, `synchronous = FULL`, `foreign_keys = ON`, `PersistWal`, optional
/// `secure_delete`, and all pending migrations applied. While opening, every
/// wait shares one call deadline; once returned, the connection keeps a
/// clock-bounded busy handler that gives each subsequent lock wait up to the
/// database's `lock_wait`. Do not replace the busy handler or set
/// `busy_timeout`.
#[derive(Debug)]
pub struct Writer {
    conn: Connection,
    path: PathBuf,
    lock_wait: Duration,
    version: u32,
    busy_deadline: DeadlineSlot,
}

impl Writer {
    /// The database's schema version (`PRAGMA user_version`) after opening and
    /// applying migrations.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The underlying connection.
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Mutable access to the underlying connection, for callers that start
    /// `rusqlite` transactions or savepoints directly.
    #[must_use]
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Consume this handle and return the configured, migrated [`Connection`].
    ///
    /// The connection keeps its pragmas and clock-bounded per-wait busy
    /// handler.
    #[must_use]
    pub fn into_connection(self) -> Connection {
        self.conn
    }

    /// Run `PRAGMA wal_checkpoint(TRUNCATE)` outside any transaction, retrying
    /// while `busy != 0` until the database's `lock_wait` expires by the clock.
    ///
    /// For callers with [`crate::Db::secure_delete`] enabled that need erased
    /// pages physically cleared from the persisted `-wal` file after commit.
    ///
    /// # Errors
    ///
    /// [`Error::Locked`] when another connection (a reader or a writer) blocks
    /// truncation past `lock_wait`; [`Error::NotWal`] when the connection is
    /// no longer in WAL mode; [`Error::Sqlite`] when called while this
    /// connection has an open transaction or on any other SQLite failure;
    /// [`Error::Corrupt`] if SQLite reports damage.
    pub fn checkpoint_truncate(&self) -> Result<()> {
        let deadline = Deadline::after(self.lock_wait);
        let _guard = self.busy_deadline.override_with(deadline);
        checkpoint_truncate(&self.conn, &self.path, deadline)
    }
}

impl Deref for Writer {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        &self.conn
    }
}

impl DerefMut for Writer {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

/// What opening a writer does when the database file does not exist yet.
#[derive(Clone, Copy)]
enum Missing {
    /// Create the file `0600` before SQLite opens it.
    Create,
    /// Fail with [`Error::Missing`] without creating any file.
    Refuse,
}

/// A read-write connection to the database at `path`, created `0600` when
/// missing, configured and migrated to the end of `migrations`. Keeps
/// `deadline` installed as the connection's busy handler for the rest of the
/// caller's single call.
pub(crate) fn open(
    path: &Path,
    migrations: &[Step],
    deadline: Deadline,
    secure_delete: bool,
) -> Result<Connection> {
    let (conn, _version) = open_with(path, migrations, deadline, secure_delete, Missing::Create)?;
    Ok(conn)
}

/// A read-write connection to an existing database at `path`, refused with
/// [`Error::Missing`] when absent, configured and migrated to the end of
/// `migrations`. Keeps `deadline` installed for the rest of the call.
pub(crate) fn open_existing(
    path: &Path,
    migrations: &[Step],
    deadline: Deadline,
    secure_delete: bool,
) -> Result<Connection> {
    let (conn, _version) = open_with(path, migrations, deadline, secure_delete, Missing::Refuse)?;
    Ok(conn)
}

/// An owned [`Writer`] handle to the database at `path`, created `0600` when
/// missing, configured and migrated under one `lock_wait` deadline, then
/// switched to a per-wait clock-bounded busy handler for later caller use.
pub(crate) fn open_handle(
    path: &Path,
    migrations: &[Step],
    lock_wait: Duration,
    secure_delete: bool,
) -> Result<Writer> {
    let deadline = Deadline::after(lock_wait);
    let (mut conn, version) =
        open_with(path, migrations, deadline, secure_delete, Missing::Create)?;
    let busy_deadline = DeadlineSlot::install(lock_wait, &mut conn)
        .map_err(|e| Error::sqlite("configure", path, e))?;
    Ok(Writer {
        conn,
        path: path.to_path_buf(),
        lock_wait,
        version,
        busy_deadline,
    })
}

/// Shared open/inspect/configure/migrate path for all writers.
fn open_with(
    path: &Path,
    migrations: &[Step],
    deadline: Deadline,
    secure_delete: bool,
    missing: Missing,
) -> Result<(Connection, u32)> {
    let real = resolve(path)?;
    if inspect(&real)? {
        refuse_newer_before_writing(path, &real, migrations, deadline)?;
    } else {
        match missing {
            Missing::Create => create_if_missing(&real)?,
            Missing::Refuse => {
                return Err(Error::Missing {
                    path: path.to_path_buf(),
                });
            }
        }
    }
    let mut conn = Connection::open_with_flags(
        &real,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|e| Error::sqlite("open", path, e))?;
    let configure = |e| Error::sqlite("configure", path, e);
    deadline.install(&mut conn).map_err(configure)?;
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
    switch_to_wal(&conn, path, deadline)?;
    set_pragma(&conn, "synchronous", "FULL").map_err(configure)?;
    set_pragma(&conn, "foreign_keys", "ON").map_err(configure)?;
    if secure_delete {
        set_pragma(&conn, "secure_delete", "ON").map_err(configure)?;
    }
    let version = migration::run(&mut conn, path, migrations, deadline)?;
    Ok((conn, version))
}

/// Outcome of one `PRAGMA wal_checkpoint(TRUNCATE)` attempt.
pub(crate) enum CheckpointAttempt {
    /// `busy == 0`: every frame was checkpointed and the WAL truncated.
    Done,
    /// `busy != 0` or SQLite returned a busy/locked error.
    Busy(rusqlite::Error),
}

/// Run one `PRAGMA wal_checkpoint(TRUNCATE)` attempt outside a transaction.
pub(crate) fn try_checkpoint_truncate_once(
    conn: &Connection,
    path: &Path,
) -> Result<CheckpointAttempt> {
    if !conn.is_autocommit() {
        return Err(Error::sqlite(
            "checkpoint the write-ahead log of",
            path,
            inside_transaction(),
        ));
    }
    let result: rusqlite::Result<(i64, i64, i64)> = conn
        .prepare_cached("PRAGMA wal_checkpoint(TRUNCATE)")
        .and_then(|mut statement| {
            statement.query_row([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        });
    match result {
        Ok((0, log, checkpointed)) if log >= 0 && checkpointed >= 0 => Ok(CheckpointAttempt::Done),
        Ok((0, _, _)) => {
            // SQLite returns (0, -1, -1) either when the database is not in
            // WAL mode or when a WAL-mode database has no active WAL yet
            // (for example a brand-new 0-page file with no migrations).
            let mode = get_pragma::<String>(conn, "journal_mode", None)
                .map_err(|e| Error::sqlite("checkpoint the write-ahead log of", path, e))?;
            if mode.eq_ignore_ascii_case("wal") {
                Ok(CheckpointAttempt::Done)
            } else {
                Err(Error::NotWal {
                    path: path.to_path_buf(),
                    mode,
                })
            }
        }
        Ok(_) => Ok(CheckpointAttempt::Busy(checkpoint_busy())),
        Err(e) if is_busy(&e) => Ok(CheckpointAttempt::Busy(e)),
        Err(e) => Err(Error::sqlite("checkpoint the write-ahead log of", path, e)),
    }
}

/// Run `PRAGMA wal_checkpoint(TRUNCATE)` outside any transaction, retrying
/// while `busy != 0` until `deadline` expires with [`Error::Locked`].
pub(crate) fn checkpoint_truncate(
    conn: &Connection,
    path: &Path,
    deadline: Deadline,
) -> Result<()> {
    let mut attempt = 0u32;
    loop {
        match try_checkpoint_truncate_once(conn, path)? {
            CheckpointAttempt::Done => return Ok(()),
            CheckpointAttempt::Busy(source) => {
                if !deadline.sleep_retry(attempt) {
                    return Err(Error::Locked {
                        path: path.to_path_buf(),
                        source,
                    });
                }
                attempt = attempt.saturating_add(1);
            }
        }
    }
}

/// Refuse a database a newer build migrated before opening it for writing:
/// a reader reads the version through SQLite, writing nothing of its own,
/// where a read-write open would run WAL recovery. The database file is
/// never opened outside SQLite: closing such a descriptor would drop the
/// POSIX locks of every SQLite connection to it in this process.
fn refuse_newer_before_writing(
    path: &Path,
    real: &Path,
    migrations: &[Step],
    deadline: Deadline,
) -> Result<()> {
    open_resolved(path, real, migrations, deadline).map(drop)
}

/// Put the database in WAL mode, which stays set in the file. SQLite may
/// wait for the change through the busy handler, or return a busy result
/// while another connection holds a new database; that is retried here.
/// Both end at the same deadline. A database that stays out of WAL is an
/// error.
fn switch_to_wal(conn: &Connection, path: &Path, deadline: Deadline) -> Result<()> {
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
            Err(e) if is_busy(&e) && !deadline.passed() => {
                std::thread::sleep(WAL_RETRY.min(deadline.left()));
            }
            Err(e) => return Err(Error::sqlite("switch to WAL", path, e)),
        }
    }
}
