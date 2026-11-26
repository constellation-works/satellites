//! An itemized command: zero or many started and outcome pairs, admitted
//! inside its transaction and settled after the reply.

#![expect(
    clippy::disallowed_methods,
    reason = "itemized fixture: reads the raw log bytes to check them on disk, and makes the log path a directory to refuse the append"
)]

use cw_app::testing::exec_admitting;
use cw_app::{
    App, AuditLog, AuditMeta, Ctx, Error as AuditError, Item, Origin, Runtime, Status, Surface,
    exec,
};
use cw_base::{Error, Kind};
use cw_fs::Cap;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Debug, thiserror::Error)]
#[error("the command failed: {0}")]
struct Failed(&'static str);

impl From<Failed> for Error {
    fn from(error: Failed) -> Self {
        Error::new(Kind::Refused, error.0, error)
    }
}

/// What an itemized command does, in order: admit, then end.
#[derive(Clone, Default)]
struct Plan {
    items: Vec<Item>,
    /// Admit, take the audit lock so the early outcome write fails, release.
    committed: bool,
    /// Ends with this code after admitting.
    fail: Option<&'static str>,
}

struct Itemized;

impl App for Itemized {
    type State = AuditLog;
    type Command = Plan;
    type Outcome = usize;
    type Error = Failed;

    fn audit(_: &Plan) -> Option<AuditMeta> {
        Some(AuditMeta::itemized("note batch"))
    }

    fn run(ctx: &Ctx<'_, AuditLog>, plan: Plan) -> Result<usize, Failed> {
        let log = ctx.state();
        let batch = ctx
            .audit()
            .admit(plan.items.clone())
            .map_err(|error| Failed(error.code()))?;
        if plan.committed {
            // A hold on this thread refuses the early write, as a busy lock would.
            let held = cw_fs::Lock::acquire(&log.lock_path(), "test", Duration::ZERO);
            batch.committed();
            drop(held);
        } else {
            // The kit keeps the batch; the app has nothing more to do with it.
            let _ = batch;
        }
        match plan.fail {
            Some(code) => Err(Failed(code)),
            None => Ok(plan.items.len()),
        }
    }

    // Never called for an itemized command.
    fn target(_: &usize) -> Option<String> {
        panic!("target is not called for an itemized command")
    }

    fn subject(_: &Ctx<'_, AuditLog>, _: &Plan) -> Option<String> {
        panic!("subject is not called for an itemized command")
    }
}

fn data_dir() -> std::io::Result<(tempfile::TempDir, std::path::PathBuf)> {
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("data");
    Ok((scratch, root))
}

fn origin() -> Origin {
    Origin::new(Surface::MCP, Some("agent-7".to_owned()))
}

fn runtime(root: &std::path::Path) -> Runtime<AuditLog> {
    Runtime::new(root, AuditLog::new(root))
}

fn wrote(id: &str) -> Item {
    Item::wrote(Some(id.to_owned()))
}

fn physical(root: &std::path::Path) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(root.join("audit.jsonl"))?;
    Ok(text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}

#[test]
fn n_items_write_n_started_lines_then_n_outcomes_after_the_reply() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = runtime(&root);
    let plan = Plan {
        items: vec![
            wrote("s-1"),
            Item::refused(Some("s-2".to_owned()), "directive_refused"),
            Item::wrote(Some("a subject with spaces".to_owned())),
        ],
        ..Plan::default()
    };

    let finished = exec::<Itemized>(&runtime, &origin(), plan);
    assert_eq!(finished.admitted(), 3);
    assert!(finished.is_audited());

    // After the run, before the reply: three started lines, no outcome.
    let lines = physical(&root)?;
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| line["phase"] == "started"));
    let mut ids: Vec<&str> = lines
        .iter()
        .filter_map(|line| line["id"].as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 3, "each item has its own id");
    assert!(ids.iter().all(|id| id.len() == 32));
    let read = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    assert!(
        read.iter()
            .all(|record| record.as_ref().is_ok_and(|r| r.status == Status::Unknown)),
        "{read:?}"
    );

    let (result, warning) = finished.settle();
    assert_eq!(result?, 3);
    assert!(warning.is_none());

    let lines = physical(&root)?;
    assert_eq!(lines.len(), 6);
    assert!(lines[3..].iter().all(|line| line["phase"] == "outcome"));
    let read = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(first), Ok(second), Ok(third)] = read.as_slice() else {
        panic!("three paired items expected, got {read:?}");
    };
    assert_eq!(first.command, "note batch");
    assert_eq!(first.status, Status::Success);
    assert_eq!(first.target.as_deref(), Some("s-1"));
    assert_eq!(first.error_code, None);
    assert_eq!(first.self_reported_actor.as_deref(), Some("agent-7"));
    assert_eq!(first.surface.as_deref(), Some("mcp"));
    assert!(first.duration_ms.is_some());
    assert_eq!(second.status, Status::Failure);
    assert_eq!(second.target.as_deref(), Some("s-2"));
    assert_eq!(second.error_code.as_deref(), Some("directive_refused"));
    // The subject guard holds for every item.
    assert_eq!(third.status, Status::Success);
    assert_eq!(
        third.target.as_deref(),
        Some(cw_app::audit::TARGET_PLACEHOLDER)
    );
    Ok(())
}

