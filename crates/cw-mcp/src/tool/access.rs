//! Whether a tool changes data.

/// Whether a tool only reads or may change data, advertised to the client
/// as the tool's read-only hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Access {
    /// The tool only reads: calling it changes nothing.
    ReadOnly,
    /// The tool may change data.
    Mutating,
}

impl Access {
    /// Whether the tool only reads.
    #[must_use]
    pub fn is_read_only(self) -> bool {
        match self {
            Self::ReadOnly => true,
            Self::Mutating => false,
        }
    }
}
