//! How a command appears in the audit log.

/// What the audit log records for a mutating command: its name, and whether
/// it is one record or itemized.
///
/// An [`crate::App`] returns one from [`crate::App::audit`] for each command
/// that changes data, and `None` for a read-only command, which must not
/// write.
///
/// - [`AuditMeta::new`]: a command that writes exactly one record. The
///   chokepoint appends its started line before it runs.
/// - [`AuditMeta::itemized`]: a command that writes zero or many records
///   (a prompt with two directives, a group add of several sessions, a stop
///   that changes nothing). The chokepoint writes nothing before it runs; the
///   command admits its items through [`crate::Ctx::audit`] inside its
///   transaction, after its last write and before the commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AuditMeta {
    command: &'static str,
    itemized: bool,
}

impl AuditMeta {
    /// A command named `command`, such as `note add`, that writes one record:
    /// a fixed name, never a value the user typed.
    #[must_use]
    pub const fn new(command: &'static str) -> Self {
        Self {
            command,
            itemized: false,
        }
    }

    /// A command named `command` that writes zero or many records, each its
    /// own started and outcome line. [`crate::App::subject`] and
    /// [`crate::App::target`] are not called for it: each item names its own
    /// subject.
    #[must_use]
    pub const fn itemized(command: &'static str) -> Self {
        Self {
            command,
            itemized: true,
        }
    }

    /// The command's name.
    #[must_use]
    pub const fn command(&self) -> &'static str {
        self.command
    }

    /// Whether the command is itemized.
    #[must_use]
    pub const fn is_itemized(&self) -> bool {
        self.itemized
    }
}
