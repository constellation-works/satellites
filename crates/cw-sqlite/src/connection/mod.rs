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
//!   path that is not valid UTF-8, as given or once resolved, is refused.
//! - **Writers** open read-write, with no create, no mutex and no following
//!   of a link. `busy_timeout` is the lock wait; the journal mode is WAL,
//!   and the switch is retried until the wait runs out, because SQLite does
//!   not call its busy handler for it; `synchronous` is `FULL`, so a
//!   committed transaction survives power loss; `foreign_keys` is on; the
//!   `-wal` and `-shm` files are kept after the last connection closes
//!   (`SQLITE_FCNTL_PERSIST_WAL`), so a read-only reader finds them. A
//!   database a newer build migrated is refused, by a reader before the
//!   read-write open and again under the write lock; the refusal writes no
//!   database content and leaves the main file and any existing `-wal`
//!   data alone, though in a writable directory SQLite may rebuild the
//!   `-shm`, its index, as any connection's first attach may, and may
//!   create a missing `-wal`, empty and owner-only, before the version
//!   check refuses. (A refused connection that closes last may still
//!   checkpoint committed WAL pages into the main file, which changes
//!   neither its content nor its version.) The database file is never
//!   opened outside SQLite: closing such a descriptor would drop the POSIX
//!   locks of every SQLite connection to it in this process.
//! - **Readers** open read-only and `query_only`, write no row, and never
//!   checkpoint on close. A reader holds one read transaction, opened by
//!   its version check, so all it reads is from the version it checked,
//!   until the caller ends that transaction. It holds no lock that blocks a
//!   writer, with one brief exception: on the first attach in a process to
//!   a WAL whose index is not initialised (after the last connection
//!   closed, say), SQLite's own recovery may take its recovery and write
//!   locks, even for a reader, and a writer meeting that waits within the
//!   lock wait. SQLite maps a database's `-shm` index once per process, so
//!   a reader opens it the way a writer in this process needs it, by the
//!   directory. In a directory this process can write, the reader shares
//!   the index: SQLite may rebuild the `-shm`, its index rather than
//!   database content, and creates a missing `-wal` (empty) and `-shm`,
//!   owner-only like the database. Where it cannot share a `-shm` this
//!   process cannot write, which no writer here could use either, it opens
//!   the index read-only (`readonly_shm=1`); SQLite may still create a
//!   missing `-wal`, owner-only. In a directory this process cannot write
//!   (a copy, a read-only mount), a reader writes nothing: the index is
//!   opened read-only, and when SQLite cannot read the WAL that way the
//!   reader opens the main file with `immutable=1` and says so, or is
//!   refused when the `-wal` has content. SQLite's WAL open asks for
//!   read-write on an existing owner-writable `-wal`, falling back to
//!   read-only, so it may hold one open read-write; a read-only connection
//!   writes nothing through it. In a writable directory the main
//!   file is never trusted alone.
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
