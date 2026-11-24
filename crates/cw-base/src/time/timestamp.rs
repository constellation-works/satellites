//! The one timestamp format: RFC 3339, in UTC.

use crate::error::Kind;
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

/// Why a timestamp could not be written or read.
///
/// Not `#[non_exhaustive]`: a unit that wraps it gives every variant a code,
/// so adding a variant is meant to break its match.
#[derive(Debug, thiserror::Error)]
pub enum TimestampError {
    /// The time cannot be written as RFC 3339 (its year is outside 0 to 9999).
    #[error("cannot write {at} as an RFC 3339 timestamp")]
    Format {
        /// The time that was to be written.
        at: OffsetDateTime,
        /// What the formatter reported.
        #[source]
        source: time::error::Format,
    },
    /// The text is not an RFC 3339 timestamp.
    #[error("{text:?} is not an RFC 3339 timestamp")]
    Parse {
        /// The text that was read.
        text: String,
        /// What the parser reported.
        #[source]
        source: time::error::Parse,
    },
}

impl TimestampError {
    /// How this failure is classified: a time no program should hold is a
    /// bug, and unreadable stored text is corrupt data.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::Format { .. } => Kind::Internal,
            Self::Parse { .. } => Kind::Corrupt,
        }
    }
}

/// Write `at` as an RFC 3339 timestamp in UTC, such as
/// `2026-11-24T09:30:00Z`.
///
/// # Errors
///
/// [`TimestampError::Format`] when the year is outside 0 to 9999.
pub fn format_rfc3339(at: OffsetDateTime) -> Result<String, TimestampError> {
    let at = at.to_offset(UtcOffset::UTC);
    at.format(&Rfc3339)
        .map_err(|source| TimestampError::Format { at, source })
}

/// Read an RFC 3339 timestamp, with any offset, as a time in UTC.
///
/// # Errors
///
/// [`TimestampError::Parse`] when `text` is not one.
pub fn parse_rfc3339(text: &str) -> Result<OffsetDateTime, TimestampError> {
    OffsetDateTime::parse(text, &Rfc3339)
        .map(|at| at.to_offset(UtcOffset::UTC))
        .map_err(|source| TimestampError::Parse {
            text: text.to_owned(),
            source,
        })
}
