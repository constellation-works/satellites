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
    assert_eq!(v1.actor, None);
    assert_eq!(v1.surface, None);
    assert_eq!(v1.duration_ms, Some(3));

    let v2 = read.next().ok_or("line 2")??;
    assert_eq!(v2.status, Status::Failure);
    assert_eq!(v2.actor.as_deref(), Some("u"));
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
