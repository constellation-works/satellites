//! What every command line repeats, as mechanism: one terminal decision, a
//! sink whose stdout carries only the payload, error rendering with the exit
//! code from its kind, logging to stderr, and grouped root help.
//!
//! - [`terminal`]: [`Terminal`], the one decision about colour, width and
//!   output mode, made from the environment [`cw_app::Env`] captured and a
//!   [`Probe`] of the streams. Nothing else asks whether a stream is a
//!   terminal.
//! - [`output`]: [`Sink`], the only path to stdout and stderr. A payload is
//!   rendered as a borderless [`Table`], as tab-separated lines, or as JSON;
//!   a closed stdout ends the writes, not the command.
//! - [`error`]: [`Sink::report`] writes a [`cw_base::Error`] as
//!   `error: <message>` or one JSON object, and returns its [`Exit`].
//! - [`logging`]: [`Logging`], `tracing` to stderr at the level `<APP>_LOG`
//!   names, `warn` by default, never fatal.
//! - [`help`]: [`help::grouped`] root help and the
//!   [`help::check_summaries`] check a program's test calls.
//!
//! The crate names no command, record or error code, and reads no variable
//! itself: a program passes its name and the captured environment.
//!
//! ```
//! use cw_app::Env;
//! use cw_cli::{Cell, Column, Format, Probe, Sink, Table, Terminal};
//!
//! let env = Env::from_vars([("NOTES_FORMAT", "table")]);
//! let terminal = Terminal::decide(&env, "notes", Probe::piped(), None::<Format>);
//! let mut sink = Sink::new(terminal, Vec::new(), Vec::new());
//! let notes = vec![("n1", Some("draft")), ("n2", None)];
//! sink.emit(&notes, |notes| {
//!     let mut table = Table::new([Column::new("ID"), Column::new("TITLE")]);
//!     for (id, title) in notes {
//!         table.push([Cell::new(*id), Cell::from(title.map(str::to_owned))]);
//!     }
//!     table
//! })?;
//! sink.finish()?;
//! let (out, _err) = sink.into_parts();
//! assert_eq!(String::from_utf8_lossy(&out), "ID  TITLE\nn1  draft\nn2  -\n");
//! # Ok::<(), cw_cli::Error>(())
//! ```

pub mod error;
pub mod help;
pub mod logging;
pub mod output;
pub mod terminal;

pub use error::{Error, Exit, Result, exit_code};
pub use logging::Logging;
pub use output::{Align, Cell, Column, Role, Sink, Table, escape};
pub use terminal::{Format, Mode, Probe, Terminal};
