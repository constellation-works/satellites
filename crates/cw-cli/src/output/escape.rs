//! Stored text reaches a terminal or a pipe as text, never as terminal
//! control.
//!
//! A value can hold any character a user, a file or another program put in
//! it, including ESC, BEL and CR. Every control character becomes a visible
//! escape (`\n`, `\t`, `\r`, else `\u{1b}`), so a value cannot move the
//! cursor, change the colour, or break a tab-separated line in two. JSON
//! output does not pass through here: its encoder escapes them itself.

use std::borrow::Cow;
use std::fmt::Write as _;

/// `value` with every control character written as a visible escape.
///
/// Borrows when there is nothing to escape.
#[must_use]
pub fn escape(value: &str) -> Cow<'_, str> {
    escape_except(value, &[])
}

/// `text` escaped, except for line breaks and tabs: free text that keeps its
/// layout, such as a notice or an error message.
pub(crate) fn escape_text(text: &str) -> Cow<'_, str> {
    escape_except(text, &['\n', '\t'])
}

fn escape_except<'a>(value: &'a str, keep: &[char]) -> Cow<'a, str> {
    let needs = |c: char| c.is_control() && !keep.contains(&c);
    if !value.chars().any(needs) {
        return Cow::Borrowed(value);
    }
    let mut out = String::with_capacity(value.len() + 8);
    for c in value.chars() {
        if !needs(c) {
            out.push(c);
            continue;
        }
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(other));
            }
        }
    }
    Cow::Owned(out)
}
