//! An itemized command's audit: zero or many started and outcome pairs,
//! admitted inside the command's transaction.

use super::line::{Phase, Record, Status};
use super::log::AuditLog;
use super::meta::AuditMeta;
use super::pending::{fresh_id, start_error};
use crate::error::Error;
use crate::exec::Origin;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

/// One record an itemized command wrote or refused: the id of the record it
/// is aimed at, and what became of it.
///
/// Each item is one ordinary started and outcome pair under its own id, so
/// [`crate::AuditLog::read`] pairs it like any other command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    subject: Option<String>,
    refusal: Option<&'static str>,
}

impl Item {
    /// An item whose write happened. `subject` is the id of the record, a
    /// resolved id and never what the user typed; the log writes it only when
    /// it reads as an identifier of bounded length
    /// ([`crate::audit::TARGET_MAX_LEN`]), and as
    /// [`crate::audit::TARGET_PLACEHOLDER`] otherwise.
    #[must_use]
    pub fn wrote(subject: Option<String>) -> Self {
        Self {
            subject,
            refusal: None,
        }
    }

    /// An item the command refused, with the stable `code` that says why.
    /// Its outcome line is a failure carrying that code and the subject.
    #[must_use]
    pub fn refused(subject: Option<String>, code: &'static str) -> Self {
        Self {
            subject,
            refusal: Some(code),
        }
    }

    /// The id of the record the item names, when it names one.
    #[must_use]
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    /// The stable code the item was refused with, or `None` when it wrote.
    #[must_use]
    pub fn refusal(&self) -> Option<&'static str> {
        self.refusal
    }
}

/// The audit handle of a [`crate::Ctx`]: how a command admits its items.
///
/// - Under [`crate::exec()`], a command [`crate::App::audit`] declares
///   itemized admits through it; a command that is not itemized gets
///   [`Error::NotItemized`].
/// - On a context built with [`crate::Ctx::new`] (a test of one command) it is
///   detached: [`AuditHandle::admit`] writes nothing and succeeds, so a
///   command's own logic can be tested without a log.
#[derive(Clone, Copy, Debug)]
pub struct AuditHandle<'a> {
    mode: Mode<'a>,
}

#[derive(Clone, Copy, Debug)]
enum Mode<'a> {
    Detached,
    NotItemized,
    Itemized(&'a Live),
}

impl<'a> AuditHandle<'a> {
    pub(crate) fn detached() -> Self {
        Self {
            mode: Mode::Detached,
        }
    }

    pub(crate) fn not_itemized() -> Self {
        Self {
            mode: Mode::NotItemized,
        }
    }

    pub(crate) fn itemized(live: &'a Live) -> Self {
        Self {
            mode: Mode::Itemized(live),
        }
    }

    /// Append one started line per item, in one append under one hold of
    /// the audit lock, and keep the batch for the surface to settle after
    /// the reply.
    ///
    /// Call it inside the command's transaction, after the last write and
    /// before the commit. Each item gets its own 128-bit id. No items writes
    /// nothing and takes no lock: the command admitted nothing. A command
    /// admits once; the kit settles the batch:
    ///
    /// - when `run` returns `Ok`, each item's outcome is what was admitted
    ///   (success, or failure with its refusal code);
    /// - when `run` returns `Err`, every item settles as a failure with the
    ///   command's code, so a rolled-back write never has a success line.
    ///
    /// # Errors
    ///
    /// `audit_start_failed` (with a typed [`crate::Error`] cause) when the
    /// append fails, or `audit_id_failed` when an id cannot be generated: the
    /// command must return it and roll its transaction back, and it leaves
    /// no outcome line. [`Error::NotItemized`] and [`Error::AlreadyAdmitted`]
    /// are programming errors, with kind `internal`.
    pub fn admit(
        &self,
        items: impl IntoIterator<Item = Item>,
    ) -> Result<Batch<'a>, cw_base::Error> {
        let items: Vec<Item> = items.into_iter().collect();
        match self.mode {
            Mode::Detached => Ok(Batch { live: None }),
            Mode::NotItemized => Err(misuse("audit_not_itemized", Error::NotItemized)),
            Mode::Itemized(live) => {
                if items.is_empty() {
                    return Ok(Batch { live: None });
                }
                live.admit(items)?;
                Ok(Batch { live: Some(live) })
            }
        }
    }
}

