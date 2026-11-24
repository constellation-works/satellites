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
/// Shutdown is bounded. Once the input closes, answers still in flight get
/// up to 5 s to be written, and a tool function still running after that
/// gets 2 s more; then it is abandoned on its thread, its answer is never
/// sent, and this returns. A function that changes data should finish its
/// change, or leave it whole, without relying on running to the end.
///
/// # Errors
///
/// [`crate::Error::DuplicateTool`] or [`crate::Error::InputSchema`] when the
/// tool list is refused; [`crate::Error::Runtime`] when the runtime cannot
/// start; [`crate::Error::Connection`] when the handshake fails or the
/// session ends abnormally. A failure to read standard input is not told
/// apart from its closing: both end the session with `Ok(())`.
pub fn serve_stdio<S: Send + Sync + 'static>(
    info: ServerInfo,
    runtime: Runtime<S>,
    tools: Vec<Tool<S>>,
) -> Result<()> {
    Server::new(info, runtime, tools)?.serve_stdio()
}
