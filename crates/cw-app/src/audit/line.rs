//! The persisted line: its fields, its format and the steps that upgrade an
//! older one.
//!
//! Formats 1 and 2 are the earlier shapes of this line, which logs written
//! before this crate already hold: format 1 had no actor, format 2 no
//! surface. Reading them through the steps below keeps such a log readable.

use super::actor::recorded;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::num::NonZeroU32;
use std::path::Path;

/// The newest line format, the one every new line is written in. Bumped,
/// with an upgrade step appended, whenever a line's shape changes.
pub const FORMAT: u32 = 3;

/// One step: a line of format `n` in, the same line in format `n + 1` out.
type Upgrade = fn(&mut Map<String, Value>);

/// The upgrade steps, append-only: `UPGRADES[i]` turns format `i + 1` into
/// format `i + 2`, so `FORMAT` is always `UPGRADES.len() + 1`. A shipped
/// step is never edited, reordered or removed.
const UPGRADES: [Upgrade; 2] = [add_actor, add_surface];

const _: () = assert!(UPGRADES.len() as u64 + 1 == FORMAT as u64);

/// Format 1 to 2: the actor, unknown on an older line.
fn add_actor(line: &mut Map<String, Value>) {
    line.entry("actor").or_insert(Value::Null);
}

/// Format 2 to 3: the surface, unknown on an older line.
fn add_surface(line: &mut Map<String, Value>) {
    line.entry("surface").or_insert(Value::Null);
}

/// How an audited command ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The command completed.
    Success,
    /// The command failed, or never reported how it ended (it panicked).
    Failure,
}

/// One audited command, as a line of the log holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Record {
    /// When the command started, as an RFC 3339 timestamp in UTC.
    pub at: String,
    /// The surface it came through, such as `cli` or `mcp`; `None` on a line
    /// written before the surface was recorded.
    pub surface: Option<String>,
    /// Who the client said it was, when it said; [`super::ACTOR_PLACEHOLDER`]
    /// when that was not an identifier.
    pub actor: Option<String>,
    /// The command's name, such as `note add`.
    pub command: String,
    /// How it ended.
    pub status: Status,
    /// The record it acted on, when it succeeded and named one.
    pub target: Option<String>,
    /// The stable error code, when it failed with one; never a message.
    pub error_code: Option<String>,
    /// Wall time from start to result, in milliseconds.
    pub duration_ms: u64,
}

/// A line as written: the record and its format.
#[derive(Serialize)]
struct Line<'a> {
    format: u32,
    #[serde(flatten)]
    record: &'a Record,
}

/// The format number alone, read first so a newer line is refused by its
/// version rather than by a parse error.
#[derive(Deserialize)]
struct Probe {
    format: NonZeroU32,
}

/// `record` as one newline-terminated JSON line in the newest format.
///
/// The actor is written only when it reads as an identifier, and as
/// [`super::ACTOR_PLACEHOLDER`] otherwise, whoever built the record.
pub(super) fn encode(record: &Record) -> Result<Vec<u8>> {
    let actor = record.actor.as_deref().map(recorded);
    let record = match actor {
        Some(actor) if Some(actor) != record.actor.as_deref() => &Record {
            actor: Some(actor.to_owned()),
            ..record.clone()
        },
        _ => record,
    };
    let mut bytes = serde_json::to_vec(&Line {
        format: FORMAT,
        record,
    })
    .map_err(Error::Encode)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Read line `number` of the log at `path`, upgrading an older format and
/// refusing a newer one.
pub(super) fn decode(path: &Path, number: usize, bytes: &[u8]) -> Result<Record> {
    let corrupt = |source| Error::Corrupt {
        path: path.to_path_buf(),
        line: number,
        source,
    };
    let mut value: Value = serde_json::from_slice(bytes).map_err(corrupt)?;
    let found = Probe::deserialize(&value).map_err(corrupt)?.format.get();
    if found > FORMAT {
        return Err(Error::NewerFormat {
            path: path.to_path_buf(),
            line: number,
            found,
            supported: FORMAT,
        });
    }
    if let Value::Object(fields) = &mut value {
        for (step, to) in UPGRADES.iter().zip(2..) {
            if to > found {
                step(fields);
            }
        }
        fields.remove("format");
    }
    serde_json::from_value(value).map_err(corrupt)
}

/// Whether `tail` holds the whole log file or the cut end of a longer one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum TailScope {
    /// `tail` is the whole file; its first line starts at the file start.
    WholeFile,
    /// `tail` is cut from a longer file; its first line may start mid-line.
    Cut,
}

impl TailScope {
    /// `true` (the whole file) as [`TailScope::WholeFile`], else [`TailScope::Cut`].
    pub(super) fn of(whole_file: bool) -> Self {
        if whole_file {
            Self::WholeFile
        } else {
            Self::Cut
        }
    }
}

impl From<bool> for TailScope {
    fn from(whole_file: bool) -> Self {
        Self::of(whole_file)
    }
}

/// The format of the newest line in `tail`, the last bytes of a log, whose
/// format can be read.
///
/// Lines are judged newest first. A last line without its line break (torn,
/// or whole JSON missing only its line break) counts like any other. A line
/// whose format cannot be read (malformed, blank) is skipped, never trusted,
/// so it cannot hide an older line in a newer format. The first line of
/// `tail` is read only when `scope` is [`TailScope::WholeFile`];
/// otherwise it may start mid-line and is skipped.
///
/// `Ok(None)` when `tail` is the whole file and no line has a readable
/// format (a corrupt line is the reader's to report, not a reason to stop
/// logging); `Err(())` when `tail` is cut from a longer file and no line in
/// it has a readable format, so the newest format is unknown.
pub(super) fn newest_format(tail: &[u8], scope: TailScope) -> std::result::Result<Option<u32>, ()> {
    let mut lines = tail.split(|&b| b == b'\n').rev().peekable();
    while let Some(line) = lines.next() {
        let is_first = lines.peek().is_none();
        if is_first {
            match scope {
                TailScope::WholeFile => {}
                TailScope::Cut => break,
            }
        }
        if let Ok(probe) = serde_json::from_slice::<Probe>(line) {
            return Ok(Some(probe.format.get()));
        }
    }
    match scope {
        TailScope::WholeFile => Ok(None),
        TailScope::Cut => Err(()),
    }
}
