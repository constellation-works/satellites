//! Failures on the wire: a failed tool is a normal result marked as an
//! error; a request the server cannot route is a protocol error and runs
//! nothing; a panic is a failed tool and the server goes on.

mod fixture;

use serde_json::json;
use std::sync::atomic::Ordering;

#[test]
fn a_tool_error_is_a_result_marked_as_an_error_with_code_message_kind_and_retryable() {
    let root = tempfile::tempdir().unwrap();
    let (mut harness, _, _) = fixture::started(root.path(), "errors-client");

    let reply = harness
        .request(&fixture::call(2, "fail", json!({"busy": false})))
        .unwrap();
    let result = &reply["result"];
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(result["isError"], true, "{reply}");
    assert_eq!(
        result["structuredContent"],
        json!({
            "code": "fixture_refused",
            "message": "the fixture failed on purpose",
            "kind": "refused",
            "retryable": false,
        }),
        "{reply}"
    );

    let reply = harness
        .request(&fixture::call(3, "fail", json!({"busy": true})))
        .unwrap();
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert_eq!(
        reply["result"]["structuredContent"],
        json!({
            "code": "fixture_busy",
            "message": "the fixture failed on purpose",
            "kind": "busy",
            "retryable": true,
        }),
        "{reply}"
    );
    harness.close().unwrap().unwrap();
}

#[test]
fn retryable_is_true_for_busy_only() {
    use cw_base::Kind;
    let kinds = [
        Kind::Usage,
        Kind::NotFound,
        Kind::Refused,
        Kind::Busy,
        Kind::Corrupt,
        Kind::Incompatible,
        Kind::Config,
        Kind::Io,
        Kind::Internal,
    ];
    for kind in kinds {
        assert_eq!(
            cw_mcp::payload::is_retryable(kind),
            kind == Kind::Busy,
            "{kind:?}"
        );
    }
}

#[test]
fn an_unknown_tool_is_a_protocol_error_and_runs_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (mut harness, calls, _) = fixture::started(root.path(), "errors-client");
    let reply = harness
        .request(&fixture::call(2, "missing", json!({})))
        .unwrap();
    assert_eq!(reply["id"], 2, "{reply}");
    assert_eq!(reply["error"]["code"], -32602, "{reply}");
    assert!(
        reply["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("missing")),
        "{reply}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    harness.close().unwrap().unwrap();
}

#[test]
fn arguments_that_do_not_match_the_schema_are_a_protocol_error_and_run_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (mut harness, calls, _) = fixture::started(root.path(), "errors-client");
    // Arguments that are not an object at all fail the SDK's typed request
    // too; they are still invalid parameters of a known method.
    for (id, arguments) in [(2, json!({})), (3, json!({"text": 7})), (4, json!(["hi"]))] {
        let reply = harness
            .request(&fixture::call(id, "echo", arguments))
            .unwrap();
        assert_eq!(reply["id"], id, "{reply}");
        assert_eq!(reply["error"]["code"], -32602, "{reply}");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    harness.close().unwrap().unwrap();
}

#[test]
fn a_panicking_tool_gives_an_error_result_and_the_server_answers_the_next_request() {
    let root = tempfile::tempdir().unwrap();
    let (mut harness, calls, _) = fixture::started(root.path(), "errors-client");
    let reply = harness
        .request(&fixture::call(2, "explode", json!({})))
        .unwrap();
    let result = &reply["result"];
    assert_eq!(result["isError"], true, "{reply}");
    let content = &result["structuredContent"];
    assert_eq!(content["code"], fixture::PANIC_CODE, "{reply}");
    assert_eq!(content["kind"], "internal", "{reply}");
    assert_eq!(content["retryable"], false, "{reply}");
    assert!(
        content["message"]
            .as_str()
            .is_some_and(|message| message.contains("panicked on purpose")),
        "{reply}"
    );

    let reply = harness
        .request(&fixture::call(3, "echo", json!({"text": "still here"})))
        .unwrap();
    assert_eq!(
        reply["result"]["structuredContent"]["text"], "still here",
        "{reply}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    harness.close().unwrap().unwrap();
}
