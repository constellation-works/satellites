//! One bounded run of a child process: the [`Run`] builder and what it
//! returns.
//!
//! ```no_run
//! use cw_supervise::{Cap, Run};
//! use std::process::Command;
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), cw_supervise::Error> {
//! let mut command = Command::new("sort");
//! command.arg("-u");
//! let outcome = Run::new(command, Duration::from_secs(10)) // the deadline is required
//!     .stdin("b\na\nb\n")                                  // default: none, the child reads EOF
//!     .stdout(Cap::kill(1 << 20))                          // stop the group past 1 MiB
//!     .grace(Duration::from_millis(500))                   // default 1 s
//!     .run()?;
//! assert_eq!(outcome.stdout.bytes, b"a\nb\n");
//! # Ok(())
//! # }
//! ```

mod cap;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod group;
mod outcome;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod reader;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod supervise;

pub use cap::{Cap, Overflow};
pub use outcome::{Captured, End, Outcome};

use crate::Error;
use std::process::Command;
use std::time::Duration;

/// The grace after `SIGTERM` when the caller sets none.
const DEFAULT_GRACE: Duration = Duration::from_secs(1);

/// The stdout cap when the caller sets none: keep 1 MiB, drain the rest.
const DEFAULT_STDOUT: Cap = Cap::truncate(1 << 20);

/// The stderr cap when the caller sets none: keep 64 KiB, drain the rest.
const DEFAULT_STDERR: Cap = Cap::truncate(64 << 10);

/// A child process to run within bounds.
///
/// Built from the caller's [`Command`] and a required deadline; the other
/// settings have defaults. [`Run::run`] sets the process group and the three
/// stdio handles on the command itself, replacing anything the caller set
/// for them.
#[derive(Debug)]
#[must_use = "a Run does nothing until `run` is called"]
pub struct Run {
    command: Command,
    deadline: Duration,
    input: Option<Vec<u8>>,
    output: Output,
    grace: Duration,
}

/// Where the child's output goes.
#[derive(Clone, Copy, Debug)]
enum Output {
    /// Each stream on its own pipe, each with its own cap.
    Separate { stdout: Cap, stderr: Cap },
    /// Both streams on one pipe, as one interleaved log.
    Combined(Cap),
}

impl Run {
    /// A run of `command` that is stopped once `deadline` has passed since
    /// the child started.
    ///
    /// Defaults: no input (the child reads end of file), stdout kept up to
    /// 1 MiB and stderr up to 64 KiB, both under [`Overflow::Truncate`], and a
    /// 1 s grace between `SIGTERM` and `SIGKILL`.
    pub fn new(command: Command, deadline: Duration) -> Self {
        Self {
            command,
            deadline,
            input: None,
            output: Output::Separate {
                stdout: DEFAULT_STDOUT,
                stderr: DEFAULT_STDERR,
            },
            grace: DEFAULT_GRACE,
        }
    }

    /// Give the child `bytes` on its stdin, written from a thread; the pipe
    /// is closed when the write ends. A child that exits without reading all
    /// of it is not an error.
    pub fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.input = Some(bytes.into());
        self
    }

    /// Capture stdout on its own pipe within `cap`. Undoes
    /// [`combined`](Self::combined); stderr keeps its cap, or the default.
    pub fn stdout(mut self, cap: Cap) -> Self {
        self.output = Output::Separate {
            stdout: cap,
            stderr: self.separate_stderr(),
        };
        self
    }

    /// Capture stderr on its own pipe within `cap`. Undoes
    /// [`combined`](Self::combined); stdout keeps its cap, or the default.
    pub fn stderr(mut self, cap: Cap) -> Self {
        self.output = Output::Separate {
            stdout: self.separate_stdout(),
            stderr: cap,
        };
        self
    }

    /// Capture stdout and stderr as one interleaved log on a single pipe,
    /// within `cap`. The log is returned in [`Outcome::stdout`], and
    /// [`Outcome::stderr`] is empty and complete.
    pub fn combined(mut self, cap: Cap) -> Self {
        self.output = Output::Combined(cap);
        self
    }

    /// How long the group has between `SIGTERM` and `SIGKILL` once the
    /// deadline has passed. The wait ends early when the leader exits;
    /// `SIGKILL` follows either way. A grace, like a deadline, longer than
    /// about ten years is treated as ten years.
    pub fn grace(mut self, grace: Duration) -> Self {
        self.grace = grace;
        self
    }

    /// Start the child and supervise it to the end: exit, deadline or output
    /// limit, then the sweep, the reap and the drain.
    ///
    /// # Errors
    ///
    /// [`Error::Spawn`], keeping the program and the operating-system error,
    /// when the child cannot be started; when it started but its supervision
    /// could not be set up (the OS could not create a reader or writer
    /// thread), after the group has been swept; or when its exit status
    /// could not be collected because it was reaped elsewhere.
    /// [`Error::Unsupported`] on a platform other than Linux and macOS.
    pub fn run(self) -> Result<Outcome, Error> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            supervise::run(self)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            drop(self);
            Err(Error::Unsupported)
        }
    }

    fn separate_stdout(&self) -> Cap {
        match self.output {
            Output::Separate { stdout, .. } => stdout,
            Output::Combined(_) => DEFAULT_STDOUT,
        }
    }

    fn separate_stderr(&self) -> Cap {
        match self.output {
            Output::Separate { stderr, .. } => stderr,
            Output::Combined(_) => DEFAULT_STDERR,
        }
    }
}
