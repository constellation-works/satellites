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
//! - [`Mode::Json`]: the payload as one JSON document, compact by default
//!   and indented with [`Sink::pretty_json`].
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
pub use style::{Color, Role};
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
    pretty: bool,
}

/// One part of a detail view, optionally painted with a semantic role.
///
/// Line breaks and tabs retain their layout; other control characters are
/// escaped before painting, just as in [`Sink::emit_text`].
#[derive(Clone, Debug)]
pub struct TextSpan {
    text: String,
    role: Role,
}

impl TextSpan {
    /// A plain span of text.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            role: Role::Neutral,
        }
    }

    /// Paint this span with `role` when stdout colour is enabled.
    #[must_use]
    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }
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
            pretty: false,
        }
    }

    /// Write the JSON form indented, as `serde_json::to_string_pretty` writes
    /// it, with one line break after it. Without this call the JSON form is
    /// compact, on one line. Only [`Mode::Json`] output changes: tables and
    /// plain lines are the same either way, and the latch and error handling
    /// are too.
    #[must_use]
    pub fn pretty_json(mut self) -> Self {
        self.pretty = true;
        self
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
            Mode::Json => self.json(payload)?,
            Mode::Table => {
                human(payload).render(self.terminal.width(), Color::of(self.terminal.color()))
            }
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
            Mode::Json => self.json(payload)?,
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

    /// Write a detail view made of spans, painting their roles when stdout
    /// colour is enabled. JSON mode writes only `payload` and does not call
    /// `human`. Human output ends in a line break, as with [`Sink::emit_text`].
    ///
    /// # Errors
    ///
    /// [`Error::Encode`] when `payload` cannot be written as JSON.
    pub fn emit_text_spans<T: Serialize + ?Sized>(
        &mut self,
        payload: &T,
        human: impl FnOnce(&T) -> Vec<TextSpan>,
    ) -> Result<()> {
        let text = match self.terminal.mode() {
            Mode::Json => self.json(payload)?,
            Mode::Table | Mode::Plain => {
                let spans = human(payload);
                let mut text = String::new();
                let mut ends_in_newline = false;
                for span in spans {
                    let escaped = escape_text(&span.text);
                    if !escaped.is_empty() {
                        ends_in_newline = escaped.ends_with('\n');
                    }
                    text.push_str(&span.role.paint(&escaped, Color::of(self.terminal.color())));
                }
                if !ends_in_newline {
                    text.push('\n');
                }
                text
            }
        };
        self.write_out(&text);
        Ok(())
    }

    /// Write one line to stderr, with every control character escaped
    /// (`\n`, `\t`, `\r`, else `\u{...}`), so the text cannot break into a
    /// second line that reads as the program's own. For several lines, see
    /// [`Sink::notice_lines`]. Never fails.
    pub fn notice(&mut self, text: &str) {
        let _ = writeln!(self.err, "{}", escape(text));
    }

    /// Write several lines to stderr, each escaped as [`Sink::notice`] does,
    /// one line per entry. This is the only way to write a multi-line
    /// notice: a line feed in [`Sink::notice`] comes out as `\n`, never as a
    /// line break. Never fails.
    pub fn notice_lines(&mut self, lines: impl IntoIterator<Item = impl AsRef<str>>) {
        for line in lines {
            self.notice(line.as_ref());
        }
    }

    /// Write `warning: <text>` to stderr, the prefix coloured where stderr
    /// is, with every control character in `text` escaped as
    /// [`Sink::notice`] does. For several lines, see [`Sink::warn_lines`].
    /// Never fails.
    pub fn warn(&mut self, text: &str) {
        let prefix = Role::Warn.paint("warning:", Color::of(self.terminal.stderr_color()));
        let _ = writeln!(self.err, "{prefix} {}", escape(text));
    }

    /// Write each of `lines` as its own `warning: <line>` line, each escaped
    /// as [`Sink::warn`] does. This is the only way to write a multi-line
    /// warning: a line feed in [`Sink::warn`] comes out as `\n`, never as a
    /// line break. Never fails.
    pub fn warn_lines(&mut self, lines: impl IntoIterator<Item = impl AsRef<str>>) {
        for line in lines {
            self.warn(line.as_ref());
        }
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

    /// `payload` as one JSON document and a line break: compact, or indented
    /// when [`Sink::pretty_json`] asked for it.
    fn json<T: Serialize + ?Sized>(&self, payload: &T) -> Result<String> {
        let mut text = if self.pretty {
            serde_json::to_string_pretty(payload)
        } else {
            serde_json::to_string(payload)
        }
        .map_err(Error::Encode)?;
        text.push('\n');
        Ok(text)
    }
}
