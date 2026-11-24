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
    /// read-only and must not write.
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
    /// audit line.
    fn target(outcome: &Self::Outcome) -> Option<String>;
}
