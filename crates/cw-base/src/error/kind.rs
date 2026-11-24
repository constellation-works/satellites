//! How a failure is classified.

/// What kind of failure an [`super::Error`] is, named for what the caller
/// can do about it.
///
/// The unit that knows the failure decides its kind, once, where it wraps its
/// typed error; a surface decides from the kind alone (an exit code, a retry)
/// and never from a message.
///
/// Not `#[non_exhaustive]`: every surface must decide what a new kind means,
/// so adding one is meant to break their matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The request as typed is malformed (an unknown flag, a missing
    /// argument, two that exclude each other): fix the invocation. A command
    /// line exits 2 for it, and 1 for every other kind.
    Usage,
    /// The record the request names does not exist: name one that does.
    NotFound,
    /// The request is not allowed as it stands, by a rule of the project or a
    /// safety check: change the request or the state it names.
    Refused,
    /// Another process holds what this one needs: retry when it finishes.
    Busy,
    /// Stored state is not valid: repair the data.
    Corrupt,
    /// Stored state was written by a newer build: upgrade this tool.
    Incompatible,
    /// The environment and the flags do not say how to run: fix them.
    Config,
    /// The operating system failed an operation: see the cause.
    Io,
    /// A state the program should not reach: a bug to report.
    Internal,
}
