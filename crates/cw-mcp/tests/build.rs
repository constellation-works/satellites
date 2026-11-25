//! What building a server refuses.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[allow(dead_code)]
mod fixture;

use cw_app::Runtime;
use cw_mcp::{Access, Error, Server, ServerInfo, Tool};
use serde_json::json;

#[test]
fn two_tools_under_one_name_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut tools = fixture::tools();
    tools.push(Tool::new::<fixture::NoInput>(
        "echo",
        "A second echo.",
        Access::ReadOnly,
        |_, _, _| Ok(json!({})),
    ));
    let info = ServerInfo::new("fixture", "1.2.3", fixture::PANIC_CODE);
    let refused = Server::new(
        info,
        Runtime::new(root.path(), fixture::Calls::default()),
        tools,
    );
    match refused {
        Err(Error::DuplicateTool { name }) => assert_eq!(name, "echo"),
        other => panic!("expected DuplicateTool, got {other:?}"),
    }
}
