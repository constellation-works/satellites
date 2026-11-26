//! The chokepoint: what one command leaves in the audit log, and how its
//! error and its audit failure reach the caller.

#![expect(
    clippy::disallowed_methods,
    reason = "exec fixture: reads and creates the raw files a command leaves, to check their bytes on disk"
)]

use cw_app::{
    App, AuditLog, AuditMeta, Ctx, Error as AuditError, Origin, Runtime, Status, Surface, exec,
};
use cw_base::{Clock, Error, Kind};
use cw_fs::Cap;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SECRET_ARG: &str = "argument-value-never-logged";
const SECRET_MESSAGE: &str = "error-message-never-logged";

#[derive(Debug, thiserror::Error)]
enum NotesError {
    #[error("{SECRET_MESSAGE}: {0}")]
    Missing(String),
}

impl From<NotesError> for Error {
    fn from(error: NotesError) -> Self {
        match &error {
            NotesError::Missing(_) => Error::new(Kind::NotFound, "note_missing", error),
        }
    }
}

enum Command {
    Add { body: String },
    Remove { id: String },
    Show,
}

struct Notes;

impl App for Notes {
    type State = ();
    type Command = Command;
    type Outcome = Option<String>;
    type Error = NotesError;

    fn audit(command: &Command) -> Option<AuditMeta> {
        match command {
            Command::Add { .. } => Some(AuditMeta::new("note add")),
            Command::Remove { .. } => Some(AuditMeta::new("note remove")),
            Command::Show => None,
        }
    }

    fn run(_: &Ctx<'_, ()>, command: Command) -> Result<Option<String>, NotesError> {
        match command {
            Command::Add { body } => Ok(Some(format!("n-{}", body.len()))),
            Command::Remove { id } => Err(NotesError::Missing(id)),
            Command::Show => Ok(None),
        }
    }

    fn target(outcome: &Option<String>) -> Option<String> {
        outcome.clone()
    }
}

/// A private data directory in a fresh scratch directory (the scratch
/// directory itself is open to other users, which the audit log refuses).
fn data_dir() -> std::io::Result<(tempfile::TempDir, std::path::PathBuf)> {
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("data");
    Ok((scratch, root))
}

fn origin() -> Origin {
    Origin::new(Surface::MCP, Some("agent-7".to_owned()))
}

fn pinned() -> Result<Clock, cw_base::TimestampError> {
    Ok(Clock::fixed(cw_base::time::parse_rfc3339(
        "2026-11-24T09:30:00Z",
    )?))
}

struct WriteNote;

impl App for WriteNote {
    type State = AuditLog;
    type Command = ();
    type Outcome = usize;
    type Error = Error;

    fn audit((): &()) -> Option<AuditMeta> {
        Some(AuditMeta::new("note write"))
    }

    fn run(ctx: &Ctx<'_, AuditLog>, (): ()) -> Result<usize, Error> {
        let log = ctx.state();
        let records_at_entry = log.read(Cap::bytes(1 << 16));
        cw_fs::write_atomic(&log.root().join("note"), b"changed")
            .map_err(|error| Error::new(error.kind(), "note_write_failed", error))?;
        records_at_entry
            .map(|records| records.len())
            .map_err(|error| Error::new(error.kind(), "note_audit_read_failed", error))
    }

    fn target(_: &usize) -> Option<String> {
        None
    }
}

#[test]
fn the_started_line_is_visible_from_inside_the_command() -> TestResult {
    let (_scratch, root) = data_dir()?;
    cw_fs::create_private_dir(&root)?;
    let runtime = Runtime::new(&root, AuditLog::new(&root));

    let finished = exec::<WriteNote>(&runtime, &origin(), ());
    assert_eq!(finished.result().ok().copied(), Some(1));
    assert_eq!(std::fs::read(root.join("note"))?, b"changed");
    assert!(finished.settle().1.is_none());
    Ok(())
}

