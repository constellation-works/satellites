//! Serving a connection on a private runtime.

use super::handler::Handler;
use crate::error::{Error, Result};
use crate::server::Server;
use rmcp::ServiceExt as _;
use rmcp::service::ServerInitializeError;
use tokio::io::{AsyncRead, AsyncWrite};

/// The runtime a server runs on: one thread for the protocol, and the
/// blocking pool for the tools' functions.
pub(crate) fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)
}

/// Serve `server` over standard input and output, blocking until the client
/// closes its input.
pub(crate) fn serve_stdio<S: Send + Sync + 'static>(server: Server<S>) -> Result<()> {
    runtime()?.block_on(serve(server, tokio::io::stdin(), tokio::io::stdout()))
}

/// Serve `server` on one connection until its input closes.
///
/// Input closed before the handshake ends is a clean end too: the client
/// went away and there is nothing to answer.
pub(crate) async fn serve<S, R, W>(server: Server<S>, input: R, output: W) -> Result<()>
where
    S: Send + Sync + 'static,
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let running = match Handler::new(server.shared).serve((input, output)).await {
        Ok(running) => running,
        Err(ServerInitializeError::ConnectionClosed(_)) => return Ok(()),
        Err(error) => return Err(Error::Connection(Box::new(error))),
    };
    match running.waiting().await {
        Ok(_) => Ok(()),
        Err(error) => Err(Error::Connection(Box::new(error))),
    }
}
