//! The one path from any surface (a command line, an MCP server) to a
//! program's commands, and what that path needs: the context a command runs
//! in, the environment captured once, and an append-only audit log.
//!
//! - [`exec()`]: the chokepoint. A program describes itself once as an
//!   [`App`]; every surface runs a command through [`exec()`], which starts
//!   its audit record, runs it against a [`Ctx`], translates its error into
//!   [`cw_base::Error`] once, and returns a [`Finished`] for the surface to
//!   render and then settle.
//! - [`env`](mod@env): [`Env::capture`], the only read of the process environment,
//!   the data directory resolved from it as a pure function, and the
//!   declaration of every variable a program reads ([`EnvVar`]), rendered as
//!   one usage table.
//! - [`audit`]: the record a mutating command leaves in `<root>/audit.jsonl`,
//!   its versioned line format, and a reader.
//! - [`error`]: the typed [`Error`] this crate fails with.
//!
//! Mechanism only: the crate is generic over what a program built at start
//! ([`App::State`]) and names no store, record, command or error code.
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

pub use audit::{AuditLog, AuditMeta, AuditWarning, Pending, Record, Status};
/// The byte limit accepted by [`AuditLog::read`].
pub use cw_fs::Cap;
pub use env::{AppEnv, Env, EnvVar};
pub use error::{Error, Result};
pub use exec::{App, Ctx, Finished, Origin, Runtime, Surface, exec};
