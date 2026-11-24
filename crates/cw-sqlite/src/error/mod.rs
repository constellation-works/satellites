//! What a database operation fails with.
//!
//! `Display` carries only this layer's sentence; the SQLite or file cause
//! stays on [`std::error::Error::source`]. The calling unit wraps this in its
//! own typed error and names the stable code; [`Error::kind`] is the
//! classification, decided once here.

use cw_base::Kind;
use rusqlite::ErrorCode;
use std::path::{Path, PathBuf};

/// Why a database operation failed or was refused.
///
/// Not `#[non_exhaustive]`: a unit that wraps it gives every variant a
/// stable code, so adding a variant is meant to break its match.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The database file or a `-wal` or `-shm` file beside it was refused or
    /// could not be inspected or created: a link, another user's, open to a
    /// group or other permission bit, or an I/O failure.
    #[error(transparent)]
    File(#[from] cw_fs::Error),
    /// A reader was asked for a database that does not exist yet. A reader
    /// never creates one.
    #[error("{} does not exist yet; nothing has been written to it", path.display())]
    Missing {
        /// The database file.
        path: PathBuf,
    },
    /// Another connection held the database past the lock wait.
    #[error("{} is locked by another connection; retry when it finishes", path.display())]
    Locked {
        /// The database file.
        path: PathBuf,
        /// SQLite's busy or locked error.
        #[source]
        source: rusqlite::Error,
    },
    /// The file is not a SQLite database, or SQLite found it damaged.
    #[error("{} is not a valid SQLite database", path.display())]
    Corrupt {
        /// The database file.
        path: PathBuf,
        /// SQLite's error.
        #[source]
        source: rusqlite::Error,
    },
    /// `PRAGMA quick_check` reported problems.
    #[error("{} failed its integrity check: {}", path.display(), problems.join("; "))]
    CheckFailed {
        /// The database file.
        path: PathBuf,
        /// Each line `quick_check` reported.
        problems: Vec<String>,
    },
    /// The database was migrated by a newer build than this one. It is
    /// refused for reads and writes, and left as it is.
    #[error(
        "{} is at schema version {found}, newer than the {supported} this build knows; upgrade this tool",
        path.display()
    )]
    Incompatible {
        /// The database file.
        path: PathBuf,
        /// The database's `user_version`.
        found: u32,
        /// How many migration steps this build has.
        supported: u32,
    },
    /// The database could not be put in WAL mode: the filesystem does not
    /// support it (some network filesystems), or something else holds the
    /// database in another mode.
    #[error("{} stayed in journal mode {mode}, not WAL", path.display())]
    NotWal {
        /// The database file.
        path: PathBuf,
        /// The journal mode SQLite reported.
        mode: String,
    },
    /// A migration step failed. Its transaction was rolled back, so the
    /// schema and the version are as they were before it.
    #[error("migration step {step} of {} failed", path.display())]
    Migration {
        /// The database file.
        path: PathBuf,
        /// The step's version: its 1-based position in the list.
        step: u32,
        /// SQLite's error, or the step function's.
        #[source]
        source: rusqlite::Error,
    },
    /// Any other SQLite failure.
    #[error("cannot {action} {}", path.display())]
    Sqlite {
        /// What was being attempted, as a verb phrase ("open", "commit to").
        action: &'static str,
        /// The database file.
        path: PathBuf,
        /// SQLite's error.
        #[source]
        source: rusqlite::Error,
    },
}

impl Error {
    /// How this failure is classified. Exhaustive, so a new variant decides.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::File(e) => e.kind(),
            Self::Missing { .. } => Kind::NotFound,
            Self::Locked { .. } => Kind::Busy,
            Self::Corrupt { .. } | Self::CheckFailed { .. } => Kind::Corrupt,
            Self::Incompatible { .. } => Kind::Incompatible,
            Self::NotWal { .. } | Self::Migration { .. } | Self::Sqlite { .. } => Kind::Io,
        }
    }

    /// Classify a `rusqlite` error from `action` on the database at `path`:
    /// busy or locked is [`Error::Locked`], not-a-database or corrupt is
    /// [`Error::Corrupt`], anything else [`Error::Sqlite`].
    ///
    /// For the statements a project runs inside [`crate::Db::write`] or on a
    /// [`crate::Reader`], so its failures are classified as this crate's own.
    #[must_use]
    pub fn sqlite(action: &'static str, path: &Path, source: rusqlite::Error) -> Self {
        let path = path.to_path_buf();
        if is_busy(&source) {
            Self::Locked { path, source }
        } else if is_corrupt(&source) {
            Self::Corrupt { path, source }
        } else {
            Self::Sqlite {
                action,
                path,
                source,
            }
        }
    }

    /// A failed migration step, unless SQLite's error is a lock or a
    /// damaged file, which keep their own classes.
    pub(crate) fn migration(path: &Path, step: u32, source: rusqlite::Error) -> Self {
        if is_busy(&source) || is_corrupt(&source) {
            Self::sqlite("migrate", path, source)
        } else {
            Self::Migration {
                path: path.to_path_buf(),
                step,
                source,
            }
        }
    }
}

/// Another connection holds the database.
pub(crate) fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

/// The file is not a database or is damaged.
fn is_corrupt(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
    )
}

/// SQLite could not open or recover the WAL read-only: the `-wal` or `-shm`
/// file is missing or unreadable and the directory is not writable.
pub(crate) fn is_wal_unreadable(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::CannotOpen | ErrorCode::ReadOnly)
    )
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