/// The items one command admitted. The kit keeps the batch and settles it
/// after the reply; hold the value only to call [`Batch::committed`].
#[derive(Debug)]
pub struct Batch<'a> {
    live: Option<&'a Live>,
}

impl Batch<'_> {
    /// The commit has happened: fix the batch's results as admitted and
    /// write the outcome lines now, in one append.
    ///
    /// For a command whose commit is not its last fallible step (a stepwise
    /// command that commits around other work). A later failure of `run`
    /// does not change these lines, and the later settle adds none. A write
    /// failure is kept as the [`crate::AuditWarning`] settle returns.
    pub fn committed(self) {
        if let Some(live) = self.live {
            live.commit();
        }
    }
}

/// The live audit of one itemized command under [`crate::exec()`]: owned by
/// `exec` while the command runs, then handed to the pending record.
#[derive(Debug)]
pub(crate) struct Live {
    log: AuditLog,
    wait: Duration,
    command: &'static str,
    at: OffsetDateTime,
    surface: String,
    actor: Option<String>,
    started: Instant,
    slot: Mutex<Slot>,
    failure: Option<&'static str>,
    ran_ms: Option<u64>,
}

#[derive(Debug)]
enum Slot {
    /// Nothing admitted yet.
    Empty,
    /// The started append failed: the log is left alone from here.
    StartFailed,
    Admitted(Admitted),
}

#[derive(Debug)]
struct Admitted {
    items: Vec<Entry>,
    /// The outcome lines were written (or their write failed) already.
    written: bool,
    /// The failure of an early write, for settle to return.
    held: Option<Error>,
}

#[derive(Debug)]
struct Entry {
    id: String,
    record: Record,
    refusal: Option<&'static str>,
}

impl Live {
    pub(crate) fn new(
        log: AuditLog,
        wait: Duration,
        meta: AuditMeta,
        at: OffsetDateTime,
        origin: &Origin,
    ) -> Self {
        Self {
            log,
            wait,
            command: meta.command(),
            at,
            surface: origin.surface().as_str().to_owned(),
            actor: origin.actor().map(str::to_owned),
            started: Instant::now(),
            slot: Mutex::new(Slot::Empty),
            failure: None,
            ran_ms: None,
        }
    }

    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn base(&self, at: &str) -> Record {
        Record {
            at: at.to_owned(),
            surface: Some(self.surface.clone()),
            self_reported_actor: self.actor.clone(),
            euid: None,
            command: self.command.to_owned(),
            status: Status::Unknown,
            target: None,
            error_code: None,
            duration_ms: None,
        }
    }

    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn admit(&self, items: Vec<Item>) -> Result<(), cw_base::Error> {
        let mut slot = self.slot();
        match *slot {
            Slot::Empty => {}
            Slot::StartFailed | Slot::Admitted(_) => {
                return Err(misuse("audit_admitted_twice", Error::AlreadyAdmitted));
            }
        }
        match self.start(items) {
            Ok(admitted) => {
                *slot = Slot::Admitted(admitted);
                Ok(())
            }
            Err(error) => {
                *slot = Slot::StartFailed;
                Err(error)
            }
        }
    }

