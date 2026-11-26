//! The record of one command between its start and its outcome.

use super::line::{Phase, Record, Status};
use super::log::AuditLog;
use super::meta::AuditMeta;
use crate::error::{Error, Result};
use crate::exec::Origin;
use std::fmt;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

/// The outcome of an audited command, waiting to be written.
///
/// [`crate::exec()`] returns it inside a [`crate::Finished`]: the surface
/// renders the command's result first, then calls [`Pending::settle`],
/// which writes the outcome and hands back any failure as a warning to print.
/// Dropping an unsettled record still attempts to write a known outcome.
/// A command that never returned leaves only its started line. Read-only
/// commands and commands refused before starting have no pending outcome.
#[must_use = "settle the record after rendering, so a failure to write it can be reported"]
#[derive(Debug)]
pub struct Pending {
    entry: Option<Entry>,
}

#[derive(Debug)]
struct Entry {
    log: AuditLog,
    wait: Duration,
    started: Instant,
    command: &'static str,
    outcome: Phase,
    record: Record,
}

impl Pending {
    /// No pending audit outcome.
    pub(crate) fn none() -> Self {
        Self { entry: None }
    }

    /// Write the started line before the command can run. `subject` is the
    /// id of the record the command is aimed at, when the app resolved one;
    /// the started line carries it as the target.
    pub(crate) fn start(
        log: AuditLog,
        wait: Duration,
        meta: AuditMeta,
        subject: Option<String>,
        at: OffsetDateTime,
        origin: &Origin,
    ) -> Result<Self, cw_base::Error> {
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(|error| {
            cw_base::Error::new(cw_base::Kind::Internal, "audit_id_failed", error)
        })?;
        let id = format!("{:032x}", u128::from_be_bytes(random));
        let record = Record {
            at: cw_base::time::format_rfc3339(at)
                .map_err(Error::from)
                .map_err(start_error)?,
            surface: Some(origin.surface().as_str().to_owned()),
            actor: origin.actor().map(str::to_owned),
            command: meta.command().to_owned(),
            status: Status::Unknown,
            target: subject,
            error_code: None,
            duration_ms: None,
        };
        log.append_phase(&record, &Phase::Started { id: id.clone() }, wait)
            .map_err(start_error)?;
        Ok(Self {
            entry: Some(Entry {
                log,
                wait,
                started: Instant::now(),
                command: meta.command(),
                outcome: Phase::Outcome { id },
                record,
            }),
        })
    }

    /// Record how the command ended: on success the record it acted on, if
    /// it names one (otherwise the started line's subject stays); on failure
    /// the stable error code, with the subject kept beside it.
    pub(crate) fn mark(&mut self, result: std::result::Result<Option<String>, &'static str>) {
        let Some(entry) = self.entry.as_mut() else {
            return;
        };
        let record = &mut entry.record;
        record.duration_ms =
            Some(u64::try_from(entry.started.elapsed().as_millis()).unwrap_or(u64::MAX));
        match result {
            Ok(target) => {
                record.status = Status::Success;
                if target.is_some() {
                    record.target = target;
                }
            }
            Err(code) => {
                record.status = Status::Failure;
                record.error_code = Some(code.to_owned());
            }
        }
    }

    /// Whether a started audit line was written for the command.
    #[must_use]
    pub fn is_audited(&self) -> bool {
        self.entry.is_some()
    }

    /// Write the outcome after the surface has rendered the result. A failure
    /// comes back as the warning for the surface to print; it never fails
    /// the command, which already ran. `None` when the line was written or
    /// the command is not audited.
    #[must_use = "a failure to write the audit line is for the surface to print"]
    pub fn settle(mut self) -> Option<AuditWarning> {
        let entry = self.entry.take()?;
        entry.write_outcome().err().map(|error| AuditWarning {
            command: entry.command,
            error,
        })
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        // A surface may drop a known outcome before settling. The start on
        // disk still records the attempt if the outcome cannot be written.
        if let Some(entry) = self.entry.take() {
            let _ = entry.write_outcome();
        }
    }
}

impl Entry {
    fn write_outcome(&self) -> Result<()> {
        if self.record.status == Status::Unknown {
            return Ok(());
        }
        self.log
            .append_phase(&self.record, &self.outcome, self.wait)
    }
}

fn start_error(error: Error) -> cw_base::Error {
    cw_base::Error::new(error.kind(), "audit_start_failed", error)
}

/// An audit outcome that could not be written. A warning for the surface to
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
            "the audit outcome for `{}` was not written: {}",
            self.command, self.error
        )
    }
}
