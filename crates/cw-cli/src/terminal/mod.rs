//! The one terminal decision: whether to colour each stream, the width a
//! table must fit, and the output mode (STD-01 R8, R17).
//!
//! It is made once, from the environment snapshot [`cw_app::Env`] captured
//! and a [`Probe`] of the streams, and every renderer reads its answers from
//! the [`Terminal`] it is handed. This folder is the only code in the crate
//! that asks whether a stream is a terminal or names the colour variables;
//! `scripts/check-dependency-direction.sh` fails when any other file does.
//!
//! - Colour is off when the stream is not a terminal, when `TERM=dumb`, and
//!   when `NO_COLOR` is non-empty. A non-empty `CLICOLOR_FORCE` turns it on
//!   for a terminal, even a dumb one, but never over `NO_COLOR` and never
//!   for a pipe.
//! - The width is known only when stdout is a terminal: `COLUMNS` when it
//!   is a positive number, else the terminal's own size. With no width a table is never cut.
//! - The format comes from the flag, then `<APP>_FORMAT`, then `auto`. An
//!   unrecognised variable value falls back to `auto` rather than failing.

mod format;
mod probe;

pub use format::{Format, Mode};
pub use probe::Probe;

use cw_app::Env;

/// The decision, made once per invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Terminal {
    mode: Mode,
    color: bool,
    stderr_color: bool,
    width: Option<usize>,
}

impl Terminal {
    /// Decide for the running process: probe its streams, then
    /// [`Terminal::decide`].
    #[must_use]
    pub fn detect(env: &Env, app: &str, requested: Option<Format>) -> Self {
        Self::decide(env, app, Probe::detect(), requested)
    }

    /// Decide from a captured environment and a probe of the streams.
    ///
    /// Pure: it reads nothing but its arguments, so a test builds both.
    /// `app` is the program's name, which names its `<APP>_FORMAT`
    /// ([`cw_app::AppEnv`]); `requested` is the format the command line
    /// asked for, if any.
    #[must_use]
    pub fn decide(env: &Env, app: &str, probe: Probe, requested: Option<Format>) -> Self {
        let from_env = env
            .app(app)
            .format()
            .and_then(|value| value.to_str())
            .and_then(Format::parse);
        let mode = match requested.or(from_env).unwrap_or(Format::Auto) {
            Format::Json => Mode::Json,
            Format::Table => Mode::Table,
            Format::Auto if probe.stdout => Mode::Table,
            Format::Auto => Mode::Plain,
        };
        let width = if probe.stdout {
            // Each is checked before the next is tried, so an unusable
            // `COLUMNS` (`0`, `wide`) falls back to the measured width.
            let usable = |columns: &usize| *columns > 0;
            env.var("COLUMNS")
                .and_then(|value| value.to_str())
                .and_then(|value| value.trim().parse::<usize>().ok())
                .filter(usable)
                .or(probe.columns.filter(usable))
        } else {
            None
        };
        Self {
            mode,
            color: colors(env, probe.stdout),
            stderr_color: colors(env, probe.stderr),
            width,
        }
    }

    /// How the payload is rendered.
    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Whether stdout may carry colour escapes.
    #[must_use]
    pub fn color(&self) -> bool {
        self.color
    }

    /// Whether stderr may carry colour escapes, decided by the same rules for
    /// its own stream: `tool | less` still colours an error on the terminal.
    #[must_use]
    pub fn stderr_color(&self) -> bool {
        self.stderr_color
    }

    /// The width a table must fit, or `None`: stdout is not a terminal, or
    /// its width is unknown. `None` means never cut.
    #[must_use]
    pub fn width(&self) -> Option<usize> {
        self.width
    }
}

/// Whether a stream that is (`tty`) or is not a terminal is coloured.
fn colors(env: &Env, tty: bool) -> bool {
    if !tty || env.no_color() {
        return false;
    }
    let dumb = env.term().is_some_and(|term| term == "dumb");
    !dumb || env.var("CLICOLOR_FORCE").is_some()
}
