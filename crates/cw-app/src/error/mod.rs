//! What this crate fails with.
//!
//! One typed enum with a [`Kind`] and no code: the program that wraps it
//! names its own stable code, because the code is that program's contract.

use cw_base::{Kind, TimestampError};
use std::path::PathBuf;

/// Why resolving the data directory, or writing or reading the audit log,
/// failed.
///
/// Not `#[non_exhaustive]`: a program that wraps it gives every variant a
/// code, so adding a variant is meant to break its match.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Nothing named a data directory: no explicit root, no root variable
    /// and no `HOME`.
    #[error("no data directory: pass a root explicitly or set {root_var} (HOME is not set either)")]
    NoDataDir {
        /// The variable that would have named it, such as `NOTES_ROOT`.
        root_var: String,
    },
    /// A file operation on the audit log or its lock failed or was refused.
    #[error(transparent)]
    Fs(#[from] cw_fs::Error),
    /// The time a command started cannot be written as a timestamp.
    #[error(transparent)]
    Timestamp(#[from] TimestampError),
    /// An audit line could not be encoded.
    #[error("cannot encode an audit line")]
    Encode(#[source] serde_json::Error),
    /// A line of the audit log is not a valid audit line.
    #[error("{}: line {line} is not a valid audit line", path.display())]
    Corrupt {
        /// The audit log.
        path: PathBuf,
        /// The line, counted from 1.
        line: usize,
        /// What the parser reported.
        #[source]
        source: serde_json::Error,
    },
    /// A line of the audit log was written in a format newer than this build
    /// reads.
    #[error(
        "{}: line {line} has audit format {found}, newer than the {supported} this build reads; upgrade this tool",
        path.display()
    )]
    NewerFormat {
        /// The audit log.
        path: PathBuf,
        /// The line, counted from 1.
        line: usize,
        /// The format the line declares.
        found: u32,
        /// The newest format this build reads.
        supported: u32,
    },
}

impl Error {
    /// How this failure is classified. Exhaustive, so a new variant decides.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::NoDataDir { .. } => Kind::Config,
            Self::Fs(error) => error.kind(),
            Self::Timestamp(error) => error.kind(),
            Self::Encode(_) => Kind::Internal,
            Self::Corrupt { .. } => Kind::Corrupt,
            Self::NewerFormat { .. } => Kind::Incompatible,
        }
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
