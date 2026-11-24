//! A unit's typed error carried as `cw_base::Error`, and a stamp from a
//! pinned clock.
//!
//! Run with `cargo run -p cw-base --example unit_error`.

use cw_base::time::{self, Clock};
use cw_base::{Error, Kind};
use std::io::Write as _;

/// A unit's own error: what it matches on internally.
#[derive(Debug, thiserror::Error)]
enum QueueError {
    #[error("the queue is full ({0} items); drain it first")]
    Full(usize),
}

/// The unit's one translator: the kind and the project's own code.
impl From<QueueError> for Error {
    fn from(error: QueueError) -> Self {
        match &error {
            QueueError::Full(_) => Self::new(Kind::Busy, "queue_full", error),
        }
    }
}

fn push(len: usize) -> Result<(), Error> {
    if len >= 3 {
        return Err(QueueError::Full(len).into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let clock = Clock::fixed(time::parse_rfc3339("2026-11-24T09:30:00Z")?);
    let mut out = std::io::stdout().lock();
    let Err(error) = push(3) else {
        return Ok(());
    };
    // A surface decides from the kind; a caller that knows the unit asks
    // for its typed error.
    let retry = matches!(error.kind(), Kind::Busy);
    writeln!(
        out,
        "{} [{}] at {} (retry: {retry})",
        error,
        error.code(),
        clock.now_rfc3339()?
    )?;
    if let Some(QueueError::Full(len)) = error.downcast_ref::<QueueError>() {
        writeln!(out, "the unit saw {len} items")?;
    }
    Ok(())
}
