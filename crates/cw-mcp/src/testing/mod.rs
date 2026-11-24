//! An in-process harness for wire tests (feature `test-support`).
//!
//! [`Harness`] serves a [`Server`] on a thread of its own over an in-memory
//! stream and holds the client end: a test writes raw JSON-RPC lines and
//! reads the server's lines back, as a real client on standard input and
//! output would. Every wait is bounded by a deadline, so a server that never
//! answers fails the test instead of hanging it.
//!
//! ```
//! use cw_app::Runtime;
//! use cw_mcp::testing::Harness;
//! use cw_mcp::{Access, Server, ServerInfo, Tool};
//! use serde_json::json;
//!
//! #[derive(serde::Deserialize, schemars::JsonSchema)]
//! struct Input {}
//!
//! let tools = vec![Tool::new::<Input>("ping", "Answer.", Access::ReadOnly, |_, _, _| {
//!     Ok(json!({ "pong": true }))
//! })];
//! let root = tempfile::tempdir()?;
//! let server = Server::new(ServerInfo::new("demo", "0.1.0", "demo_internal"), Runtime::new(root.path(), ()), tools)?;
//! let mut harness = Harness::start(server)?;
//! harness.initialize("demo-client")?;
//! let reply = harness.request(&json!({
//!     "jsonrpc": "2.0", "id": 2, "method": "tools/call",
//!     "params": { "name": "ping", "arguments": {} }
//! }))?;
//! assert_eq!(reply["result"]["structuredContent"], json!({ "pong": true }));
//! harness.close()??;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::error::Result;
use crate::server::Server;
use crate::wire::DuplexClient;
use serde_json::{Value, json};
use std::fmt;
use std::io;
use std::time::Duration;

/// How long a harness waits for any one line, by default.
const DEADLINE: Duration = Duration::from_secs(10);

/// The protocol version [`Harness::initialize`] asks for.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// The client end of an in-memory connection to a running server.
pub struct Harness {
    client: DuplexClient,
}

impl Harness {
    /// Serve `server` on a thread of its own, waiting up to 10 s for any one
    /// line.
    ///
    /// # Errors
    ///
    /// [`crate::Error::Runtime`] when a runtime cannot start.
    pub fn start<S: Send + Sync + 'static>(server: Server<S>) -> Result<Self> {
        Self::with_deadline(server, DEADLINE)
    }

    /// Serve `server`, waiting up to `deadline` for any one line.
    ///
    /// # Errors
    ///
    /// [`crate::Error::Runtime`] when a runtime cannot start.
    pub fn with_deadline<S: Send + Sync + 'static>(
        server: Server<S>,
        deadline: Duration,
    ) -> Result<Self> {
        Ok(Self {
            client: DuplexClient::start(server, deadline)?,
        })
    }

    /// Write one raw line to the server; the newline is added.
    ///
    /// # Errors
    ///
    /// The write failed or did not finish within the deadline.
    pub fn send(&mut self, line: &str) -> io::Result<()> {
        self.client.send_line(line)
    }

    /// The next line the server wrote, parsed as JSON.
    ///
    /// # Errors
    ///
    /// No line came within the deadline, the server closed its output, or
    /// the line is not JSON.
    pub fn receive(&mut self) -> io::Result<Value> {
        let line = self.client.read_line()?;
        serde_json::from_str(&line).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("the server wrote a line that is not JSON ({error}): {line}"),
            )
        })
    }

    /// Send `message` as one line and return the next line the server
    /// writes.
    ///
    /// # Errors
    ///
    /// As [`Harness::send`] and [`Harness::receive`].
    pub fn request(&mut self, message: &Value) -> io::Result<Value> {
        self.send(&message.to_string())?;
        self.receive()
    }

    /// The handshake as client `client`: `initialize` with id 1, then the
    /// `notifications/initialized` notification. Returns the `initialize`
    /// response.
    ///
    /// # Errors
    ///
    /// As [`Harness::request`].
    pub fn initialize(&mut self, client: &str) -> io::Result<Value> {
        let response = self.request(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": client, "version": "0" },
            },
        }))?;
        self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string())?;
        Ok(response)
    }

    /// Close the server's input and wait for it to return. The outer result
    /// is the harness's own (closing, or the wait, failed); the inner one is
    /// what the server returned.
    ///
    /// # Errors
    ///
    /// The input could not be closed, or the server did not return within
    /// the deadline.
    pub fn close(self) -> io::Result<Result<()>> {
        self.client.close()
    }
}

impl fmt::Debug for Harness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Harness").finish_non_exhaustive()
    }
}
