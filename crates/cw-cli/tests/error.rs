//! A failure on stderr, and the exit code its kind calls for.

use cw_app::Env;
use cw_base::{Error, Kind};
use cw_cli::{Format, Probe, Sink, Terminal, exit_code};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Debug, thiserror::Error)]
#[error("no note named {0}")]
struct Missing(&'static str);

fn report(
    format: Option<Format>,
    hint: Option<&str>,
) -> Result<(u8, String, Vec<u8>), Box<dyn std::error::Error>> {
    let terminal = Terminal::decide(&Env::default(), "notes", Probe::piped(), format);
    let mut sink = Sink::new(terminal, Vec::new(), Vec::new());
    let error = Error::new(Kind::NotFound, "note_not_found", Missing("draft"));
    let exit = sink.report(&error, hint);
    let (out, err) = sink.into_parts();
    Ok((exit.code(), String::from_utf8(err)?, out))
}

#[test]
fn text_mode_prints_error_first_on_stderr() -> TestResult {
    let (code, err, out) = report(None, Some("run `notes list` to see the names"))?;
    assert_eq!(
        err,
        "error: no note named draft\nhint: run `notes list` to see the names\n"
    );
    assert!(out.is_empty());
    assert_eq!(code, 1);
    Ok(())
}

#[test]
fn json_mode_prints_one_object_with_error_code_and_hint() -> TestResult {
    let (_, err, out) = report(Some(Format::Json), Some("run `notes list`"))?;
    assert!(out.is_empty());
    assert_eq!(err.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(&err)?;
    assert_eq!(
        value,
        serde_json::json!({"error": "no note named draft", "code": "note_not_found", "hint": "run `notes list`"})
    );

    let (_, err, _) = report(Some(Format::Json), None)?;
    let value: serde_json::Value = serde_json::from_str(&err)?;
    assert_eq!(
        value,
        serde_json::json!({"error": "no note named draft", "code": "note_not_found", "hint": null})
    );
    Ok(())
}

#[test]
fn usage_exits_2_and_every_other_kind_exits_1() {
    for kind in [
        Kind::Usage,
        Kind::NotFound,
        Kind::Refused,
        Kind::Busy,
        Kind::Corrupt,
        Kind::Incompatible,
        Kind::Config,
        Kind::Io,
        Kind::Internal,
    ] {
        // Exhaustive: a new kind fails to compile here until it is listed.
        let expected = match kind {
            Kind::Usage => 2,
            Kind::NotFound
            | Kind::Refused
            | Kind::Busy
            | Kind::Corrupt
            | Kind::Incompatible
            | Kind::Config
            | Kind::Io
            | Kind::Internal => 1,
        };
        assert_eq!(exit_code(kind), expected, "{kind:?}");
    }
}