#[test]
fn a_started_line_with_no_outcome_reads_as_unknown_with_its_target() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = runtime(&root);
    let finished = exec::<Itemized>(
        &runtime,
        &origin(),
        Plan {
            items: vec![wrote("s-1"), wrote("s-2")],
            ..Plan::default()
        },
    );
    // The surface never settles, and the value never drops: a crash.
    std::mem::forget(finished);

    let read = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(first), Ok(second)] = read.as_slice() else {
        panic!("two unfinished items expected, got {read:?}");
    };
    assert_eq!(first.status, Status::Unknown);
    assert_eq!(first.target.as_deref(), Some("s-1"));
    assert_eq!(second.status, Status::Unknown);
    assert_eq!(second.target.as_deref(), Some("s-2"));
    assert_eq!(first.duration_ms, None);
    Ok(())
}

#[test]
fn admitting_nothing_and_succeeding_writes_no_line_and_takes_no_lock() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = runtime(&root);

    // An empty batch is nothing admitted.
    let finished = exec::<Itemized>(&runtime, &origin(), Plan::default());
    assert_eq!(finished.admitted(), 0);
    assert!(!finished.is_audited());
    let (result, warning) = finished.settle();
    assert_eq!(result?, 0);
    assert!(warning.is_none());
    assert!(!root.exists());
    assert!(!runtime.audit_log().lock_path().exists());
    Ok(())
}

#[test]
fn failing_with_nothing_admitted_writes_one_standalone_failure_line() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = runtime(&root);
    let finished = exec::<Itemized>(
        &runtime,
        &origin(),
        Plan {
            fail: Some("store_busy"),
            ..Plan::default()
        },
    );
    // Nothing is written before the reply is settled.
    assert!(!root.exists());
    let (result, warning) = finished.settle();
    assert_eq!(result.expect_err("it failed").code(), "store_busy");
    assert!(warning.is_none());

    let lines = physical(&root)?;
    let [line] = lines.as_slice() else {
        panic!("one line expected, got {lines:?}");
    };
    assert_eq!(line["phase"], "standalone");
    let read = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = read.as_slice() else {
        panic!("one record expected, got {read:?}");
    };
    assert_eq!(record.command, "note batch");
    assert_eq!(record.status, Status::Failure);
    assert_eq!(record.error_code.as_deref(), Some("store_busy"));
    assert_eq!(record.target, None);
    assert_eq!(record.self_reported_actor.as_deref(), Some("agent-7"));
    Ok(())
}

