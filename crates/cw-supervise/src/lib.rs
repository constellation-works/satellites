//! Run a child process within bounds: in its own process group, with a
//! required deadline, a polite-then-forced stop, a sweep of the whole group on
//! every exit path, and capped output.
//!
//! ```no_run
//! use cw_supervise::{Cap, End, Run, env};
//! use std::process::Command;
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), cw_supervise::Error> {
//! let mut command = Command::new("git");
//! command.args(["status", "--porcelain"]);
//! env::allow(&mut command, &["PATH", "HOME", "LANG"]);
//!
//! let outcome = Run::new(command, Duration::from_secs(30))
//!     .stdout(Cap::truncate(1 << 20))
//!     .stderr(Cap::truncate(64 << 10))
//!     .run()?;
//! match outcome.end {
//!     End::Exited(status) if status.success() => {}
//!     End::Exited(_) | End::TimedOut | End::OutputLimit => {
//!         // The stderr of a failed or hung child is the diagnosis.
//!         let _diagnosis = String::from_utf8_lossy(&outcome.stderr.bytes);
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! - [`run`](mod@run): [`Run`], the builder, and what a run returns: an
//!   [`Outcome`] with its [`End`] and each stream's [`Captured`] bytes, each
//!   kept within a [`Cap`].
//! - [`env`](mod@env): clear a command's environment and pass only named variables.
//! - [`error`]: the [`Error`] a run fails with, which is only a failure to
//!   start or an unsupported platform. A timeout and an output overflow are
//!   outcomes that carry what was captured.
//!
//! # What the crate guarantees
//!
//! - **The crate isolates the child.** It puts the child in a new process
//!   group and sets all three stdio handles itself, so a caller cannot forget
//!   either. Program, arguments, environment, working directory and inherited
//!   descriptors stay the caller's; the crate closes no descriptor.
//! - **At the deadline** the group gets `SIGTERM`, then up to the grace
//!   (ending early once the leader exits), then `SIGKILL` regardless, then the
//!   leader is reaped.
//! - **Every path ends with a sweep**, a clean exit included: `SIGKILL` to
//!   the group while its leader is still unreaped, then the reap. No signal is
//!   sent after the reap, so a reused process id is never hit. A drop guard
//!   does the same on a panic or an early return. The final wait for the
//!   leader is bounded.
//! - **Output is never lost silently.** After the sweep the readers get up to
//!   2 s to reach end of file. A stream whose pipe is still held is returned
//!   with what was read and `complete: false`, and the real exit status is
//!   kept.
//!
//! # Limits
//!
//! - A descendant that calls `setsid` or daemonizes has left the group and is
//!   not swept; a pipe it still holds is reported with `complete: false`.
//!   Stopped descendants are not continued before the sweep (`SIGKILL` ends
//!   them anyway).
//! - Such a descendant can also keep a reader thread (and a stdin writer
//!   thread, if it holds stdin without reading) blocked, with its pipe, after
//!   the run has returned; it ends when the pipe closes. The crate is sized
//!   for a command-line tool, where the process exits soon after; a
//!   long-lived caller that runs children which escape their group pays a
//!   thread and a descriptor per escaped pipe.
//! - The grace after `SIGTERM` ends as soon as the leader exits, as the
//!   design decided: a descendant still shutting down then gets `SIGKILL`
//!   before the rest of the grace. This is deliberate.
//! - When cleanup fails or cannot be verified (the group signal is refused,
//!   or the leader is still alive 2 s after its own `SIGKILL`), the run still
//!   returns, with [`Outcome::swept`] `false`, instead of waiting without
//!   bound. A leader left unreaped that way stays a zombie once it exits,
//!   until the calling process exits. On macOS `swept` is also `false` when
//!   the leader exited before the sweep: the kernel refuses a signal to a
//!   group of zombies with the error it uses for a member it may not signal.
//! - A deadline or grace longer than about ten years is treated as ten
//!   years, so every run reaches `SIGKILL`.
//!
//! # Preconditions
//!
//! - The calling process reaps its children only through this crate: no
//!   other thread waits on any child (`wait`, `waitpid(-1, ..)`, a global
//!   child reaper), and `SIGCHLD` is neither ignored nor set with
//!   `SA_NOCLDWAIT`. The crate signals the group only while the leader is
//!   unreaped, and only this crate's own wait guarantees that; a reaper
//!   elsewhere could let a signal reach a reused process group id. When the
//!   crate sees the leader was reaped elsewhere it sends nothing more, and a
//!   run that needs the exit status fails with [`Error::Spawn`].
//!
//! # Platforms
//!
//! Linux and macOS. On any other platform every run returns
//! [`Error::Unsupported`]; there is no weaker fallback that would let a
//! caller believe a group was swept.

pub mod env;
pub mod error;
pub mod run;

pub use error::Error;
pub use run::{Cap, Captured, End, Outcome, Overflow, Run};
