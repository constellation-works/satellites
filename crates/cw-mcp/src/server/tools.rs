//! A server built from a checked tool list.

use super::info::ServerInfo;
use crate::error::{Error, Result};
use crate::tool::Tool;
use cw_app::Runtime;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// A server ready to serve: its [`ServerInfo`], the program's [`Runtime`]
/// and a tool list checked once, when it is built.
pub struct Server<S> {
    pub(crate) shared: Arc<Shared<S>>,
}

/// What every connection's handler reads.
pub(crate) struct Shared<S> {
    pub(crate) info: ServerInfo,
    pub(crate) runtime: Runtime<S>,
    /// The tools in the order they were given, which `tools/list` keeps.
    pub(crate) tools: Vec<Tool<S>>,
    index: BTreeMap<String, usize>,
}

impl<S> Shared<S> {
    /// The tool a call names.
    pub(crate) fn tool(&self, name: &str) -> Option<&Tool<S>> {
        self.index.get(name).and_then(|&at| self.tools.get(at))
    }
}

impl<S: Send + Sync + 'static> Server<S> {
    /// A server over `runtime` offering `tools`.
    ///
    /// # Errors
    ///
    /// [`Error::DuplicateTool`] when two tools share a name, and
    /// [`Error::InputSchema`] when a tool's input type has no object schema.
    pub fn new(info: ServerInfo, runtime: Runtime<S>, tools: Vec<Tool<S>>) -> Result<Self> {
        let mut index = BTreeMap::new();
        for (at, tool) in tools.iter().enumerate() {
            if let Err(reason) = tool.schema() {
                return Err(Error::InputSchema {
                    name: tool.name().to_owned(),
                    reason: reason.to_owned(),
                });
            }
            if index.insert(tool.name().to_owned(), at).is_some() {
                return Err(Error::DuplicateTool {
                    name: tool.name().to_owned(),
                });
            }
        }
        Ok(Self {
            shared: Arc::new(Shared {
                info,
                runtime,
                tools,
                index,
            }),
        })
    }

    /// Serve over standard input and output until the client closes its
    /// input, then return. Blocks the calling thread. Shutdown is bounded as
    /// [`crate::serve_stdio`] describes: a tool function still running is
    /// abandoned rather than waited for.
    ///
    /// Standard output carries the protocol: nothing else in the process may
    /// write to it while the server runs.
    ///
    /// # Errors
    ///
    /// [`Error::Runtime`] when the runtime cannot start, and
    /// [`Error::Connection`] when the handshake or the transport fails before
    /// the client closes its input.
    pub fn serve_stdio(self) -> Result<()> {
        crate::wire::serve_stdio(self)
    }
}

impl<S> Server<S> {
    /// What the server says about itself.
    #[must_use]
    pub fn info(&self) -> &ServerInfo {
        &self.shared.info
    }

    /// The tools, in the order `tools/list` gives them.
    #[must_use]
    pub fn tools(&self) -> &[Tool<S>] {
        &self.shared.tools
    }
}

impl<S> fmt::Debug for Server<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server")
            .field("info", &self.shared.info)
            .field("tools", &self.shared.tools)
            .finish_non_exhaustive()
    }
}
