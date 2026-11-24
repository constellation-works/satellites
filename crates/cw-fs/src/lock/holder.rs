//! The holder record: one JSON line in the lock file saying who holds it.
//!
//! The record only explains a timeout to the next waiter. It is written
//! through the descriptor the holder locked, never by name, so it cannot
//! follow a link planted since the open; it is not durable state, so a
//! reader that finds it torn, empty or unparsable treats it as no record.
//!
//! Writer and reader share one bound, [`RECORD_CAP`]: the label is cut to
//! fit it, so a record the holder wrote is always one a waiter can read.
//!
//! A waiter names the holder only while the recorded process is alive, and
//! how well that can be told depends on the platform:
//!
//! - **Linux** records the process's start time beside its pid and reads
//!   `/proc/<pid>/stat` back: a process that has exited, a zombie, or a
//!   later process that reused the pid is not named, and neither is the pid
//!   of a record without a start time, whose process cannot be identified.
//! - **Other Unix** asks the kernel whether the pid exists (signal 0). A
//!   zombie that its parent has not reaped, and an unrelated process that
//!   reused the pid, still count as alive there.
//! - **Native Windows** names no holder, this process included: a second
//!   handle cannot read the record while the holder's lock covers the file,
//!   and there is no liveness probe here.

use cw_base::Clock;
use std::fmt;
use std::fs::File;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};

/// The most bytes a record may take, for the writer and the reader alike.
const RECORD_CAP: usize = 4096;

/// Who holds a lock, as the holder recorded it.
///
/// A waiter names the holder only while the process that wrote the record
/// is alive. On Linux that is checked against the process's start time, so
/// an exited process, a zombie, a reused pid or a record without a start
/// time is not named. Elsewhere on Unix only the pid's existence is checked:
/// a zombie, or an unrelated process that reused the pid, may be named.
/// Native Windows names no holder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    /// The holder's process id.
    pub pid: u32,
    /// When it took the lock, as an RFC 3339 timestamp in UTC.
    pub time: String,
    /// What it said it was doing, cut at a character boundary when the whole
    /// record would not fit in 4096 bytes.
    pub label: String,
}

impl fmt::Display for Holder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pid {} ({}) since {}", self.pid, self.label, self.time)
    }
}

/// A record as read back: the holder, and the start time of its process
/// when the writer's platform records one.
struct Record {
    holder: Holder,
    start: Option<u64>,
}

/// Replace the record in `file` with this process, now, and `label`.
pub(super) fn record(file: &File, label: &str) -> io::Result<()> {
    let pid = std::process::id();
    let time = Clock::system().now_rfc3339().unwrap_or_default();
    overwrite(
        file,
        encode(pid, identity::start_of(pid), &time, label).as_bytes(),
    )
}

/// Clear the record on release.
pub(super) fn clear(file: &File) -> io::Result<()> {
    overwrite(file, b"")
}

/// The recorded holder, when there is a readable record and the process
/// that wrote it is still alive, as far as this platform can tell (see the
/// module docs). Nothing decides ownership from it.
pub(super) fn read_live(file: &File) -> Option<Holder> {
    let mut bytes = Vec::new();
    let mut reader = file;
    reader.seek(SeekFrom::Start(0)).ok()?;
    reader
        .take(u64::try_from(RECORD_CAP).ok()?)
        .read_to_end(&mut bytes)
        .ok()?;
    let record = decode(std::str::from_utf8(&bytes).ok()?.trim())?;
    identity::wrote_and_lives(record.holder.pid, record.start).then_some(record.holder)
}

fn overwrite(file: &File, bytes: &[u8]) -> io::Result<()> {
    let mut writer = file;
    writer.set_len(0)?;
    writer.seek(SeekFrom::Start(0))?;
    writer.write_all(bytes)?;
    writer.sync_data()
}

#[cfg(target_os = "linux")]
mod identity {
    //! On Linux, a process is its pid and its start time, both read from
    //! `/proc/<pid>/stat`.

    use std::io::Read as _;

    /// The most bytes of a `stat` line read; the line is far shorter.
    const STAT_CAP: u64 = 4096;

