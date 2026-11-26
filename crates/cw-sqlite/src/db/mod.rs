//! The database a project names once, and what it does.

use crate::connection::{
    Deadline, Reader, Writer, checkpoint_truncate, open_existing_writer, open_reader, open_writer,
    open_writer_handle,
};
use crate::error::{Error, Result, ended_transaction};
use crate::migration;
use crate::migration::Step;
use rusqlite::{Transaction, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a call waits for another connection by default.
const DEFAULT_LOCK_WAIT: Duration = Duration::from_secs(5);

/// A SQLite database at one path with one schema. Cheap to build and to
/// clone: it holds no connection, and each call opens its own.
///
/// ```no_run
/// use cw_sqlite::{Db, Step};
/// use std::time::Duration;
///
/// const MIGRATIONS: &[Step] = &[Step::sql("CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT) STRICT;")];
///
/// let db = Db::new("/path/to/store.db", MIGRATIONS).lock_wait(Duration::from_secs(2));
/// ```
#[derive(Clone, Debug)]
pub struct Db {
    path: PathBuf,
    migrations: &'static [Step],
    lock_wait: Duration,
    secure_delete: bool,
}

impl Db {
    /// The database at `path`, whose schema is `migrations`. Nothing is
    /// opened or created until the first call.
    ///
    /// The lock wait is 5 seconds until [`Db::lock_wait`] sets it, and
    /// `secure_delete` is off.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, migrations: &'static [Step]) -> Self {
        Self {
            path: path.into(),
            migrations,
            lock_wait: DEFAULT_LOCK_WAIT,
            secure_delete: false,
        }
    }

    /// How long a call waits for another connection before it fails with
    /// [`Error::Locked`], by the clock.
    ///
    /// It is one deadline for the whole call: the lock wait after the call
    /// started, by the clock (`Instant`), at most `i32::MAX` milliseconds
    /// (about 24.8 days), as `busy_timeout` capped it. It bounds the waits that
    /// go through the connection's busy handler: a lock for the version check
    /// before a write and for the writer's own version read, each migration
    /// step, any migration WAL-truncate barrier, [`Db::checkpoint_truncate`],
    /// and the write lock (`BEGIN IMMEDIATE`); the switch to WAL, whose
    /// waits end at the same deadline whether SQLite waits through the busy
    /// handler (as it can when another connection holds a rollback-journal
    /// database) or returns a busy result, which the crate retries. One call
    /// can wait at several of those points, and all of them end at the same
    /// deadline, so the call stops waiting close to one lock wait after it
    /// started, past it by at most the last step's oversleep and one retry; a
    /// wait that begins after the deadline fails at once. The busy handler is
    /// the crate's own, not SQLite's `busy_timeout`, which counts its planned
    /// sleeps and never reads the clock: it reads the clock before each retry,
    /// sleeps short steps (1 to 16 ms) that end no later than the deadline, and
    /// gives up once it has passed, with [`Error::Locked`]. Outside the
    /// deadline is SQLite's own retry for a WAL read mark (`walTryBeginRead`),
    /// which never calls the busy handler: it backs off on its own schedule
    /// and, when its retries run out, fails with `SQLITE_PROTOCOL`, reported as
    /// [`Error::Sqlite`], not [`Error::Locked`]. A [`Writer`] returned by
    /// [`Db::open_writer`] shares one deadline while opening and migrating,
    /// and then keeps a clock-bounded busy handler that gives each subsequent
    /// lock wait (and each [`Writer::checkpoint_truncate`] call) up to
    /// `lock_wait`.
    #[must_use]
    pub fn lock_wait(self, wait: Duration) -> Self {
        Self {
            lock_wait: wait,
            ..self
        }
    }

    /// Whether writers set `PRAGMA secure_delete = ON`, so a deleted row's
    /// content is overwritten rather than left in a free page. Off by
    /// default: it costs a write per freed page, and only a project that
    /// promises to forget text needs it. Pair it with
    /// [`Db::checkpoint_truncate`] or [`Writer::checkpoint_truncate`] (or
    /// [`Step::checkpoint_truncate_before`] during migrations) to physically
    /// clear older pages from the persisted `-wal` file after commit.
    #[must_use]
    pub fn secure_delete(self, on: bool) -> Self {
        Self {
            secure_delete: on,
            ..self
        }
    }

    /// The database file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run `f` in one `IMMEDIATE` write transaction on a new read-write
    /// connection: committed when `f` returns `Ok`, rolled back when it
    /// returns `Err`.
    ///
    /// The connection is opened, the database created `0600` when missing,
    /// and every migration step it has not had applied, before `f` runs. The
    /// write lock is taken when the transaction begins, so `f` never meets a
    /// busy database, and the schema version is checked again under it, so
    /// `f` never runs against a schema a newer build migrated.
    ///
    /// `f` must not run transaction control (`COMMIT`, `ROLLBACK`, `BEGIN`,
    /// `SAVEPOINT` release): the transaction is this call's. When `f` returns
    /// with the transaction ended, the call fails; what `f` committed itself
    /// stays committed.
    ///
    /// # Errors
    ///
    /// `f`'s own error, or an [`Error`] converted into `E`:
    /// [`Error::Locked`] when another connection holds the database past the
    /// lock wait; [`Error::Incompatible`] when a newer build migrated it;
    /// [`Error::Migration`] when a step fails; [`Error::File`] when a file is
    /// refused; [`Error::Corrupt`], [`Error::NotWal`] or [`Error::Sqlite`]
    /// otherwise.
    pub fn write<T, E>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<T, E>) -> Result<T, E>
    where
        E: From<Error>,
    {
        let mut conn = open_writer(
            &self.path,
            self.migrations,
            Deadline::after(self.lock_wait),
            self.secure_delete,
        )?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| Error::sqlite("begin a write to", &self.path, e))?;
        // A newer build may have migrated between this connection's
        // migration and its write lock; under the lock, the version is final.
        migration::refuse_newer(&tx, &self.path, self.migrations)?;
        // On `Err` the transaction is dropped, which rolls it back.
        let value = f(&tx)?;
        if tx.is_autocommit() {
            return Err(Error::sqlite("commit to", &self.path, ended_transaction()).into());
        }
        tx.commit()
            .map_err(|e| Error::sqlite("commit to", &self.path, e))?;
        Ok(value)
    }

    /// Open an owned, configured, migrated read-write connection.
    ///
    /// Enforces the same `cw-fs` inspection, `0600` creation when missing,
    /// newer-version refusal, WAL switch, `synchronous = FULL`,
    /// `foreign_keys = ON`, `PersistWal`, optional `secure_delete`, deadline
    /// busy handler and migrations as [`Db::write`], without starting a write
    /// transaction.
    ///
    /// Use [`Db::write`] for a single `IMMEDIATE` transaction. Use
    /// `open_writer` when a caller owns its own transaction scope (for
    /// example, savepoints, thread-local write scopes, or post-commit
    /// [`Writer::checkpoint_truncate`]).
    ///
    /// While opening and migrating, every wait shares one call deadline of
    /// `lock_wait`. Once returned, the [`Writer`] keeps a clock-bounded busy
    /// handler that gives each subsequent lock wait up to `lock_wait`.
    ///
    /// # Errors
    ///
    /// [`Error::Locked`] when another connection holds the database past the
    /// lock wait; [`Error::Incompatible`] when a newer build migrated it;
    /// [`Error::Migration`] when a step fails; [`Error::File`] when a file is
    /// refused; [`Error::Corrupt`], [`Error::NotWal`] or [`Error::Sqlite`]
    /// otherwise.
    pub fn open_writer(&self) -> Result<Writer> {
        open_writer_handle(
            &self.path,
            self.migrations,
            self.lock_wait,
            self.secure_delete,
        )
    }

    /// Run `PRAGMA wal_checkpoint(TRUNCATE)` outside any transaction on an
    /// existing database, retrying while `busy != 0` until the call's
    /// `lock_wait` expires by the clock.
    ///
    /// Because writers keep the `-wal` file after the last connection closes
    /// (`SQLITE_FCNTL_PERSIST_WAL`), a project using [`Db::secure_delete`]
    /// calls this (or [`Writer::checkpoint_truncate`]) after commit to move
    /// committed pages into the main file and truncate `-wal` to zero bytes so
    /// deleted text no longer sits in older WAL frames.
    ///
    /// Does not create a missing database.
    ///
    /// # Errors
    ///
    /// [`Error::Missing`] when the database does not exist yet;
    /// [`Error::Locked`] when another connection (a reader or a writer) blocks
    /// truncation past the lock wait; [`Error::Incompatible`] when a newer
    /// build migrated it; [`Error::Migration`] when a pending migration step
    /// fails; [`Error::File`] when a file is refused; [`Error::Corrupt`],
    /// [`Error::NotWal`] or [`Error::Sqlite`] otherwise.
    pub fn checkpoint_truncate(&self) -> Result<()> {
        let deadline = Deadline::after(self.lock_wait);
        let conn = open_existing_writer(&self.path, self.migrations, deadline, self.secure_delete)?;
        checkpoint_truncate(&conn, &self.path, deadline)
    }

    /// A new read-only connection. It writes no row, and blocks no writer,
    /// in this process or another, beyond SQLite's brief recovery on the
    /// first attach to an uninitialised index; it sees the last committed
    /// state, even while a write is in progress. In a directory this process
    /// can write, it shares the `-shm` index, which SQLite may rebuild, and
    /// SQLite creates a missing `-wal` or `-shm`, owner-only; in one it
    /// cannot write, it writes nothing. See
    /// [`crate::Reader::immutable`] for that case.
    ///
    /// # Errors
    ///
    /// [`Error::Missing`] when the database does not exist yet, or holds
    /// nothing yet (a first writer has created it and not yet committed its
    /// first migration step);
    /// [`Error::Incompatible`] when a newer build migrated it;
    /// [`Error::File`] when a file is refused; [`Error::Corrupt`] or
    /// [`Error::Sqlite`] otherwise.
    pub fn read(&self) -> Result<Reader> {
        open_reader(&self.path, self.migrations, Deadline::after(self.lock_wait))
    }

    /// Run `PRAGMA quick_check` on a read-only connection, for a doctor
    /// command.
    ///
    /// # Errors
    ///
    /// [`Error::CheckFailed`] with every problem it reported, or any error
    /// of [`Db::read`].
    pub fn check(&self) -> Result<()> {
        let reader = self.read()?;
        let problems = reader
            .prepare_cached("PRAGMA quick_check")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|e| Error::sqlite("check", &self.path, e))?;
        if problems.len() == 1 && problems.first().is_some_and(|line| line == "ok") {
            return Ok(());
        }
        Err(Error::CheckFailed {
            path: self.path.clone(),
            problems,
        })
    }
}
