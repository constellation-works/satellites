//! What a run returns.

use std::process::ExitStatus;

/// How a run ended, with what each stream gave.
#[derive(Debug)]
#[must_use]
pub struct Outcome {
    /// Why the run ended.
    pub end: End,
    /// What the child wrote to stdout, or the interleaved log of both streams
    /// when the run was [`combined`](crate::Run::combined).
    pub stdout: Captured,
    /// What the child wrote to stderr; empty and complete when the run was
    /// [`combined`](crate::Run::combined).
    pub stderr: Captured,
    /// The final `SIGKILL` reached the group and the leader was reaped.
    ///
    /// `false` when cleanup failed or could not be verified: the group could
    /// not be signalled (the leader left it while alive, or this process may
    /// not signal it), or the leader was still alive 2 s after its own
    /// `SIGKILL` and is left unreaped rather than waited for without bound. `true` cannot vouch for a member this process may not
    /// signal: the kernel reports a group signal as sent once any member got
    /// it, and once the leader has exited a refused group signal is read as
    /// "only the exited leader was left" (macOS refuses a group of zombies).
    pub swept: bool,
}

/// Why a run ended. The group has been sent `SIGKILL` and the leader
/// reaped in every case, unless [`Outcome::swept`] says otherwise.
#[derive(Debug)]
pub enum End {
    /// The leader exited on its own, with this status. A signal that ended it
    /// is in the status too.
    Exited(ExitStatus),
    /// The deadline passed. The group got `SIGTERM`, the grace, then
    /// `SIGKILL`.
    TimedOut,
    /// A stream under [`Cap::kill`](crate::Cap::kill) passed its cap, and the
    /// group was stopped.
    OutputLimit,
}

/// The bytes read from one stream, and whether anything is missing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Captured {
    /// The bytes kept, at most the stream's cap.
    pub bytes: Vec<u8>,
    /// The stream wrote more than its cap; what followed was discarded.
    pub truncated: bool,
    /// Reading ended as it should: at end of file, or at a
    /// [`Cap::kill`](crate::Cap::kill) cap. `false` when the pipe was still
    /// held 2 s after the sweep (a process that left the group has it) or a
    /// read failed: `bytes` is then what was read so far, not the whole
    /// output.
    pub complete: bool,
}
