//! What a run fails with.
//!
//! Only two things are errors: the child could not be started, and the
//! platform cannot run one within bounds. A timeout and an output overflow are
//! [`End`](crate::End) values that carry what was captured, because the
//! stderr of a hung child is the diagnosis.
//!
//! The crate stands alone: a unit that wraps [`Error`] maps each variant to
//! its own kind and code in its `From` impl.

use std::ffi::OsString;
use std::io;

/// Why a run did not produce an [`Outcome`](crate::Outcome).
///
/// Not `#[non_exhaustive]`: a unit that wraps it gives every variant a stable
/// code, so adding a variant is meant to break its match.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The child could not be started (or, when the calling process ignores
    /// `SIGCHLD`, its exit status could not be collected).
    ///
    /// `source.kind()` tells "not installed" ([`io::ErrorKind::NotFound`])
    /// from "not allowed" or any other failure.
    #[error("cannot run {}", program.to_string_lossy())]
    Spawn {
        /// The program as the caller named it on the command.
        program: OsString,
        /// The operating-system error.
        #[source]
        source: io::Error,
    },
    /// This platform has no process groups to sweep, so nothing is run.
    #[error("cannot run a supervised child on this platform: it needs Unix process groups")]
    Unsupported,
}
