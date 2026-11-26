//! The server the wire tests run: five tools over a call counter.

use cw_app::{Origin, Runtime};
use cw_base::{Error, Kind};
use cw_mcp::testing::Harness;
use cw_mcp::{Access, Server, ServerInfo, Tool};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How many times any tool function ran.
pub type Calls = Arc<AtomicUsize>;

/// The code a panicked tool is reported with.
pub const PANIC_CODE: &str = "fixture_panicked";

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct EchoInput {
    /// What to say back.
    pub text: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct NoInput {}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct FailInput {
    /// Fail as busy (retryable) rather than refused.
    pub busy: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("the fixture failed on purpose")]
pub struct Failed;

fn count(runtime: &Runtime<Calls>) {
    runtime.state().fetch_add(1, Ordering::SeqCst);
}

fn echo(runtime: &Runtime<Calls>, origin: &Origin, input: EchoInput) -> Result<Value, Error> {
    count(runtime);
    Ok(json!({
        "text": input.text,
        "surface": origin.surface().as_str(),
        "actor": origin.actor(),
    }))
}

fn list(runtime: &Runtime<Calls>, _: &Origin, _: NoInput) -> Result<Value, Error> {
    count(runtime);
    Ok(json!([1, 2, 3]))
}

fn fail(runtime: &Runtime<Calls>, _: &Origin, input: FailInput) -> Result<Value, Error> {
    count(runtime);
    Err(if input.busy {
        Error::new(Kind::Busy, "fixture_busy", Failed)
    } else {
        Error::new(Kind::Refused, "fixture_refused", Failed)
    })
}

fn explode(runtime: &Runtime<Calls>, _: &Origin, _: NoInput) -> Result<Value, Error> {
    count(runtime);
    panic!("the fixture panicked on purpose");
}

/// The fixture's tools.
pub fn tools() -> Vec<Tool<Calls>> {
    vec![
        Tool::new::<EchoInput>("echo", "Say the text back.", Access::ReadOnly, echo),
        Tool::new::<NoInput>("list", "Return a list.", Access::ReadOnly, list),
        Tool::new::<FailInput>("fail", "Fail on purpose.", Access::Mutating, fail),
        Tool::new::<NoInput>("explode", "Panic on purpose.", Access::Mutating, explode),
    ]
}

/// The fixture server over a temp root, and its call counter.
#[expect(
    clippy::expect_used,
    reason = "fixture setup: a failure here is a broken fixture, not the case under test"
)]
pub fn server(root: &std::path::Path) -> (Server<Calls>, Calls) {
    let calls = Calls::default();
    let info = ServerInfo::new("fixture", "1.2.3", PANIC_CODE);
    let server = Server::new(info, Runtime::new(root, Arc::clone(&calls)), tools())
        .expect("the fixture's tools are valid");
    (server, calls)
}

/// A harness on the fixture server, through the handshake as `client`.
#[expect(
    clippy::expect_used,
    reason = "fixture setup: a failure here is a broken fixture, not the case under test"
)]
pub fn started(root: &std::path::Path, client: &str) -> (Harness, Calls, Value) {
    let (server, calls) = server(root);
    let mut harness = Harness::start(server).expect("the server starts");
    let initialized = harness.initialize(client).expect("initialize answers");
    (harness, calls, initialized)
}

/// A `tools/call` request for `name` with `arguments`, as a raw message.
pub fn call(id: u64, name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    })
}
