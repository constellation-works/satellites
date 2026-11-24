//! The protocol on the wire: raw JSON-RPC lines over an in-memory stream.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod fixture;

use serde_json::json;

#[test]
fn initialize_succeeds_and_advertises_the_tools_capability_only() {
    let root = tempfile::tempdir().unwrap();
    let (harness, _, initialized) = fixture::started(root.path(), "wire-client");
    let result = &initialized["result"];
    assert_eq!(initialized["id"], 1, "{initialized}");
    assert_eq!(result["serverInfo"]["name"], "fixture", "{initialized}");
    assert_eq!(result["serverInfo"]["version"], "1.2.3", "{initialized}");
    let capabilities = result["capabilities"].as_object().expect("capabilities");
    assert_eq!(
        capabilities.keys().collect::<Vec<_>>(),
        ["tools"],
        "{initialized}"
    );
    harness.close().unwrap().unwrap();
}

#[test]
fn tools_list_gives_each_tool_its_name_description_derived_schema_and_access() {
    let root = tempfile::tempdir().unwrap();
    let (mut harness, _, _) = fixture::started(root.path(), "wire-client");
    let reply = harness
        .request(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
        .unwrap();
    let tools = reply["result"]["tools"].as_array().expect("a tool list");
    let names: Vec<_> = tools.iter().map(|tool| &tool["name"]).collect();
    assert_eq!(names, ["echo", "list", "fail", "explode"], "{reply}");

    // Each tool's description and the schema derived from its input type:
    // draft 2020-12, an object, without the type's own title.
    let schema = "https://json-schema.org/draft/2020-12/schema";
    let empty = json!({"$schema": schema, "type": "object"});
    let expected = [
        (
            "Say the text back.",
            json!({
                "$schema": schema,
                "type": "object",
                "properties": {"text": {"type": "string", "description": "What to say back."}},
                "required": ["text"],
            }),
        ),
        ("Return a list.", empty.clone()),
        (
            "Fail on purpose.",
            json!({
                "$schema": schema,
                "type": "object",
                "properties": {"busy": {
                    "type": "boolean",
                    "description": "Fail as busy (retryable) rather than refused.",
                }},
                "required": ["busy"],
            }),
        ),
        ("Panic on purpose.", empty),
    ];
    for (tool, (description, input_schema)) in tools.iter().zip(expected) {
        assert_eq!(tool["description"], description, "{tool}");
        assert_eq!(tool["inputSchema"], input_schema, "{tool}");
    }

    let read_only: Vec<_> = tools
        .iter()
        .map(|tool| &tool["annotations"]["readOnlyHint"])
        .collect();
    assert_eq!(read_only, [true, true, false, false], "{reply}");
    harness.close().unwrap().unwrap();
}

#[test]
fn tools_call_returns_the_functions_json_as_structured_content() {
    let root = tempfile::tempdir().unwrap();
    let (mut harness, _, _) = fixture::started(root.path(), "wire-client");
    let reply = harness
        .request(&fixture::call(2, "echo", json!({"text": "hi"})))
        .unwrap();
    let result = &reply["result"];
    assert_eq!(
        result["structuredContent"],
        json!({"text": "hi", "surface": "mcp", "actor": "wire-client"}),
        "{reply}"
    );
    assert_ne!(result["isError"], true, "{reply}");
    harness.close().unwrap().unwrap();
}

#[test]
fn a_list_is_wrapped_as_a_record() {
    let root = tempfile::tempdir().unwrap();
    let (mut harness, _, _) = fixture::started(root.path(), "wire-client");
    let reply = harness
        .request(&fixture::call(2, "list", json!({})))
        .unwrap();
    assert_eq!(
        reply["result"]["structuredContent"],
        json!({"items": [1, 2, 3]}),
        "{reply}"
    );
    harness.close().unwrap().unwrap();
}

#[test]
fn the_origin_names_the_mcp_surface_and_the_initialize_client() {
    let root = tempfile::tempdir().unwrap();
    let (mut harness, _, _) = fixture::started(root.path(), "  origin-client ");
    let reply = harness
        .request(&fixture::call(2, "echo", json!({"text": "who"})))
        .unwrap();
    let content = &reply["result"]["structuredContent"];
    assert_eq!(content["surface"], "mcp", "{reply}");
    assert_eq!(content["actor"], "  origin-client ", "{reply}");
    harness.close().unwrap().unwrap();
}
