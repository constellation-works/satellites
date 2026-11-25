//! The exit code a failure calls for (STD-01 R20).

use cw_base::Kind;
use std::process::ExitCode;

/// The code a process exits with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Exit(u8);

impl Exit {
    /// The command succeeded: 0.
    pub const SUCCESS: Self = Self(0);

    /// The code a failure of `kind` calls for ([`exit_code`]).
    #[must_use]
    pub fn of(kind: Kind) -> Self {
        Self(exit_code(kind))
    }

    /// The code as a number.
    #[must_use]
    pub fn code(self) -> u8 {
        self.0
    }
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        Self::from(exit.0)
    }
}

/// The exit code for a failure of `kind`: 2 for a usage error, 1 for every
/// other kind. Exhaustive, so a new kind decides here; any further code would
/// need documenting in the command's help.
#[must_use]
pub fn exit_code(kind: Kind) -> u8 {
    match kind {
        Kind::Usage => 2,
        Kind::NotFound
        | Kind::Refused
        | Kind::Busy
        | Kind::Corrupt
        | Kind::Incompatible
        | Kind::Config
        | Kind::Io
        | Kind::Internal => 1,
    }
}
