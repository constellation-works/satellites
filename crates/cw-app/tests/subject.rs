//! The subject: the record a mutating command is aimed at, written on its
//! started line and kept beside its outcome.

#![expect(
    clippy::disallowed_methods,
    reason = "subject fixture: creates and reads the raw files a command writes, to check their bytes on disk"
)]

use cw_app::{
    App, AuditLog, AuditMeta, Ctx, Origin, Runtime, Status, Surface, audit::TARGET_MAX_LEN,
    audit::TARGET_PLACEHOLDER, exec,
};
use cw_base::{Error, Kind};
use cw_fs::Cap;
use std::sync::atomic::{AtomicUsize, Ordering};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SECRET_ARG: &str = "argument value: never logged";
const SECRET_MESSAGE: &str = "error text: never logged";

#[derive(Debug, thiserror::Error)]
#[error("{SECRET_MESSAGE}")]
struct Refused;

impl From<Refused> for Error {
    fn from(error: Refused) -> Self {
        Error::new(Kind::Refused, "write_refused", error)
    }
}

enum Command {
    /// Mutating, names no record on success.
    Touch { subject: String },
    /// Mutating, creates a record and names it.
    Create { subject: String },
    /// Mutating, refused.
    Refuse { subject: String },
    /// Read-only: its subject must never be asked.
    Look { subject: String },
}

/// The state counts how often the app is asked for a subject.
struct Subjects;

impl App for Subjects {
    type State = AtomicUsize;
    type Command = Command;
    type Outcome = Option<String>;
    type Error = Refused;

    fn audit(command: &Command) -> Option<AuditMeta> {
        match command {
            Command::Touch { .. } => Some(AuditMeta::new("note touch")),
            Command::Create { .. } => Some(AuditMeta::new("note create")),
            Command::Refuse { .. } => Some(AuditMeta::new("note refuse")),
            Command::Look { .. } => None,
        }
    }

    fn subject(ctx: &Ctx<'_, AtomicUsize>, command: &Command) -> Option<String> {
        ctx.state().fetch_add(1, Ordering::SeqCst);
        match command {
            Command::Touch { subject }
            | Command::Create { subject }
            | Command::Refuse { subject }
            | Command::Look { subject } => Some(subject.clone()),
        }
    }

    fn run(_: &Ctx<'_, AtomicUsize>, command: Command) -> Result<Option<String>, Refused> {
        match command {
            Command::Touch { .. } | Command::Look { .. } => Ok(None),
            Command::Create { .. } => Ok(Some("n-new".to_owned())),
            Command::Refuse { .. } => Err(Refused),
        }
    }

    fn target(outcome: &Option<String>) -> Option<String> {
        outcome.clone()
    }
}

/// An app that does not implement `subject`.
struct Plain;

impl App for Plain {
    type State = ();
    type Command = ();
    type Outcome = ();
    type Error = Refused;

    fn audit((): &()) -> Option<AuditMeta> {
        Some(AuditMeta::new("note plain"))
    }

    fn run(_: &Ctx<'_, ()>, (): ()) -> Result<(), Refused> {
        Ok(())
    }

    fn target((): &()) -> Option<String> {
        None
    }
}

fn origin() -> Origin {
    Origin::new(Surface::MCP, Some("agent-7".to_owned()))
}

fn runtime(scratch: &tempfile::TempDir) -> Runtime<AtomicUsize> {
    Runtime::new(scratch.path().join("data"), AtomicUsize::new(0))
}

fn read(log: &AuditLog) -> Result<Vec<cw_app::Record>, Box<dyn std::error::Error>> {
    Ok(log
        .read(Cap::bytes(1 << 16))?
        .into_iter()
        .collect::<Result<_, _>>()?)
}

fn raw_lines(log: &AuditLog) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(log.path())?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}

#[test]
fn a_subject_is_on_the_started_line_and_the_success_line_that_names_no_record() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let runtime = runtime(&scratch);
    let finished = exec::<Subjects>(
        &runtime,
        &origin(),
        Command::Touch {
            subject: "n-1".to_owned(),
        },
    );

    // Before the outcome: the start reads as unknown, with its target.
    let [started] = read(runtime.audit_log())?
        .try_into()
        .map_err(|_| "one start")?;
    assert_eq!(started.status, Status::Unknown);
    assert_eq!(started.target.as_deref(), Some("n-1"));
    assert_eq!(started.error_code, None);

    let (result, warning) = finished.settle();
    assert!(result?.is_none());
    assert!(warning.is_none());

    let [record] = read(runtime.audit_log())?
        .try_into()
        .map_err(|_| "one record")?;
    assert_eq!(record.status, Status::Success);
    assert_eq!(record.target.as_deref(), Some("n-1"));
    assert_eq!(record.error_code, None);

    let lines = raw_lines(runtime.audit_log())?;
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["phase"], "started");
    assert_eq!(lines[0]["target"], "n-1");
    assert_eq!(lines[1]["phase"], "outcome");
    assert_eq!(lines[1]["target"], "n-1");
    assert_eq!(lines[0]["format"], cw_app::audit::FORMAT);
    Ok(())
}

