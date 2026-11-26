//! The actor as the audit line records it: an identifier, never free text.

use super::identifier::is_identifier;

/// The longest actor, in bytes, a line records.
pub const ACTOR_MAX_LEN: usize = 64;

/// What a line records in place of an actor that does not read as an
/// identifier. Its `<` and `>` are outside the identifier set, so no real
/// actor reads the same.
pub const ACTOR_PLACEHOLDER: &str = "<invalid>";

/// `actor` as the line records it.
///
/// The actor is what the client said about itself (an MCP handshake name, an
/// operating-system user), so it may be any text, a credential pasted into
/// a label included. An identifiers-only log writes it only when it is 1 to
/// [`ACTOR_MAX_LEN`] bytes of ASCII letters, digits and `.` `_` `-` `@` `+`
/// `:` `/`; anything else is written as [`ACTOR_PLACEHOLDER`], never
/// verbatim and never trimmed to a prefix.
pub(super) fn recorded(actor: &str) -> &str {
    if is_identifier(actor, ACTOR_MAX_LEN) {
        actor
    } else {
        ACTOR_PLACEHOLDER
    }
}
