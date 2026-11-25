//! A failed tool, as the client reads it.

use cw_base::{Error, Kind};
use serde_json::{Value, json};

/// The record a failed tool answers with: the program's stable `code`, the
/// error's `message`, its `kind` ([`kind_name`]) and whether retrying the
/// same call may succeed (`retryable`, [`is_retryable`]).
///
/// ```
/// use cw_base::{Error, Kind};
/// use serde_json::json;
///
/// #[derive(Debug, thiserror::Error)]
/// #[error("the store is locked by another process")]
/// struct Locked;
///
/// let error = Error::new(Kind::Busy, "store_locked", Locked);
/// assert_eq!(
///     cw_mcp::payload::tool_error(&error),
///     json!({
///         "code": "store_locked",
///         "message": "the store is locked by another process",
///         "kind": "busy",
///         "retryable": true,
///     })
/// );
/// ```
#[must_use]
pub fn tool_error(error: &Error) -> Value {
    json!({
        "code": error.code(),
        "message": error.to_string(),
        "kind": kind_name(error.kind()),
        "retryable": is_retryable(error.kind()),
    })
}

/// The `snake_case` name of `kind` on the wire.
#[must_use]
pub fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Usage => "usage",
        Kind::NotFound => "not_found",
        Kind::Refused => "refused",
        Kind::Busy => "busy",
        Kind::Corrupt => "corrupt",
        Kind::Incompatible => "incompatible",
        Kind::Config => "config",
        Kind::Io => "io",
        Kind::Internal => "internal",
    }
}

/// Whether the same call may succeed if retried unchanged: only when
/// another process held what it needed ([`Kind::Busy`]).
#[must_use]
pub fn is_retryable(kind: Kind) -> bool {
    match kind {
        Kind::Busy => true,
        Kind::Usage
        | Kind::NotFound
        | Kind::Refused
        | Kind::Corrupt
        | Kind::Incompatible
        | Kind::Config
        | Kind::Io
        | Kind::Internal => false,
    }
}
