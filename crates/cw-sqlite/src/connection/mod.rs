//! Opening a connection: a writer for each write, a [`Reader`] for each
//! read. There is no pool; a command-line tool opens one per call.
//!
//! - **Files.** Before SQLite sees them, the database and its `-wal` and
//!   `-shm` files are inspected through `cw-fs`: a link, another user's
//!   file, or one with any group or other permission bit is refused. A new
//!   database is created `0600` by `cw-fs`, so SQLite, which gives its
//!   `-wal` and `-shm` files the database's mode, never creates one open to
//!   others.
//! - **Writers** open read-write, with no create, no mutex and no following
//!   of a link (the directory is resolved first, so only the file itself
//!   is held to that). `busy_timeout` is the lock wait; the journal mode is WAL,
//!   and the switch is retried until the wait runs out, because SQLite does
//!   not call its busy handler for it; `synchronous` is `FULL`, so a
//!   committed transaction survives power loss; `foreign_keys` is on; the
//!   `-wal` and `-shm` files are kept after the last connection closes
//!   (`SQLITE_FCNTL_PERSIST_WAL`), so a read-only reader finds them.
//! - **Readers** open read-only and `query_only`, take no lock and write
//!   nothing: even the `-shm` index is opened read-only (`readonly_shm=1`),
//!   so a read leaves every file as it found it. When the WAL cannot be read (a copy on a read-only mount, with
//!   no `-wal` or `-shm`), the reader retries with `immutable=1`, which is
//!   safe there because no writer can exist, and says so.
//! - **Pragmas** are set with plain SQL, not `rusqlite`'s helper, which reads
//!   the schema first and so fails on a read-only directory.

mod files;
mod pragma;
mod reader;
mod writer;

pub use reader::Reader;

pub(crate) use files::{create_if_missing, inspect, resolve};
pub(crate) use pragma::{get as get_pragma, set as set_pragma};
pub(crate) use reader::open as open_reader;
pub(crate) use writer::open as open_writer;
