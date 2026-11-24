//! The base every kit crate and every unit of a Rust CLI builds on: one
//! error type that keeps a unit's typed error whole, and a clock a test can
//! pin.
//!
//! - [`error`]: [`Error`], a unit's typed error with its [`Kind`] and the
//!   stable code its project gives it.
//! - [`time`]: [`Clock`], and the one timestamp format, RFC 3339 in UTC.
//!
//! No I/O and no environment: nothing here touches a file, a variable or a
//! process, and the clock is read only when a caller asks a [`Clock`] for
//! the time.

pub mod error;
pub mod time;

pub use error::{Error, Kind};
pub use time::{Clock, TimestampError};
