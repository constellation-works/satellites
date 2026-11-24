//! The server: a checked list of tools over a program's runtime, served
//! over standard input and output.

mod info;
mod tools;

pub use info::ServerInfo;
pub use tools::Server;
pub(crate) use tools::Shared;

use crate::error::Result;
use crate::tool::Tool;
use cw_app::Runtime;

/// Serve `tools` over standard input and output until the client closes its
/// input, then return.
///
/// Builds the [`Server`] (refusing two tools with one name, or an input type
/// with no object schema) and calls [`Server::serve_stdio`]. Blocks the
/// calling thread: the server runs on a runtime of its own, and each call
/// runs its tool's function on a blocking thread.
///
/// # Errors
///
/// [`crate::Error::DuplicateTool`] or [`crate::Error::InputSchema`] when the
/// tool list is refused; [`crate::Error::Runtime`] when the runtime cannot
/// start; [`crate::Error::Connection`] when the handshake or the transport
/// fails before the client closes its input.
pub fn serve_stdio<S: Send + Sync + 'static>(
    info: ServerInfo,
    runtime: Runtime<S>,
    tools: Vec<Tool<S>>,
) -> Result<()> {
    Server::new(info, runtime, tools)?.serve_stdio()
}
