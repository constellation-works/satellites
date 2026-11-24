//! Time as a program records it: a [`Clock`] a test can pin, and one
//! timestamp format, RFC 3339 in UTC.
//!
//! Code that stamps a record asks a [`Clock`] it was handed, never the
//! system directly, so a test that pins the clock pins every stamp.
//!
//! ```
//! use cw_base::time::{self, Clock};
//!
//! let at = time::parse_rfc3339("2026-11-24T09:30:00Z")?;
//! let clock = Clock::fixed(at);
//! assert_eq!(clock.now_rfc3339()?, "2026-11-24T09:30:00Z");
//! # Ok::<(), cw_base::TimestampError>(())
//! ```

mod clock;
mod timestamp;

pub use clock::Clock;
pub use timestamp::{TimestampError, format_rfc3339, parse_rfc3339};
