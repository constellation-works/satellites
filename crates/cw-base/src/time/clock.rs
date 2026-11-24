//! A source of the current time that a test can pin.

use super::timestamp::{TimestampError, format_rfc3339};
use time::OffsetDateTime;

/// Where the current time comes from.
///
/// A program builds [`Clock::system`] once, at its edge, and hands it down;
/// a test hands down [`Clock::fixed`] instead. Cheap to clone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    source: Source,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    System,
    Fixed(OffsetDateTime),
}

impl Clock {
    /// The operating system's clock, read at each [`Clock::now`].
    #[must_use]
    pub fn system() -> Self {
        Self {
            source: Source::System,
        }
    }

    /// A clock that always answers `at`, in UTC.
    #[must_use]
    pub fn fixed(at: OffsetDateTime) -> Self {
        Self {
            source: Source::Fixed(at.to_offset(time::UtcOffset::UTC)),
        }
    }

    /// The current time, in UTC.
    #[must_use]
    pub fn now(&self) -> OffsetDateTime {
        match self.source {
            Source::System => OffsetDateTime::now_utc(),
            Source::Fixed(at) => at,
        }
    }

    /// The current time as an RFC 3339 timestamp in UTC.
    ///
    /// # Errors
    ///
    /// [`TimestampError::Format`] when the time is outside the years RFC 3339
    /// can write (0 to 9999), which only a pinned clock can reach.
    pub fn now_rfc3339(&self) -> Result<String, TimestampError> {
        format_rfc3339(self.now())
    }
}
