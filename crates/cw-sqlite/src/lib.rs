//! One private SQLite database for a command-line tool: a writer or a reader
//! opened per call, fixed pragmas, one `IMMEDIATE` write transaction, and an
//! append-only list of migrations keyed on `PRAGMA user_version`.
//!
//! Mechanism only. It knows no table and no record: a project hands it a
//! path and its migrations, and wraps its [`Error`] in its own typed error
//! with its own code.
//!
//! ```no_run
//! use cw_sqlite::{Db, Step};
//! use std::time::Duration;
//!
//! const MIGRATIONS: &[Step] = &[Step::sql(
//!     "CREATE TABLE note (id INTEGER PRIMARY KEY, body TEXT NOT NULL) STRICT;",
//! )];
//!
//! # fn main() -> Result<(), cw_sqlite::Error> {
//! let db = Db::new("/path/to/store.db", MIGRATIONS).lock_wait(Duration::from_secs(5));
//! db.write(|tx| {
//!     tx.prepare_cached("INSERT INTO note (body) VALUES (?1)")
//!         .and_then(|mut insert| insert.execute(["first"]))
//!         .map_err(|e| cw_sqlite::Error::sqlite("insert a note into", db.path(), e))
//! })?;
//! let reader = db.read()?;
//! let count: i64 = reader
//!     .prepare_cached("SELECT count(*) FROM note")
//!     .and_then(|mut count| count.query_row([], |row| row.get(0)))
//!     .map_err(|e| cw_sqlite::Error::sqlite("count the notes in", db.path(), e))?;
//! db.check()?;
//! # let _ = count;
//! # Ok(())
//! # }
//! ```
//!
//! - [`db`]: [`Db`], the database a project names once, and what it does:
//!   [`Db::write`], [`Db::open_writer`], [`Db::checkpoint_truncate`],
//!   [`Db::read`], [`Db::check`].
//! - [`connection`]: [`Writer`], the owned read-write connection
//!   [`Db::open_writer`] returns; [`Reader`], the read-only connection
//!   [`Db::read`] returns; and how both kinds of connection are opened.
//! - [`migration`]: [`Step`], one append-only schema step (with optional
//!   `foreign_keys` and WAL `TRUNCATE` checkpoint guards).
//! - [`error`]: the typed [`Error`] all of them fail with.
//!
//! `rusqlite` is re-exported, so a project names one version of it, through
//! this crate.

pub mod connection;
pub mod db;
pub mod error;
pub mod migration;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use connection::{Reader, Writer};
pub use db::Db;
pub use error::{Error, Result};
pub use migration::Step;
pub use rusqlite;
