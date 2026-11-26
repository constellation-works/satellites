//! The target as the audit line records it: an identifier, never free text.

use super::identifier::is_identifier;

/// The longest target, in bytes, a line records.
pub const TARGET_MAX_LEN: usize = 128;

/// What a line records in place of a target that does not read as an
/// identifier. Its `<` and `>` are outside the identifier set, so no real
/// target reads the same.
pub const TARGET_PLACEHOLDER: &str = "<invalid>";

/// `target` as the line records it.
///
/// The target is the id of the record a command acted on or was aimed at. An
/// [`crate::App`] supplies it, and an app that hands over what the user typed
/// (a selector, a title, a path) would put that text in the log. An
/// identifiers-only log writes it only when it is 1 to [`TARGET_MAX_LEN`]
/// bytes of ASCII letters, digits and `.` `_` `-` `@` `+` `:` `/`; anything
/// else is written as [`TARGET_PLACEHOLDER`], never verbatim and never
/// trimmed to a prefix.
pub(super) fn recorded(target: &str) -> &str {
    if is_identifier(target, TARGET_MAX_LEN) {
        target
    } else {
        TARGET_PLACEHOLDER
    }
}
