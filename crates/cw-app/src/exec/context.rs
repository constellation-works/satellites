//! What a command runs against.

use super::origin::Origin;
use crate::audit::{AuditHandle, AuditLog};
use cw_base::Clock;
use std::path::{Path, PathBuf};
use std::time::Duration;
use time::OffsetDateTime;

/// How long an audit append waits for the audit lock by default. The lock
/// is held only for one append, so a longer wait means a stuck holder.
const AUDIT_WAIT: Duration = Duration::from_secs(5);

/// What a program built at start, owned for the life of the process: its
/// state, its data directory (where the audit log lives) and its clock.
#[derive(Debug)]
pub struct Runtime<S> {
    state: S,
    audit: AuditLog,
    clock: Clock,
    audit_wait: Duration,
}

impl<S> Runtime<S> {
    /// A runtime over `state`, auditing to `<root>/audit.jsonl`, with the
    /// system clock and a 5 s wait for the audit lock.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, state: S) -> Self {
        Self {
            state,
            audit: AuditLog::new(root),
            clock: Clock::system(),
            audit_wait: AUDIT_WAIT,
        }
    }

    /// The same runtime reading the time from `clock`: a test pins it.
    #[must_use]
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// The same runtime waiting up to `wait` for the audit lock.
    #[must_use]
    pub fn with_audit_wait(mut self, wait: Duration) -> Self {
        self.audit_wait = wait;
        self
    }

    /// The program's state.
    #[must_use]
    pub fn state(&self) -> &S {
        &self.state
    }

    /// The data directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.audit.root()
    }

    /// The clock.
    #[must_use]
    pub fn clock(&self) -> Clock {
        self.clock
    }

    /// The audit log, for a surface that reads it.
    #[must_use]
    pub fn audit_log(&self) -> &AuditLog {
        &self.audit
    }

    /// How long an audit append waits for the audit lock.
    #[must_use]
    pub fn audit_wait(&self) -> Duration {
        self.audit_wait
    }

    /// The context a command from `origin` runs against. Its audit handle is
    /// detached, as with [`Ctx::new`]; [`super::exec()`] builds its own.
    #[must_use]
    pub fn ctx<'a>(&'a self, origin: &'a Origin) -> Ctx<'a, S> {
        Ctx::new(&self.state, self.clock, origin)
    }

    /// The context [`super::exec()`] runs a command in, with `audit` as the
    /// handle through which an itemized command admits its items.
    pub(super) fn ctx_with<'a>(&'a self, origin: &'a Origin, audit: AuditHandle<'a>) -> Ctx<'a, S> {
        Ctx {
            audit,
            ..Ctx::new(&self.state, self.clock, origin)
        }
    }
}

/// The borrowed view a command runs against: the program's state, the
/// clock, the client and the audit handle. A command reads the time and the actor from here,
/// never from the system, so a test that pins them pins every record.
#[derive(Debug)]
pub struct Ctx<'a, S> {
    state: &'a S,
    clock: Clock,
    origin: &'a Origin,
    audit: AuditHandle<'a>,
}

impl<'a, S> Ctx<'a, S> {
    /// A context over `state`, reading the time from `clock`, for `origin`.
    /// A test of one command builds it directly; its audit handle is
    /// detached ([`AuditHandle::admit`] writes nothing and succeeds).
    #[must_use]
    pub fn new(state: &'a S, clock: Clock, origin: &'a Origin) -> Self {
        Self {
            state,
            clock,
            origin,
            audit: AuditHandle::detached(),
        }
    }

    /// The program's state.
    #[must_use]
    pub fn state(&self) -> &'a S {
        self.state
    }

    /// The clock.
    #[must_use]
    pub fn clock(&self) -> Clock {
        self.clock
    }

    /// The current time, in UTC.
    #[must_use]
    pub fn now(&self) -> OffsetDateTime {
        self.clock.now()
    }

    /// Where the command came from.
    #[must_use]
    pub fn origin(&self) -> &'a Origin {
        self.origin
    }

    /// Who the client said it was.
    #[must_use]
    pub fn actor(&self) -> Option<&'a str> {
        self.origin.actor()
    }

    /// How an itemized command admits its items: call
    /// [`AuditHandle::admit`] inside the transaction, after the last write
    /// and before the commit.
    #[must_use]
    pub fn audit(&self) -> &AuditHandle<'a> {
        &self.audit
    }
}
