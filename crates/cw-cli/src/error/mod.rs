//! Errors: how a failure is shown and what the process exits with, and the
//! typed error this crate fails with.
//!
//! - [`Sink::report`](crate::Sink::report) writes a [`cw_base::Error`] to
//!   stderr: `error: <message>` as text, or one JSON object
//!   `{"error", "code", "hint"}` in [`Mode::Json`](crate::Mode::Json), and
//!   returns the [`Exit`] its kind calls for.
//! - [`exit_code`] is the one mapping from [`Kind`] to an exit
//!   code: 2 for a usage error, 1 for every other kind.
//! - [`Error`] is what this crate itself fails with. It has a kind and no
//!   code: the program that wraps it names its own.

mod exit;
mod render;

pub use exit::{Exit, exit_code};

use cw_base::Kind;
use std::io;

/// Why writing output failed.
///
/// Not `#[non_exhaustive]`: a program that wraps it gives every variant a
/// code, so adding a variant is meant to break its match.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A write to stdout failed for a reason other than a closed reader.
    #[error("cannot write to stdout")]
    Output(#[source] io::Error),
    /// The payload cannot be written as JSON.
    #[error("cannot encode the output as JSON")]
    Encode(#[source] serde_json::Error),
    /// A command's one-line summary is empty or over the limit: the check
    /// a program's test runs ([`crate::help::check_summaries`]).
    #[error("{} command summaries are empty or over {limit} characters: {}", .violations.len(), .violations.join("; "))]
    Summaries {
        /// The limit the summaries were held to.
        limit: usize,
        /// One line per command that broke it: its path, its length and its
        /// summary.
        violations: Vec<String>,
    },
}

impl Error {
    /// How this failure is classified. Exhaustive, so a new variant decides.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::Output(_) => Kind::Io,
            Self::Encode(_) | Self::Summaries { .. } => Kind::Internal,
        }
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
