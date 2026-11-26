//! A session that ends while a tool is still running: the server returns
//! within its bounded shutdown instead of waiting for the tool.

use cw_app::Runtime;
use cw_mcp::testing::Harness;
use cw_mcp::{Access, Server, ServerInfo, Tool};
use serde_json::json;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The SDK's 5 s wait for in-flight answers, the runtime's 2 s grace, and
/// room for a slow runner.
const BOUND: Duration = Duration::from_secs(15);

/// A tool that says it started, then holds until the test lets it go.
struct Gate {
    started: Mutex<Sender<()>>,
    release: Mutex<Receiver<()>>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct NoInput {}

#[test]
fn serve_returns_within_the_bound_when_the_input_closes_during_a_running_tool() {
    let (started, tool_started) = mpsc::channel();
    let (let_go, release) = mpsc::channel::<()>();
    let gate = Arc::new(Gate {
        started: Mutex::new(started),
        release: Mutex::new(release),
    });
    let tools = vec![Tool::new::<NoInput>(
        "hold",
        "Hold until released.",
        Access::ReadOnly,
        |runtime: &Runtime<Arc<Gate>>, _, _| {
            let gate = runtime.state();
            let _ = gate.started.lock().unwrap().send(());
            let _ = gate.release.lock().unwrap().recv();
            Ok(json!({}))
        },
    )];
    let root = tempfile::tempdir().unwrap();
    let info = ServerInfo::new("shutdown", "0.1.0", "shutdown_panicked");
    let server = Server::new(info, Runtime::new(root.path(), Arc::clone(&gate)), tools).unwrap();
    let mut harness = Harness::with_deadline(server, BOUND).unwrap();
    harness.initialize("shutdown-client").unwrap();
    let call = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "hold", "arguments": {} },
    });
    harness.send(&call.to_string()).unwrap();
    tool_started
        .recv_timeout(BOUND)
        .expect("the tool starts running");

    let closing = Instant::now();
    let served = harness
        .close()
        .expect("the server returns within the bound");
    let elapsed = closing.elapsed();
    served.expect("the session ends cleanly");
    assert!(elapsed < BOUND, "the server took {elapsed:?} to return");

    // The abandoned tool finishes now that it is let go.
    drop(let_go);
}
