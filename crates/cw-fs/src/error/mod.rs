//! What a file operation fails with.
//!
//! `Display` carries only this layer's sentence; the operating-system cause
//! stays on [`std::error::Error::source`]. The calling unit wraps this in its
//! own typed error and names the stable code; [`Error::kind`] is the
//! classification, decided once here.

mod entry;

pub use entry::EntryKind;

use crate::lock::Holder;
use cw_base::Kind;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Why a file operation failed or was refused.
///
/// Not `#[non_exhaustive]`: a unit that wraps it gives every variant a
/// stable code, so adding a variant is meant to break its match.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A filesystem operation failed.
    #[error("cannot {action} {}", path.display())]
    Io {
        /// What was being attempted, as a verb phrase ("read", "create directory").
        action: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// The operating-system error.
        #[source]
        source: io::Error,
    },
    /// Another holder kept the lock past the deadline.
    #[error(
        "{} is locked by {} (waited {} ms); retry when it finishes",
        path.display(),
        holder.as_ref().map_or_else(|| String::from("another process"), ToString::to_string),
        waited.as_millis()
    )]
    Locked {
        /// The lock file.
        path: PathBuf,
        /// Who holds it, as the holder recorded it, when that process is
        /// still alive. `None` when there is no record or its process has
        /// exited.
        holder: Option<Holder>,
        /// How long this call waited before giving up.
        waited: Duration,
    },
    /// The path is a symbolic link, which is never followed.
    #[error(
        "refusing {}: it is a symbolic link; replace the link with the file it points to",
        path.display()
    )]
    Symlink {
        /// The link.
        path: PathBuf,
    },
    /// The file or directory belongs to another user.
    #[error(
        "refusing {}: it is owned by uid {owner}, but this process runs as uid {uid}",
        path.display()
    )]
    ForeignOwner {
        /// The directory or file.
        path: PathBuf,
        /// Its owner.
        owner: u32,
        /// This process's effective uid.
        uid: u32,
    },
    /// The file or directory is open to other users: a group or other
    /// permission bit is set.
    #[error(
        "refusing {}: mode {mode:o} is open to other users; {}",
        path.display(),
        chmod_remedy(fix, path)
    )]
    Permissions {
        /// The directory or file.
        path: PathBuf,
        /// Its permission bits.
        mode: u32,
        /// The owner-only mode to set: `700` for a directory, `600` for a file.
        fix: &'static str,
    },
    /// A regular file was required and something else is there.
    #[error("refusing {}: it is {found}, not a regular file", path.display())]
    NotRegular {
        /// The entry.
        path: PathBuf,
        /// What it turned out to be.
        found: EntryKind,
    },
    /// The file is larger than the caller's cap.
    #[error("refusing {}: it is larger than {cap} bytes", path.display())]
    TooLarge {
        /// The file.
        path: PathBuf,
        /// The cap the caller stated, in bytes.
        cap: u64,
    },
    /// This thread already holds the lock it asked for; waiting would wait
    /// out its own deadline.
    #[error(
        "refusing to lock {}: this thread already holds it",
        path.display()
    )]
    Reentrant {
        /// The lock file.
        path: PathBuf,
    },
}

impl Error {
    /// How this failure is classified. Exhaustive, so a new variant decides.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::Io { .. } => Kind::Io,
            Self::Locked { .. } => Kind::Busy,
            Self::Symlink { .. }
            | Self::ForeignOwner { .. }
            | Self::Permissions { .. }
            | Self::NotRegular { .. }
            | Self::TooLarge { .. } => Kind::Refused,
            Self::Reentrant { .. } => Kind::Internal,
        }
    }

    pub(crate) fn io(action: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }
}

/// The `chmod` that makes `path` owner-only, safe to paste into a shell:
/// `--` comes first and ends the options, so a name starting with `-` is
/// not read as one by GNU or BSD `chmod` (BSD stops at the first operand,
/// so a `--` after the mode would itself be taken as a file name),
/// and the path is one single-quoted word, each `'` written as `'\''`. A
/// path that is not valid UTF-8 cannot be printed exactly, so the remedy
/// says so instead of printing a command that would name another file.
fn chmod_remedy(fix: &str, path: &Path) -> String {
    match path.to_str() {
        Some(text) => format!("fix with: chmod -- {fix} '{}'", text.replace('\'', r"'\''")),
        None => format!(
            "set its mode to {fix}; its name is not valid UTF-8, so no command can be printed for it"
        ),
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
