//! The protocol and the runtime, kept private: the only folder that names
//! the MCP SDK or the async runtime, and nothing in it is `pub` beyond the
//! crate (scripts/check-mcp-private-runtime.sh).

#[cfg(feature = "test-support")]
mod duplex;
mod handler;
mod intake;
mod serve;

#[cfg(feature = "test-support")]
pub(crate) use duplex::DuplexClient;
pub(crate) use serve::serve_stdio;
