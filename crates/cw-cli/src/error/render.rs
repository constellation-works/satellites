//! A failure on stderr (STD-01 R19, R21).
//!
//! As text, the first line is `error: <message>`, where the message is the
//! error's own sentence followed by each cause in its source chain, and a
//! known fix follows on a `hint:` line. In JSON mode it is one object on one
//! line: `error` (the same message), `code` (the program's stable code) and
//! `hint` (`null` when there is none), so a script reads the fix as a field.

use super::Exit;
use crate::output::{Color, Role, Sink};
use crate::terminal::Mode;
use serde::Serialize;
use std::error::Error as _;
use std::io::Write;

/// The JSON form of a failure.
#[derive(Serialize)]
struct Envelope<'a> {
    error: &'a str,
    code: &'a str,
    hint: Option<&'a str>,
}

impl<O: Write, E: Write> Sink<O, E> {
    /// Write `error` to stderr as this sink's mode calls for, with `hint`,
    /// the fix where the program knows it, and return the exit code its kind
    /// calls for. Never fails: when stderr is gone there is nowhere to say
    /// so, and the exit code still reports the failure.
    pub fn report(&mut self, error: &cw_base::Error, hint: Option<&str>) -> Exit {
        let message = message(error);
        if self.terminal().mode() == Mode::Json {
            let envelope = Envelope {
                error: &message,
                code: error.code(),
                hint,
            };
            if serde_json::to_writer(&mut self.err, &envelope).is_ok() {
                let _ = writeln!(self.err);
            }
        } else {
            let color = Color::of(self.terminal().stderr_color());
            let prefix = Role::Error.paint("error:", color);
            let _ = writeln!(self.err, "{prefix} {}", crate::output::escape(&message));
            if let Some(hint) = hint {
                let _ = writeln!(self.err, "hint: {}", crate::output::escape(hint));
            }
        }
        Exit::of(error.kind())
    }
}

/// The error's sentence and every cause after it, each once.
fn message(error: &cw_base::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}