    /// The state and start time of `pid`, or `None` when it has no entry.
    fn stat(pid: u32) -> Option<(char, u64)> {
        let mut bytes = Vec::new();
        std::fs::File::open(format!("/proc/{pid}/stat"))
            .ok()?
            .take(STAT_CAP)
            .read_to_end(&mut bytes)
            .ok()?;
        // The command name, in parentheses, may hold any bytes, `)` and
        // invalid UTF-8 included; the fields after the last `)` are plain
        // ASCII. State is field 3, start time field 22: the first and the
        // twentieth after the name.
        let after = bytes.iter().rposition(|b| *b == b')')? + 1;
        let rest = std::str::from_utf8(&bytes[after..]).ok()?;
        let mut fields = rest.split_whitespace();
        let state = fields.next()?.chars().next()?;
        let start = fields.nth(18)?.parse().ok()?;
        Some((state, start))
    }

    /// The start time to record for `pid`.
    pub(super) fn start_of(pid: u32) -> Option<u64> {
        stat(pid).map(|(_, start)| start)
    }

    /// Whether `pid` is a running process (not exited, not a zombie) with
    /// the start time the record holds. A record without one (written by an
    /// older build, or when `/proc` could not be read) names nobody: the
    /// process behind its pid cannot be known to be the one that wrote it.
    pub(super) fn wrote_and_lives(pid: u32, start: Option<u64>) -> bool {
        let Some(recorded) = start else {
            return false;
        };
        match stat(pid) {
            None | Some(('Z' | 'X' | 'x', _)) => false,
            Some((_, now)) => recorded == now,
        }
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
mod identity {
    //! Elsewhere on Unix, only whether the pid exists can be asked cheaply.

    /// No start time is recorded here.
    pub(super) fn start_of(_pid: u32) -> Option<u64> {
        None
    }

    /// Whether `pid` exists. A zombie, or another process that reused the
    /// pid, counts as alive: signal 0 cannot tell them apart.
    pub(super) fn wrote_and_lives(pid: u32, _start: Option<u64>) -> bool {
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
}

#[cfg(not(unix))]
mod identity {
    //! Native Windows names no holder: the holder's lock covers the record,
    //! so even a second handle of the same process cannot read it, and
    //! there is no liveness probe here.

    /// No start time is recorded here.
    pub(super) fn start_of(_pid: u32) -> Option<u64> {
        None
    }

    /// Never: see the module docs.
    pub(super) fn wrote_and_lives(_pid: u32, _start: Option<u64>) -> bool {
        false
    }
}

/// `{"pid":1,"start":2,"time":"…","label":"…"}` and a newline, at most
/// [`RECORD_CAP`] bytes: the label is cut at a character boundary to fit.
fn encode(pid: u32, start: Option<u64>, time: &str, label: &str) -> String {
    let mut out = format!("{{\"pid\":{pid},");
    if let Some(start) = start {
        out.push_str(&format!("\"start\":{start},"));
    }
    out.push_str("\"time\":");
    push_string(&mut out, time, usize::MAX);
    out.push_str(",\"label\":");
    // Around the label's body: its two quotes, then the brace and the
    // newline.
    let budget = RECORD_CAP.saturating_sub(out.len() + 4);
    push_string(&mut out, label, budget);
    out.push_str("}\n");
    out
}

/// Push `text` as a JSON string, stopping before a character whose escape
/// would take its body past `budget` bytes.
fn push_string(out: &mut String, text: &str, budget: usize) {
    out.push('"');
    let mut used = 0;
    for c in text.chars() {
        let escaped = match c {
            '"' => String::from("\\\""),
            '\\' => String::from("\\\\"),
            c if u32::from(c) < 0x20 => format!("\\u{:04x}", u32::from(c)),
            c => c.to_string(),
        };
        used += escaped.len();
        if used > budget {
            break;
        }
        out.push_str(&escaped);
    }
    out.push('"');
}

/// Read a record [`encode`] wrote: an object with `pid`, `time`, `label`
/// and an optional `start`, in any order. Anything else is no record.
fn decode(text: &str) -> Option<Record> {
    let mut chars = text.chars().peekable();
    let (mut pid, mut start, mut time, mut label) = (None, None, None, None);
    expect(&mut chars, '{')?;
    loop {
        let key = parse_string(&mut chars)?;
        expect(&mut chars, ':')?;
        match key.as_str() {
            "pid" => pid = Some(parse_number(&mut chars)?.try_into().ok()?),
            "start" => start = Some(parse_number(&mut chars)?),
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
    Some(Record {
        holder: Holder {
            pid: pid?,
            time: time?,
            label: label?,
        },
        start,
    })
}

fn parse_number(chars: &mut Chars<'_>) -> Option<u64> {
    let mut digits = String::new();
    while let Some(c) = chars.next_if(char::is_ascii_digit) {
        digits.push(c);
    }
    digits.parse().ok()
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
