//! What this crate fails with.
//!
//! One typed enum with a [`Kind`] and no code: the program that serves the
//! tools names its own stable codes, because the code is that program's
//! contract.

use cw_base::Kind;
use std::error::Error as StdError;

/// Why building or serving a server failed, or why one tool call did.
///
/// Not `#[non_exhaustive]`: a program that wraps it gives every variant a
/// code, so adding a variant is meant to break its match.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Two tools in the list share a name, so a call could not say which it
    /// means.
    #[error("two tools are named `{name}`; give each tool its own name")]
    DuplicateTool {
        /// The name both tools have.
        name: String,
    },
    /// A tool's input type has no schema a client can use: the protocol
    /// requires an object schema.
    #[error("tool `{name}` has no usable input schema: {reason}; make its input type a struct")]
    InputSchema {
        /// The tool.
        name: String,
        /// Why the derived schema was refused.
        reason: String,
    },
    /// A tool's function panicked. The call is answered as a failed tool and
    /// the server goes on serving.
    #[error("tool `{name}` panicked: {message}")]
    Panicked {
        /// The tool.
        name: String,
        /// The panic's message, when it carried one.
        message: String,
    },
    /// The server's private runtime could not be started.
    #[error("cannot start the MCP server's runtime")]
    Runtime(#[source] std::io::Error),
    /// The connection failed before the client closed it: the handshake or
    /// the transport broke.
    #[error("the MCP connection failed")]
    Connection(#[source] Box<dyn StdError + Send + Sync + 'static>),
}

impl Error {
    /// How this failure is classified. Exhaustive, so a new variant decides.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::DuplicateTool { .. } | Self::InputSchema { .. } | Self::Panicked { .. } => {
                Kind::Internal
            }
            Self::Runtime(_) | Self::Connection(_) => Kind::Io,
        }
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
