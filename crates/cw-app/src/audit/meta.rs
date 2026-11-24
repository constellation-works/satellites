//! How a command appears in the audit log.

/// What the audit log records for a mutating command: its name.
///
/// An [`crate::App`] returns one from [`crate::App::audit`] for each command
/// that changes data, and `None` for a read-only command, which must not
/// write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AuditMeta {
    command: &'static str,
}

impl AuditMeta {
    /// A command named `command`, such as `note add`: a fixed name, never a
    /// value the user typed.
    #[must_use]
    pub const fn new(command: &'static str) -> Self {
        Self { command }
    }

    /// The command's name.
    #[must_use]
    pub const fn command(&self) -> &'static str {
        self.command
    }
}