#[test]
fn a_failed_admit_returns_the_error_and_writes_no_outcome() -> TestResult {
    let (_scratch, root) = data_dir()?;
    cw_fs::create_private_dir(&root)?;
    let log = AuditLog::new(&root);
    // A directory where the log file belongs: the append is refused.
    std::fs::create_dir(log.path())?;
    let runtime = Runtime::new(&root, log);

    let finished = exec::<Itemized>(
        &runtime,
        &origin(),
        Plan {
            items: vec![wrote("s-1"), wrote("s-2")],
            ..Plan::default()
        },
    );
    assert_eq!(finished.admitted(), 0);
    let (result, warning) = finished.settle();
    let error = result.expect_err("a failed admit must stop the command");
    assert_eq!(error.code(), "audit_start_failed");
    assert!(warning.is_none());
    assert!(runtime.audit_log().path().is_dir());
    Ok(())
}

#[test]
fn a_failed_admit_keeps_the_kind_and_typed_cause() -> TestResult {
    struct Direct;
    impl App for Direct {
        type State = AuditLog;
        type Command = ();
        type Outcome = ();
        type Error = Error;
        fn audit((): &()) -> Option<AuditMeta> {
            Some(AuditMeta::itemized("note direct"))
        }
        fn run(ctx: &Ctx<'_, AuditLog>, (): ()) -> Result<(), Error> {
            ctx.audit().admit([wrote("s-1")]).map(|_| ())
        }
        fn target(_: &()) -> Option<String> {
            None
        }
    }

    let (_scratch, root) = data_dir()?;
    cw_fs::create_private_dir(&root)?;
    let log = AuditLog::new(&root);
    std::fs::create_dir(log.path())?;
    let runtime = Runtime::new(&root, log);
    let (result, _) = exec::<Direct>(&runtime, &origin(), ()).settle();
    let error = result.expect_err("refused");
    assert_eq!(error.kind(), Kind::Refused);
    assert_eq!(error.code(), "audit_start_failed");
    assert!(matches!(
        error.downcast_ref::<AuditError>(),
        Some(AuditError::Fs(_))
    ));
    Ok(())
}

#[test]
fn a_run_error_after_admit_settles_every_item_as_a_failure_with_its_code() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = runtime(&root);
    let finished = exec::<Itemized>(
        &runtime,
        &origin(),
        Plan {
            items: vec![wrote("s-1"), Item::refused(None, "directive_refused")],
            fail: Some("commit_failed"),
            ..Plan::default()
        },
    );
    let (result, warning) = finished.settle();
    assert_eq!(result.expect_err("it failed").code(), "commit_failed");
    assert!(warning.is_none());

    let read = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(first), Ok(second)] = read.as_slice() else {
        panic!("two items expected, got {read:?}");
    };
    for record in [first, second] {
        assert_eq!(record.status, Status::Failure);
        assert_eq!(record.error_code.as_deref(), Some("commit_failed"));
    }
    // The subject stays beside the code.
    assert_eq!(first.target.as_deref(), Some("s-1"));
    // No standalone line on top of the items.
    assert_eq!(physical(&root)?.len(), 4);
    Ok(())
}

