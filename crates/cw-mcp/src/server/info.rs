//! What the server says about itself.

/// The server's name and version, as the client sees them in the
/// handshake, what it tells the client to do, and the program's code for a
/// tool that panicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerInfo {
    name: String,
    version: String,
    panic_code: &'static str,
    instructions: Option<String>,
}

impl ServerInfo {
    /// A server called `name` at `version`.
    ///
    /// `panic_code` is the program's stable `snake_case` code for a tool
    /// whose function panicked: the call is answered as a failed tool of
    /// kind `internal` with this code. The code is the program's, like
    /// every code its tools fail with.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        panic_code: &'static str,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            panic_code,
            instructions: None,
        }
    }

    /// The same server, telling the client how to use it.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// The server's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The server's version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The code a panicked tool is reported with.
    #[must_use]
    pub fn panic_code(&self) -> &'static str {
        self.panic_code
    }

    /// What the server tells the client, if anything.
    #[must_use]
    pub fn instructions(&self) -> Option<&str> {
        self.instructions.as_deref()
    }
}
