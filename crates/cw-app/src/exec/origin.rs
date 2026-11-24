//! Where a command came from.

/// The kind of surface a command came through, written to its audit line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Surface(&'static str);

impl Surface {
    /// A command line.
    pub const CLI: Self = Self("cli");
    /// An MCP server.
    pub const MCP: Self = Self("mcp");

    /// Another surface, named in `snake_case`, such as `http`.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// The surface's name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// The surface a command came through and what the client said about
/// itself. Its audit line records both: the surface, and the client as the
/// actor.
///
/// A command line names the operating-system user ([`crate::Env::user`]);
/// an MCP server names the client from its handshake. The actor is a claim,
/// handed to the command as given; the audit line records it only when it
/// reads as an identifier ([`crate::audit::ACTOR_PLACEHOLDER`] otherwise).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Origin {
    surface: Surface,
    actor: Option<String>,
}

impl Origin {
    /// A command from `surface`, by `actor` when the client named itself.
    #[must_use]
    pub fn new(surface: Surface, actor: Option<String>) -> Self {
        Self { surface, actor }
    }

    /// The surface.
    #[must_use]
    pub fn surface(&self) -> Surface {
        self.surface
    }

    /// Who the client said it was.
    #[must_use]
    pub fn actor(&self) -> Option<&str> {
        self.actor.as_deref()
    }
}