    fn start(&self, items: Vec<Item>) -> Result<Admitted, cw_base::Error> {
        let at = cw_base::time::format_rfc3339(self.at)
            .map_err(Error::from)
            .map_err(start_error)?;
        let mut entries = Vec::with_capacity(items.len());
        for item in items {
            let mut record = self.base(&at);
            record.target = item.subject;
            entries.push(Entry {
                id: fresh_id()?,
                record,
                refusal: item.refusal,
            });
        }
        let phases: Vec<Phase> = entries
            .iter()
            .map(|entry| Phase::Started {
                id: entry.id.clone(),
            })
            .collect();
        let lines: Vec<(&Record, &Phase)> = entries
            .iter()
            .map(|entry| &entry.record)
            .zip(&phases)
            .collect();
        self.log
            .append_lines(&lines, self.wait)
            .map_err(start_error)?;
        Ok(Admitted {
            items: entries,
            written: false,
            held: None,
        })
    }

    fn commit(&self) {
        let mut slot = self.slot();
        if let Slot::Admitted(admitted) = &mut *slot
            && !admitted.written
        {
            let duration = self.elapsed_ms();
            admitted.written = true;
            admitted.held = self.write_items(admitted, None, duration).err();
        }
    }

    /// The outcome lines of every admitted item, in one append: as admitted
    /// when `failure` is `None`, else every one a failure with that code.
    fn write_items(
        &self,
        admitted: &Admitted,
        failure: Option<&'static str>,
        duration_ms: u64,
    ) -> Result<(), Error> {
        let outcomes: Vec<(Record, Phase)> = admitted
            .items
            .iter()
            .map(|entry| {
                let mut record = entry.record.clone();
                record.duration_ms = Some(duration_ms);
                match failure.or(entry.refusal) {
                    Some(code) => {
                        record.status = Status::Failure;
                        record.error_code = Some(code.to_owned());
                    }
                    None => record.status = Status::Success,
                }
                (
                    record,
                    Phase::Outcome {
                        id: entry.id.clone(),
                    },
                )
            })
            .collect();
        let lines: Vec<(&Record, &Phase)> = outcomes
            .iter()
            .map(|(record, phase)| (record, phase))
            .collect();
        self.log.append_lines(&lines, self.wait)
    }

    /// Record how `run` ended: `Some(code)` when it returned an error.
    pub(crate) fn mark(&mut self, failure: Option<&'static str>) {
        self.failure = failure;
        self.ran_ms = Some(self.elapsed_ms());
    }

    /// How many items were admitted.
    pub(crate) fn admitted(&self) -> usize {
        match &*self.slot() {
            Slot::Admitted(admitted) => admitted.items.len(),
            Slot::Empty | Slot::StartFailed => 0,
        }
    }

    pub(crate) fn command(&self) -> &'static str {
        self.command
    }

    /// Write whatever outcome is still owed: the admitted items' lines, or
    /// one standalone failure line when the command failed with nothing
    /// admitted. A failed admit and an already written batch owe nothing; an
    /// early write that failed is returned here, once, as the warning.
    pub(crate) fn write_outcomes(mut self) -> Result<(), Error> {
        let duration = self.ran_ms.unwrap_or_else(|| self.elapsed_ms());
        let slot = std::mem::replace(
            self.slot.get_mut().unwrap_or_else(PoisonError::into_inner),
            Slot::Empty,
        );
        match slot {
            Slot::StartFailed => Ok(()),
            Slot::Empty => {
                let Some(code) = self.failure else {
                    return Ok(());
                };
                let at = cw_base::time::format_rfc3339(self.at)?;
                let mut record = self.base(&at);
                record.status = Status::Failure;
                record.error_code = Some(code.to_owned());
                record.duration_ms = Some(duration);
                self.log.append(&record, self.wait)
            }
            Slot::Admitted(admitted) if admitted.written => admitted.held.map_or(Ok(()), Err),
            Slot::Admitted(admitted) => self.write_items(&admitted, self.failure, duration),
        }
    }
}

/// A programming error in how an app uses the handle.
fn misuse(code: &'static str, error: Error) -> cw_base::Error {
    cw_base::Error::new(error.kind(), code, error)
}
