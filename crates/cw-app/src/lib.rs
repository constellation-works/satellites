//! The one path from any surface (a command line, an MCP server) to a
//! program's commands, and what that path needs: the context a command runs
//! in, the environment captured once, and an append-only audit log.
//!
//! - [`exec()`]: the chokepoint. A program describes itself once as an
//!   [`App`]; every surface runs a command through [`exec()`], which appends
//!   a started line for a mutating command before running it against a
//!   [`Ctx`], translates its error into
//!   [`cw_base::Error`] once, and returns a [`Finished`] for the surface to
//!   render and then settle. A command that writes zero or many records is
//!   itemized ([`AuditMeta::itemized`]): it admits its items through
//!   [`Ctx::audit`] inside its transaction, before its commit.
//! - [`env`](mod@env): [`Env::capture`], the only read of the process environment,
//!   the data directory resolved from it as a pure function, and the
//!   declaration of every variable a program reads ([`EnvVar`]), rendered as
//!   one usage table.
//! - [`audit`]: the started and outcome lines a mutating command leaves in
//!   `<root>/audit.jsonl`, their versioned format, and a reader that pairs them.
//!   A started line already names the record the command is aimed at when the
//!   app implements [`App::subject`].
//! - [`testing`]: a helper that fails when an itemized command returns `Ok`
//!   having written without admitting its items.
//! - [`error`]: the typed [`Error`] this crate fails with.
//!
//! The crate is generic over what the program built at start ([`App::State`]),
//! including its store, records, commands and command-specific error codes.
//!
//! ```
//! use cw_app::{App, AuditMeta, Ctx, Origin, Runtime, Surface, exec};
//! use cw_base::{Error, Kind};
//!
//! #[derive(Debug, thiserror::Error)]
//! #[error("nothing to greet")]
//! struct Empty;
//!
//! impl From<Empty> for Error {
//!     fn from(error: Empty) -> Self {
//!         Error::new(Kind::Usage, "greet_empty", error)
//!     }
//! }
//!
//! struct Greeter;
//!
//! impl App for Greeter {
//!     type State = ();
//!     type Command = String;
//!     type Outcome = String;
//!     type Error = Empty;
//!
//!     fn audit(_: &String) -> Option<AuditMeta> {
//!         None // read-only: no audit line
//!     }
//!
//!     fn run(_: &Ctx<'_, ()>, name: String) -> Result<String, Empty> {
//!         if name.is_empty() { Err(Empty) } else { Ok(format!("hello, {name}")) }
//!     }
//!
//!     fn target(_: &String) -> Option<String> {
//!         None
//!     }
//!
//!     // Optional. A mutating command returns the id of the record it is
//!     // aimed at, resolved from its own state, never what the user typed;
//!     // the default is `None`.
//!     fn subject(_: &Ctx<'_, ()>, _: &String) -> Option<String> {
//!         None
//!     }
//! }
//!
//! let root = tempfile::tempdir()?;
//! let runtime = Runtime::new(root.path(), ());
//! let origin = Origin::new(Surface::CLI, None);
//! let finished = exec::<Greeter>(&runtime, &origin, "world".to_owned());
//! assert_eq!(finished.result().ok().map(String::as_str), Some("hello, world"));
//! let (_result, warning) = finished.settle();
//! assert!(warning.is_none());
//! # Ok::<(), std::io::Error>(())
//! ```

pub mod audit;
pub mod env;
pub mod error;
pub mod exec;
pub mod testing;

pub use audit::{
    AuditHandle, AuditLog, AuditMeta, AuditWarning, Batch, Item, Pending, Record, Status,
};
/// The byte limit accepted by [`AuditLog::read`].
pub use cw_fs::Cap;
pub use env::{AppEnv, Env, EnvVar};
pub use error::{Error, Result};
pub use exec::{App, Ctx, Finished, Origin, Runtime, Surface, exec};