#[test]
fn a_failed_command_keeps_its_subject_beside_the_error_code() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let runtime = runtime(&scratch);
    let finished = exec::<Subjects>(
        &runtime,
        &origin(),
        Command::Refuse {
            subject: "p-42".to_owned(),
        },
    );
    let (result, warning) = finished.settle();
    assert!(result.is_err());
    assert!(warning.is_none());

    let [record] = read(runtime.audit_log())?
        .try_into()
        .map_err(|_| "one record")?;
    assert_eq!(record.status, Status::Failure);
    assert_eq!(record.target.as_deref(), Some("p-42"));
    assert_eq!(record.error_code.as_deref(), Some("write_refused"));

    let text = std::fs::read_to_string(runtime.audit_log().path())?;
    assert!(!text.contains(SECRET_MESSAGE));
    Ok(())
}

#[test]
fn a_target_the_outcome_names_wins_over_the_subject_on_success() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let runtime = runtime(&scratch);
    let finished = exec::<Subjects>(
        &runtime,
        &origin(),
        Command::Create {
            subject: "parent-1".to_owned(),
        },
    );
    assert!(finished.settle().1.is_none());

    let lines = raw_lines(runtime.audit_log())?;
    assert_eq!(lines[0]["target"], "parent-1");
    assert_eq!(lines[1]["target"], "n-new");
    let [record] = read(runtime.audit_log())?
        .try_into()
        .map_err(|_| "one record")?;
    assert_eq!(record.target.as_deref(), Some("n-new"));
    Ok(())
}

#[test]
fn a_subject_that_is_not_an_identifier_is_written_as_the_placeholder() -> TestResult {
    let too_long = "n".repeat(TARGET_MAX_LEN + 1);
    for subject in [SECRET_ARG, "", "a b", "n-1\n", "naïve", too_long.as_str()] {
        for command in [
            Command::Touch {
                subject: subject.to_owned(),
            },
            Command::Refuse {
                subject: subject.to_owned(),
            },
        ] {
            let scratch = tempfile::tempdir()?;
            let runtime = runtime(&scratch);
            let (result, warning) = exec::<Subjects>(&runtime, &origin(), command).settle();
            assert!(warning.is_none());
            drop(result);

            let text = std::fs::read_to_string(runtime.audit_log().path())?;
            assert!(!text.contains(SECRET_ARG), "{subject:?}");
            assert!(!text.contains(SECRET_MESSAGE), "{subject:?}");
            assert!(!text.contains(&too_long), "{subject:?}");
            let lines = raw_lines(runtime.audit_log())?;
            assert_eq!(lines.len(), 2, "{subject:?}");
            for line in &lines {
                assert_eq!(line["target"], TARGET_PLACEHOLDER, "{subject:?}");
            }
            let [record] = read(runtime.audit_log())?
                .try_into()
                .map_err(|_| "one record")?;
            assert_eq!(record.target.as_deref(), Some(TARGET_PLACEHOLDER));
        }
    }
    Ok(())
}

#[test]
fn a_subject_of_the_longest_length_is_written_verbatim() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let runtime = runtime(&scratch);
    let subject = "n".repeat(TARGET_MAX_LEN);
    let finished = exec::<Subjects>(
        &runtime,
        &origin(),
        Command::Touch {
            subject: subject.clone(),
        },
    );
    assert!(finished.settle().1.is_none());
    let [record] = read(runtime.audit_log())?
        .try_into()
        .map_err(|_| "one record")?;
    assert_eq!(record.target.as_deref(), Some(subject.as_str()));
    Ok(())
}

#[test]
fn a_read_only_command_asks_for_no_subject_and_touches_no_file() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let runtime = runtime(&scratch);
    let finished = exec::<Subjects>(
        &runtime,
        &origin(),
        Command::Look {
            subject: "n-1".to_owned(),
        },
    );
    assert!(!finished.is_audited());
    assert!(finished.settle().1.is_none());
    assert_eq!(runtime.state().load(Ordering::SeqCst), 0);
    assert!(!scratch.path().join("data").exists());
    Ok(())
}

#[test]
fn a_mutating_command_asks_for_its_subject_once() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let runtime = runtime(&scratch);
    let finished = exec::<Subjects>(
        &runtime,
        &origin(),
        Command::Touch {
            subject: "n-1".to_owned(),
        },
    );
    assert!(finished.settle().1.is_none());
    assert_eq!(runtime.state().load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn an_app_without_a_subject_writes_the_lines_it_always_wrote() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let runtime = Runtime::new(scratch.path().join("data"), ());
    assert!(exec::<Plain>(&runtime, &origin(), ()).settle().1.is_none());

    let lines = raw_lines(runtime.audit_log())?;
    assert_eq!(lines.len(), 2);
    for line in &lines {
        assert!(line["target"].is_null(), "{line}");
    }
    Ok(())
}

#[test]
fn the_started_append_is_still_required_before_the_command_runs() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("data");
    cw_fs::create_private_dir(&root)?;
    let log = AuditLog::new(&root);
    // A directory where the log file belongs refuses the append.
    std::fs::create_dir(log.path())?;
    let runtime = Runtime::new(&root, AtomicUsize::new(0));

    let (result, _) = exec::<Subjects>(
        &runtime,
        &origin(),
        Command::Create {
            subject: "n-1".to_owned(),
        },
    )
    .settle();
    let error = result.expect_err("a refused started append must stop the command");
    assert_eq!(error.code(), "audit_start_failed");
    Ok(())
}
