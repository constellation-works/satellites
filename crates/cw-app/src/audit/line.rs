//! The persisted line: its fields, its format and the steps that upgrade an
//! older one.
//!
//! Formats 1 through 3 held a single record per command. Format 4 adds a
//! phase and pairs the two lines of a write-ahead record by ID. Format 5
//! names the client's claim `self_reported_actor` and adds the host's `euid`.

use super::actor::recorded as recorded_actor;
use super::host;
use super::target::recorded as recorded_target;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::num::NonZeroU32;
use std::path::Path;

/// The newest line format, the one every new line is written in. Bumped,
/// with an upgrade step appended, whenever a line's shape changes.
pub const FORMAT: u32 = 5;

/// One step: a line of format `n` in, the same line in format `n + 1` out.
type Upgrade = fn(&mut Map<String, Value>);

/// The upgrade steps, append-only: `UPGRADES[i]` turns format `i + 1` into
/// format `i + 2`, so `FORMAT` is always `UPGRADES.len() + 1`. A shipped
/// step is never edited, reordered or removed.
const UPGRADES: [Upgrade; 4] = [add_actor, add_surface, add_phase, name_claim];

const _: () = assert!(UPGRADES.len() as u64 + 1 == FORMAT as u64);

/// Format 1 to 2: the actor, unknown on an older line.
fn add_actor(line: &mut Map<String, Value>) {
    line.entry("actor").or_insert(Value::Null);
}

/// Format 2 to 3: the surface, unknown on an older line.
fn add_surface(line: &mut Map<String, Value>) {
    line.entry("surface").or_insert(Value::Null);
}

/// Format 3 to 4. Each older line is a complete record on its own.
fn add_phase(line: &mut Map<String, Value>) {
    line.insert("phase".to_owned(), Value::String("standalone".to_owned()));
}

/// Format 4 to 5: the client's claim, which format 4 wrote as `actor`, is
/// `self_reported_actor`. No host identity was recorded on an older line, so
/// its `euid` is unknown.
fn name_claim(line: &mut Map<String, Value>) {
    let claim = line.remove("actor").unwrap_or(Value::Null);
    line.entry("self_reported_actor").or_insert(claim);
    line.entry("euid").or_insert(Value::Null);
}

/// How an audited command ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The command completed.
    Success,
    /// The command reported a failure.
    Failure,
    /// A start was recorded and no outcome was recorded.
    Unknown,
}

/// One audited command, with its outcome when available.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Record {
    /// When the command started, as an RFC 3339 timestamp in UTC.
    pub at: String,
    /// The surface it came through, such as `cli` or `mcp`; `None` on a line
    /// written before the surface was recorded.
    pub surface: Option<String>,
    /// Who the client said it was, when it said: a claim, never identity.
    /// [`super::ACTOR_PLACEHOLDER`] when that was not an identifier. `None`
    /// when the client named nothing. A line in format 4 or older kept its
    /// claim as `actor`, which reads under this name.
    pub self_reported_actor: Option<String>,
    /// The effective user ID of the host process that wrote the line,
    /// stamped at write time: a value the caller passes in is replaced. `None`
    /// where the platform has no user ID (Windows), and on a line written
    /// before format 5 recorded it.
    pub euid: Option<u32>,
    /// The command's name, such as `note add`.
    pub command: String,
    /// How it ended.
    pub status: Status,
    /// The id of the record the command acted on or was aimed at, when known:
    /// the app's resolved subject on the started line (and so on a start with
    /// no outcome), kept on a failure, and replaced on success by the record
    /// the outcome names, if it names one. An identifier, or
    /// [`super::TARGET_PLACEHOLDER`] when the app supplied anything else.
    pub target: Option<String>,
    /// The stable error code, when it failed with one; never a message.
    pub error_code: Option<String>,
    /// Wall time from start to result, in milliseconds, when recorded.
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "lowercase")]
pub(super) enum Phase {
    Standalone,
    Started { id: String },
    Outcome { id: String },
}

#[derive(Serialize)]
struct Line<'a> {
    format: u32,
    #[serde(flatten)]
    phase: &'a Phase,
    #[serde(flatten)]
    record: &'a Record,
}

#[derive(Deserialize)]
pub(super) struct DecodedLine {
    #[serde(flatten)]
    pub(super) phase: Phase,
    #[serde(flatten)]
    pub(super) record: Record,
}

/// The format number alone, read first so a newer line is refused by its
/// version rather than by a parse error.
#[derive(Deserialize)]
struct Probe {
    format: NonZeroU32,
}

/// `record` as one newline-terminated JSON line in the newest format.
///
/// The self-reported actor and the target are each written only when they read
/// as an identifier, and as [`super::ACTOR_PLACEHOLDER`] or
/// [`super::TARGET_PLACEHOLDER`] otherwise, whoever built the record. `euid`
/// is stamped from the host here, so the record's own value never reaches the
/// line.
pub(super) fn encode(record: &Record, phase: &Phase) -> Result<Vec<u8>> {
    let guarded = Record {
        self_reported_actor: record
            .self_reported_actor
            .as_deref()
            .map(|actor| recorded_actor(actor).to_owned()),
        euid: host::euid(),
        target: record
            .target
            .as_deref()
            .map(|target| recorded_target(target).to_owned()),
        ..record.clone()
    };
    let mut bytes = serde_json::to_vec(&Line {
        format: FORMAT,
        phase,
        record: &guarded,
    })
    .map_err(Error::Encode)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Read line `number` of the log at `path`, upgrading an older format and
/// refusing a newer one.
pub(super) fn decode(path: &Path, number: usize, bytes: &[u8]) -> Result<DecodedLine> {
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
