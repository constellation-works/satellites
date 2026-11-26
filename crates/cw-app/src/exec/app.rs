//! What a program supplies to run through the chokepoint.

use super::context::Ctx;
use crate::audit::AuditMeta;

/// A program's commands, as the chokepoint runs them.
///
/// Implemented once per program (or per group of commands) on a marker
/// type; every surface then runs commands with [`super::exec()`].
pub trait App {
    /// What the program built at start and every command runs against: its
    /// store, its configuration.
    type State;
    /// What the program can be asked to do.
    type Command;
    /// What a command produced.
    type Outcome;
    /// Why a command failed, as the program's own typed error. Its one
    /// translator into [`cw_base::Error`] names the kind and the code.
    type Error: Into<cw_base::Error>;

    /// What the audit log records for `command`, or `None` when it is
    /// read-only and must not write. [`AuditMeta::new`] is a command that
    /// writes one record; [`AuditMeta::itemized`] one that writes zero or
    /// many, and admits them through [`Ctx::audit`].
    fn audit(command: &Self::Command) -> Option<AuditMeta>;

    /// Run `command`: dispatch only. The audit record and the error
    /// translation are the chokepoint's.
    ///
    /// # Errors
    ///
    /// The program's own error when the command fails.
    fn run(
        ctx: &Ctx<'_, Self::State>,
        command: Self::Command,
    ) -> Result<Self::Outcome, Self::Error>;

    /// The id of the record `outcome` acted on, when it names one, for the
    /// audit line. On success it wins over [`App::subject`]: a command that
    /// creates a record names the new id here. Not called for an itemized
    /// command: each item names its own subject.
    fn target(outcome: &Self::Outcome) -> Option<String>;

    /// The id of the record `command` is aimed at, resolved against the
    /// program's own state, or `None` when it names none or does not resolve.
    /// The default is `None`.
    ///
    /// [`super::exec()`] calls it only for a command [`App::audit`] declares
    /// mutating and not itemized, before the started line is appended, so the line names the
    /// record even when the command is refused or never returns. The
    /// outcome line keeps it when the command fails. It is a read: it
    /// must not change the store, and it cannot fail the command.
    ///
    /// Return a resolved id, never the selector the user typed (a title, a
    /// path, a prefix): the log writes the value only when it reads as an
    /// identifier of bounded length, and as [`crate::audit::TARGET_PLACEHOLDER`]
    /// otherwise, but that guard only bounds the damage.
    fn subject(ctx: &Ctx<'_, Self::State>, command: &Self::Command) -> Option<String> {
        let _ = (ctx, command);
        None
    }
}
