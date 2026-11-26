//! Reading the audit log: older formats through their upgrade steps, a
//! newer format refused.

use cw_app::{AuditLog, Cap, Error, Status};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn older_lines_are_upgraded_and_a_newer_line_is_refused() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("data");
    cw_fs::create_private_dir(&root)?;
    let log = AuditLog::new(&root);
    let lines = [
        r#"{"format":1,"at":"2026-01-01T00:00:00Z","command":"note add","status":"success","target":"n-1","error_code":null,"duration_ms":3}"#,
        r#"{"format":2,"at":"2026-01-02T00:00:00Z","actor":"u","command":"note remove","status":"failure","target":null,"error_code":"note_missing","duration_ms":1}"#,
        r#"{"format":3,"at":"2026-01-03T00:00:00Z","surface":"cli","actor":"u","command":"note add","status":"success","target":"n-2","error_code":null,"duration_ms":0}"#,
        r#"{"format":99,"at":"2026-01-04T00:00:00Z","command":"note add"}"#,
    ];
    cw_fs::append(&log.path(), format!("{}\n", lines.join("\n")).as_bytes())?;

    let mut read = log.read(Cap::bytes(1 << 16))?.into_iter();
    assert_eq!(read.len(), 4);

    let v1 = read.next().ok_or("line 1")??;
    assert_eq!(v1.command, "note add");
    assert_eq!(v1.target.as_deref(), Some("n-1"));
    assert_eq!(v1.self_reported_actor, None);
    assert_eq!(v1.euid, None);
    assert_eq!(v1.surface, None);
    assert_eq!(v1.duration_ms, Some(3));

    let v2 = read.next().ok_or("line 2")??;
    assert_eq!(v2.status, Status::Failure);
    assert_eq!(v2.self_reported_actor.as_deref(), Some("u"));
    assert_eq!(v2.euid, None);
    assert_eq!(v2.surface, None);
    assert_eq!(v2.error_code.as_deref(), Some("note_missing"));

    let v3 = read.next().ok_or("line 3")??;
    assert_eq!(v3.status, Status::Success);
    assert_eq!(v3.surface.as_deref(), Some("cli"));
    assert_eq!(v3.target.as_deref(), Some("n-2"));
    assert_eq!(v3.duration_ms, Some(0));

    assert!(matches!(
        read.next(),
        Some(Err(Error::NewerFormat {
            line: 4,
            found: 99,
            supported: cw_app::audit::FORMAT,
            ..
        }))
    ));
    Ok(())
}

#[test]
fn a_started_line_keeps_its_target_and_the_outcome_line_replaces_the_record() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("data");
    cw_fs::create_private_dir(&root)?;
    let log = AuditLog::new(&root);
    let lines = [
        r#"{"format":4,"phase":"started","id":"a","at":"2026-01-01T00:00:00Z","surface":"mcp","actor":"u","command":"note touch","status":"unknown","target":"n-1","error_code":null,"duration_ms":null}"#,
        r#"{"format":4,"phase":"started","id":"b","at":"2026-01-02T00:00:00Z","surface":"mcp","actor":"u","command":"note touch","status":"unknown","target":"n-2","error_code":null,"duration_ms":null}"#,
        r#"{"format":4,"phase":"outcome","id":"b","at":"2026-01-02T00:00:00Z","surface":"mcp","actor":"u","command":"note touch","status":"failure","target":"n-2","error_code":"write_refused","duration_ms":2}"#,
        r#"{"format":4,"phase":"started","id":"c","at":"2026-01-03T00:00:00Z","surface":"mcp","actor":"u","command":"note touch","status":"unknown","target":null,"error_code":null,"duration_ms":null}"#,
    ];
    cw_fs::append(&log.path(), format!("{}\n", lines.join("\n")).as_bytes())?;

    let mut read = log.read(Cap::bytes(1 << 16))?.into_iter();
    let unknown = read.next().ok_or("start a")??;
    assert_eq!(unknown.status, Status::Unknown);
    assert_eq!(unknown.target.as_deref(), Some("n-1"));
    assert_eq!(unknown.error_code, None);
    assert_eq!(unknown.duration_ms, None);

    let failed = read.next().ok_or("pair b")??;
    assert_eq!(failed.status, Status::Failure);
    assert_eq!(failed.target.as_deref(), Some("n-2"));
    assert_eq!(failed.error_code.as_deref(), Some("write_refused"));

    let unnamed = read.next().ok_or("start c")??;
    assert_eq!(unnamed.status, Status::Unknown);
    assert_eq!(unnamed.target, None);
    Ok(())
}

