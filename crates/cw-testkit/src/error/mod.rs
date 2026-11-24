//! The one error type of the crate.

use std::path::PathBuf;

use crate::golden::UPDATE_VAR;

/// Why a temp root could not be made or a golden did not hold.
///
/// A golden mismatch is an error, not a panic, so a test can check every
/// golden it owns and report all the stale ones at once. Its message names
/// the golden file and the update variable.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The environment prefix is not a valid variable name prefix: it must be
    /// an ASCII capital letter followed by capitals, digits or `_`, and must
    /// not end in `_`.
    #[error(
        "environment prefix {prefix:?} is not valid: use capitals, digits and `_`, starting with a capital and not ending in `_`"
    )]
    InvalidPrefix {
        /// The prefix as given.
        prefix: String,
    },

    /// A golden name is empty, absolute or leaves the goldens folder.
    #[error(
        "golden name {name:?} is not valid: it must be a relative path inside the goldens folder"
    )]
    InvalidGoldenName {
        /// The name as given.
        name: PathBuf,
    },

    /// The golden file, or a directory between it and the goldens folder, is
    /// a symbolic link. Goldens are never read or written through a link,
    /// so an update cannot write outside the goldens folder.
    #[error(
        "golden path {} is a symbolic link; goldens are not read or written through links",
        path.display()
    )]
    GoldenSymlink {
        /// The link found.
        path: PathBuf,
    },

    /// A filesystem operation failed.
    #[error("cannot {action} {}: {source}", path.display())]
    Io {
        /// What was being done, as a verb phrase ("create the temp root").
        action: &'static str,
        /// The path it was done to.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// No golden file exists for the output.
    #[error(
        "golden {} is missing; to create it, rerun with {UPDATE_VAR}=1 and review the new file\n--- actual\n{actual}",
        path.display()
    )]
    GoldenMissing {
        /// The golden file that was expected.
        path: PathBuf,
        /// The output that had nothing to compare with.
        actual: String,
    },

    /// The output differs from its golden file.
    #[error(
        "golden {} differs; if the change is intended, rerun with {UPDATE_VAR}=1 and review the diff\n--- expected\n{expected}\n--- actual\n{actual}",
        path.display()
    )]
    GoldenMismatch {
        /// The golden file compared with.
        path: PathBuf,
        /// The golden file's content.
        expected: String,
        /// The output.
        actual: String,
    },
}
