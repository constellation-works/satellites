//! A stdout that a closed reader cannot end early.
//!
//! `tool list | head -1` closes the pipe while the tool is still writing. The
//! first write that fails latches the stream shut: it and every later write
//! return as though they had succeeded, and nothing more is written. So the
//! command runs to its end, including the commit that records its change,
//! and the process exits as the command decided, with nothing on stderr
//! (STD-01 R13). A failure other than a closed pipe (a full disk behind a
//! redirect) latches too, for the same reason, and is handed back once the
//! command is done.

use std::io::{self, ErrorKind, Write};

/// What the stream has done so far.
#[derive(Debug)]
enum State {
    /// Every write so far has landed.
    Open,
    /// The reader went away, or a failure was already handed back.
    Closed,
    /// A write failed some other way.
    Failed(io::Error),
}

/// A writer that stops at its first failed write.
#[derive(Debug)]
pub(crate) struct Latch<W> {
    inner: W,
    state: State,
}

impl<W: Write> Latch<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self {
            inner,
            state: State::Open,
        }
    }

    /// Run `op` while the stream is open. A failure latches it and reads as
    /// success. `Interrupted` is not a failure: it goes back to the caller,
    /// whose `write_all` retries it.
    fn pass<T>(&mut self, op: impl FnOnce(&mut W) -> io::Result<T>) -> io::Result<Option<T>> {
        if !matches!(self.state, State::Open) {
            return Ok(None);
        }
        match op(&mut self.inner) {
            Ok(done) => Ok(Some(done)),
            Err(error) if error.kind() == ErrorKind::Interrupted => Err(error),
            Err(error) if error.kind() == ErrorKind::BrokenPipe => {
                self.state = State::Closed;
                Ok(None)
            }
            Err(error) => {
                self.state = State::Failed(error);
                Ok(None)
            }
        }
    }

    /// Whether writes have stopped reaching a reader.
    pub(crate) fn is_closed(&self) -> bool {
        !matches!(self.state, State::Open)
    }

    /// The failure that stopped the stream, unless it was a closed pipe,
    /// which is how a pipe is meant to end. Taken once.
    pub(crate) fn take_failure(&mut self) -> Option<io::Error> {
        match std::mem::replace(&mut self.state, State::Closed) {
            State::Failed(error) => Some(error),
            State::Open => {
                self.state = State::Open;
                None
            }
            State::Closed => None,
        }
    }

    pub(crate) fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for Latch<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(self.pass(|w| w.write(buf))?.unwrap_or(buf.len()))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.pass(Write::flush).map(drop)
    }
}