#[test]
fn a_failed_started_append_leaves_command_data_untouched() -> TestResult {
    let (_scratch, root) = data_dir()?;
    cw_fs::create_private_dir(&root)?;
    let path = root.join("note");
    cw_fs::write_atomic(&path, b"original")?;
    let log = AuditLog::new(&root);
    std::fs::create_dir(log.path())?;
    let runtime = Runtime::new(&root, log);

    let finished = exec::<WriteNote>(&runtime, &origin(), ());
    assert_eq!(std::fs::read(path)?, b"original");
    let (result, warning) = finished.settle();
    let error = result.expect_err("a failed started append must stop the command");
    assert_eq!(error.kind(), Kind::Refused);
    assert_eq!(error.code(), "audit_start_failed");
    assert!(matches!(
        error.downcast_ref::<AuditError>(),
        Some(AuditError::Fs(_))
    ));
    assert!(warning.is_none());
    Ok(())
}

#[test]
fn a_mutating_command_writes_two_lines_of_identifiers_with_the_same_id() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = Runtime::new(&root, ()).with_clock(pinned()?);

    let finished = exec::<Notes>(
        &runtime,
        &origin(),
        Command::Add {
            body: SECRET_ARG.to_owned(),
        },
    );
    let lines = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(started)] = lines.as_slice() else {
        panic!("one started command expected, got {lines:?}");
    };
    assert_eq!(started.status, Status::Unknown);
    assert_eq!(started.target, None);
    assert_eq!(started.error_code, None);
    assert_eq!(started.duration_ms, None);

    let (result, warning) = finished.settle();
    assert_eq!(result?.as_deref(), Some("n-27"));
    assert!(warning.is_none());

    let text = std::fs::read_to_string(runtime.audit_log().path())?;
    assert!(!text.contains(SECRET_ARG));
    let physical_lines: Vec<serde_json::Value> = text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let [started, outcome] = physical_lines.as_slice() else {
        panic!("expected two physical lines, got {physical_lines:?}");
    };
    assert_eq!(started["format"], cw_app::audit::FORMAT);
    assert_eq!(started["phase"], "started");
    assert_eq!(outcome["phase"], "outcome");
    assert!(started["id"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(started["id"], outcome["id"]);

    let lines = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = lines.as_slice() else {
        panic!("one readable line expected, got {lines:?}");
    };
    assert_eq!(record.command, "note add");
    assert_eq!(record.target.as_deref(), Some("n-27"));
    assert_eq!(record.status, Status::Success);
    assert_eq!(record.error_code, None);
    assert!(record.duration_ms.is_some());
    assert_eq!(record.actor.as_deref(), Some("agent-7"));
    assert_eq!(record.surface.as_deref(), Some("mcp"));
    Ok(())
}

#[test]
fn a_failed_command_logs_its_code_never_its_message_or_arguments() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = Runtime::new(&root, ());

    let finished = exec::<Notes>(
        &runtime,
        &origin(),
        Command::Remove {
            id: SECRET_ARG.to_owned(),
        },
    );
    let (result, warning) = finished.settle();
    assert!(result.is_err());
    assert!(warning.is_none());

    let text = std::fs::read_to_string(runtime.audit_log().path())?;
    assert_eq!(text.lines().count(), 2);
    assert!(!text.contains(SECRET_ARG));
    assert!(!text.contains(SECRET_MESSAGE));
    let lines = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = lines.as_slice() else {
        panic!("one readable line expected, got {lines:?}");
    };
    assert_eq!(record.command, "note remove");
    assert_eq!(record.status, Status::Failure);
    assert_eq!(record.error_code.as_deref(), Some("note_missing"));
    assert_eq!(record.target, None);
    assert_eq!(record.actor.as_deref(), Some("agent-7"));
    assert_eq!(record.surface.as_deref(), Some("mcp"));
    Ok(())
}

#[test]
fn a_read_only_command_creates_no_file_and_takes_no_lock() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("data");
    let runtime = Runtime::new(&root, ());

    let finished = exec::<Notes>(&runtime, &origin(), Command::Show);
    assert!(!finished.is_audited());
    let (result, warning) = finished.settle();
    assert!(result.is_ok());
    assert!(warning.is_none());

    // The data directory, the log and the audit lock file are all absent.
    assert!(!root.exists());
    assert!(!runtime.audit_log().path().exists());
    assert!(!runtime.audit_log().lock_path().exists());
    Ok(())
}

