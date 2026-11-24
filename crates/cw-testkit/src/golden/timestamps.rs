//! Timestamp redaction, so a golden never depends on the clock.

/// What [`redact_timestamps`] puts in place of each timestamp.
pub const TIMESTAMP_PLACEHOLDER: &str = "<timestamp>";

/// Replace every RFC 3339 timestamp in `text` with [`TIMESTAMP_PLACEHOLDER`].
///
/// A timestamp is `YYYY-MM-DDTHH:MM:SS`, optionally followed by fractional
/// seconds (`.` and one or more digits), then `Z` or an offset `+HH:MM` /
/// `-HH:MM`. `T` and `Z` may be lowercase, as RFC 3339 allows. The shape is matched, not the calendar: `2026-99-99T…Z` is
/// replaced too. Anything else is left as it is.
///
/// ```
/// use cw_testkit::redact_timestamps;
///
/// assert_eq!(
///     redact_timestamps("at 2026-01-02T03:04:05.123Z and 2026-01-02T03:04:05+09:00"),
///     "at <timestamp> and <timestamp>",
/// );
/// ```
#[must_use]
pub fn redact_timestamps(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        match timestamp_len(&bytes[i..]) {
            Some(len) => {
                // A timestamp is ASCII, so `i` and `i + len` are char
                // boundaries and the slices below are valid.
                out.push_str(&text[copied..i]);
                out.push_str(TIMESTAMP_PLACEHOLDER);
                i += len;
                copied = i;
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// The length of the timestamp at the start of `s`, if one starts there.
fn timestamp_len(s: &[u8]) -> Option<usize> {
    const BASE: &[u8] = b"dddd-dd-ddTdd:dd:dd";
    const OFFSET: &[u8] = b"dd:dd";
    if !shape_at(s, BASE) {
        return None;
    }
    let mut len = BASE.len();
    if s.get(len) == Some(&b'.') {
        let digits = s[len + 1..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        len += 1 + digits;
    }
    match s.get(len) {
        Some(b'Z' | b'z') => Some(len + 1),
        Some(b'+' | b'-') if shape_at(&s[len + 1..], OFFSET) => Some(len + 1 + OFFSET.len()),
        _ => None,
    }
}

/// Whether `s` starts with `shape`, where `d` is any ASCII digit and every
/// other byte stands for itself, letters in either case.
fn shape_at(s: &[u8], shape: &[u8]) -> bool {
    s.len() >= shape.len()
        && shape.iter().zip(s).all(|(&want, &got)| match want {
            b'd' => got.is_ascii_digit(),
            other => got.eq_ignore_ascii_case(&other),
        })
}
