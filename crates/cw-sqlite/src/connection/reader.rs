//! Opening a read-only connection.

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
/// It took no lock and wrote nothing to open. A statement that would write
/// through it fails.
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
pub(crate) fn open(path: &Path, migrations: &[Step], wait: Duration) -> Result<Reader> {
    if !inspect(path)? {
        return Err(Error::Missing {
            path: path.to_path_buf(),
        });
    }
    let real = resolve(path)?;
    match open_as(path, &real, migrations, wait, false) {
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
