//! The holder record: one JSON line in the lock file saying who holds it.
//!
//! The record only explains a timeout to the next waiter. It is written
//! through the descriptor the holder locked, never by name, so it cannot
//! follow a link planted since the open; it is not durable state, so a
//! reader that finds it torn, empty or unparsable treats it as no record.

use cw_base::Clock;
use std::fmt;
use std::fs::File;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};

/// The most bytes of a record a waiter reads.
const RECORD_CAP: u64 = 4096;

/// Who holds a lock, as the holder recorded it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    /// The holder's process id.
    pub pid: u32,
    /// When it took the lock, as an RFC 3339 timestamp in UTC.
    pub time: String,
    /// What it said it was doing.
    pub label: String,
}

impl fmt::Display for Holder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pid {} ({}) since {}", self.pid, self.label, self.time)
    }
}

/// Replace the record in `file` with this process, now, and `label`.
pub(super) fn record(file: &File, label: &str) -> io::Result<()> {
    let holder = Holder {
        pid: std::process::id(),
        time: Clock::system().now_rfc3339().unwrap_or_default(),
        label: label.to_owned(),
    };
    overwrite(file, encode(&holder).as_bytes())
}

/// Clear the record on release.
pub(super) fn clear(file: &File) -> io::Result<()> {
    overwrite(file, b"")
}

/// The recorded holder, when there is a readable record and its process is
/// still alive. A process that has exited is never named: its pid may
/// already belong to something else.
pub(super) fn read_live(file: &File) -> Option<Holder> {
    let mut bytes = Vec::new();
    let mut reader = file;
    reader.seek(SeekFrom::Start(0)).ok()?;
    reader.take(RECORD_CAP).read_to_end(&mut bytes).ok()?;
    let holder = decode(std::str::from_utf8(&bytes).ok()?.trim())?;
    is_alive(holder.pid).then_some(holder)
}

fn overwrite(file: &File, bytes: &[u8]) -> io::Result<()> {
    let mut writer = file;
    writer.set_len(0)?;
    writer.seek(SeekFrom::Start(0))?;
    writer.write_all(bytes)?;
    writer.sync_data()
}

#[cfg(unix)]
fn is_alive(pid: u32) -> bool {
    use rustix::io::Errno;
    use rustix::process::{Pid, test_kill_process};
    if pid == std::process::id() {
        return true;
    }
    let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
        return false;
    };
    // EPERM: the process exists but belongs to another user.
    matches!(test_kill_process(pid), Ok(()) | Err(Errno::PERM))
}

/// Without a way to ask whether another process lives, only this process
/// is known to.
#[cfg(not(unix))]
fn is_alive(pid: u32) -> bool {
    pid == std::process::id()
}

/// `{"pid":1,"time":"…","label":"…"}` and a newline.
fn encode(holder: &Holder) -> String {
    let mut out = format!("{{\"pid\":{},\"time\":", holder.pid);
    push_string(&mut out, &holder.time);
    out.push_str(",\"label\":");
    push_string(&mut out, &holder.label);
    out.push_str("}\n");
    out
}

fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Read a record [`encode`] wrote: an object with `pid`, `time` and `label`
/// in any order. Anything else is no record.
fn decode(text: &str) -> Option<Holder> {
    let mut chars = text.chars().peekable();
    let (mut pid, mut time, mut label) = (None, None, None);
    expect(&mut chars, '{')?;
    loop {
        let key = parse_string(&mut chars)?;
        expect(&mut chars, ':')?;
        match key.as_str() {
            "pid" => {
                let mut digits = String::new();
                while let Some(c) = chars.next_if(char::is_ascii_digit) {
                    digits.push(c);
                }
                pid = Some(digits.parse::<u32>().ok()?);
            }
            "time" => time = Some(parse_string(&mut chars)?),
            "label" => label = Some(parse_string(&mut chars)?),
            _ => return None,
        }
        match chars.next()? {
            ',' => {}
            '}' => break,
            _ => return None,
        }
    }
    if chars.next().is_some() {
        return None;
    }
    Some(Holder {
        pid: pid?,
        time: time?,
        label: label?,
    })
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

fn expect(chars: &mut Chars<'_>, want: char) -> Option<()> {
    (chars.next()? == want).then_some(())
}

fn parse_string(chars: &mut Chars<'_>) -> Option<String> {
    expect(chars, '"')?;
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
}
