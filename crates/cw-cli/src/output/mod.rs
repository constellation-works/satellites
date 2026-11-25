//! The sink: the only path from a command to stdout and stderr.
//!
//! stdout carries the payload and nothing else; notices, warnings and errors
//! go to stderr in every mode (STD-01 R12). A command builds one payload and
//! hands it to [`Sink::emit`] with the table that is its human view, so every
//! rendering comes from the same value (STD-01 R6):
//!
//! - [`Mode::Table`]: a borderless table ([`Table`]), fitted to the
//!   terminal's width when there is one, and nothing at all when it has no
//!   rows;
//! - [`Mode::Plain`]: the same rows, tab-separated, with no header;
//! - [`Mode::Json`]: the payload as one JSON document.
//!
//! stdout is latched: a closed reader (`tool list | head -1`) stops the
//! writes but not the command, which runs to its end; [`Sink::finish`] then
//! reports success, and the process exits 0 (STD-01 R13). A write that fails
//! some other way comes back from [`Sink::finish`] once the command is done.
//! Writes to stderr are best effort: when stderr is gone there is nowhere to
//! say so, and a notice must never cost the command.

mod escape;
mod latch;
mod style;
mod table;

pub use escape::escape;
pub use style::Role;
pub use table::{Align, Cell, Column, Table};

use crate::error::{Error, Result};
use crate::terminal::{Mode, Terminal};
use escape::escape_text;
use latch::Latch;
use serde::Serialize;
use std::io::{self, Write};

/// Where a command's output goes, and how it is rendered there.
///
/// [`Sink::stdio`] writes to the process's streams; [`Sink::new`] takes any
/// pair of writers, so a test reads back what was written.
#[derive(Debug)]
pub struct Sink<O: Write = io::Stdout, E: Write = io::Stderr> {
    terminal: Terminal,
    out: Latch<O>,
    pub(crate) err: E,
}

impl Sink {
    /// A sink on the process's stdout and stderr.
    #[must_use]
    pub fn stdio(terminal: Terminal) -> Self {
        Self::new(terminal, io::stdout(), io::stderr())
    }
}

impl<O: Write, E: Write> Sink<O, E> {
    /// A sink that writes the payload to `out` and diagnostics to `err`, as
    /// `terminal` decided.
    #[must_use]
    pub fn new(terminal: Terminal, out: O, err: E) -> Self {
        Self {
            terminal,
            out: Latch::new(out),
            err,
        }
    }

    /// The decision this sink renders by.
    #[must_use]
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    /// Write `payload`: as JSON in [`Mode::Json`], else as the table `human`
    /// builds from it.
    ///
    /// # Errors
    ///
    /// [`Error::Encode`] when `payload` cannot be written as JSON; nothing is
    /// written then. A failed write is not an error here: see
    /// [`Sink::finish`].
    pub fn emit<T: Serialize + ?Sized>(
        &mut self,
        payload: &T,
        human: impl FnOnce(&T) -> Table,
    ) -> Result<()> {
        let text = match self.terminal.mode() {
            Mode::Json => json(payload)?,
            Mode::Table => human(payload).render(self.terminal.width(), self.terminal.color()),
            Mode::Plain => human(payload).render_plain(),
        };
        self.write_out(&text);
        Ok(())
    }

    /// Write `payload`: as JSON in [`Mode::Json`], else as the text `human`
    /// writes from it, for a detail view that is not a list. The text is
    /// escaped except for its line breaks and tabs, and ends in a line break.
    ///
    /// # Errors
    ///
    /// [`Error::Encode`] when `payload` cannot be written as JSON.
    pub fn emit_text<T: Serialize + ?Sized>(
        &mut self,
        payload: &T,
        human: impl FnOnce(&T) -> String,
    ) -> Result<()> {
        let text = match self.terminal.mode() {
            Mode::Json => json(payload)?,
            Mode::Table | Mode::Plain => {
                let mut text = escape_text(&human(payload)).into_owned();
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                text
            }
        };
        self.write_out(&text);
        Ok(())
    }

    /// Write one line to stderr, escaped: progress, a count, the line naming
    /// what an empty query searched. Never fails.
    pub fn notice(&mut self, text: &str) {
        let _ = writeln!(self.err, "{}", escape_text(text));
    }

    /// Write `warning: <text>` to stderr, the prefix coloured where stderr
    /// is. Never fails.
    pub fn warn(&mut self, text: &str) {
        let prefix = Role::Warn.paint("warning:", self.terminal.stderr_color());
        let _ = writeln!(self.err, "{prefix} {}", escape_text(text));
    }

    /// Whether stdout has stopped reaching a reader: a command with nothing
    /// left to do but write may stop early.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.out.is_closed()
    }

    /// Flush stdout once the command is done. A closed reader is success.
    ///
    /// # Errors
    ///
    /// [`Error::Output`] when a write to stdout failed for any other reason;
    /// the program wraps it with its own code and reports it.
    pub fn finish(&mut self) -> Result<()> {
        // The latch hands back only `Interrupted`: retry it until the flush
        // lands or latches.
        while let Err(error) = self.out.flush() {
            if error.kind() != io::ErrorKind::Interrupted {
                self.out.fail(error);
                break;
            }
        }
        match self.out.take_failure() {
            Some(error) => Err(Error::Output(error)),
            None => Ok(()),
        }
    }

    /// The two writers, for a test to read back.
    #[must_use]
    pub fn into_parts(self) -> (O, E) {
        (self.out.into_inner(), self.err)
    }

    fn write_out(&mut self, text: &str) {
        // The latch returns no error but `Interrupted`, which `write_all`
        // retries, and keeps any failure for `finish`. Anything else that
        // reaches here is kept the same way.
        if let Err(error) = self.out.write_all(text.as_bytes()) {
            self.out.fail(error);
        }
    }
}

/// `payload` as one JSON document and a line break.
fn json<T: Serialize + ?Sized>(payload: &T) -> Result<String> {
    let mut text = serde_json::to_string(payload).map_err(Error::Encode)?;
    text.push('\n');
    Ok(text)
}
