//! Opening a read-only connection.

use super::files::{directory_writable, wal_has_content};
use super::{get_pragma, inspect, resolve, set_pragma};
use crate::error::{Error, Result, is_wal_unreadable};
use crate::migration;
use crate::migration::Step;
use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags};
use std::ops::Deref;
use std::path::Path;
use std::time::Duration;

/// A read-only, `query_only` connection, from [`crate::Db::read`]. It
/// dereferences to the [`Connection`].
///
/// It holds one read transaction from the moment its schema version was
/// checked until it is dropped, so every statement through it reads the
/// same committed snapshot, the one whose version [`Reader::version`]
/// reports; a write committed meanwhile is seen by the next reader. Keep it
/// for one command's reads: while it lives, a checkpoint cannot pass its
/// snapshot. A statement that would write through it fails, and so does
/// `BEGIN`, since its transaction is already open.
///
/// The version check holds until that transaction ends. A caller that runs
/// `COMMIT` or `ROLLBACK` through it ends the snapshot: later statements
/// read whatever is committed then, perhaps a newer build's schema, while
/// [`Reader::version`] still reports the old version. Don't; open another
/// reader instead.
#[derive(Debug)]
pub struct Reader {
    conn: Connection,
    version: u32,
    immutable: bool,
}

impl Reader {
    /// The database's schema version (`PRAGMA user_version`) when it was
    /// opened. A reader never migrates, so this can be below the last step
    /// when no writer has opened the database since this build added one.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Whether the WAL could not be read and the database was opened with
    /// `immutable=1`: a copy, or a database on a read-only mount, in a
    /// directory this process cannot write, with no readable index and
    /// nothing in its `-wal`. What it reads is the main file alone, so a
    /// caller may say that the view is a copy's.
    #[must_use]
    pub fn immutable(&self) -> bool {
        self.immutable
    }

    /// The connection.
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

impl Deref for Reader {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        &self.conn
    }
}

/// A [`Reader`] on the database at `path`, refused when it is missing or a
/// newer build migrated it.
pub(crate) fn open(path: &Path, migrations: &[Step], wait: Duration) -> Result<Reader> {
    open_resolved(path, &resolve(path)?, migrations, wait)
}

/// [`open`] on `real`, the already resolved `path`.
///
/// SQLite keeps one mapping of a database's `-shm` index per process, made
/// by the first connection that opens it and shared by every later one. So
/// how a reader opens the index decides it for a writer in this process,
/// and the directory decides how:
///
/// - **Writable:** a writer may exist, here or in another process. The
///   reader shares the index, so a writer in this process finds it
///   writable whichever connection opened it first; SQLite creates a
///   missing `-wal` (empty) and `-shm`, owner-only like the database, and
///   may rebuild the `-shm`, which is its index, not database content.
///   Where it cannot share it (a `-shm` this process cannot write, which no
///   writer here could use either), the reader opens it read-only
///   (`readonly_shm=1`) instead, and SQLite may still create a missing
///   `-wal`, owner-only. The main file is never trusted alone.
/// - **Not writable:** no file there is opened writable. The index is
///   opened read-only; when SQLite cannot read the WAL that way (the `-shm`
///   or the `-wal` is missing), no writer can exist, so the main file is a
///   static copy. With nothing in the `-wal` it is the whole database and
///   is opened `immutable=1`, and the reader says so; with WAL content the
///   main file may lack committed pages, and the read is refused.
pub(crate) fn open_resolved(
    path: &Path,
    real: &Path,
    migrations: &[Step],
    wait: Duration,
) -> Result<Reader> {
    if !inspect(real)? {
        return Err(Error::Missing {
            path: path.to_path_buf(),
        });
    }
    let open = |how| open_as(path, real, migrations, wait, how);
    let fail = |attempt: Attempt| attempt.into_error(path);
    if directory_writable(real)? {
        return match open(Open::SharedIndex) {
            Err(Attempt::WalUnreadable(_)) => open(Open::ReadOnlyIndex).map_err(fail),
            result => result.map_err(fail),
        };
    }
    match open(Open::ReadOnlyIndex) {
        Err(Attempt::WalUnreadable(e)) => {
            if wal_has_content(real)? {
                return Err(Error::sqlite("read the write-ahead log of", path, e));
            }
            open(Open::Immutable).map_err(fail)
        }
        result => result.map_err(fail),
    }
}

/// How a reader opens the database.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Open {
    /// Read-only, with the `-shm` index opened read-only too.
    ReadOnlyIndex,
    /// Read-only, with the `-shm` index shared, so SQLite may create it.
    SharedIndex,
    /// The main file alone, `immutable=1`.
    Immutable,
}

/// Why one attempt to open a reader failed.
enum Attempt {
    /// SQLite could not open or recover the WAL read-only.
    WalUnreadable(rusqlite::Error),
    /// Anything else.
    Failed(Error),
}

impl Attempt {
    fn into_error(self, path: &Path) -> Error {
        match self {
            Self::WalUnreadable(e) => Error::sqlite("read", path, e),
            Self::Failed(e) => e,
        }
    }
}

/// One attempt at a reader on `real`, the resolved `path`.
fn open_as(
    path: &Path,
    real: &Path,
    migrations: &[Step],
    wait: Duration,
    how: Open,
) -> Result<Reader, Attempt> {
    let immutable = how == Open::Immutable;
    // Only the main file alone has no fallback; see `open_resolved`.
    let may_fall_back = how != Open::Immutable;
    let sqlite = |action| {
        move |e: rusqlite::Error| {
            if may_fall_back && is_wal_unreadable(&e) {
                Attempt::WalUnreadable(e)
            } else {
                Attempt::Failed(Error::sqlite(action, path, e))
            }
        }
    };
    let query = match how {
        Open::ReadOnlyIndex => "?readonly_shm=1",
        Open::SharedIndex => "",
        Open::Immutable => "?immutable=1",
    };
    let uri = format!("file:{}{query}", uri_path(real));
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(sqlite("open"))?;
    // A reader that closes last must not try to checkpoint: that would take
    // the database's exclusive lock, and a read-only connection could not
    // write the pages anyway.
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
        .map_err(sqlite("configure"))?;
    set_pragma(&conn, "busy_timeout", super::writer::millis(wait)).map_err(sqlite("configure"))?;
    set_pragma(&conn, "query_only", "ON").map_err(sqlite("configure"))?;
    // One read transaction for the reader's life: the version checked below
    // is the version of every row the caller reads.
    conn.prepare_cached("BEGIN DEFERRED")
        .and_then(|mut begin| begin.execute([]))
        .map_err(sqlite("begin a read of"))?;
    let found: u32 =
        get_pragma(&conn, "user_version", None).map_err(sqlite("read the schema version of"))?;
    let version = migration::refuse_past(path, found, migrations).map_err(Attempt::Failed)?;
    Ok(Reader {
        conn,
        version,
        immutable,
    })
}

/// `path` for a `file:` URI: `%`, `?` and `#` escaped, as SQLite requires.
fn uri_path(path: &Path) -> String {
    let mut out = String::new();
    // `resolve` refused a path that is not UTF-8, as given or resolved, so
    // nothing is lost here.
    for c in path.to_string_lossy().chars() {
        match c {
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3f"),
            '#' => out.push_str("%23"),
            _ => out.push(c),
        }
    }
    out
}