#[test]
fn committed_writes_the_outcomes_at_once_and_settle_adds_none() -> TestResult {
    let (_scratch, root) = data_dir()?;
    cw_fs::create_private_dir(&root)?;
    let log = AuditLog::new(&root);
    let runtime = Runtime::new(&root, log.clone());

    // `committed` outside any hold: the outcomes land before the run ends.
    struct Stepwise;
    impl App for Stepwise {
        type State = AuditLog;
        type Command = bool;
        type Outcome = ();
        type Error = Failed;
        fn audit(_: &bool) -> Option<AuditMeta> {
            Some(AuditMeta::itemized("note step"))
        }
        fn run(ctx: &Ctx<'_, AuditLog>, fail_after: bool) -> Result<(), Failed> {
            let batch = ctx
                .audit()
                .admit([
                    wrote("s-1"),
                    Item::refused(Some("s-2".to_owned()), "refused"),
                ])
                .map_err(|error| Failed(error.code()))?;
            batch.committed();
            let inside = ctx
                .state()
                .read(Cap::bytes(1 << 16))
                .map_err(|_| Failed("read"))?;
            assert!(
                inside
                    .iter()
                    .all(|record| record.as_ref().is_ok_and(|r| r.status != Status::Unknown)),
                "the outcomes are written when committed returns"
            );
            if fail_after {
                Err(Failed("later_failure"))
            } else {
                Ok(())
            }
        }
        fn target(_: &()) -> Option<String> {
            None
        }
    }

    for fail_after in [false, true] {
        let finished = exec::<Stepwise>(&runtime, &origin(), fail_after);
        let before = physical(&root)?.len();
        let (_, warning) = finished.settle();
        assert!(warning.is_none());
        assert_eq!(physical(&root)?.len(), before, "settle adds no line");
    }

    let read = log.read(Cap::bytes(1 << 16))?;
    assert_eq!(read.len(), 4);
    for pair in read.chunks(2) {
        let [Ok(wrote), Ok(refused)] = pair else {
            panic!("a pair of items expected, got {pair:?}");
        };
        // A later failure of `run` does not rewrite what was committed.
        assert_eq!(wrote.status, Status::Success);
        assert_eq!(refused.status, Status::Failure);
        assert_eq!(refused.error_code.as_deref(), Some("refused"));
    }
    Ok(())
}

#[test]
fn a_failed_committed_write_is_the_warning_settle_returns() -> TestResult {
    let (_scratch, root) = data_dir()?;
    cw_fs::create_private_dir(&root)?;
    let log = AuditLog::new(&root);
    let runtime = Runtime::new(&root, log.clone()).with_audit_wait(Duration::ZERO);

    let finished = exec::<Itemized>(
        &runtime,
        &origin(),
        Plan {
            items: vec![wrote("s-1"), wrote("s-2")],
            committed: true,
            ..Plan::default()
        },
    );
    let (result, warning) = finished.settle();
    assert_eq!(result?, 2);
    let Some(warning) = warning else {
        return Err("the failed early write was expected to come back as a warning".into());
    };
    assert_eq!(warning.command(), "note batch");
    assert!(matches!(
        warning.error(),
        AuditError::Fs(cw_fs::Error::Reentrant { .. })
    ));

    // No second attempt: both items stay unknown.
    let read = log.read(Cap::bytes(1 << 16))?;
    let [Ok(first), Ok(second)] = read.as_slice() else {
        panic!("two unfinished items expected, got {read:?}");
    };
    assert_eq!(first.status, Status::Unknown);
    assert_eq!(second.status, Status::Unknown);
    Ok(())
}

#[test]
fn dropping_an_unsettled_record_still_writes_its_outcomes() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = runtime(&root);
    drop(exec::<Itemized>(
        &runtime,
        &origin(),
        Plan {
            items: vec![wrote("s-1")],
            ..Plan::default()
        },
    ));
    let read = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = read.as_slice() else {
        panic!("one item expected, got {read:?}");
    };
    assert_eq!(record.status, Status::Success);
    Ok(())
}

#[test]
fn a_command_admits_once() -> TestResult {
    struct Twice;
    impl App for Twice {
        type State = ();
        type Command = ();
        type Outcome = ();
        type Error = Error;
        fn audit((): &()) -> Option<AuditMeta> {
            Some(AuditMeta::itemized("note twice"))
        }
        fn run(ctx: &Ctx<'_, ()>, (): ()) -> Result<(), Error> {
            ctx.audit().admit([wrote("s-1")])?;
            ctx.audit().admit([wrote("s-2")]).map(|_| ())
        }
        fn target(_: &()) -> Option<String> {
            None
        }
    }

    let (_scratch, root) = data_dir()?;
    let runtime = Runtime::new(&root, ());
    let (result, _) = exec::<Twice>(&runtime, &origin(), ()).settle();
    let error = result.expect_err("the second admit is refused");
    assert_eq!(error.code(), "audit_admitted_twice");
    assert_eq!(error.kind(), Kind::Internal);
    // The first item settled as a failure with the command's code.
    let read = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = read.as_slice() else {
        panic!("one item expected, got {read:?}");
    };
    assert_eq!(record.status, Status::Failure);
    assert_eq!(record.error_code.as_deref(), Some("audit_admitted_twice"));
    Ok(())
}

