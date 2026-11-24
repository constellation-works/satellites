//! Opening a read-only connection.

use super::files::{header, sidecars};
use super::{get_pragma, inspect, resolve, set_pragma};
use crate::error::{Error, Result, is_wal_unreadable};
use crate::migration;
use crate::migration::Step;
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
    /// `immutable=1`: a copy, or a database on a read-only mount, with no
    /// readable `-wal` or `-shm`. What it reads is the main file alone, so a
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
///
/// It creates and changes no file, with one exception SQLite does not let it
/// avoid: a WAL-mode database with a `-shm` index and no `-wal` gets an
/// empty `-wal`, owner-only like the database.
pub(crate) fn open(path: &Path, migrations: &[Step], wait: Duration) -> Result<Reader> {
    let real = resolve(path)?;
    if !inspect(&real)? {
        return Err(Error::Missing {
            path: path.to_path_buf(),
        });
    }
    let sidecars = sidecars(&real)?;
    // With no index and nothing in the WAL, the main file is the whole
    // database. SQLite's read-only open would create an empty `-wal` and then
    // fail on the missing index, so open it immutable at once.
    if !sidecars.shm && !sidecars.wal_has_content && header(&real)?.is_some_and(|header| header.wal)
    {
        return open_as(path, &real, migrations, wait, true)
            .map_err(|attempt| attempt.into_error(path));
    }
    match open_as(path, &real, migrations, wait, false) {
        // The fallback reads the main file alone, so it is refused when the
        // WAL may hold committed pages the main file lacks.
        Err(Attempt::WalUnreadable(e)) if sidecars.wal_has_content => {
            Err(Error::sqlite("read the write-ahead log of", path, e))
        }
        Err(Attempt::WalUnreadable(_)) => {
            open_as(path, &real, migrations, wait, true).map_err(|attempt| attempt.into_error(path))
        }
        result => result.map_err(|attempt| attempt.into_error(path)),
    }
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
    immutable: bool,
) -> Result<Reader, Attempt> {
    let sqlite = |action| {
        move |e: rusqlite::Error| {
            if !immutable && is_wal_unreadable(&e) {
                Attempt::WalUnreadable(e)
            } else {
                Attempt::Failed(Error::sqlite(action, path, e))
            }
        }
    };
    let uri = if immutable {
        format!("file:{}?immutable=1", uri_path(real))
    } else {
        format!("file:{}?readonly_shm=1", uri_path(real))
    };
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(sqlite("open"))?;
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
    // `resolve` refused a path that is not UTF-8, so nothing is lost here.
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
