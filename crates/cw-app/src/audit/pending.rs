//! The record of one command between its start and its line.

use super::actor::recorded;
use super::line::{Record, Status};
use super::log::AuditLog;
use super::meta::AuditMeta;
use crate::error::{Error, Result};
use crate::exec::Origin;
use std::fmt;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

/// The audit record of one command, waiting to be written.
///
/// [`crate::exec()`] returns it inside a [`crate::Finished`]: the surface
/// renders the command's result first, then calls [`Pending::settle`],
/// which writes the line and hands back any failure as a warning to print.
/// A record dropped without being settled (the command panicked, or a
/// surface forgot) still writes its line, and a failure to do so is lost.
/// A read-only command's record is empty and writes nothing.
#[must_use = "settle the record after rendering, so a failure to write it can be reported"]
#[derive(Debug)]
pub struct Pending {
    entry: Option<Entry>,
}

#[derive(Debug)]
struct Entry {
    log: AuditLog,
    wait: Duration,
    at: OffsetDateTime,
    started: Instant,
    took: Option<Duration>,
    surface: &'static str,
    actor: Option<String>,
    command: &'static str,
    status: Status,
    target: Option<String>,
    error_code: Option<&'static str>,
}

impl Pending {
    /// A record for a read-only command: nothing to write.
    pub(crate) fn none() -> Self {
        Self { entry: None }
    }

    /// Start the record of `meta`, begun at `at` through `origin`, to go to
    /// `log`, waiting up to `wait` for the audit lock. Until marked, the
    /// command reads as failed.
    pub(crate) fn start(
        log: AuditLog,
        wait: Duration,
        meta: AuditMeta,
        at: OffsetDateTime,
        origin: &Origin,
    ) -> Self {
        Self {
            entry: Some(Entry {
                log,
                wait,
                at,
                started: Instant::now(),
                took: None,
                surface: origin.surface().as_str(),
                actor: origin.actor().map(str::to_owned),
                command: meta.command(),
                status: Status::Failure,
                target: None,
                error_code: None,
            }),
        }
    }

    /// Record how the command ended: on success the record it acted on, if
    /// it names one; on failure the stable error code.
    pub(crate) fn mark(&mut self, result: std::result::Result<Option<String>, &'static str>) {
        let Some(entry) = self.entry.as_mut() else {
            return;
        };
        entry.took = Some(entry.started.elapsed());
        match result {
            Ok(target) => {
                entry.status = Status::Success;
                entry.target = target;
                entry.error_code = None;
            }
            Err(code) => {
                entry.status = Status::Failure;
                entry.target = None;
                entry.error_code = Some(code);
            }
        }
    }

    /// Whether the command is audited: `false` for a read-only command.
    #[must_use]
    pub fn is_audited(&self) -> bool {
        self.entry.is_some()
    }

    /// Write the line, after the surface has rendered the result. A failure
    /// comes back as the warning for the surface to print; it never fails
    /// the command, which already ran. `None` when the line was written or
    /// the command is not audited.
    #[must_use = "a failure to write the audit line is for the surface to print"]
    pub fn settle(mut self) -> Option<AuditWarning> {
        let entry = self.entry.take()?;
        entry.write().err().map(|error| AuditWarning {
            command: entry.command,
            error,
        })
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        // Fail open, and never panic in a destructor: an unsettled record
        // still writes its line, and a failure here has no one to tell.
        if let Some(entry) = self.entry.take() {
            let _ = entry.write();
        }
    }
}

impl Entry {
    fn write(&self) -> Result<()> {
        let took = self.took.unwrap_or_else(|| self.started.elapsed());
        let record = Record {
            at: cw_base::time::format_rfc3339(self.at)?,
            surface: Some(self.surface.to_owned()),
            actor: self
                .actor
                .as_deref()
                .map(|actor| recorded(actor).to_owned()),
            command: self.command.to_owned(),
            status: self.status,
            target: self.target.clone(),
            error_code: self.error_code.map(str::to_owned),
            duration_ms: u64::try_from(took.as_millis()).unwrap_or(u64::MAX),
        };
        self.log.append(&record, self.wait)
    }
}

/// An audit line that could not be written: a warning for the surface to
/// print, never a failure of the command.
#[derive(Debug)]
pub struct AuditWarning {
    command: &'static str,
    error: Error,
}

impl AuditWarning {
    /// The command whose line was not written.
    #[must_use]
    pub fn command(&self) -> &'static str {
        self.command
    }

    /// Why it was not written.
    #[must_use]
    pub fn error(&self) -> &Error {
        &self.error
    }

    /// The error, for a surface that wraps it with its own code.
    #[must_use]
    pub fn into_error(self) -> Error {
        self.error
    }
}

impl fmt::Display for AuditWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the audit line for `{}` was not written: {}",
            self.command, self.error
        )
    }
}