#[test]
fn single_and_read_only_commands_cannot_admit() -> TestResult {
    struct Single;
    impl App for Single {
        type State = ();
        type Command = ();
        type Outcome = ();
        type Error = Error;
        fn audit((): &()) -> Option<AuditMeta> {
            Some(AuditMeta::new("note single"))
        }
        fn run(ctx: &Ctx<'_, ()>, (): ()) -> Result<(), Error> {
            ctx.audit().admit([wrote("s-1")]).map(|_| ())
        }
        fn target(_: &()) -> Option<String> {
            None
        }
    }

    struct ReadOnly;
    impl App for ReadOnly {
        type State = ();
        type Command = ();
        type Outcome = ();
        type Error = Error;
        fn audit((): &()) -> Option<AuditMeta> {
            None
        }
        fn run(ctx: &Ctx<'_, ()>, (): ()) -> Result<(), Error> {
            ctx.audit().admit([wrote("s-1")]).map(|_| ())
        }
        fn target(_: &()) -> Option<String> {
            None
        }
    }

    let (_scratch, root) = data_dir()?;
    let runtime = Runtime::new(&root, ());
    let (result, _) = exec::<Single>(&runtime, &origin(), ()).settle();
    let error = result.expect_err("a single command cannot admit");
    assert_eq!(error.code(), "audit_not_itemized");
    // The single record is the only line pair.
    let read = runtime.audit_log().read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = read.as_slice() else {
        panic!("one record expected, got {read:?}");
    };
    assert_eq!(record.command, "note single");
    assert_eq!(record.error_code.as_deref(), Some("audit_not_itemized"));

    // A read-only command takes no lock and writes nothing, even asking.
    let (_scratch, root) = data_dir()?;
    let runtime = Runtime::new(&root, ());
    let (result, _) = exec::<ReadOnly>(&runtime, &origin(), ()).settle();
    assert_eq!(result.expect_err("refused").code(), "audit_not_itemized");
    assert!(!root.exists());
    Ok(())
}

#[test]
fn a_detached_context_admits_without_writing() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let log = AuditLog::new(&root);
    let origin = origin();
    let ctx = Ctx::new(&log, cw_base::Clock::system(), &origin);
    let batch = ctx.audit().admit([wrote("s-1")])?;
    batch.committed();
    assert!(!root.exists());
    Ok(())
}

#[test]
fn the_helper_fails_an_itemized_command_that_wrote_without_admitting() -> TestResult {
    let (_scratch, root) = data_dir()?;
    let runtime = runtime(&root);

    // Admitting, or changing nothing, or failing: all pass.
    let finished = exec_admitting::<Itemized>(
        &runtime,
        &origin(),
        Plan {
            items: vec![wrote("s-1")],
            ..Plan::default()
        },
        || true,
    );
    assert_eq!(finished.admitted(), 1);
    drop(finished);
    drop(exec_admitting::<Itemized>(
        &runtime,
        &origin(),
        Plan::default(),
        || false,
    ));
    drop(exec_admitting::<Itemized>(
        &runtime,
        &origin(),
        Plan {
            fail: Some("store_busy"),
            ..Plan::default()
        },
        || true,
    ));

    // Ok, wrote, admitted nothing: fails.
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(exec_admitting::<Itemized>(
            &runtime,
            &origin(),
            Plan::default(),
            || true,
        ));
    }));
    assert!(caught.is_err(), "the helper must fail the test");
    Ok(())
}
