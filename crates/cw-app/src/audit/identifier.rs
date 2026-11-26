//! The one test for text a line may record verbatim.

/// Whether `text` is 1 to `max_len` bytes of ASCII letters, digits and
/// `.` `_` `-` `@` `+` `:` `/`: an identifier, never free text.
pub(super) fn is_identifier(text: &str, max_len: usize) -> bool {
    !text.is_empty()
        && text.len() <= max_len
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_@+:/".contains(&b))
}
