//! The run itself, on Linux and macOS: spawn, watch, stop, sweep, reap, drain.

use super::group::{Group, POLL_MAX, POLL_MIN};
use super::reader::Reader;
use super::{Captured, End, Outcome, Output, Overflow, Run};
use crate::Error;
use rustix::process::Signal;
use std::io::{self, Write as _};
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// The longest deadline or grace a run honours: about ten years. A longer
/// one, up to `Duration::MAX`, is treated as this.
const LONGEST_WAIT: Duration = Duration::from_secs(10 * 365 * 24 * 60 * 60);

/// How long the readers get after the sweep to reach end of file.
const DRAIN_WAIT: Duration = Duration::from_secs(2);

/// Why watching the leader stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Watched {
    Exited,
    TimedOut,
    OutputLimit,
}

pub(super) fn run(run: Run) -> Result<Outcome, Error> {
    let Run {
        mut command,
        deadline,
        input,
        output,
        grace,
    } = run;
    let program = command.get_program().to_owned();
    let spawn_error = |source| Error::Spawn {
        program: program.clone(),
        source,
    };

    command.process_group(0);
    command.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let combined = match output {
        Output::Separate { .. } => {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            None
        }
        Output::Combined(_) => {
            let (log, writer) = io::pipe().map_err(spawn_error)?;
            command
                .stdout(writer.try_clone().map_err(spawn_error)?)
                .stderr(writer);
            Some(log)
        }
    };
    let spawned = command.spawn();
    // The command holds the parent's copy of a combined log's write end; the
    // log reaches end of file only once it is closed.
    drop(command);
    let mut group = Group::new(spawned.map_err(spawn_error)?);
    let started = Instant::now();

    let (overflow_tx, overflow) = mpsc::channel();
    // A thread the OS cannot create fails the run; the guard sweeps.
    let readers = match group.child_mut() {
        Some(child) => {
            start_io(child, input, output, combined, &overflow_tx).map_err(spawn_error)?
        }
        None => Vec::new(),
    };
    drop(overflow_tx);

    // A deadline or grace too far to represent (`Duration::MAX`) saturates
    // to a finite bound, so the run always reaches SIGKILL and never panics.
    let watched = watch(&mut group, after(started, deadline), &overflow);
    if watched == Watched::TimedOut {
        // A failure here is met by the SIGKILL and the reap that follow.
        let _ = group.signal(Signal::TERM);
        group.wait_for_leader(after(Instant::now(), grace));
    }
    let reaped = group.reap();
    let swept = reaped.as_ref().is_some_and(|reaped| reaped.swept);

    let drained_by = Instant::now() + DRAIN_WAIT;
    let mut streams = readers.into_iter().map(|reader| reader.collect(drained_by));
    let stdout = streams.next();
    let stderr = streams.next();
    let kill_overflowed = [&stdout, &stderr]
        .into_iter()
        .flatten()
        .any(|(cap, captured)| cap.overflow() == Overflow::Kill && captured.truncated);
    let end = match watched {
        Watched::TimedOut => End::TimedOut,
        Watched::OutputLimit => End::OutputLimit,
        Watched::Exited if kill_overflowed => End::OutputLimit,
        Watched::Exited => match reaped.and_then(|reaped| reaped.status) {
            Some(Ok(status)) => End::Exited(status),
            Some(Err(source)) => return Err(spawn_error(source)),
            None => {
                return Err(spawn_error(io::Error::other(
                    "the child's exit status could not be collected",
                )));
            }
        },
    };
    Ok(Outcome {
        end,
        stdout: stdout.map_or_else(empty, |(_, captured)| captured),
        stderr: stderr.map_or_else(empty, |(_, captured)| captured),
        swept,
    })
}

/// Start the input writer and one reader per output pipe, stdout's first.
fn start_io(
    child: &mut Child,
    input: Option<Vec<u8>>,
    output: Output,
    combined: Option<io::PipeReader>,
    overflow: &Sender<()>,
) -> io::Result<Vec<Reader>> {
    if let (Some(mut pipe), Some(bytes)) = (child.stdin.take(), input) {
        // A child that stops reading gets its pipe closed by the sweep at the
        // latest; the write then fails and the thread ends.
        thread::Builder::new()
            .name(String::from("cw-supervise-stdin"))
            .spawn(move || {
                let _ = pipe.write_all(&bytes);
            })?;
    }
    let mut readers = Vec::with_capacity(2);
    match (output, combined) {
        (Output::Combined(cap), Some(log)) => {
            readers.push(Reader::start(log, cap, overflow.clone())?);
        }
        (Output::Separate { stdout, stderr }, _) => {
            if let Some(pipe) = child.stdout.take() {
                readers.push(Reader::start(pipe, stdout, overflow.clone())?);
            }
            if let Some(pipe) = child.stderr.take() {
                readers.push(Reader::start(pipe, stderr, overflow.clone())?);
            }
        }
        (Output::Combined(_), None) => {}
    }
    Ok(readers)
}

/// Watch until the leader exits, the deadline passes, or a stream passes a
/// kill cap, whichever comes first.
fn watch(group: &mut Group, deadline: Instant, overflow: &Receiver<()>) -> Watched {
    let mut poll = POLL_MIN;
    loop {
        if overflow.try_recv().is_ok() {
            return Watched::OutputLimit;
        }
        if group.leader_exited() {
            return Watched::Exited;
        }
        let now = Instant::now();
        if now >= deadline {
            return Watched::TimedOut;
        }
        let wait = poll.min(deadline - now);
        match overflow.recv_timeout(wait) {
            Ok(()) => return Watched::OutputLimit,
            Err(RecvTimeoutError::Timeout) => {}
            // Every reader has finished; nothing can overflow any more.
            Err(RecvTimeoutError::Disconnected) => thread::sleep(wait),
        }
        poll = (poll * 2).min(POLL_MAX);
    }
}

/// `from + wait`, with `wait` saturated to [`LONGEST_WAIT`] so the sum is
/// always representable.
fn after(from: Instant, wait: Duration) -> Instant {
    from.checked_add(wait.min(LONGEST_WAIT)).unwrap_or(from)
}

/// A stream that was not captured on a pipe of its own: nothing, and nothing
/// missing.
fn empty() -> Captured {
    Captured {
        bytes: Vec::new(),
        truncated: false,
        complete: true,
    }
}