/// The effective user ID of a file, read from the file system rather than
/// from the process: the cross-check on the host's `euid`.
#[cfg(unix)]
fn owner_of(path: &std::path::Path) -> Result<u32, Box<dyn std::error::Error>> {
    Ok(rustix::fs::stat(path)?.st_uid)
}

/// A log holding one format 5 line, read back as the record a caller appends.
fn seeded_log() -> Result<(tempfile::TempDir, AuditLog, cw_app::Record), Box<dyn std::error::Error>>
{
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("data");
    cw_fs::create_private_dir(&root)?;
    let log = AuditLog::new(&root);
    let seed = r#"{"format":5,"phase":"standalone","at":"2026-01-01T00:00:00Z","surface":"mcp","self_reported_actor":"agent-7","euid":null,"command":"note add","status":"success","target":"n-1","error_code":null,"duration_ms":1}"#;
    cw_fs::append(&log.path(), format!("{seed}\n").as_bytes())?;
    let read = log.read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = read.as_slice() else {
        return Err(format!("one readable line expected, got {read:?}").into());
    };
    Ok((scratch, log, record.clone()))
}

/// The last line the log holds, as JSON, read whole.
fn last_line(log: &AuditLog) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let bytes = cw_fs::read(&log.path(), Cap::bytes(1 << 16))?;
    let text = String::from_utf8(bytes)?;
    let line = text.lines().last().ok_or("the log has no line")?;
    Ok(serde_json::from_str(line)?)
}

#[test]
fn each_line_carries_the_host_euid_beside_the_self_reported_actor() -> TestResult {
    let (_scratch, log, seeded) = seeded_log()?;
    // The caller passes a forged euid; the host's value is written instead.
    let mut forged = seeded.clone();
    forged.euid = Some(4_000_000_000);
    log.append(&forged, std::time::Duration::from_secs(1))?;

    let line = last_line(&log)?;
    assert_eq!(line["format"], cw_app::audit::FORMAT);
    assert_eq!(line["self_reported_actor"], "agent-7");
    assert!(
        line.get("actor").is_none(),
        "the claim is named, not `actor`"
    );
    #[cfg(unix)]
    assert_eq!(line["euid"], serde_json::json!(owner_of(&log.path())?));
    #[cfg(not(unix))]
    assert!(line["euid"].is_null(), "no host identity on this platform");

    let read = log.read(Cap::bytes(1 << 16))?;
    let [_, Ok(stamped)] = read.as_slice() else {
        return Err(format!("two readable lines expected, got {read:?}").into());
    };
    assert_eq!(stamped.self_reported_actor.as_deref(), Some("agent-7"));
    #[cfg(unix)]
    assert_eq!(stamped.euid, Some(owner_of(&log.path())?));
    #[cfg(not(unix))]
    assert_eq!(stamped.euid, None);
    Ok(())
}

#[test]
fn a_format_4_line_names_its_claim_and_records_no_euid() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("data");
    cw_fs::create_private_dir(&root)?;
    let log = AuditLog::new(&root);
    let older = r#"{"format":4,"phase":"standalone","at":"2026-01-01T00:00:00Z","surface":"mcp","actor":"agent-7","command":"note add","status":"success","target":"n-1","error_code":null,"duration_ms":2}"#;
    cw_fs::append(&log.path(), format!("{older}\n").as_bytes())?;

    let read = log.read(Cap::bytes(1 << 16))?;
    let [Ok(record)] = read.as_slice() else {
        return Err(format!("one readable line expected, got {read:?}").into());
    };
    assert_eq!(record.self_reported_actor.as_deref(), Some("agent-7"));
    assert_eq!(record.euid, None);
    assert_eq!(record.surface.as_deref(), Some("mcp"));
    Ok(())
}