#[test]
fn an_app_error_reaches_the_caller_with_its_kind_code_and_typed_cause() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = Runtime::new(&root, ());

    let (result, _) = exec::<Notes>(
        &runtime,
        &origin(),
        Command::Remove {
            id: "n-9".to_owned(),
        },
    )
    .settle();
    let Err(error) = result else {
        return Err("the command was expected to fail".into());
    };
    assert_eq!(error.kind(), Kind::NotFound);
    assert_eq!(error.code(), "note_missing");
    assert!(matches!(
        error.downcast_ref::<NotesError>(),
        Some(NotesError::Missing(id)) if id == "n-9"
    ));
    Ok(())
}

#[test]
fn a_failed_outcome_append_warns_and_the_start_reads_as_unknown() -> TestResult {
    use std::time::Duration;

    let (_scratch, root) = data_dir()?;
    cw_fs::create_private_dir(&root)?;
    let log = AuditLog::new(&root);
    let runtime = Runtime::new(&root, log.clone()).with_audit_wait(Duration::ZERO);

    let finished = exec::<WriteNote>(&runtime, &origin(), ());
    assert_eq!(std::fs::read(root.join("note"))?, b"changed");
    // Holding the lock on this thread refuses reentry during settle.
    let _lock = cw_fs::Lock::acquire(&log.lock_path(), "test", Duration::ZERO)?;

    let (result, warning) = finished.settle();
    assert_eq!(result?, 1);
    let Some(warning) = warning else {
        return Err("the failed append was expected to come back as a warning".into());
    };
    assert_eq!(warning.command(), "note write");
    assert!(matches!(
        warning.error(),
        AuditError::Fs(cw_fs::Error::Reentrant { .. })
    ));

    let records = log.read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = records.as_slice() else {
        panic!("one unfinished command expected, got {records:?}");
    };
    assert_eq!(record.status, Status::Unknown);
    assert_eq!(record.command, "note write");
    assert_eq!(record.duration_ms, None);
    Ok(())
}

#[test]
fn interleaved_commands_pair_by_id_in_start_order() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = Runtime::new(&root, ()).with_clock(pinned()?);
    let first = exec::<Notes>(&runtime, &origin(), Command::Add { body: "a".into() });
    let second = exec::<Notes>(&runtime, &origin(), Command::Add { body: "bb".into() });
    assert!(second.settle().1.is_none());
    // An unsettled but completed command still writes its known outcome on drop.
    drop(first);

    let records = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(first), Ok(second)] = records.as_slice() else {
        panic!("two paired commands expected, got {records:?}");
    };
    assert_eq!(first.at, second.at);
    assert_eq!(first.command, second.command);
    assert_eq!(first.target.as_deref(), Some("n-1"));
    assert_eq!(second.target.as_deref(), Some("n-2"));
    Ok(())
}

#[test]
fn the_clock_and_actor_come_from_the_context() -> TestResult {
    struct Stamp;
    impl App for Stamp {
        type State = ();
        type Command = ();
        type Outcome = (String, Option<String>);
        type Error = NotesError;
        fn audit((): &()) -> Option<AuditMeta> {
            Some(AuditMeta::new("stamp"))
        }
        fn run(ctx: &Ctx<'_, ()>, (): ()) -> Result<Self::Outcome, NotesError> {
            let at = cw_base::time::format_rfc3339(ctx.now()).unwrap_or_default();
            Ok((at, ctx.actor().map(str::to_owned)))
        }
        fn target(_: &Self::Outcome) -> Option<String> {
            None
        }
    }

    let (_scratch, root) = data_dir()?;
    let runtime = Runtime::new(&root, ()).with_clock(pinned()?);
    let (result, warning) = exec::<Stamp>(&runtime, &origin(), ()).settle();
    assert!(warning.is_none());
    let (at, actor) = result?;
    assert_eq!(at, "2026-11-24T09:30:00Z");
    assert_eq!(actor.as_deref(), Some("agent-7"));

    let lines = AuditLog::new(&root).read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = lines.as_slice() else {
        panic!("one readable line expected, got {lines:?}");
    };
    assert_eq!(record.at, "2026-11-24T09:30:00Z");
    Ok(())
}

