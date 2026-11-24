//! Opening a connection: a writer for each write, a [`Reader`] for each
//! read. There is no pool; a command-line tool opens one per call.
//!
//! - **Files.** Before SQLite sees them, the database and its `-wal` and
//!   `-shm` files are inspected through `cw-fs`: a link, another user's
//!   file, or one with any group or other permission bit is refused. A new
//!   database is created `0600` by `cw-fs`, so SQLite, which gives its
//!   `-wal` and `-shm` files the database's mode, never creates one open to
//!   others.
//! - **Paths.** The directory is resolved first, and the resolved path is
//!   what is inspected, created and opened. The directory is trusted, as in
//!   `cw-fs`; only the file and its companions are held to the checks. A
//!   path that is not valid UTF-8 is refused.
//! - **Writers** open read-write, with no create, no mutex and no following
//!   of a link. `busy_timeout` is the lock wait; the journal mode is WAL,
//!   and the switch is retried until the wait runs out, because SQLite does
//!   not call its busy handler for it; `synchronous` is `FULL`, so a
//!   committed transaction survives power loss; `foreign_keys` is on; the
//!   `-wal` and `-shm` files are kept after the last connection closes
//!   (`SQLITE_FCNTL_PERSIST_WAL`), so a read-only reader finds them. A
//!   database a newer build migrated is refused before it is opened for
//!   writing, so the refusal creates and changes no file, except where the
//!   `-wal` holds pages and the `-shm` is gone: there only a read-write open
//!   can read the version, and it recreates the `-shm`.
//! - **Readers** open read-only and `query_only`, take no lock and write
//!   nothing: even the `-shm` index is opened read-only (`readonly_shm=1`).
//!   A reader holds one read transaction, opened by its version check, so
//!   all it reads is from the version it checked. When the WAL cannot be
//!   read and holds nothing (a copy, or a database whose writers never left
//!   an index), the reader opens the main file with `immutable=1` and says
//!   so; when the WAL holds pages the main file may lack, it is refused. The
//!   one file a reader can create is an empty `-wal`, owner-only, for a
//!   WAL-mode database with a `-shm` and no `-wal`: SQLite gives it no way
//!   to avoid that.
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
