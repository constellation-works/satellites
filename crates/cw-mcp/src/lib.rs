//! A Model Context Protocol server over stdio, serving a program's commands
//! as a list of typed tools through the same entry point its command line
//! calls.
//!
//! - [`tool`]: a [`Tool`] is a name, a description, an input type whose
//!   schema is derived from it, a read-only or mutating mark ([`Access`]),
//!   and a synchronous function.
//! - [`server`]: [`Server`] checks the tool list once when it is built, and
//!   [`serve_stdio`] serves it until standard input closes.
//! - [`payload`]: what a client receives: a tool's JSON as structured
//!   content, and a failed tool as a normal result marked as an error that
//!   carries `code`, `message`, `kind` and `retryable`.
//! - [`error`]: the typed [`Error`] this crate fails with.
//! - `testing` (feature `test-support`): an in-process harness that sends raw
//!   JSON-RPC lines to a server over an in-memory stream, for a project's own
//!   wire tests.
//!
//! Synchronous on the outside: the server builds its own runtime, and every
//! call runs its tool's function on a blocking thread, so no public
//! signature names an async runtime or the protocol SDK. Mechanism only: the
//! crate names no command, record or error code of any project.
//!
//! ```no_run
//! use cw_app::{Origin, Runtime};
//! use cw_mcp::{Access, ServerInfo, Tool, serve_stdio};
//! use serde_json::{Value, json};
//!
//! #[derive(serde::Deserialize, schemars::JsonSchema)]
//! struct EchoInput {
//!     /// What to say back.
//!     text: String,
//! }
//!
//! fn echo(_: &Runtime<()>, _: &Origin, input: EchoInput) -> Result<Value, cw_base::Error> {
//!     Ok(json!({ "text": input.text }))
//! }
//!
//! let tools = vec![Tool::new::<EchoInput>("echo", "Say it back.", Access::ReadOnly, echo)];
//! let info = ServerInfo::new("echo", "0.1.0", "echo_internal");
//! let root = std::env::temp_dir().join("echo");
//! serve_stdio(info, Runtime::new(root, ()), tools)?; // blocks until stdin closes
//! # Ok::<(), cw_mcp::Error>(())
//! ```

pub mod error;
pub mod payload;
pub mod server;
#[cfg(feature = "test-support")]
pub mod testing;
pub mod tool;

mod wire;

pub use error::{Error, Result};
pub use server::{Server, ServerInfo, serve_stdio};
pub use tool::{Access, Tool};

/// The `schemars` this crate derives input schemas with, so a project
/// derives `JsonSchema` from the same version.
pub use schemars;