#[test]
fn an_actor_that_is_not_an_identifier_is_never_written_verbatim() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = Runtime::new(&root, ());
    let claim = format!("client v1 token={SECRET_ARG}");
    let origin = Origin::new(Surface::MCP, Some(claim.clone()));

    let (result, warning) = exec::<Notes>(
        &runtime,
        &origin,
        Command::Add {
            body: "x".to_owned(),
        },
    )
    .settle();
    assert!(result.is_ok());
    assert!(warning.is_none());

    let text = std::fs::read_to_string(runtime.audit_log().path())?;
    assert!(!text.contains(SECRET_ARG));
    let lines = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = lines.as_slice() else {
        panic!("one readable line expected, got {lines:?}");
    };
    assert_eq!(
        record.actor.as_deref(),
        Some(cw_app::audit::ACTOR_PLACEHOLDER)
    );

    // The same rule holds for a record appended to the log directly.
    let mut direct = record.clone();
    direct.actor = Some(claim);
    runtime
        .audit_log()
        .append(&direct, std::time::Duration::from_secs(1))?;
    let text = std::fs::read_to_string(runtime.audit_log().path())?;
    assert!(!text.contains(SECRET_ARG));
    let lines = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(_), Ok(appended)] = lines.as_slice() else {
        panic!("two readable lines expected, got {lines:?}");
    };
    assert_eq!(
        appended.actor.as_deref(),
        Some(cw_app::audit::ACTOR_PLACEHOLDER)
    );
    Ok(())
}

#[test]
fn a_torn_last_line_does_not_swallow_the_next_record() -> TestResult {
    let (_scratch, root) = data_dir()?;
    cw_fs::create_private_dir(&root)?;
    let log = AuditLog::new(&root);
    let whole = r#"{"format":3,"at":"2026-01-01T00:00:00Z","surface":"cli","actor":null,"command":"note add","status":"success","target":"n-1","error_code":null,"duration_ms":1}"#;
    // A crash during the second append left part of its line.
    cw_fs::append(
        &log.path(),
        format!("{whole}\n{{\"format\":3,\"at").as_bytes(),
    )?;
    let runtime = Runtime::new(&root, ());

    let (result, warning) = exec::<Notes>(
        &runtime,
        &origin(),
        Command::Add {
            body: "x".to_owned(),
        },
    )
    .settle();
    assert!(result.is_ok());
    assert!(warning.is_none(), "{warning:?}");

    let lines = log.read(Cap::bytes(1 << 16))?;
    let [Ok(first), Err(AuditError::Corrupt { line: 2, .. }), Ok(new)] = lines.as_slice() else {
        panic!("a whole line, the torn one, then the new record expected, got {lines:?}");
    };
    assert_eq!(first.target.as_deref(), Some("n-1"));
    assert_eq!(new.target.as_deref(), Some("n-1"));
    assert_eq!(new.actor.as_deref(), Some("agent-7"));
    Ok(())
}

#[test]
fn a_log_in_a_newer_format_is_not_appended_to() -> TestResult {
    let newer = r#"{"format":99,"at":"2026-01-03T00:00:00Z","command":"note add"}"#;
    // A log of exactly the 64 KiB an append reads, its newer line first:
    // read whole, not as the cut end of a longer file.
    let mut at_the_cap = format!("{newer}\n");
    while at_the_cap.len() < 64 * 1024 - 1 {
        at_the_cap.push(' ');
    }
    at_the_cap.push('\n');
    let cases = [
        ("the newest line", format!("{newer}\n")),
        // A malformed newer line does not hide the newer one before it.
        ("under a malformed line", format!("{newer}\nnot json\n")),
        // Whole JSON missing only its line break still counts.
        ("without its line break", newer.to_owned()),
        ("at the start of a log at the cap", at_the_cap),
    ];
    for (case, content) in cases {
        let (_scratch, root) = data_dir()?;
        cw_fs::create_private_dir(&root)?;
        let log = AuditLog::new(&root);
        cw_fs::append(&log.path(), content.as_bytes())?;
        let runtime = Runtime::new(&root, ());

        let (result, warning) = exec::<Notes>(
            &runtime,
            &origin(),
            Command::Add {
                body: "x".to_owned(),
            },
        )
        .settle();
        assert!(warning.is_none(), "{case}");
        let error = result.expect_err("a newer log must stop the command before running");
        assert!(
            matches!(
                error.downcast_ref::<AuditError>(),
                Some(AuditError::NewerLog {
                    found: 99,
                    supported: cw_app::audit::FORMAT,
                    ..
                })
            ),
            "{case}: {error}"
        );
        assert_eq!(std::fs::read(log.path())?, content.as_bytes(), "{case}");
    }
    Ok(())
}
